"""Structural check of an eval set before it is locked (ROADMAP, held-out set specification).

Usage: python -I eval/heldout/validate.py <set.json>
Checks the seeding rules the API enforces and the case quotas; prints the SHA-256 to record.
"""

import collections
import hashlib
import json
import sys
from pathlib import Path

KINDS = {"decision", "fact", "constraint", "preference", "gotcha", "attempt", "keystone"}
CATEGORIES = {"session-recall": 20, "knowledge-recall": 16, "changed-fact": 10, "temporal": 10,
              "flagged": 10, "abstention": 14, "permission": 10, "semantic": 10}


def main(path):
    raw = Path(path).read_bytes()
    data = json.loads(raw)
    errors = []
    # A set may carry its own "spec" (held-out set 2: three smaller per-language projects).
    spec = data.get("spec", {})
    quotas = spec.get("categories", CATEGORIES)
    lang_need = spec.get("languages")
    files, sessions, items, approved = {}, {}, {}, {}
    deleted, slots, hidden = set(), {}, set()
    stale_candidates = {}
    for n, s in enumerate(data["steps"]):
        op, key = s.get("op"), s.get("key")
        where = f"step {n} ({op} {key})"
        if op == "file":
            if key not in files and not s.get("name"):
                errors.append(f"{where}: first version needs a name")
            files[key] = files.get(key, 0) + 1
            for k, f in stale_candidates.items():
                if f == key:
                    approved.setdefault(k, {})["stale"] = True
        elif op == "session":
            if key in sessions:
                errors.append(f"{where}: duplicate session key")
            if s.get("actor") not in {"agent", "editor", "other"}:
                errors.append(f"{where}: actor")
            if s.get("client") not in {"codex", "claude-code"}:
                errors.append(f"{where}: client")
            ev = s.get("events", [])
            if not 1 <= len(ev) <= 60:
                errors.append(f"{where}: 1..60 events")
            for e in ev:
                if e[0] not in {"user", "assistant", "tool"} or not e[1].strip():
                    errors.append(f"{where}: bad event {e}")
            sessions[key] = s
            if s["actor"] != "agent":
                hidden.update(f"{key}#{i}" for i in range(len(ev)))
        elif op == "delete_session":
            if key not in sessions or sessions[key]["actor"] != "agent":
                errors.append(f"{where}: delete needs an earlier agent session")
            deleted.add(key)
            hidden.update(f"{key}#{i}" for i in range(len(sessions.get(key, {}).get("events", []))))
        elif op == "propose":
            if key in items:
                errors.append(f"{where}: duplicate item key")
            if s.get("kind") not in KINDS:
                errors.append(f"{where}: kind")
            if (s.get("kind") == "attempt") != (s.get("outcome") in {"failed", "partial", "worked"}):
                errors.append(f"{where}: attempt needs outcome, others none")
            if not 1 <= len(s.get("statement", "")) <= 2000:
                errors.append(f"{where}: statement length")
            for ref in s.get("evidence", []):
                if ref.startswith("file:"):
                    if ref[5:] not in files:
                        errors.append(f"{where}: evidence {ref} before the file exists")
                else:
                    sk, _, o = ref.partition("#")
                    if sk not in sessions or not o.isdigit() or int(o) >= len(sessions[sk]["events"]) or sk in deleted:
                        errors.append(f"{where}: evidence {ref}")
                    elif sessions[sk]["actor"] != "agent":
                        errors.append(f"{where}: evidence {ref} is not visible to the owner's assistant that proposes")
            sup = s.get("supersedes")
            if sup and (sup not in approved or approved[sup].get("superseded")):
                errors.append(f"{where}: supersedes must name a current approved item")
            slot = (s.get("kind"), (s.get("topic") or "").strip().lower())
            if s.get("decide") == "approved":
                if s["kind"] != "attempt" and slot[1]:
                    holder = slots.get(slot)
                    if holder and holder != sup:
                        errors.append(f"{where}: slot {slot} already held by {holder}; use supersedes")
                    slots[slot] = key
                if sup:
                    approved[sup]["superseded"] = True
                approved[key] = {"slot": slot, "stale": False}
                for ref in s.get("evidence", []):
                    if ref.startswith("file:"):
                        stale_candidates[key] = ref[5:]
            else:
                hidden.add(key)
                if s.get("decide") is not None:
                    errors.append(f"{where}: decide is 'approved' or null")
                if slot in slots and s["kind"] != "attempt":
                    approved[slots[slot]]["disputed"] = True
            items[key] = s
        else:
            errors.append(f"{where}: unknown op")

    visible_events = {f"{k}#{i}" for k, s in sessions.items() if k not in hidden and s["actor"] == "agent" and k not in deleted
                      for i in range(len(s["events"]))}
    current = {k for k, v in approved.items() if not v.get("superseded")}
    cats, langs, ids = collections.Counter(), collections.Counter(), set()
    for c in data["cases"]:
        cid = c.get("id")
        if cid in ids:
            errors.append(f"case {cid}: duplicate id")
        ids.add(cid)
        cats[c.get("category")] += 1
        langs[c.get("lang")] += 1
        if c.get("lang") not in (set(lang_need) if lang_need else {"en", "hy"}):
            errors.append(f"case {cid}: lang")
        if c.get("tool") not in {"sessions", "memory", "both"}:
            errors.append(f"case {cid}: tool")
        if not 1 <= len(c.get("query", "")) <= 200:
            errors.append(f"case {cid}: query length")
        exp = c.get("expect", [])
        if len(exp) > 10:
            errors.append(f"case {cid}: more than 10 expected sources")
        for e in exp:
            if c["tool"] == "sessions" and e not in visible_events:
                errors.append(f"case {cid}: expect {e} is not a visible event")
            if c["tool"] == "memory" and not c.get("as_of") and e not in current:
                errors.append(f"case {cid}: expect {e} is not current knowledge")
            if c.get("as_of") and e not in approved:
                errors.append(f"case {cid}: expect {e} unknown")
        if c.get("as_of") and c["as_of"] not in approved:
            errors.append(f"case {cid}: as_of must name an approved item")
        if c.get("category") == "abstention" and (exp or c["tool"] != "both"):
            errors.append(f"case {cid}: abstention is tool both with expect []")
        if c.get("flag"):
            for e in exp:
                if not approved.get(e, {}).get(c["flag"]):
                    errors.append(f"case {cid}: {e} will not be {c['flag']}")
        for f in c.get("forbid", []):
            if f not in hidden and f not in items and not any(f == ev for ev in visible_events):
                errors.append(f"case {cid}: forbid {f} unknown")
    for cat, need in quotas.items():
        if cats[cat] != need:
            errors.append(f"category {cat}: {cats[cat]} cases, spec {need}")
    if lang_need:
        for lang, need in lang_need.items():
            if langs[lang] != need:
                errors.append(f"language {lang}: {langs[lang]} cases, spec {need}")
    else:
        for lang in ("en", "hy"):
            if langs[lang] < 10:
                errors.append(f"language {lang}: fewer than 10 cases")
    print(f"cases {sum(cats.values())}; by category {dict(cats)}; by language {dict(langs)}")
    print(f"visible events {len(visible_events)}; current knowledge {len(current)}; hidden {len(hidden)}")
    for e in errors:
        print("ERROR", e)
    print("sha256", hashlib.sha256(raw).hexdigest())
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
