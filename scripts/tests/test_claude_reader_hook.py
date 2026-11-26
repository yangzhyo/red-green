import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


HOOK = Path(__file__).resolve().parents[2] / "hooks/session-status.sh"


class ReaderHookTest(unittest.TestCase):
    def test_running_session_acquires_mapping_and_ends(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            session = "reader-test-1"
            status = root / ".claude/session-status"
            status.mkdir(parents=True)
            snapshot = status / f"{session}.json"
            snapshot.write_text('{"state":"running"}')
            transcript = root / f"{session}.jsonl"
            mapping = status / ".reader" / session
            event = dict(session_id=session, cwd=folder, hook_event_name="PreToolUse",
                         transcript_path=str(transcript))
            def send():
                subprocess.run(["bash", str(HOOK)], input=json.dumps(event), text=True,
                               env={**os.environ, "HOME": folder}, check=True)
            send()
            self.assertEqual(mapping.read_text().strip(), str(transcript))
            self.assertEqual(snapshot.read_text(), '{"state":"running"}')
            event.update(hook_event_name="SessionEnd")
            send()
            self.assertFalse(snapshot.exists())
            self.assertEqual(mapping.read_text().strip(), "ended")
