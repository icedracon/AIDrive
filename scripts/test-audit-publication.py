import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCANNER = Path(__file__).resolve().with_name("audit-publication.py")


@unittest.skipUnless(shutil.which("git"), "Git is required for the reachable-history audit")
class PublicationAuditTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="publication-audit-test-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.root = Path(cls.temporary.name)
        cls.repo = cls.root / "repo"
        cls.repo.mkdir()
        cls.env = {key: value for key, value in os.environ.items() if not key.upper().startswith("GIT_")}
        cls.env.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
                        "GIT_TERMINAL_PROMPT": "0"})
        cls.credentials = {name: "aidrive_agent_" + character * 64 for name, character in (
            ("history", "H"), ("current", "C"), ("branch", "R"),
            ("ignored", "I"), ("untracked", "U"), ("binary", "B"),
        )}
        cls.git("init", "--quiet", "--initial-branch=main", "--template=")
        cls.git("config", "user.name", "Synthetic Audit Test")
        cls.git("config", "user.email", "audit-fixture@example.invalid")
        cls.git("config", "commit.gpgsign", "false")
        cls.git("config", "core.autocrlf", "false")
        cls.git("config", "core.hooksPath", str(cls.root / "no-hooks"))
        cls.write(".gitignore", "ignored.txt\n")
        cls.write(".env", "# Synthetic historical fixture\n" + cls.credentials["history"] + "\n")
        cls.write(".env.example", "EXAMPLE_SETTING=replace-me\n")
        cls.git("add", ".gitignore", ".env", ".env.example")
        cls.git("commit", "--quiet", "-m", "Synthetic historical fixture")
        cls.history_blob = cls.git("rev-parse", "HEAD:.env").strip()

        cls.git("checkout", "--quiet", "-b", "synthetic-retained-history")
        cls.write("branch-only.txt", cls.credentials["branch"] + "\n")
        cls.git("add", "branch-only.txt")
        cls.git("commit", "--quiet", "-m", "Synthetic side ref fixture")
        cls.branch_blob = cls.git("rev-parse", "HEAD:branch-only.txt").strip()
        cls.git("checkout", "--quiet", "main")
        cls.git("rm", "--quiet", ".env")
        cls.write("current.txt", "Synthetic current fixture\n" + cls.credentials["current"] + "\n")
        cls.binary = b"\x00synthetic binary\xff\n" + cls.credentials["binary"].encode() + b"\n"
        (cls.repo / "asset.bin").write_bytes(cls.binary)
        cls.git("add", "current.txt", "asset.bin")
        cls.git("commit", "--quiet", "-m", "Remove historical credential and add current fixtures")
        cls.head = cls.git("rev-parse", "HEAD").strip()
        cls.current_blob = cls.git("rev-parse", "HEAD:current.txt").strip()
        cls.binary_blob = cls.git("rev-parse", "HEAD:asset.bin").strip()
        cls.write("ignored.txt", cls.credentials["ignored"] + "\n")
        cls.write("untracked.txt", cls.credentials["untracked"] + "\n")
        cls.report_path = cls.root / "reports" / "audit.json"
        cls.scan = cls.run_scan(cls.report_path)
        if cls.scan.returncode != 0:
            raise AssertionError("Synthetic publication scan failed")
        cls.report_text = cls.report_path.read_text(encoding="utf-8")
        cls.report = json.loads(cls.report_text)

    @classmethod
    def git(cls, *args):
        return subprocess.check_output(["git", *args], cwd=cls.repo, env=cls.env,
                                       stderr=subprocess.PIPE, text=True, timeout=30)

    @classmethod
    def write(cls, path, content):
        (cls.repo / path).write_text(content, encoding="utf-8")

    @classmethod
    def run_scan(cls, report):
        return subprocess.run([sys.executable, "-I", "-B", str(SCANNER), "--report", str(report)],
                              cwd=cls.repo, env=cls.env, capture_output=True, text=True, timeout=30)

    def candidates(self, name):
        fingerprint = hashlib.sha256(self.credentials[name].encode()).hexdigest()[:16]
        return [item for item in self.report["credential_candidates"]
                if item["fingerprint"] == fingerprint]

    def test_removed_credential_remains_reachable_and_has_no_head_paths(self):
        self.assertFalse((self.repo / ".env").exists())
        matches = self.candidates("history")
        self.assertEqual(len(matches), 1)
        self.assertEqual(matches[0], {
            "blob": self.history_blob, "path": ".env", "head_paths": [],
            "rule": "aidrive_credential", "line": 2,
            "fingerprint": hashlib.sha256(self.credentials["history"].encode()).hexdigest()[:16],
        })

    def test_all_local_refs_include_history_outside_head_ancestry(self):
        self.assertFalse((self.repo / "branch-only.txt").exists())
        matches = self.candidates("branch")
        self.assertEqual(len(matches), 1)
        self.assertEqual(matches[0]["blob"], self.branch_blob)
        self.assertEqual(matches[0]["path"], "branch-only.txt")
        self.assertEqual(matches[0]["head_paths"], [])
        self.assertTrue(any(ref.startswith("refs/heads/synthetic-retained-history ")
                            for ref in self.report["refs"]))
        self.assertEqual(self.report["head"], self.head)

    def test_current_tracked_candidate_and_untracked_exclusions(self):
        matches = self.candidates("current")
        self.assertEqual(len(matches), 1)
        self.assertEqual(matches[0]["blob"], self.current_blob)
        self.assertEqual(matches[0]["path"], "current.txt")
        self.assertEqual(matches[0]["head_paths"], ["current.txt"])
        self.assertEqual(matches[0]["line"], 2)
        self.assertEqual(self.git("check-ignore", "ignored.txt").strip(), "ignored.txt")
        self.assertIn("untracked.txt", self.git("ls-files", "--others", "--exclude-standard").splitlines())
        self.assertEqual(self.candidates("ignored"), [])
        self.assertEqual(self.candidates("untracked"), [])
        reachable_blobs = set()
        for row in self.git("rev-list", "--objects", "--all").splitlines():
            oid = row.split(" ", 1)[0]
            if self.git("cat-file", "-t", oid).strip() == "blob":
                reachable_blobs.add(oid)
        self.assertEqual(self.report["blob_count"], len(reachable_blobs))
        for section in ("credential_candidates", "privacy_candidates", "sensitive_paths", "binary_blobs"):
            self.assertFalse(any(item["path"] in ("ignored.txt", "untracked.txt")
                                 for item in self.report[section]))

    def test_historical_env_is_sensitive_but_example_is_not(self):
        sensitive = [item for item in self.report["sensitive_paths"] if item["path"] == ".env"]
        self.assertEqual(sensitive, [{"blob": self.history_blob, "path": ".env", "head_paths": []}])
        self.assertIn(".env.example", self.git("ls-tree", "--name-only", "HEAD").splitlines())
        self.assertFalse(any(item["path"] == ".env.example" for item in self.report["sensitive_paths"]))

    def test_binary_is_reported_and_credentials_are_redacted_everywhere(self):
        self.assertEqual(self.report["binary_blobs"], [{
            "blob": self.binary_blob, "path": "asset.bin", "head_paths": ["asset.bin"],
            "bytes": len(self.binary),
        }])
        self.assertEqual(self.candidates("binary")[0]["blob"], self.binary_blob)
        self.assertEqual(self.report["text_blob_count"], self.report["blob_count"] - 1)
        output = self.report_text + self.scan.stdout + self.scan.stderr
        for value in self.credentials.values():
            self.assertNotIn(value, output)
        summary = json.loads(self.scan.stdout)
        self.assertEqual(summary["credential_candidates"], len(self.report["credential_candidates"]))

    def test_existing_report_cannot_be_overwritten(self):
        before = self.report_path.read_bytes()
        result = self.run_scan(self.report_path)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.report_path.read_bytes(), before)
        self.assertEqual(result.stdout, "")
        for value in self.credentials.values():
            self.assertNotIn(value, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
