import argparse
import http.server
import json
import os
import pathlib
import queue
import subprocess
import tempfile
import threading
import unittest
import uuid


class RoutingTest(unittest.TestCase):
    def test_metadata_survives_process_restart_and_separates_threads(self):
        with tempfile.TemporaryDirectory(prefix="aidrive-routing-test-") as folder:
            root = pathlib.Path(folder)
            workspace = str(uuid.uuid4())
            sessions = {}
            calls = []
            failing = [False]

            class Handler(http.server.BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_GET(self):
                    calls.append(("GET", self.path, None))
                    self.answer({"id": workspace, "name": "Synthetic routing lab", "chats": [], "messages": []})

                def do_POST(self):
                    body = json.loads(self.rfile.read(int(self.headers["content-length"])))
                    calls.append(("POST", self.path, body))
                    if failing[0]:
                        self.answer({"error": "unavailable"}, 503)
                    elif self.path.endswith("/chats"):
                        session = body["session_id"]
                        sessions.setdefault(session, str(uuid.uuid4()))
                        self.answer({"chat_id": sessions[session]})
                    elif self.path.endswith("/messages"):
                        self.answer({"id": str(uuid.uuid4()), "thread_id": str(uuid.uuid4()), "from_chat": body["from_chat"], "to": body["to"], "to_chat": body["to_chat"]}, 201)
                    else:
                        self.answer({"messages": []})

                def answer(self, body, status=200):
                    data = json.dumps(body).encode()
                    self.send_response(status)
                    self.send_header("content-type", "application/json")
                    self.send_header("content-length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)

            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            self.addCleanup(server.server_close)
            self.addCleanup(server.shutdown)
            profiles = root / "AIDrive/profiles"
            profiles.mkdir(parents=True)
            (profiles / "codex-routing.json").write_text(json.dumps({"api_url": f"http://127.0.0.1:{server.server_port}", "workspace_id": workspace, "workspace_name": "Synthetic routing lab", "scope": "files:write", "token": "synthetic-no-real-credential"}))
            environment = dict(os.environ, APPDATA=str(root), LOCALAPPDATA=str(root), USERPROFILE=str(root), HOME=str(root), XDG_CONFIG_HOME=str(root))
            first, second = str(uuid.uuid4()), str(uuid.uuid4())
            ids = []
            for run in range(2):
                process = subprocess.Popen([str(EXE), "serve", "--profile", "codex-routing"], cwd=root, env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
                lines = queue.Queue()

                def read_lines(stream=process.stdout, target=lines):
                    for line in stream:
                        target.put(json.loads(line))

                threading.Thread(target=read_lines, daemon=True).start()

                def rpc(method, params, request_id):
                    process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}) + "\n")
                    process.stdin.flush()
                    while True:
                        reply = lines.get(timeout=15)
                        if reply.get("id") == request_id:
                            return reply

                try:
                    rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "synthetic-routing-test", "version": "1"}}, 0)
                    process.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
                    process.stdin.flush()
                    results = []
                    for index, session in enumerate([first, second, first], 1):
                        reply = rpc("tools/call", {"name": "get_workspace", "arguments": {}, "_meta": {"threadId": session}}, index)
                        value = json.loads(reply["result"]["content"][0]["text"])
                        self.assertEqual(value["chat_identity_source"], "native_metadata")
                        results.append(value["chat_id"])
                    self.assertEqual(results[0], results[2])
                    self.assertNotEqual(results[0], results[1])
                    ids.append(results[0])
                    for index, session in enumerate([first, second], 10):
                        params = {"name": "send_message", "arguments": {"to": "synthetic-recipient", "to_chat": str(uuid.uuid4()), "body": "Synthetic routing test"}, "_meta": {"threadId": session}}
                        process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": index, "method": "tools/call", "params": params}) + "\n")
                    process.stdin.flush()
                    pending = {10, 11}
                    while pending:
                        reply = lines.get(timeout=15)
                        if reply.get("id") in pending:
                            self.assertFalse(reply["result"].get("isError", False))
                            pending.remove(reply["id"])
                    sends = [body for method, path, body in calls if method == "POST" and path.endswith("/messages")]
                    self.assertEqual({body["from_chat"] for body in sends}, {sessions[first], sessions[second]})
                    reply = rpc("tools/call", {"name": "get_messages", "arguments": {}, "_meta": {"threadId": second}}, 12)
                    self.assertFalse(reply["result"].get("isError", False))
                    claims = [path for method, path, _ in calls if method == "POST" and path.endswith("/claim")]
                    self.assertEqual(claims[-1], f"/agent/workspaces/{workspace}/chats/{sessions[second]}/claim")
                    before = len(calls)
                    reply = rpc("tools/call", {"name": "get_messages", "arguments": {}, "_meta": {"threadId": "invalid"}}, 4)
                    self.assertTrue(reply["result"]["isError"])
                    self.assertEqual(len(calls), before)
                    failing[0] = True
                    reply = rpc("tools/call", {"name": "get_messages", "arguments": {}, "_meta": {"threadId": first}}, 5)
                    self.assertTrue(reply["result"]["isError"])
                    self.assertTrue(calls[-1][1].endswith("/chats"))
                    failing[0] = False
                finally:
                    process.stdin.close()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                    process.stdout.close()
            self.assertEqual(ids[0], ids[1])
            self.assertEqual(set(sessions), {first, second})


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True, type=pathlib.Path)
    options, remaining = parser.parse_known_args()
    EXE = options.exe.resolve(strict=True)
    unittest.main(argv=[__file__, *remaining])
