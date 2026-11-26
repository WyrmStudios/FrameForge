import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class NodeAmountsTest(unittest.TestCase):
    def test_refresh_preserves_zero_and_refuses_incomplete_or_conflicting_data(self):
        script = Path(__file__).with_name("update_mastery_nodes.py")
        scratch = script.resolve().parents[2] / "tmp"
        scratch.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch) as directory:
            directory = Path(directory)
            runner = directory / script.name
            runner.write_bytes(script.read_bytes())
            table = directory / "mastery_nodes.rs"
            table.write_text('        ("SolNode27", "E Prime", 0),\n        ("SolNode94", "Apollodorus", 24),\n')
            data = directory / "missions.lua"
            valid = '{ InternalName = "SolNode27", MasteryExp = 24 },\n{ MasteryExp = 0, InternalName = "SolNode94" },\n'

            def run(source):
                data.write_text(source)
                return subprocess.run([sys.executable, str(runner), str(data)], capture_output=True).returncode

            self.assertEqual(run(valid), 0)
            expected = '        ("SolNode27", "E Prime", 24),\n        ("SolNode94", "Apollodorus", 0),\n'
            self.assertEqual(table.read_text(), expected)
            self.assertEqual(run(valid), 0)
            self.assertEqual(table.read_text(), expected)
            for invalid in [valid.splitlines()[0], valid + '{ InternalName = "SolNode27", MasteryExp = 99 },', valid.replace("24", "2.4")]:
                self.assertNotEqual(run(invalid), 0)
                self.assertEqual(table.read_text(), expected)


if __name__ == "__main__":
    unittest.main()
