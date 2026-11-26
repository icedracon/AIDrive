"""Agent loop: an LLM gets the AIDrive MCP tools, the harness executes its calls over stdio."""
import json
import os
import re
import time

from llm import EXTERNAL, chat
from mcp_client import MCPClient

USER = os.environ.get("USERNAME", "")


def redact(text):
    if USER and len(USER) > 2:
        text = re.sub(re.escape(USER), "tester", text, flags=re.I)
    return text


SYSTEM = """You are {label}, an AI coding assistant working on a small project together with other AI assistants and a human owner.
The project's shared state lives in an AIDrive space that you reach through the provided tools. The tool server says:
---
{instructions}
---
Work only through the tools. Be concise. When the task is finished, reply with a short final answer and no tool call.{extra}"""


def run_agent(label, backend, model, profile, cwd, task, log, max_steps=14, allow=None,
              system_extra="", client=None, on_step=None):
    own = client is None
    c = client or MCPClient(profile, client_name=f"eval-{label}", cwd=cwd)
    clean = redact if EXTERNAL[backend] else (lambda s: s)
    tools = c.openai_tools(allow)
    messages = [
        {"role": "system", "content": clean(SYSTEM.format(
            label=label, instructions=c.instructions,
            extra=("\n" + system_extra) if system_extra else ""))},
        {"role": "user", "content": task},
    ]
    res = {"label": label, "backend": backend, "model": model, "calls": [], "llm_seconds": [],
           "final": "", "error": None, "steps": 0, "served_by": set()}

    def emit(kind, **kw):
        log.write(json.dumps({"t": round(time.time(), 2), "agent": label, "kind": kind, **kw},
                             ensure_ascii=False) + "\n")
        log.flush()

    emit("start", backend=backend, model=model, task=task)
    try:
        for step in range(max_steps):
            res["steps"] = step + 1
            msg, secs, served = chat(backend, model, messages, tools)
            res["llm_seconds"].append(round(secs, 2))
            res["served_by"].add(served)
            emit("llm", seconds=round(secs, 2), served_by=served, content=(msg.get("content") or "")[:2000],
                 tool_calls=[(x["function"]["name"], x["function"]["arguments"][:1500])
                             for x in msg.get("tool_calls", [])])
            messages.append(msg)
            if not msg.get("tool_calls"):
                res["final"] = msg.get("content") or ""
                break
            for call in msg["tool_calls"]:
                name = call["function"]["name"]
                raw = call["function"]["arguments"]
                try:
                    args = json.loads(raw) if raw else {}
                    if not isinstance(args, dict):
                        raise ValueError("arguments must be an object")
                except ValueError as e:
                    text, err, secs = f"invalid JSON arguments: {e}", True, 0.0
                    args = raw
                else:
                    text, err, secs = c.call(name, args)
                res["calls"].append({"tool": name, "args": args, "is_error": err,
                                     "seconds": round(secs, 3), "result": text[:4000]})
                emit("tool", tool=name, args=args, is_error=err, seconds=round(secs, 3), result=text[:3000])
                messages.append({"role": "tool", "tool_call_id": call["id"], "name": name,
                                 "content": clean(text[:8000])})
                if on_step:
                    on_step(res)
        else:
            res["error"] = "max_steps reached"
    except Exception as e:
        res["error"] = f"{type(e).__name__}: {e}"
        emit("error", error=res["error"])
    finally:
        if own:
            c.close()
    res["served_by"] = sorted(res["served_by"])
    emit("end", final=res["final"][:2000], error=res["error"])
    return res
