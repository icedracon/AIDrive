"""T3 embeddings measurement on eval/dev (offline, no production code).

Inputs: eval/dev/tutorbook.json and eval/t3/lexical-dev.json, written by the
real harness (AIDRIVE_EVAL_DUMP=... scripts/api-check.sh): per case the ranked
lexical sources, and the pools the assistant may see (visible sessions,
current and as_of knowledge with stale/disputed flags). Vector search runs
only over those pools, so permission and validity filtering is the API's,
exactly as an RLS-scoped pgvector query would be.

Usage: python -I eval/t3/embed_eval.py [--set F] [--dump F] [--out F] [model ...]
(local Ollama at 127.0.0.1:11434). Defaults: the dev set, eval/t3/lexical-dev.json,
eval/t3/results-dev.json. Prints a markdown table.
"""

import argparse
import json
import math
import random
import statistics
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OLLAMA = "http://127.0.0.1:11434/api/embed"
K = 10
RRF_K = 60
QWEN_INSTRUCT = "Instruct: Given a question about a software project, retrieve the notes or conversation turns that answer it\nQuery: "
DEV_TARGETS = ["C39", "C50", "C11", "C51"]
NEVER, SUPERSEDED, TARGETS = set(), set(), []


def derive(fixture):
    """Same rule as eval_tests.rs: what the owner's assistant must never see, and superseded items."""
    steps = fixture["steps"]
    deleted = {s["key"] for s in steps if s["op"] == "delete_session"}
    for s in steps:
        if s["op"] == "session" and (s["actor"] != "agent" or s["key"] in deleted):
            NEVER.update(f"{s['key']}#{i}" for i in range(len(s["events"])))
        if s["op"] == "propose" and s.get("decide") != "approved":
            NEVER.add(s["key"])
        if s.get("supersedes"):
            SUPERSEDED.add(s["supersedes"])


def embed(model, texts):
    body = json.dumps({"model": model, "input": texts, "truncate": True}).encode()
    req = urllib.request.Request(OLLAMA, body, {"Content-Type": "application/json"})
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=600) as r:
        out = json.load(r)
    ms = (time.perf_counter() - t0) * 1000
    vecs = []
    for v in out["embeddings"]:
        n = math.sqrt(sum(x * x for x in v)) or 1.0
        vecs.append([x / n for x in v])
    return vecs, ms


def cos(a, b):
    return sum(x * y for x, y in zip(a, b))


def query_text(model, q):
    return QWEN_INSTRUCT + q if model.startswith("qwen3") else q


def rrf(*lists):
    score = {}
    for lst in lists:
        for rank, key in enumerate(lst):
            score[key] = score.get(key, 0.0) + 1.0 / (RRF_K + rank + 1)
    return [k for k, _ in sorted(score.items(), key=lambda kv: -kv[1])]


def evaluate(cases, results, pools):
    """Mirror of eval_tests.rs scoring. results[id] = (sessions, memory)."""
    flags = {p: {i["key"]: i for i in items} for p, items in pools["knowledge_pools"].items()}
    m = {"lang": {}, "cat": {}, "total": 0.0, "n": 0, "abst_ok": 0, "abst_n": 0, "abst_hits": 0,
         "flags_ok": 0, "flags_n": 0, "leaks": 0, "stale_current": 0, "temporal_wrong": 0, "misses": [], "per_case": {}}
    for c in cases:
        sess, mem = results[c["id"]]
        got = (sess[:K] + mem[:K])[: 2 * K] if c["tool"] == "both" else (sess[:K] if c["tool"] == "sessions" else mem[:K])
        forbid = c.get("forbid", [])
        if not c.get("as_of"):
            m["stale_current"] += sum(g in SUPERSEDED for g in mem[:K])
            m["leaks"] += sum(g in NEVER or (g in forbid and g not in SUPERSEDED) for g in got)
        else:
            m["leaks"] += sum(g in NEVER for g in got)
            m["temporal_wrong"] += sum(g in forbid for g in got)
        expect = c["expect"]
        if c["category"] == "abstention":
            m["abst_n"] += 1
            m["abst_hits"] += len(got)
            m["abst_ok"] += not got
            m["per_case"][c["id"]] = "ok" if not got else f"{len(got)} hits"
            if got:
                m["misses"].append(f"{c['id']}(abstain:{len(got)})")
            continue
        if not expect:
            continue
        found = sum(e in got for e in expect)
        r = found / len(expect)
        m["per_case"][c["id"]] = f"{found}/{len(expect)}"
        if c.get("flag"):
            m["flags_n"] += 1
            pool = flags[c.get("as_of") or "current"]
            m["flags_ok"] += all(e in got and pool.get(e, {}).get(c["flag"]) for e in expect)
        m["n"] += 1
        m["total"] += r
        for d, k in ((m["lang"], c["lang"]), (m["cat"], c["category"])):
            s = d.setdefault(k, [0.0, 0])
            s[0] += r
            s[1] += 1
        if r < 1:
            m["misses"].append(f"{c['id']}({found}/{len(expect)} got {','.join(got[:3])})")
    return m


def summary(name, m):
    cat = {k: v[0] / v[1] for k, v in m["cat"].items()}
    lang = {k: v[0] / v[1] for k, v in m["lang"].items()}
    return {"setting": name, "recall": m["total"] / m["n"], "en": lang.get("en"), "hy": lang.get("hy"), "langs": lang,
            "session": cat["session-recall"], "knowledge": cat["knowledge-recall"], "changed": cat["changed-fact"],
            "temporal": cat["temporal"], "flags": f"{m['flags_ok']}/{m['flags_n']}",
            "abstention": f"{m['abst_ok']}/{m['abst_n']}", "abst_hits": m["abst_hits"],
            "violations": m["leaks"] + m["stale_current"] + m["temporal_wrong"],
            "semantic": cat.get("semantic"),
            "targets": {c: m["per_case"].get(c) for c in TARGETS}, "misses": m["misses"],
            # Raw sums so several sets can be pooled (eval/heldout2/run.py).
            "per_case": m["per_case"], "lang_sum": m["lang"], "cat_sum": m["cat"],
            "counts": {k: m[k] for k in ("total", "n", "abst_ok", "abst_n", "abst_hits", "flags_ok", "flags_n",
                                         "leaks", "stale_current", "temporal_wrong")}}


def f2(x):
    return "-" if x is None else f"{x:.2f}"


def check_frozen(frozen, set_path):
    """Refuse to run unless the set file and the pulled models match what was locked."""
    import hashlib
    sha = hashlib.sha256(Path(set_path).read_bytes()).hexdigest()
    if sha != frozen["set_sha256"]:
        sys.exit(f"set sha256 {sha} != frozen {frozen['set_sha256']}")
    with urllib.request.urlopen(OLLAMA.replace("/api/embed", "/api/tags"), timeout=10) as r:
        tags = {m["name"]: m["digest"] for m in json.load(r)["models"]}
    for model, cfg in frozen["models"].items():
        digest = tags.get(model) or tags.get(f"{model}:latest", "")
        if digest != cfg["digest"]:
            sys.exit(f"{model} digest {digest} != frozen {cfg['digest']}")
    if frozen["rrf_k"] != RRF_K or frozen["k"] != K or frozen["qwen_instruct"] != QWEN_INSTRUCT:
        sys.exit("retrieval configuration differs from the frozen one")


def main(models, set_path, dump_path, out_path, frozen=None):
    fixture = json.loads(Path(set_path).read_text(encoding="utf-8"))
    dump = json.loads(Path(dump_path).read_text(encoding="utf-8"))
    derive(fixture)
    if Path(set_path).resolve() == (ROOT / "eval/dev/tutorbook.json").resolve():
        TARGETS.extend(DEV_TARGETS)
    cases = fixture["cases"]
    lex = {c["id"]: (c["sessions"], c["memory"]) for c in dump["cases"]}
    visible = set(dump["visible_sessions"])
    events = {}
    statements = {}
    for s in fixture["steps"]:
        if s["op"] == "session" and s["key"] in visible:
            for i, (_, text) in enumerate(s["events"]):
                events[f"{s['key']}#{i}"] = text
        if s["op"] == "propose":
            statements[s["key"]] = f"{s['topic']} {s['statement']}"
    pools = {p: [i["key"] for i in items] for p, items in dump["knowledge_pools"].items()}
    session_pool = len(events)

    out = {"pools": {"session_events": session_pool, **{p: len(v) for p, v in pools.items()}}, "rows": [], "latency": {}}
    rows = [summary("lexical planner (M5b)", evaluate(cases, lex, dump))]
    rnd = random.Random(7)
    rand_res = {}
    for c in cases:
        mp = pools[c.get("as_of") or "current"]
        rand_res[c["id"]] = (rnd.sample(sorted(events), K), rnd.sample(mp, min(K, len(mp))))
    rows.append(summary("random-k (seed 7, sampled)", evaluate(cases, rand_res, dump)))
    lex_ms = [c["sessions_ms"] + c["memory_ms"] for c in dump["cases"]]
    out["latency"]["lexical"] = {"median_ms": statistics.median(lex_ms), "max_ms": max(lex_ms)}

    for model in models:
        _, load_ms = embed(model, ["warm-up"])
        keys_e = sorted(events)
        keys_k = sorted(statements)
        ve, ms_e = embed(model, [events[k] for k in keys_e])
        vk, ms_k = embed(model, [statements[k] for k in keys_k])
        evec = dict(zip(keys_e, ve))
        kvec = dict(zip(keys_k, vk))
        qms, sims = [], {}
        for c in cases:
            (qv,), ms = embed(model, [query_text(model, c["query"])])
            qms.append(ms)
            se = sorted(((cos(qv, evec[k]), k) for k in keys_e), reverse=True) if c["tool"] in ("sessions", "both") else []
            mp = pools[c.get("as_of") or "current"]
            sm = sorted(((cos(qv, kvec[k]), k) for k in mp), reverse=True) if c["tool"] in ("memory", "both") else []
            sims[c["id"]] = (se, sm)
        # Threshold calibrated on the dev set: the highest similarity any
        # abstention question reaches. Dev-tuned by construction; reported as such.
        abst = [c["id"] for c in cases if c["category"] == "abstention"]
        tau = frozen["models"][model]["tau"] if frozen else max(s for cid in abst for lst in sims[cid] for s, _ in lst[:1])
        tau_note = "frozen from dev" if frozen else "dev-tuned"
        dims = len(ve[0])
        out["latency"][model] = {"query_median_ms": statistics.median(qms), "query_p95_ms": sorted(qms)[int(0.95 * len(qms)) - 1],
                                 "cold_load_ms": load_ms, "index_events_ms": ms_e, "index_knowledge_ms": ms_k, "dims": dims,
                                 "index_bytes_float32": dims * 4 * (len(ve) + len(vk)), "tau_dev": tau}

        def vec(t):
            return {cid: ([k for s, k in se if s > t][:K], [k for s, k in sm if s > t][:K]) for cid, (se, sm) in sims.items()}

        v0, vt = vec(-1.0), vec(tau)
        hyb0 = {cid: (rrf(lex[cid][0], v0[cid][0])[:K], rrf(lex[cid][1], v0[cid][1])[:K]) for cid in lex}
        hybt = {cid: (rrf(lex[cid][0], vt[cid][0])[:K], rrf(lex[cid][1], vt[cid][1])[:K]) for cid in lex}
        gated = {cid: (rrf(lex[cid][0], v0[cid][0])[:K] if lex[cid][0] else [],
                       rrf(lex[cid][1], v0[cid][1])[:K] if lex[cid][1] else []) for cid in lex}
        for name, res in ((f"{model} vector", v0), (f"{model} vector, tau {tau:.3f} ({tau_note})", vt),
                          (f"{model} hybrid RRF", hyb0), (f"{model} hybrid RRF, tau {tau:.3f} ({tau_note})", hybt),
                          (f"{model} hybrid RRF, vector only where lexical matched", gated)):
            rows.append(summary(name, evaluate(cases, res, dump)))
        out[f"{model}_abstention_top_similarity"] = {cid: round(max((s for lst in sims[cid] for s, _ in lst[:1]), default=0), 4) for cid in abst}
        out[f"{model}_answerable_top_similarity_min"] = round(min(
            max((s for lst in sims[c["id"]] for s, _ in lst[:1]), default=0) for c in cases if c["expect"]), 4)

    out["rows"] = rows
    Path(out_path).write_text(json.dumps(out, ensure_ascii=False, indent=2), encoding="utf-8")
    print("| Setting | recall@10 | EN | HY | session | knowledge | changed | temporal | semantic | flags | abstention | violations | "
          + " | ".join(TARGETS) + " |")
    print("|---" * (12 + len(TARGETS)) + "|")
    for r in rows:
        sem = "-" if r["semantic"] is None else f"{r['semantic']:.2f}"
        print(f"| {r['setting']} | {r['recall']:.2f} | {f2(r['en'])} | {f2(r['hy'])} | {r['session']:.2f} | {r['knowledge']:.2f} | "
              f"{r['changed']:.2f} | {r['temporal']:.2f} | {sem} | {r['flags']} | {r['abstention']} ({r['abst_hits']} hits) | {r['violations']} | "
              + " | ".join(str(r["targets"][c]) for c in TARGETS) + " |")
    print(json.dumps({"pools": out["pools"], "latency": out["latency"]}, indent=1))
    for r in rows:
        print(r["setting"], "misses:", " ".join(r["misses"]))


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--set", default=str(ROOT / "eval/dev/tutorbook.json"))
    ap.add_argument("--dump", default=str(ROOT / "eval/t3/lexical-dev.json"))
    ap.add_argument("--out", default=str(ROOT / "eval/t3/results-dev.json"))
    ap.add_argument("--frozen", help="locked configuration (eval/heldout/frozen.json); checks set hash and model digests")
    ap.add_argument("models", nargs="*", default=["qwen3-embedding:0.6b", "bge-m3"])
    a = ap.parse_args()
    frozen = json.loads(Path(a.frozen).read_text(encoding="utf-8")) if a.frozen else None
    if frozen:
        check_frozen(frozen, a.set)
    main(a.models, a.set, a.dump, a.out, frozen)
