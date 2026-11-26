import hashlib
import io
import pathlib
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile


BUNDLER = pathlib.Path(__file__).with_name("bundle-release.py")


class BundleTests(unittest.TestCase):
    def fixture(self, root, windows, missing=False):
        name = "aidrive-windows-x86_64.zip" if windows else "aidrive-linux-x86_64.tar.gz"
        required = ["aidrive.exe", "aidrive-mcp-server.exe"] if windows else ["aidrive", "aidrive-mcp-server"]
        required += ["web/index.html", "LICENSE-APACHE", "LICENSE-MIT", "THIRD-PARTY-NOTICES.md"]
        if missing:
            required.remove("web/index.html")
        if windows:
            with zipfile.ZipFile(root / name, "w") as archive:
                for path in required:
                    archive.writestr(path, b"synthetic fixture")
        else:
            with tarfile.open(root / name, "w:gz") as archive:
                for path in required:
                    entry = tarfile.TarInfo("./" + path)
                    entry.size = 17
                    archive.addfile(entry, io.BytesIO(b"synthetic fixture"))
        digest = hashlib.sha256((root / name).read_bytes()).hexdigest()
        (root / "SHA256SUMS").write_text(digest + "  " + name + "\n")
        for path in ["README.md", "LICENSE-APACHE", "LICENSE-MIT", "THIRD-PARTY-NOTICES.md", "install.cmd", "install.ps1", "install.sh"]:
            (root / path).write_text("synthetic fixture")

    def run_bundle(self, root, windows):
        return subprocess.run([sys.executable, "-I", "-B", str(BUNDLER), "--dist", str(root),
                               "--platform", "windows-x86_64" if windows else "linux-x86_64",
                               "--revision", "synthetic-revision"], capture_output=True)

    def test_both_platforms_include_installer_payload_and_checksums(self):
        for windows in [True, False]:
            with self.subTest(windows=windows), tempfile.TemporaryDirectory() as scratch:
                root = pathlib.Path(scratch)
                self.fixture(root, windows)
                result = self.run_bundle(root, windows)
                self.assertEqual(result.returncode, 0, result.stderr)
                if windows:
                    with zipfile.ZipFile(root / "AIDrive-setup-windows-x86_64.zip") as archive:
                        names = set(archive.namelist())
                    self.assertTrue({"AIDrive/install.cmd", "AIDrive/install.ps1", "AIDrive/aidrive-windows-x86_64.zip"} <= names)
                else:
                    with tarfile.open(root / "AIDrive-setup-linux-x86_64.tar.gz") as archive:
                        names = set(archive.getnames())
                    self.assertTrue({"AIDrive/install.sh", "AIDrive/aidrive-linux-x86_64.tar.gz"} <= names)
                self.assertTrue({"AIDrive/SHA256SUMS", "AIDrive/BUILD.json"} <= names)
                self.assertNotEqual(self.run_bundle(root, windows).returncode, 0)

    def test_missing_web_or_bad_checksum_refused(self):
        for missing in [True, False]:
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as scratch:
                root = pathlib.Path(scratch)
                self.fixture(root, True, missing)
                if not missing:
                    (root / "SHA256SUMS").write_text("bad checksum\n")
                self.assertNotEqual(self.run_bundle(root, True).returncode, 0)
                self.assertFalse((root / "AIDrive-setup-windows-x86_64.zip").exists())


if __name__ == "__main__":
    unittest.main()
