#!/usr/bin/env python3
"""只读备份指定 Mosaico；仅临时切换 ROM，不写 Flash/eFuse。需要 esptool 5.6。"""

import argparse
import hashlib
import json
import os
import signal
from pathlib import Path
import time

import serial
from serial.tools import list_ports
from esptool import cmds
from esptool.targets.esp32s31 import ESP32S31ROM
from espefuse.efuse.esp32s31.fields import EspEfuses


def mac_id(value):
    return value.lower().replace(":", "").replace("-", "")


def enter_rom(port):
    # 官方 esp_iris_usb_download.h 的 opposing DTR/RTS 序列。
    with serial.Serial(port=None, baudrate=115200, timeout=0.2, exclusive=True) as s:
        s.dtr = s.rts = False
        s.port = port
        s.open()
        s.rts = True
        time.sleep(0.1)
        s.dtr = True
        s.rts = False
        time.sleep(0.01)
        s.dtr = False
        time.sleep(0.1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", required=True)
    parser.add_argument("--expected-mac", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    expected = mac_id(args.expected_mac)
    assert len(expected) == 12 and all(c in "0123456789abcdef" for c in expected)
    port = next(p for p in list_ports.comports() if p.device == args.port)
    assert (
        port.vid == 0x303A and mac_id(port.serial_number or "") == expected
    ), "目标身份不符"
    assert port.pid in (0x4002, 0x0020), "不支持的维护/ROM接口"
    args.output.mkdir(parents=True, mode=0o700, exist_ok=False)
    os.chmod(args.output, 0o700)
    if port.pid == 0x4002:
        enter_rom(port.device)
    rom = None
    for _ in range(100):
        matches = [
            p
            for p in list_ports.comports()
            if p.vid == 0x303A
            and p.pid == 0x0020
            and mac_id(p.serial_number or "") == expected
        ]
        if len(matches) == 1:
            rom = matches[0].device
            break
        time.sleep(0.1)
    assert rom, "未发现同一设备的 ROM 接口；未写入任何设备数据"
    link = serial.Serial(port=None, baudrate=115200, timeout=3, exclusive=True)
    link.dtr = link.rts = False
    link.port = rom
    link.open()
    try:
        esp = ESP32S31ROM(link)
        esp.connect("no-reset")
        actual = "".join(f"{b:02x}" for b in esp.read_mac())
        assert actual == expected, "ROM MAC 不符"
        efuses = EspEfuses(esp)
        board = int.from_bytes(bytes.fromhex(efuses["USER_DATA"].get())[:2], "little")
        assert board in (0x0101, 0x0102), f"不支持的板型 {board:#06x}"
        cmds.attach_flash(esp)
        flash_id = esp.flash_id()
        size = 1 << ((flash_id >> 16) & 255)
        assert 4 * 1024 * 1024 <= size <= 64 * 1024 * 1024, "未知 Flash 容量"
        print(
            f"已验证 ESP32-S31，板型 eFuse={board:#06x}，Flash={size//1024//1024} MiB",
            flush=True,
        )
        # esptool 官方 S31 stub 仅装入 RAM，加速读取；本脚本没有擦写命令。
        link.dtr = True  # 与 esptool CLI 一致，USB stub 的大块发送需要主机已打开。
        esp = esp.run_stub()

        def timed_out(signum, frame):
            raise TimeoutError("只读备份块超过 60 秒；未写入 Flash")

        signal.signal(signal.SIGALRM, timed_out)

        def read_all():
            blocks = []
            for offset in range(0, size, 256 * 1024):
                signal.alarm(60)
                blocks.append(
                    cmds.read_flash(
                        esp, offset, min(256 * 1024, size - offset), no_progress=True
                    )
                )
                signal.alarm(0)
                if (offset + len(blocks[-1])) % (1024 * 1024) == 0:
                    print(
                        f"读取 {(offset+len(blocks[-1]))//1024//1024}/{size//1024//1024} MiB",
                        flush=True,
                    )
            return b"".join(blocks)

        data = read_all()
        verify = read_all()
        assert data == verify, "两次只读备份不一致；不能部署"
        backup = args.output / "flash.bin"
        with backup.open("xb") as f:
            os.fchmod(f.fileno(), 0o600)
            f.write(data)
        (args.output / "partition-table.bin").write_bytes(data[0x8000:0x9000])
        os.chmod(args.output / "partition-table.bin", 0o600)
        metadata = {
            "chip": "esp32s31",
            "mac": actual,
            "board_efuse": board,
            "flash_id": flash_id,
            "flash_bytes": size,
            "sha256": hashlib.sha256(data).hexdigest(),
            "partition_sha256": hashlib.sha256(data[0x8000:0x9000]).hexdigest(),
            "verified": "two identical full reads",
            "writes": 0,
        }
        (args.output / "manifest.json").write_text(
            json.dumps(metadata, indent=2) + "\n"
        )
        os.chmod(args.output / "manifest.json", 0o600)
        print(
            f"两次完整读取一致；备份保存在 {args.output}。没有写入 Flash/eFuse。",
            flush=True,
        )
    finally:
        signal.alarm(0)
        link.close()


if __name__ == "__main__":
    main()
