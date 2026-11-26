"""Synthetic live file-index QA. Run with python -I -B; standard library only."""
import hashlib
import http.client
import json
import os
from pathlib import Path
import queue
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import uuid


class Failure(Exception):
    pass


def require(condition, label):
    if not condition:
        raise Failure(label)
    print(json.dumps({"pass": label}), flush=True)


class API:
    def __init__(self):
        self.cookie = ""
        self.token = ""

    def call(self, method, path, body=None, agent=False, expected=200):
        conn = http.client.HTTPConnection("127.0.0.1", 7480, timeout=15)
        headers = {"Origin": "http://127.0.0.1:7480", "Sec-Fetch-Site": "same-origin",
                   "Content-Type": "application/json"}
        if agent:
            headers["Authorization"] = "Bearer " + self.token
        elif self.cookie:
            headers["Cookie"] = self.cookie
        try:
            conn.request(method, path, None if body is None else json.dumps(body).encode(), headers)
            response = conn.getresponse()
            raw = response.read()
            cookie = response.getheader("Set-Cookie")
            if cookie and not agent:
                self.cookie = cookie.split(";", 1)[0]
            if response.status != expected:
                raise Failure("HTTP " + method + " " + path + " status " + str(response.status))
            return json.loads(raw) if raw else {}
        finally:
            conn.close()


class MCP:
    def __init__(self, exe, env, cwd):
        self.proc = subprocess.Popen([str(exe), "serve"], cwd=cwd, env=env,
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL, text=True, encoding="utf-8",
                                     creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.lines = queue.Queue()
        self.counter = 0
        threading.Thread(target=self.read, daemon=True).start()
        init = self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                            "clientInfo": {"name": "synthetic-file-index-qa", "version": "1"}})
        self.info = init.get("serverInfo", {})
        self.instructions = init.get("instructions", "")
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        self.proc.stdin.flush()
        self.schemas = {t["name"]: t["inputSchema"] for t in self.request("tools/list", {})["tools"]}

    def read(self):
        for line in self.proc.stdout:
            self.lines.put(line)
        self.lines.put(None)

    def request(self, method, params):
        self.counter += 1
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.counter,
                                         "method": method, "params": params}) + "\n")
        self.proc.stdin.flush()
        deadline = time.monotonic() + 40
        while True:
            line = self.lines.get(timeout=max(0.01, deadline - time.monotonic()))
            if line is None:
                raise Failure("MCP exited")
            value = json.loads(line)
            if value.get("id") == self.counter:
                if "error" in value:
                    raise Failure("MCP protocol error")
                return value["result"]

    def call(self, name, **args):
        schema = self.schemas[name]
        require(set(args) <= set(schema.get("properties", {}))
                and set(schema.get("required", [])) <= set(args), "tools/list args: " + name)
        value = self.request("tools/call", {"name": name, "arguments": args})
        if value.get("isError"):
            raise Failure("MCP tool error: " + name)
        return "\n".join(c.get("text", "") for c in value.get("content", []))

    def close(self):
        self.proc.stdin.close()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=5)


def main():
    repo = Path(__file__).resolve().parents[2]
    main_repo = repo.parents[2] if repo.parent.name == "worktrees" else repo
    exe = Path(os.environ.get("AIDRIVE_FILE_QA_MCP", str(main_repo / "target/debug/aidrive-mcp-server.exe")))
    require(exe.is_file(), "actual MCP executable exists")
    binary_data = exe.read_bytes()
    binary_hash = hashlib.sha256(binary_data).hexdigest()
    print(json.dumps({"source_binary": str(exe), "sha256": binary_hash}), flush=True)
    api = API()
    require(api.call("GET", "/local/mode").get("local") is True, "private 7480 local mode")
    api.call("POST", "/local/session", {})
    projects = api.call("GET", "/workspaces")["workspaces"]
    require(any(p["name"] == "Design lab" for p in projects), "known synthetic Design lab fixture present")
    name = "File index synthetic QA " + uuid.uuid4().hex[:12]
    workspace = api.call("POST", "/workspaces", {"name": name}, expected=201)["workspace"]["id"]
    owner = "/workspaces/" + workspace
    base = "/agent" + owner
    grant = api.call("POST", owner + "/agents", {"request_id": str(uuid.uuid4()),
                    "name": name, "scope": "files:write"}, expected=201)
    api.token = grant.pop("token")
    grant_id = grant["grant"]["id"]
    root = Path(tempfile.mkdtemp(prefix="aidrive-file-index-qa-" )).resolve()
    mcp = None
    watcher = None
    registered = set()
    try:
        exe = root / "aidrive-mcp-server-frozen.exe"
        exe.write_bytes(binary_data)
        del binary_data
        require(hashlib.sha256(exe.read_bytes()).hexdigest() == binary_hash, "private immutable binary snapshot matches recorded hash")
        binding_paths = [Path(os.environ[k]) / "AIDrive/profiles/bindings.json"
                         for k in ("APPDATA", "LOCALAPPDATA") if k in os.environ]

        def binding_hashes():
            return {str(p): hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None
                    for p in binding_paths}

        original_bindings = binding_hashes()
        bound = root / "synthetic-bound"
        bound.mkdir()
        scratch = bound / "probe.txt"
        scratch.write_bytes(b"synthetic scratch\x00\xff\n")
        (bound / "Cargo.toml").write_text('[package]\nname="synthetic-qa"\nversion="0.0.0"\n', encoding="utf-8")
        build = bound / "target"
        build.mkdir()
        payload = build / "fixture.bin"
        payload.write_bytes(bytes(range(256)) * 5)
        env = {k: v for k, v in os.environ.items() if not k.startswith("AIDRIVE_")}
        for key in ("APPDATA", "LOCALAPPDATA", "HOME", "USERPROFILE", "CODEX_HOME", "CLAUDE_CONFIG_DIR", "XDG_CONFIG_HOME"):
            location = root / key.lower()
            location.mkdir()
            env[key] = str(location)
        env.update(AIDRIVE_API_URL="http://127.0.0.1:7480", AIDRIVE_AGENT_TOKEN=api.token,
                   AIDRIVE_WORKSPACE_ID=workspace, RUST_LOG="off")
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            env["AIDRIVE_WATCH_PORT"] = str(sock.getsockname()[1])
        profile_dir = Path(env["APPDATA"]) / "AIDrive/profiles"
        profile_dir.mkdir(parents=True)
        profile_name = "codex-file-index-qa"
        for filename, data in {
            profile_name + ".json": {"api_url": env["AIDRIVE_API_URL"], "workspace_id": workspace,
                                       "workspace_name": name, "scope": "files:write", "token": api.token},
            "bindings.json": {"codex": {str(bound): profile_name}},
            "capture-consent.json": {"codex": {str(bound): True}},
        }.items():
            (profile_dir / filename).write_text(json.dumps(data), encoding="utf-8")
        mcp = MCP(exe, env, bound)
        print(json.dumps({"server": mcp.info, "workspace": workspace, "fixture_root": str(root),
                          "process_host": env.get("COMPUTERNAME", env.get("HOSTNAME", ""))}), flush=True)

        def rows():
            return api.call("GET", base + "/artifacts?status=all&limit=500", agent=True)["artifacts"]

        def row(path):
            matches = [r for r in rows() if r["path"] == str(path)]
            if len(matches) != 1:
                raise Failure("Expected one fixture index row: " + path.name)
            registered.add(matches[0]["id"])
            return matches[0]

        require(rows() == [], "new synthetic index starts empty")
        result = mcp.call("register_file", path=str(scratch), purpose="Own synthetic cleanup fixture",
                          kind="temp", keep_days=3, session=name)
        item = row(scratch)
        require(result.startswith("Registered.") and item["size_bytes"] == scratch.stat().st_size
                and item["kind"] == "temp" and item["status"] == "active", "MCP manual register measured exact bytes")
        print(json.dumps({"registered_proof": {k: item[k] for k in
              ("id", "path", "host", "size_bytes", "kind", "status", "keep_until", "session_ref")}}), flush=True)
        require(str(scratch) in mcp.call("list_registered_files", this_computer=True), "MCP this-computer listing finds registration")
        mcp.call("register_file", path=str(scratch), purpose="Own synthetic cleanup fixture",
                 kind="temp", keep_days=3, session=name)
        require(row(scratch)["id"] == item["id"] and len(rows()) == 1,
                "manual registration retry keeps one index row")

        def scan():
            nonlocal watcher
            watcher = subprocess.Popen([str(exe), "watch", "--interval", "15"], cwd=bound, env=env,
                                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                       creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            deadline = time.monotonic() + 25
            expected_size = payload.stat().st_size
            try:
                while time.monotonic() < deadline:
                    matches = [r for r in rows() if r["path"] == str(build)]
                    if matches and matches[0]["size_bytes"] == expected_size:
                        return
                    if watcher.poll() is not None:
                        raise Failure("isolated watcher exited")
                    time.sleep(0.25)
                raise Failure("automatic build index timed out")
            finally:
                watcher.terminate()
                watcher.wait(timeout=5)
                watcher = None

        scan()
        built = row(build)
        require(built["kind"] == "build" and built["size_bytes"] == 1280
                and built["session_ref"] == "aidrive watch", "watcher automatically indexed build exact bytes")
        print(json.dumps({"automatic_proof": {k: built[k] for k in
              ("id", "path", "host", "size_bytes", "kind", "status", "keep_until", "session_ref")}}), flush=True)
        require(all(r["path"] != str(bound / "Cargo.toml") for r in rows()), "ordinary new project file is not universally auto-indexed")
        host = (env.get("COMPUTERNAME") or env.get("HOSTNAME") or "").strip().lower()
        require(item["host"] == built["host"] == host, "MCP registration and watcher host agree with process environment")
        before = row(build)
        payload.write_bytes(b"a" * 2111)
        scan()
        after = row(build)
        require(after["id"] == before["id"] and after["keep_until"] == before["keep_until"]
                and after["status"] == before["status"] and after["size_bytes"] == 2111,
                "automatic refresh updates size without duplicate or deadline reset")
        past = "2020-01-01T00:00:00Z"
        for path in (scratch, build):
            current = row(path)
            expired = api.call("POST", base + "/artifacts", {
                "path": current["path"], "host": current["host"], "purpose": current["purpose"],
                "kind": current["kind"], "size_bytes": current["size_bytes"],
                "session": current["session_ref"], "keep_until": past,
            }, agent=True, expected=201)
            require(expired["id"] == current["id"], "synthetic past-deadline registration preserves id")
        brief = mcp.call("get_project_brief")
        require("STILL EXISTS: " + str(scratch) in brief and "STILL EXISTS: " + str(build) in brief,
                "expired scratch and build appear in MCP brief with real existence")
        require(scratch.exists() and payload.exists() and row(scratch)["due"] and row(build)["due"]
                and row(scratch)["last_checked_at"] is not None, "expiry checks persist without auto-delete or auto-resolution")
        require(str(scratch) in mcp.call("list_registered_files", due_only=True, this_computer=True), "MCP due listing includes expired fixture")
        require("Nothing is deleted automatically" in brief and "ask the owner" in mcp.instructions,
                "cleanup review retains owner-approval instruction")
        scan()
        require(row(build)["due"] and row(build)["keep_until"] == past, "automatic refresh preserves expired review date")
        require("active" in mcp.call("resolve_file", file_id=item["id"], action="extend", keep_days=9,
                                    note="Synthetic still needed"), "MCP extend succeeds")
        require(not row(scratch)["due"] and scratch.exists(), "extend retains file and clears due")
        require("kept" in mcp.call("resolve_file", file_id=built["id"], action="keep", note="Synthetic owner keep decision"),
                "MCP keep succeeds")
        kept = row(build)
        payload.write_bytes(b"b" * 3073)
        scan()
        refreshed = row(build)
        require(refreshed["status"] == "kept" and refreshed["keep_until"] is None
                and refreshed["note"] == kept["note"] and refreshed["size_bytes"] == 3073,
                "automatic refresh preserves keep decision and note while updating bytes")
        require(str(build) not in mcp.call("list_registered_files", due_only=True), "kept fixture excluded from due list")
        current = row(scratch)
        api.call("POST", base + "/artifacts", {
            "path": current["path"], "host": current["host"], "purpose": current["purpose"],
            "kind": current["kind"], "size_bytes": current["size_bytes"], "keep_until": past,
        }, agent=True, expected=201)
        scratch.unlink()
        missing_brief = mcp.call("get_project_brief")
        require("gone: " + str(scratch) in missing_brief and row(scratch)["status"] == "missing",
                "brief observes physically missing own fixture and persists missing status")
        require("removed" in mcp.call("resolve_file", file_id=item["id"], action="removed",
                                     note="Physically deleted own synthetic fixture first"), "MCP removed recorded after own-file deletion")
        payload.unlink()
        build.rmdir()
        mcp.call("resolve_file", file_id=built["id"], action="removed", note="Deleted own synthetic build first")
        require(not scratch.exists() and not build.exists() and all(r["status"] == "removed" for r in rows()),
                "physical deletion and removed statuses agree")
        require(mcp.call("list_registered_files") == "No registered files match.", "MCP open index empty after cleanup")
        require(binding_hashes() == original_bindings, "real profile bindings unchanged")
        log = Path(env["APPDATA"]) / "AIDrive/watch.log"
        require(not log.exists() or api.token not in log.read_text(encoding="utf-8"), "watcher log contains no synthetic grant token")
        require(hashlib.sha256(exe.read_bytes()).hexdigest() == binary_hash, "same frozen binary used throughout run")
        print(json.dumps({"result": "PASS", "limitations": [
            "Synthetic API 7480 and selected binary only; not real-space or deployment certification",
            "Watcher first passes exercised by restarting; hourly timer not waited out",
            "Expiry injected through API; no wall-clock wait",
            "Owner approval text and retention verified; deletion consent is agent policy, not filesystem enforcement",
            "Automatic recognized build folders only; scratch registration remains explicit"
        ]}), flush=True)
    finally:
        if watcher is not None:
            watcher.terminate()
            watcher.wait(timeout=5)
        if mcp is not None:
            mcp.close()
        require(root.parent == Path(tempfile.gettempdir()).resolve() and root.name.startswith("aidrive-file-index-qa-"),
                "cleanup confined to own resolved temporary fixture root")
        shutil.rmtree(root)
        try:
            for artifact_id in registered:
                api.call("POST", base + "/artifacts/" + artifact_id + "/resolve",
                         {"action": "removed", "note": "Own synthetic fixture cleanup"}, agent=True)
        finally:
            api.call("DELETE", owner + "/agents/" + grant_id, expected=204)
        print(json.dumps({"cleanup": "own fixture removed; synthetic grant revoked", "workspace": workspace}), flush=True)


if __name__ == "__main__":
    try:
        main()
    except Failure as error:
        print(json.dumps({"result": "FAIL", "check": str(error)}), flush=True)
        raise SystemExit(1)
    except Exception as error:
        print(json.dumps({"result": "FAIL", "error_type": type(error).__name__}), flush=True)
        raise SystemExit(1)
