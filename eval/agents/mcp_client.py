"""Minimal MCP stdio client for the AIDrive server (standard library only)."""
import json
import os
import subprocess
import threading
import time

EXE = os.environ.get(
    "AIDRIVE_MCP",
    os.path.join(os.environ.get("LOCALAPPDATA", ""), "AIDrive", "bin", "aidrive-mcp-server.exe"),
)


class MCPError(Exception):
    pass


class MCPClient:
    def __init__(self, profile, client_name="aidrive-agent-eval", cwd=None):
        self.profile = profile
        self.proc = subprocess.Popen(
            [EXE, "serve", "--profile", profile],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8", cwd=cwd,
        )
        self.stderr_lines = []
        threading.Thread(target=self._drain_stderr, daemon=True).start()
        self._id = 0
        r = self.request("initialize", {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": client_name, "version": "0"},
        })
        self.server_info = r.get("serverInfo", {})
        self.instructions = r.get("instructions", "")
        self.notify("notifications/initialized")
        self.tools = self.request("tools/list", {}).get("tools", [])

    def _drain_stderr(self):
        for line in self.proc.stderr:
            self.stderr_lines.append(line.rstrip())

    def notify(self, method, params=None):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()

    def request(self, method, params):
        self._id += 1
        i = self._id
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": i, "method": method, "params": params}) + "\n")
        self.proc.stdin.flush()
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise MCPError("MCP server exited: " + " | ".join(self.stderr_lines[-3:]))
            r = json.loads(line)
            if r.get("id") == i:
                if "error" in r:
                    raise MCPError(json.dumps(r["error"]))
                return r["result"]

    def call(self, name, arguments=None):
        """Returns (text, is_error, seconds)."""
        t = time.monotonic()
        try:
            r = self.request("tools/call", {"name": name, "arguments": arguments or {}})
        except MCPError as e:
            return str(e), True, time.monotonic() - t
        text = "".join(c.get("text", "") for c in r.get("content", []))
        return text, bool(r.get("isError")), time.monotonic() - t

    def call_json(self, name, arguments=None):
        text, err, _ = self.call(name, arguments)
        if err:
            raise MCPError(text)
        try:
            return json.loads(text)
        except ValueError:
            return text

    def openai_tools(self, allow=None):
        out = []
        for t in self.tools:
            if allow and t["name"] not in allow:
                continue
            out.append({"type": "function", "function": {
                "name": t["name"], "description": t.get("description", ""),
                "parameters": t.get("inputSchema") or {"type": "object", "properties": {}},
            }})
        return out

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=10)
        except Exception:
            self.proc.kill()

    def kill(self):
        self.proc.kill()
