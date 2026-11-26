import json
import hashlib
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest


EXPORTER = pathlib.Path(__file__).with_name("prepare-public-snapshot.py").resolve()


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.env = {key: value for key, value in os.environ.items() if not key.upper().startswith("GIT_")}
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        self.git("init", "-q", "--template=")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "core.autocrlf", "false")
        self.git("config", "core.hooksPath", str(self.root / "no-hooks"))

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, env=self.env, stderr=subprocess.PIPE)

    def commit(self):
        self.git("add", ".")
        self.git("-c", "commit.gpgsign=false", "commit", "-qm", "Fixture")

    def run_export(self):
        return subprocess.run([sys.executable, "-I", "-B", str(EXPORTER), "--revision", "HEAD", "--output", str(self.root / "out")], cwd=self.repo, env=self.env, capture_output=True)

    def test_export_current_only_and_preserve_original(self):
        (self.repo / "internal.txt").write_text("Historical private fixture")
        self.commit()
        self.git("rm", "internal.txt")
        original = b'C:/Users/' + b'fixture/work C:\\\\Users\\\\' + b'fixture\\\\work /home/' + b'fixture/work'
        (self.repo / "notes.md").write_bytes(original)
        (self.repo / "asset.bin").write_bytes(b"\0binary")
        self.commit()
        (self.repo / "untracked.txt").write_text("Untracked")
        result = self.run_export()
        self.assertEqual(result.returncode, 0, result.stderr)
        out = self.root / "out"
        self.assertFalse((out / ".git").exists())
        self.assertFalse((out / "internal.txt").exists())
        self.assertFalse((out / "untracked.txt").exists())
        self.assertNotIn(b"fixture", (out / "notes.md").read_bytes())
        self.assertEqual((self.repo / "notes.md").read_bytes(), original)
        self.assertEqual((out / "asset.bin").read_bytes(), b"\0binary")
        manifest = json.loads((out / "PUBLIC-SNAPSHOT-MANIFEST.json").read_text())
        self.assertEqual(sum(row["transformed"] for row in manifest["files"]), 1)
        self.assertNotEqual(self.run_export().returncode, 0)

    def test_normalized_rust_still_passes_formatter_and_manifest_matches(self):
        line = b'        unbound.contains(r#"bind --client claude --profile NAME --project "C:\\Users\\ab\\SideTest""#)'
        self.assertEqual(len(line), 99)
        original = b'fn main() {\n    assert!(\n' + line + b'\n    );\n}\n'
        baseline = subprocess.run(
            ["rustfmt", "--emit", "stdout", "--edition", "2024"],
            input=original, capture_output=True, check=True,
        )
        self.assertEqual(baseline.stdout.replace(b"\r\n", b"\n"), original)
        (self.repo / "main.rs").write_bytes(original)
        self.commit()
        result = self.run_export()
        self.assertEqual(result.returncode, 0, result.stderr)
        out = self.root / "out"
        data = (out / "main.rs").read_bytes()
        self.assertIn(b"Users\\USER", data)
        check = subprocess.run(
            ["rustfmt", "--check", "--edition", "2024", str(out / "main.rs")],
            capture_output=True,
        )
        self.assertEqual(check.returncode, 0, check.stdout + check.stderr)
        manifest = json.loads((out / "PUBLIC-SNAPSHOT-MANIFEST.json").read_text())
        row = next(row for row in manifest["files"] if row["path"] == "main.rs")
        self.assertEqual(row["export_sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(row["source_sha256"], hashlib.sha256(original).hexdigest())

    def test_frozen_change_refused_before_output_created(self):
        path = self.repo / "eval/heldout"
        path.mkdir(parents=True)
        (path / "frozen.json").write_text(json.dumps({"set": "eval/heldout/data.json"}))
        (path / "data.json").write_text('"C:/Users/' + 'fixture/work"')
        self.commit()
        self.assertNotEqual(self.run_export().returncode, 0)
        self.assertFalse((self.root / "out").exists())


if __name__ == "__main__":
    unittest.main()
