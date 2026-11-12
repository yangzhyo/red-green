#!/usr/bin/env python3
"""通过真实 LVGL 像素检查气泡，捕获 ASCII 替代和颜色/尺寸丢失。"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

preview = Path(sys.argv[1]).resolve()
destination = Path(sys.argv[2]) if len(sys.argv) > 2 else None
if destination:
    destination.mkdir(parents=True, exist_ok=True)


def render(directory, skin, state):
    stem = directory / f"{skin}-{state}"
    sequence = ["completed", "running", "idle", "awaiting", "aborted", "your_turn", state]
    stem.with_suffix(".json").write_text(json.dumps([{
        "sessions": [{"id": "probe", "slot": 0, "name": "bubble",
                      "skin": skin, "state": item, "source": "codex"}],
        "usage": [],
    } for item in sequence]))
    subprocess.run([str(preview), str(stem.with_suffix(".json")),
                    str(stem.with_suffix(".ppm")), str(200 * len(sequence))], check=True,
                   stdout=subprocess.DEVNULL)
    header, pixels = stem.with_suffix(".ppm").read_bytes().split(b"\n255\n", 1)
    assert header == b"P6\n480 480"
    return [(x, y, tuple(pixels[(y * 480 + x) * 3:(y * 480 + x) * 3 + 3]))
            for y in range(140, 184) for x in range(24, 277)]


with tempfile.TemporaryDirectory(prefix="rg-bubbles-") as temporary:
    directory = destination or Path(temporary)
    for skin in ("robot", "crab", "cat"):
        distinct = set()
        for state, color in (("completed", "yellow"), ("idle", "blue"),
                             ("awaiting", "red"), ("aborted", "yellow"),
                             ("your_turn", "yellow"), ("running", "text")):
            points = render(directory, skin, state)
            colored = [(x, y) for x, y, (r, g, b) in points
                       if (color == "yellow" and r > 180 and g > 110 and b < 120)
                       or (color == "blue" and b > 150 and b > r * 1.4)
                       or (color == "red" and r > 160 and r > g * 1.5)]
            if color == "text":
                # 运行中仍是白色动词，不能残留彩色符号。
                assert sum(r > 180 and g > 180 and b > 180
                           for _, _, (r, g, b) in points) > 100
                assert not any(max(rgb) - min(rgb) > 80 for _, _, rgb in points)
            else:
                assert len(colored) > 80, f"{skin}/{state}: 缺少可辨认的彩色气泡（{len(colored)} 像素）"
                # 尺寸按整个图形测量；💥 的黄色内芯小于外侧红色轮廓。
                xs, ys = zip(*[(x, y) for x, y, rgb in points if max(rgb) - min(rgb) > 80])
                assert max(xs) - min(xs) >= 18 and max(ys) - min(ys) >= 18, \
                    f"{skin}/{state}: 气泡被缩成小符号"
                assert min(xs) > 24 and max(xs) < 276 and min(ys) > 140 and max(ys) < 183, \
                    f"{skin}/{state}: 气泡超出安全区域"
                signature = bytes(c for _, _, rgb in points for c in rgb)
                assert signature not in distinct, f"{skin}/{state}: 不同状态复用了同一气泡"
                distinct.add(signature)
    print("18 个真实 LVGL 状态通过：五组彩色气泡、运行中文字、可辨认尺寸及状态切换")
