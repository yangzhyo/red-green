#!/usr/bin/env python3
"""真实 Rust 串口循环 ↔ PTY ↔ 设备 C 核心。--serve 可供 Tauri 隔离验收。"""

import argparse
import ctypes
import json
import os
from pathlib import Path
import pty
import select
import subprocess
import tempfile
import time
import tty

ROOT = Path(__file__).resolve().parents[1]
VERSION = (ROOT / "firmware/mosaico/main/generated/version.txt").read_text().strip()


def milliseconds():
    return int(time.monotonic() * 1000)


def frame(value):
    body = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()
    return b"RGP1" + len(body).to_bytes(4, "little") + body


class Simulator:
    def __init__(self, library, probe=None):
        self.core = ctypes.CDLL(str(library))
        self.core.rg_sim_init.argtypes = [ctypes.c_char_p]
        self.core.rg_sim_receive.argtypes = [ctypes.c_char_p, ctypes.c_uint64]
        self.core.rg_sim_request.argtypes = [ctypes.c_uint64, ctypes.c_int]
        self.core.rg_sim_request.restype = ctypes.c_char_p
        self.core.rg_sim_active.argtypes = [ctypes.c_uint64]
        self.core.rg_sim_pop.argtypes = [ctypes.c_uint64]
        self.core.rg_sim_pop.restype = ctypes.c_char_p
        self.boot = "sim-boot"
        self.core.rg_sim_init(self.boot.encode())
        self.master, self.slave = pty.openpty()
        tty.setraw(self.slave)
        os.set_blocking(self.master, False)
        self.path = os.ttyname(self.slave)
        self.rx = b""
        self.tx = b""
        self.logs = b""
        self.events = []
        self.calls = []
        self.grants = []
        self.last_hello = 0
        self.last_request = 0
        self.last_submit = 0
        self.bound = False
        self.was_active = False
        self.healthy = True
        self.drop_up = False
        self.delay_grants = False
        self.heartbeat = True
        self.pending = []
        self.snapshot = {
            "pets": [
                {
                    "sid": "test-1",
                    "slot": 0,
                    "model": {
                        "project": "中文长名称工具箱",
                        "source": "codex",
                        "skin": "cat",
                        "state": "running",
                    },
                }
            ],
            "desktop_usage": [],
            "device_usage": [],
        }
        self.proc = (
            subprocess.Popen(
                [str(probe)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                env={**os.environ, "RED_GREEN_MOSAICO_PORT": self.path},
            )
            if probe
            else None
        )

    def close(self):
        if self.proc:
            self.proc.terminate()
            self.proc.wait(timeout=5)
        os.close(self.master)
        os.close(self.slave)

    def send(self, j):
        if not self.drop_up:
            data = frame(j)
            if len(self.tx) + len(data) <= 8192:
                self.tx += data

    def submit(self, calls=None):
        if self.proc:
            self.proc.stdin.write(
                (
                    json.dumps({"snapshot": self.snapshot, "calls": calls or []}) + "\n"
                ).encode()
            )
            self.proc.stdin.flush()
            self.last_submit = milliseconds()

    def active(self):
        return bool(self.core.rg_sim_active(milliseconds()))

    def receive(self, j):
        now = milliseconds()
        if j["type"] == "grant":
            self.grants.append(now)
            if self.delay_grants:
                self.pending.append((now + 2100, j))
                return
        result = self.core.rg_sim_receive(
            json.dumps(j, ensure_ascii=False).encode(), now
        )
        if result < 0:
            # 和固件 consume(RG_ERROR) 一样解除绑定；重启边界可能仍有旧 Bind 在途。
            self.core.rg_sim_init(self.boot.encode())
            self.bound = False
            self.events.append((now, {"device": "rejected", "type": j["type"]}))
            return
        if j["type"] == "bind":
            self.bound = True
        if j["type"] == "call":
            assert (
                self.core.rg_sim_receive(json.dumps(j).encode(), now) == 0
            ), "重复 Call 被接受"
        while True:
            name = self.core.rg_sim_pop(now)
            if not name:
                break
            self.calls.append(name.decode())

    def step(self):
        now = milliseconds()
        if self.proc and self.heartbeat and now - self.last_submit >= 1000:
            self.submit()
        if not self.bound and now - self.last_hello >= 500:
            self.send(
                {
                    "type": "hello",
                    "app": "red-green.mosaico",
                    "protocol": 1,
                    "assets": VERSION,
                    "boot": self.boot,
                    "board": "v1.2",
                    "display_ok": True,
                    "audio_ok": True,
                }
            )
            self.last_hello = now
        if self.bound and now - self.last_request >= 500 and not self.pending:
            request = self.core.rg_sim_request(now, self.healthy)
            if request:
                self.send(json.loads(request))
            self.last_request = now
        for when, j in self.pending[:]:
            if now >= when:
                assert (
                    self.core.rg_sim_receive(json.dumps(j).encode(), now) == 0
                ), "迟到 Grant 延长了授权"
                self.pending.remove((when, j))
        streams = [self.master] + ([self.proc.stdout.fileno()] if self.proc else [])
        if self.tx:
            try:
                self.tx = self.tx[os.write(self.master, self.tx) :]
            except BlockingIOError:
                pass
        for fd in select.select(streams, [], [], 0.02)[0]:
            raw = os.read(fd, 65536)
            if fd == self.master:
                self.rx += raw
                while len(self.rx) >= 8:
                    assert self.rx[:4] == b"RGP1"
                    size = int.from_bytes(self.rx[4:8], "little")
                    assert 0 < size <= 65536
                    if len(self.rx) < size + 8:
                        break
                    body = self.rx[8 : size + 8]
                    self.rx = self.rx[size + 8 :]
                    self.receive(json.loads(body))
            else:
                self.logs += raw
                while b"\n" in self.logs:
                    line, self.logs = self.logs.split(b"\n", 1)
                    self.events.append((milliseconds(), json.loads(line)))
        active = self.active()
        if self.was_active and not active:
            self.events.append((milliseconds(), {"device": "expired"}))
            self.core.rg_sim_init(self.boot.encode())
            self.bound = False
        self.was_active = active

    def until(self, predicate, seconds=10):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            self.step()
            if predicate():
                return
        raise AssertionError(f"等待超时；events={self.events[-12:]}")

    def run_for(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.step()


def compile_core(directory):
    base = ROOT / "firmware/mosaico"
    library = directory / "device.dylib"
    subprocess.run(
        [
            "cc",
            "-shared",
            "-fPIC",
            "-Wno-deprecated-declarations",
            "-DCJSON_NESTING_LIMIT=16",
            "-I" + str(base / "main"),
            "-I" + str(base / "components/cjson"),
            str(base / "main/device.c"),
            str(base / "components/cjson/cJSON.c"),
            str(base / "tests/simulator.c"),
            "-lm",
            "-o",
            str(library),
        ],
        check=True,
    )
    return library


def exercise(library, probe, scenario):
    sim = Simulator(library, probe)
    try:
        if scenario == "delayed":
            sim.delay_grants = True
            sim.until(lambda: bool(sim.pending))
            sim.run_for(2.2)
            assert not sim.active()
            print("通过：迟到 2.1 秒的 Grant 未接管", flush=True)
            return
        sim.until(sim.active)
        sim.run_for(0.1)
        assert any(e.get("desktop") == "stop" for _, e in sim.events)
        start = len(sim.events)
        sim.snapshot["pets"][0]["model"]["state"] = "awaiting"
        sim.submit([{"sid": "test-1", "name": "cat-awaiting"}])
        sim.until(lambda: len(sim.calls) == 1)
        assert sim.calls == ["cat-awaiting"]
        assert not any(e.get("desktop") == "call" for _, e in sim.events[start:])
        if scenario == "one-way":
            sim.drop_up = True
        elif scenario == "renderer-stall":
            sim.healthy = False
            sim.drop_up = False
        elif scenario == "manager-stall":
            sim.heartbeat = False
        start = milliseconds()
        sim.snapshot["pets"] = []
        if sim.heartbeat:
            sim.submit()
        sim.until(
            lambda: any(
                t >= start and e.get("desktop") == "show" for t, e in sim.events
            ),
            8,
        )
        show = next(
            t for t, e in sim.events if t >= start and e.get("desktop") == "show"
        )
        assert not sim.active()
        assert show - sim.grants[-1] >= 2400, (show, sim.grants[-1])
        if sim.heartbeat:
            assert [
                e["pets"]
                for t, e in sim.events
                if t >= start and e.get("desktop") == "show"
            ][-1] == []
        assert sim.calls == ["cat-awaiting"], "回退重放了旧叫声"
        print(
            f"通过：{scenario}；最后 Grant 后 {show-sim.grants[-1]} ms 回退，无双播",
            flush=True,
        )
        # 用全新启动身份重新接管，只同步当前空快照，不能重播上一代 Call。
        sim.boot = "sim-reboot"
        sim.core.rg_sim_init(sim.boot.encode())
        sim.bound = False
        sim.healthy = True
        sim.drop_up = False
        sim.heartbeat = True
        sim.submit()
        sim.until(sim.active)
        sim.run_for(0.6)
        assert sim.calls == ["cat-awaiting"], "重启后重放了旧叫声"
        print(f"通过：{scenario} 恢复后新启动代次重新接管，无旧叫声", flush=True)
    finally:
        sim.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serve", action="store_true")
    parser.add_argument(
        "--probe",
        type=Path,
        default=ROOT / "app/src-tauri/target/debug/examples/mosaico_probe",
    )
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="rg-mosaico-") as directory:
        library = compile_core(Path(directory))
        if args.serve:
            sim = Simulator(library)
            print(f"RED_GREEN_MOSAICO_PORT={sim.path}", flush=True)
            try:
                while True:
                    sim.step()
            finally:
                sim.close()
        else:
            for scenario in ["one-way", "renderer-stall", "manager-stall", "delayed"]:
                exercise(library, args.probe, scenario)


if __name__ == "__main__":
    main()
