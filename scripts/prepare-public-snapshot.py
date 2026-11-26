import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import tomllib


USER_PATH = re.compile(rb"(?i)([A-Z]:[\\/]+Users[\\/]+|/home/|/Users/)([A-Za-z0-9_.-]+)")


def git(*args):
    return subprocess.check_output(["git", *args])


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description="Export a reviewed public snapshot without source Git history.")
    parser.add_argument("--revision", required=True)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    output = args.output.absolute()
    if output.exists():
        raise SystemExit("Output already exists; nothing overwritten")
    revision = git("rev-parse", "--verify", args.revision + "^{commit}").decode().strip()
    files = {}
    for row in git("ls-tree", "-rz", revision).split(b"\0"):
        if not row:
            continue
        meta, raw_path = row.split(b"\t", 1)
        mode, kind, oid = meta.split()
        path = raw_path.decode("utf-8", "strict")
        parts = pathlib.PurePosixPath(path).parts
        if kind != b"blob" or mode not in (b"100644", b"100755"):
            raise SystemExit("Unsupported link or tree entry")
        if not parts or any(part in ("..", ".git") or ":" in part or "\\" in part for part in parts) or path.startswith("/"):
            raise SystemExit("Unsafe export path")
        files[path] = (mode, git("cat-file", "blob", oid.decode()))
    frozen = {"eval/heldout/frozen.json", "eval/heldout2/frozen.json"}
    for path in tuple(frozen):
        if path not in files:
            continue
        config = json.loads(files[path][1])
        if isinstance(config.get("set"), str):
            frozen.add(config["set"])
        frozen.update(config.get("sets", {}))
        frozen.update(config.get("code", {}))
    cargo = tomllib.loads(files["Cargo.toml"][1].decode()) if "Cargo.toml" in files else {}
    edition = str(cargo.get("workspace", {}).get("package", {}).get("edition", "2024"))
    planned = []
    manifest = {"source_revision": revision, "history_included": False,
                "note": "User path components normalized in listed text copies; changed Rust sources formatted with rustfmt. Transformed evaluation reports are redacted copies, not byte-exact raw evidence. No Git identity, repository or publication created.",
                "files": []}
    for path, (mode, original) in sorted(files.items()):
        data = original if b"\0" in original else USER_PATH.sub(lambda match: match[1] + b"USER", original)
        if path in frozen and data != original:
            raise SystemExit("A frozen evaluation file would change; export refused")
        if path.endswith(".rs") and data != original:
            try:
                formatted = subprocess.run(
                    ["rustfmt", "--emit", "stdout", "--edition", edition, "--config", "skip_children=true"],
                    input=data, capture_output=True, check=True,
                )
            except (OSError, subprocess.CalledProcessError):
                raise SystemExit("Cannot format normalized Rust source; export refused") from None
            data = formatted.stdout
        planned.append((path, mode, data))
        manifest["files"].append({"path": path, "source_sha256": digest(original), "export_sha256": digest(data), "transformed": data != original})
    manifest_name = "PUBLIC-SNAPSHOT-MANIFEST.json"
    if manifest_name in files:
        raise SystemExit("Manifest path collision")
    output.mkdir(parents=True, exist_ok=False)
    for path, mode, data in planned:
        target = output.joinpath(*pathlib.PurePosixPath(path).parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        if os.name != "nt" and mode == b"100755":
            target.chmod(0o755)
    (output / manifest_name).write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"source_revision": revision, "files": len(planned), "transformed": sum(item["transformed"] for item in manifest["files"]), "output": str(output)}))


if __name__ == "__main__":
    main()
