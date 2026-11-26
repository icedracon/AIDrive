import argparse
import hashlib
import json
import pathlib
import re
import subprocess
from datetime import datetime, timezone


RULES = {
    "aidrive_credential": rb"aidrive_(?:agent|device)_[A-Za-z0-9_-]{24,}",
    "provider_key": rb"(?:sk-(?:proj-|ant-)?[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{30,}|AIza[A-Za-z0-9_-]{30,}|xox[baprs]-[A-Za-z0-9-]{15,}|AKIA[A-Z0-9]{16})",
    "private_key": rb"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----",
    "url_password": rb"[a-zA-Z][a-zA-Z0-9+.-]*://[^\s/@:\"'<>]+:[^\s/@\"'<>]+@",
    "labelled_credential": rb"(?i)[\"']?(?:api[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret|secret[_-]?key|password|token)[\"']?\s*[:=]\s*[\"']([A-Za-z0-9_./+=-]{16,})[\"']",
    "bearer_literal": rb"(?i)Bearer\s+[A-Za-z0-9_./+=-]{24,}",
}
PRIVACY = {
    "absolute_user_path": rb"(?i)(?:[A-Z]:[\\/]+Users[\\/]+|/home/|/Users/)[A-Za-z0-9_.-]+",
    "email": rb"[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+",
}
SENSITIVE_PATH = re.compile(r"(?i)(?:^|/)(?:\.env(?:\..*)?|[^/]*\.(?:pem|key|p12|pfx|dump|sqlite|db)|(?:profiles|sessions|rollouts)/.*)$")


def git(*args):
    return subprocess.check_output(["git", *args])


def records(data, patterns):
    result = []
    for rule, pattern in patterns.items():
        for match in re.finditer(pattern, data):
            value = match.group(1) if match.lastindex else match.group()
            result.append({"rule": rule, "line": data.count(b"\n", 0, match.start()) + 1,
                           "fingerprint": hashlib.sha256(value).hexdigest()[:16]})
    return result


def main():
    parser = argparse.ArgumentParser(description="Read-only reachable Git blob audit; never prints matching values.")
    parser.add_argument("--report", required=True, type=pathlib.Path)
    args = parser.parse_args()
    head = git("rev-parse", "HEAD").decode().strip()
    refs = git("for-each-ref", "--format=%(refname) %(objectname)").decode().splitlines()
    paths = {}
    for row in git("rev-list", "--objects", "--all").splitlines():
        oid, _, path = row.partition(b" ")
        paths[oid.decode()] = path.decode("utf-8", "replace")
    current = {}
    for row in git("ls-tree", "-rz", "HEAD").split(b"\0"):
        if not row:
            continue
        meta, path = row.split(b"\t", 1)
        _, kind, oid = meta.split()
        if kind == b"blob":
            current.setdefault(oid.decode(), []).append(path.decode("utf-8", "replace"))
    checked = subprocess.run(["git", "cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)"],
                             input=("\n".join(paths) + "\n").encode(), check=True, stdout=subprocess.PIPE).stdout
    blobs = [(oid, int(size)) for oid, kind, size in (line.decode().split() for line in checked.splitlines()) if kind == "blob"]
    report = {"created_at": datetime.now(timezone.utc).isoformat(), "head": head, "refs": refs,
              "scope": "All blobs reachable from all local refs at scan start; untracked files excluded; no network or credential validation.",
              "limits": "Pattern candidates require triage; unknown credential shapes and private prose may be missed. Binary content is not semantically reviewed. Object paths are representative, not exhaustive historical aliases. Git author metadata needs separate review.",
              "blob_count": len(blobs), "text_blob_count": 0, "bytes_scanned": 0,
              "binary_blobs": [], "credential_candidates": [], "privacy_candidates": [], "sensitive_paths": []}
    with subprocess.Popen(["git", "cat-file", "--batch"], stdin=subprocess.PIPE, stdout=subprocess.PIPE) as process:
        for oid, size in blobs:
            process.stdin.write((oid + "\n").encode())
            process.stdin.flush()
            header = process.stdout.readline().decode().split()
            if len(header) != 3 or header[1] != "blob" or int(header[2]) != size:
                raise RuntimeError("Unexpected object response")
            data = process.stdout.read(size)
            if len(data) != size or process.stdout.read(1) != b"\n":
                raise RuntimeError("Truncated Git object")
            path = paths.get(oid, "")
            where = {"blob": oid, "path": path, "head_paths": current.get(oid, [])}
            report["bytes_scanned"] += size
            if SENSITIVE_PATH.search(path) and path != ".env.example":
                report["sensitive_paths"].append(where)
            if b"\0" in data:
                report["binary_blobs"].append({**where, "bytes": size})
            else:
                report["text_blob_count"] += 1
                report["privacy_candidates"].extend({**where, **item} for item in records(data, PRIVACY))
            report["credential_candidates"].extend({**where, **item} for item in records(data, RULES))
        process.stdin.close()
    args.report.parent.mkdir(parents=True, exist_ok=True)
    with args.report.open("x", encoding="utf-8") as target:
        json.dump(report, target, ensure_ascii=False, indent=2)
    print(json.dumps({"head": head, "blobs": len(blobs), "text_blobs": report["text_blob_count"],
                      "binary_blobs": len(report["binary_blobs"]), "credential_candidates": len(report["credential_candidates"]),
                      "privacy_candidates": len(report["privacy_candidates"]), "sensitive_paths": len(report["sensitive_paths"]),
                      "report": str(args.report)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
