import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("memory_report", Path(__file__).resolve().parents[1] / "mosaico-memory-report.py")
report = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(report)


def sample(minute, boot="boot1", free=100000, largest=50000, minimum=10000):
    return {"schema": 1, "received_at_ms": 1800000000000 + minute * 60000,
            "boot": boot, "generation": "g1", "firmware": "test", "uptime_ms": minute * 60000,
            "sessions": 2, "idle": True, "screen_on": False,
            "internal": {"free": free, "largest": largest, "minimum": minimum},
            "psram": {"free": 3000000, "largest": 2500000, "minimum": 1000000}}


class MemoryReportTests(unittest.TestCase):
    def test_stable_two_hours_and_only_low_watermark_decreases(self):
        data = [sample(i, minimum=90000 - i * 100) for i in range(121)]
        result = report.analyze(data)
        self.assertTrue(result["boots"][0]["two_hours_contiguous"])
        group = result["groups"][0]
        self.assertEqual(group["status"], "observed")
        self.assertEqual(group["changes"]["internal.free"], 0)
        self.assertEqual(group["declining"], [])

    def test_decline_is_separate_from_fragmentation_and_startup_warmup(self):
        data = [sample(i, free=100000 - i * 100, largest=50000 - i * 50) for i in range(121)]
        group = report.analyze(data)["groups"][0]
        self.assertIn("internal.free", group["declining"])
        self.assertLess(group["changes"]["internal.free"], -9000)
        for i in range(10):
            data[i]["internal"]["free"] = 200000
        self.assertIn("internal.free", report.analyze(data)["groups"][0]["declining"])
        data = [sample(i, largest=50000 - i * 50) for i in range(121)]
        group = report.analyze(data)["groups"][0]
        self.assertEqual(group["changes"]["internal.free"], 0)
        self.assertEqual(group["declining"], ["internal.largest"])

    def test_short_boots_gaps_and_contexts_cannot_be_combined(self):
        data = [sample(i, "boot1") for i in range(61)] + [sample(i, "boot2") for i in range(61)]
        result = report.analyze(data)
        self.assertEqual(len(result["boots"]), 2)
        self.assertFalse(any(b["two_hours_contiguous"] for b in result["boots"]))
        data = [sample(i) for i in range(121) if i != 60]
        result = report.analyze(data)
        self.assertFalse(result["boots"][0]["two_hours_contiguous"])
        self.assertEqual(result["boots"][0]["gaps"], 1)
        for i, row in enumerate(data):
            row["sessions"] = i % 2 + 1
        self.assertEqual(len(report.analyze(data)["groups"]), 2)
        self.assertEqual(report.analyze([sample(i) for i in range(5)])["groups"][0]["status"], "insufficient")

    def test_input_rejects_bad_rows_and_deduplicates_rotation_overlap(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "samples.jsonl"
            bad = sample(2); bad["internal"]["largest"] = 200000
            rows = [sample(1), sample(1), bad, {"schema": 99}]
            path.write_text("\n".join(json.dumps(r) for r in rows) + "\n{bad\n")
            records, invalid, duplicates = report.load_records([path])
            self.assertEqual(len(records), 1)
            self.assertEqual(invalid, 3)
            self.assertEqual(duplicates, 1)

if __name__ == "__main__":
    unittest.main()
