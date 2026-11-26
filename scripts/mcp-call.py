#!/usr/bin/env python3
"""Call AIDrive MCP tools from a shell, as an assistant would.

usage: mcp-call.py PROFILE TOOL [JSON_ARGS] [--field PATH]
Prints the tool's text result, or one field of its JSON (dot path).
"""
import json
import os
import subprocess
import sys


def main() -> int:
    args = sys.argv[1:]
    field = None
    if "--field" in args:
        i = args.index("--field")
        field = args[i + 1]
        del args[i : i + 2]
    if len(args) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    profile, tool = args[0], args[1]
    arguments = json.loads(args[2]) if len(args) > 2 else {}
    exe = os.environ.get("AIDRIVE_MCP", "aidrive-mcp-server")
    p = subprocess.Popen([exe, "serve", "--profile", profile], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, text=True)

    def send(msg):
        p.stdin.write(json.dumps(msg) + "\n")
        p.stdin.flush()

    def reply(i):
        while True:
            line = p.stdout.readline()
            if not line:
                raise SystemExit("MCP server exited")
            r = json.loads(line)
            if r.get("id") == i:
                return r

    send({"jsonrpc": "2.0", "id": 0, "method": "initialize",
          "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                     "clientInfo": {"name": "mcp-call", "version": "0"}}})
    reply(0)
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    send({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
          "params": {"name": tool, "arguments": arguments}})
    r = reply(1)
    p.stdin.close()
    p.wait()
    if "error" in r:
        print(json.dumps(r["error"]), file=sys.stderr)
        return 1
    text = "".join(c.get("text", "") for c in r["result"].get("content", []))
    if field:
        value = json.loads(text)
        for part in field.split("."):
            value = value[int(part)] if isinstance(value, list) else value[part]
        print(value)
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
