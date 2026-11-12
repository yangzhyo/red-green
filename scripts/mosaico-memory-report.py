#!/usr/bin/env python3
"""只读分析 Mosaico 内存 JSONL；不连接设备，不自动判定零泄漏。"""
import argparse
from collections import defaultdict
from datetime import datetime, timezone
import json
from pathlib import Path
from statistics import median

PERIOD_MS = 60000
WARMUP_MS = 10 * PERIOD_MS
MAX_GAP_MS = 90000
METRICS = [(heap, field) for heap in ("internal", "psram") for field in ("free", "largest")]


def valid(row):
    def integer(value, maximum):
        return type(value) is int and 0 <= value <= maximum
    if not isinstance(row, dict) or row.get("schema") != 1:
        return False
    if not all(isinstance(row.get(k), str) and 0 < len(row[k]) <= 80
               for k in ("boot", "generation", "firmware")):
        return False
    if not all(integer(row.get(k), 9007199254740991) for k in ("uptime_ms", "received_at_ms")):
        return False
    try:
        datetime.fromtimestamp(row["received_at_ms"] / 1000, timezone.utc).astimezone()
    except (ValueError, OverflowError, OSError):
        return False
    if not integer(row.get("sessions"), 32) or any(type(row.get(k)) is not bool for k in ("idle", "screen_on")):
        return False
    for name in ("internal", "psram"):
        heap = row.get(name)
        if not isinstance(heap, dict) or not all(integer(heap.get(k), 64 * 1024 * 1024) for k in ("free", "largest", "minimum")):
            return False
        if heap["largest"] > heap["free"] or heap["minimum"] > heap["free"]:
            return False
    return True


def load_records(paths):
    records, seen, invalid, duplicates = [], set(), 0, 0
    for path in paths:
        with path.open() as stream:
            for line in stream:
                try:
                    row = json.loads(line)
                except (ValueError, RecursionError):
                    invalid += 1
                    continue
                if not valid(row):
                    invalid += 1
                    continue
                key = (row["boot"], row["firmware"], row["uptime_ms"])
                if key in seen:
                    duplicates += 1
                    continue
                seen.add(key)
                records.append(row)
    return records, invalid, duplicates


def analyze(records):
    by_boot = defaultdict(list)
    for row in records:
        by_boot[(row["boot"], row["firmware"])].append(row)
    result = {"boots": [], "groups": []}
    for (boot, firmware), rows in sorted(by_boot.items()):
        rows.sort(key=lambda r: r["uptime_ms"])
        start = rows[0]["uptime_ms"]
        span = rows[-1]["uptime_ms"] - start
        gaps = sum(max(b["uptime_ms"] - a["uptime_ms"], b["received_at_ms"] - a["received_at_ms"]) > MAX_GAP_MS
                   or b["received_at_ms"] < a["received_at_ms"] for a, b in zip(rows, rows[1:]))
        result["boots"].append({"boot": boot, "firmware": firmware, "samples": len(rows),
            "span_ms": span, "gaps": gaps, "last_received_at_ms": rows[-1]["received_at_ms"],
            "two_hours_contiguous": span >= 120 * PERIOD_MS and gaps == 0})
        contexts = defaultdict(list)
        for row in rows:
            contexts[(row["idle"], row["screen_on"], row["sessions"])].append(row)
        for (idle, screen_on, sessions), context in sorted(contexts.items()):
            usable = [r for r in context if r["uptime_ms"] >= start + WARMUP_MS]
            enough = len(usable) >= 30 and usable[-1]["uptime_ms"] - usable[0]["uptime_ms"] >= 30 * PERIOD_MS
            group = {"boot": boot, "firmware": firmware, "idle": idle, "screen_on": screen_on,
                     "sessions": sessions, "samples": len(context), "after_warmup": len(usable),
                     "status": "observed" if enough else "insufficient", "changes": {}, "declining": []}
            if enough:
                for heap, field in METRICS:
                    values = [r[heap][field] for r in usable]
                    name = f"{heap}.{field}"
                    group["changes"][name] = median(values[-5:]) - median(values[:5])
                    buckets = defaultdict(list)
                    for row in usable:
                        buckets[(row["uptime_ms"] - start) // WARMUP_MS].append(row[heap][field])
                    levels = [median(v) for _, v in sorted(buckets.items()) if len(v) >= 3]
                    if len(levels) >= 4 and all(b < a for a, b in zip(levels, levels[1:])):
                        group["declining"].append(name)
            result["groups"].append(group)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=Path, help="可同时提供当前和轮转 JSONL")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    paths = args.paths
    if not paths:
        current = Path.home() / "Library/Logs/red-green-mosaico-memory.jsonl"
        paths = [p for p in (current.with_suffix(".jsonl.1"), current) if p.exists()]
    try:
        records, invalid, duplicates = load_records(paths)
    except OSError as error:
        parser.exit(2, f"读取失败：{error}\n")
    result = analyze(records)
    result.update(invalid_rows=invalid, duplicates=duplicates)
    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return
    if not records:
        print("尚无有效内存样本；不能判断趋势。")
    for boot in result["boots"]:
        end = datetime.fromtimestamp(boot["last_received_at_ms"] / 1000, timezone.utc).astimezone().isoformat(timespec="seconds")
        coverage = "已覆盖两小时连续采样" if boot["two_hours_contiguous"] else "尚未覆盖两小时连续采样"
        print(f"启动 {boot['boot']} / 固件 {boot['firmware']}：{boot['samples']} 个样本，跨度 {boot['span_ms']/3600000:.2f} 小时，断档 {boot['gaps']} 次；{coverage}。最后接收 {end}")
        for group in [g for g in result["groups"] if (g["boot"], g["firmware"]) == (boot["boot"], boot["firmware"])]:
            label = f"{'全空闲' if group['idle'] else '有活动'} / {'亮屏' if group['screen_on'] else '熄屏'} / {group['sessions']} 会话"
            if group["status"] == "insufficient":
                print(f"  {label}：预热后 {group['after_warmup']} 个样本，可比记录不足。")
                continue
            changes = "，".join(f"{k} {v/1024:+.1f} KiB" for k, v in group["changes"].items())
            print(f"  {label}：首尾各 5 点中位数变化：{changes}")
            if group["declining"]:
                print("  持续下降信号：" + "、".join(group["declining"]) + "；需进一步定位，不能单凭此判定泄漏。")
    print(f"忽略无效行 {invalid}，重复行 {duplicates}。预热排除最初 10 分钟；不同启动、固件和状态分开比较。")
    print("历史最低剩余用于观察峰值，不参与泄漏判断；报告不自动宣称内存测试通过。")


if __name__ == "__main__":
    main()
