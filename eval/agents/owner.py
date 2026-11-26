"""Owner actions over local trust: a one-time sign-in link becomes a session cookie held in memory only."""
import http.client
import json
import os
import re
import subprocess

from mcp_client import EXE

API = ("127.0.0.1", 8080)


class Owner:
    def __init__(self):
        out = subprocess.run([EXE, "open", "--no-browser"], capture_output=True, text=True, timeout=30).stdout
        m = re.search(r"https?://\S+/local/login/([0-9a-f]{64})", out)
        if not m:
            raise RuntimeError("no local sign-in link")
        conn = http.client.HTTPConnection(*API, timeout=10)
        conn.request("GET", f"/local/login/{m.group(1)}", headers={"Host": "127.0.0.1:8080"})
        r = conn.getresponse()
        r.read()
        cookie = r.getheader("Set-Cookie") or ""
        self.cookie = cookie.split(";", 1)[0]
        if not self.cookie:
            raise RuntimeError(f"sign-in gave no session (HTTP {r.status})")

    def req(self, method, path, body=None):
        conn = http.client.HTTPConnection(*API, timeout=15)
        headers = {"Cookie": self.cookie, "Content-Type": "application/json",
                   "Origin": "http://localhost:3000", "Host": "127.0.0.1:8080"}
        conn.request(method, path, body=json.dumps(body) if body is not None else None, headers=headers)
        r = conn.getresponse()
        data = r.read().decode("utf-8", "replace")
        try:
            return r.status, json.loads(data)
        except ValueError:
            return r.status, data

    def approve_knowledge(self, ws, item):
        return self.req("POST", f"/workspaces/{ws}/knowledge/{item}/decision", {"decision": "approved"})
