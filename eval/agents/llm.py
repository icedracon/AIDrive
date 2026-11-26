"""Chat backends: local Ollama (native /api/chat, for num_ctx) and the local FreeLLMAPI router (/v1)."""
import json
import os
import subprocess
import time
import urllib.error
import urllib.request

OLLAMA = "http://127.0.0.1:11434"
FREELLM = "http://127.0.0.1:3001/v1"
EXTERNAL = {"ollama": False, "freellm": True}
_cached = {}


def _freellm_key():
    if "k" not in _cached:
        k = os.environ.get("FREELLMAPI_KEY", "")
        if not k:
            k = subprocess.run(
                ["powershell", "-NoProfile", "-Command",
                 "[Environment]::GetEnvironmentVariable('FREELLMAPI_KEY','User')"],
                capture_output=True, text=True, timeout=30).stdout.strip()
        if not k:
            raise RuntimeError("FREELLMAPI_KEY is not set")
        _cached["k"] = k
    return _cached["k"]


def _scrub(text):
    k = _cached.get("k")
    return text.replace(k, "<redacted>") if k else text


def _post(url, body, headers, timeout):
    req = urllib.request.Request(url, data=json.dumps(body).encode("utf-8"),
                                 headers={"Content-Type": "application/json", **headers})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read().decode("utf-8")), r.headers.get("X-Routed-Via", "")
    except urllib.error.HTTPError as e:
        raise RuntimeError(_scrub(f"HTTP {e.code}: {e.read().decode('utf-8', 'replace')[:400]}")) from None
    except Exception as e:
        raise RuntimeError(_scrub(f"{type(e).__name__}: {e}")) from None


def _args(a):
    if isinstance(a, dict):
        return a
    try:
        v = json.loads(a or "{}")
        return v if isinstance(v, dict) else {}
    except ValueError:
        return {}


def _to_ollama(messages):
    out = []
    for m in messages:
        m = dict(m)
        if m.get("tool_calls"):
            m["tool_calls"] = [{"function": {"name": c["function"]["name"],
                                             "arguments": _args(c["function"]["arguments"])}}
                               for c in m["tool_calls"]]
        if m["role"] == "tool":
            m = {"role": "tool", "content": m["content"], "tool_name": m.get("name", "")}
        if m.get("content") is None:
            m["content"] = ""
        out.append(m)
    return out


def chat(backend, model, messages, tools=None, timeout=600):
    """Returns (openai_style_message, seconds, served_by)."""
    t = time.monotonic()
    if backend == "ollama":
        body = {"model": model, "messages": _to_ollama(messages), "stream": False, "think": False,
                "keep_alive": "30m", "options": {"num_ctx": int(os.environ.get("OLLAMA_NUM_CTX", "16384")), "temperature": 0.2}}
        if tools:
            body["tools"] = tools
        data, _ = _post(OLLAMA + "/api/chat", body, {}, timeout)
        m = data["message"]
        calls = [{"id": f"call_{i}_{int(t * 1000) % 100000}", "type": "function",
                  "function": {"name": c["function"]["name"],
                               "arguments": json.dumps(c["function"].get("arguments", {}), ensure_ascii=False)}}
                 for i, c in enumerate(m.get("tool_calls") or [])]
        msg = {"role": "assistant", "content": m.get("content", "")}
        if calls:
            msg["tool_calls"] = calls
        return msg, time.monotonic() - t, model
    body = {"model": model, "messages": messages, "temperature": 0.2}
    if tools:
        body["tools"] = tools
    last = None
    for attempt in range(4):
        try:
            data, routed = _post(FREELLM + "/chat/completions", body,
                                 {"Authorization": "Bearer " + _freellm_key()}, timeout)
            m = data["choices"][0]["message"]
            msg = {"role": "assistant", "content": m.get("content") or ""}
            if m.get("tool_calls"):
                msg["tool_calls"] = [{"id": c.get("id") or f"call_{i}", "type": "function",
                                      "function": {"name": c["function"]["name"],
                                                   "arguments": c["function"].get("arguments") or "{}"}}
                                     for i, c in enumerate(m["tool_calls"])]
            return msg, time.monotonic() - t, routed or data.get("model", model)
        except (RuntimeError, KeyError, IndexError, TypeError) as e:
            last = e
            time.sleep(5 * (attempt + 1))
    raise RuntimeError(f"freellm failed after retries: {last}")
