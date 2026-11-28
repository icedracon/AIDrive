import argparse
import hashlib
import json
import pathlib
import tarfile
import tempfile
import zipfile
import shutil

# Visual C++ 2015-2022 runtime the bundled PostgreSQL needs on a clean Windows.
VC_RUNTIME = ("vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll")


def main():
    parser = argparse.ArgumentParser(description="Verify a release payload and bundle its local installers.")
    parser.add_argument("--dist", required=True, type=pathlib.Path)
    parser.add_argument("--platform", required=True, choices=["windows-x86_64", "linux-x86_64"])
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    dist = args.dist.resolve()
    windows = args.platform.startswith("windows")
    payload = dist / ("aidrive-" + args.platform + (".zip" if windows else ".tar.gz"))
    if windows:
        with zipfile.ZipFile(payload) as archive:
            names = set(archive.namelist())
    else:
        with tarfile.open(payload) as archive:
            names = {name.removeprefix("./") for name in archive.getnames()}
    required = {"aidrive.exe", "aidrive-mcp-server.exe", *VC_RUNTIME} if windows else {"aidrive", "aidrive-mcp-server"}
    required.update({"web/index.html", "LICENSE-APACHE", "LICENSE-MIT", "THIRD-PARTY-NOTICES.md"})
    missing = required - names
    if missing:
        raise SystemExit("Release payload missing: " + ", ".join(sorted(missing)))
    expected = hashlib.sha256(payload.read_bytes()).hexdigest()
    checksums = (dist / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
    if not any(line.split() in ([expected, payload.name], [expected, "*" + payload.name]) for line in checksums):
        raise SystemExit("Payload checksum missing or incorrect")
    bundle_name = "AIDrive-setup-" + args.platform
    extension = ".zip" if windows else ".tar.gz"
    output = dist / (bundle_name + extension)
    build = dist / ("BUILD-" + args.platform + ".json")
    if output.exists() or build.exists():
        raise SystemExit("Release output exists; nothing overwritten")
    with tempfile.TemporaryDirectory(prefix="aidrive-bundle-") as scratch:
        stage = pathlib.Path(scratch) / "AIDrive"
        stage.mkdir()
        files = [payload.name, "SHA256SUMS", "README.md", "LICENSE-APACHE", "LICENSE-MIT", "THIRD-PARTY-NOTICES.md"]
        files += ["install.cmd", "install.ps1"] if windows else ["install.sh"]
        for name in files:
            source = dist / name
            if name == "README.md" and not source.exists():
                source = pathlib.Path(__file__).resolve().parents[1] / name
            shutil.copy2(source, stage / name)
        metadata = {"source_revision": args.revision, "platform": args.platform,
                    "payload_sha256": expected, "clean_machine_acceptance": "not established by build"}
        (stage / "BUILD.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
        shutil.make_archive(str(dist / bundle_name), "zip" if windows else "gztar", scratch, "AIDrive")
    build.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"bundle": output.name, "payload_verified": True, "source_revision": args.revision}))


if __name__ == "__main__":
    main()
