"""Held-out set 2: one T4 run per set, pooled, scored against the pass rule frozen in frozen.json.

Usage (after eval/heldout2/lexical.sh wrote the lexical dumps):
    python -I eval/heldout2/run.py            # checks the frozen config, runs T4 once per set, reports
    python -I eval/heldout2/run.py --report   # re-scores existing outputs only (no model calls)

A set whose T4 output already exists is never run again: held-out means one run.
"""

import argparse
import hashlib
import json
import subprocess
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "eval" / "heldout2"
FROZEN = json.loads((HERE / "frozen.json").read_text(encoding="utf-8"))


def text_sha(path):
    # Line endings normalised: the repo checks text out with CRLF on Windows.
    return hashlib.sha256((ROOT / path).read_bytes().replace(b"\r\n", b"\n")).hexdigest()


def check_frozen():
    bad = []
    for path, sha in FROZEN["sets"].items():
        got = hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
        if got != sha:
            bad.append(f"{path}: {got} != {sha}")
    for path, sha in FROZEN["code"].items():
        if text_sha(path) != sha:
            bad.append(f"{path}: changed since the freeze")
    with urllib.request.urlopen("http://127.0.0.1:11434/api/tags", timeout=10) as r:
        tags = {m["name"]: m["digest"] for m in json.load(r)["models"]}
    for model, digest in FROZEN["models"].items():
        if (tags.get(model) or tags.get(f"{model}:latest")) != digest:
            bad.append(f"{model}: digest differs or model missing")
    if bad:
        sys.exit("frozen configuration mismatch:\n" + "\n".join(bad))


def pool(rows_by_set, setting):
    """Pool one setting's raw sums across sets."""
    tot = {"total": 0.0, "n": 0, "abst_ok": 0, "abst_n": 0, "abst_hits": 0, "flags_ok": 0, "flags_n": 0,
           "leaks": 0, "stale_current": 0, "temporal_wrong": 0}
    lang, cat, misses = {}, {}, []
    for name, rows in rows_by_set.items():
        r = next(r for r in rows if r["setting"] == setting)
        for k in tot:
            tot[k] += r["counts"][k]
        for src, dst in ((r["lang_sum"], lang), (r["cat_sum"], cat)):
            for k, (s, n) in src.items():
                d = dst.setdefault(k, [0.0, 0])
                d[0] += s
                d[1] += n
        misses += [f"{name}:{m}" for m in r["misses"]]
    return {"setting": setting, "recall": tot["total"] / tot["n"],
            "lang": {k: s / n for k, (s, n) in sorted(lang.items())},
            "cat": {k: s / n for k, (s, n) in sorted(cat.items())},
            "n": {"answerable": tot["n"], **{f"lang_{k}": n for k, (_, n) in lang.items()}},
            "flags": f"{tot['flags_ok']}/{tot['flags_n']}",
            "abstention": f"{tot['abst_ok']}/{tot['abst_n']}", "abst_rate": tot["abst_ok"] / tot["abst_n"],
            "abst_hits": tot["abst_hits"],
            "violations": tot["leaks"] + tot["stale_current"] + tot["temporal_wrong"], "misses": misses}


def p95(xs):
    xs = sorted(xs)
    return xs[max(int(0.95 * len(xs)) - 1, 0)]


def decide(r, judge_ms):
    rule = FROZEN["pass_rule"]
    checks = [(f"recall@10 {r['recall']:.2f} >= {rule['recall']}", r["recall"] >= rule["recall"])]
    checks += [(f"{k} {v:.2f} >= {rule['per_language']}", v >= rule["per_language"]) for k, v in r["lang"].items()]
    checks += [(f"{k} {v:.2f} >= {rule['per_category']}", v >= rule["per_category"]) for k, v in r["cat"].items()]
    checks.append((f"abstention {r['abstention']} ({r['abst_rate']:.0%}) >= {rule['abstention']:.0%}", r["abst_rate"] >= rule["abstention"]))
    checks.append((f"violations {r['violations']} == 0", r["violations"] == 0))
    checks.append((f"judge p95 {p95(judge_ms):.0f} ms <= {rule['judge_p95_ms']}", p95(judge_ms) <= rule["judge_p95_ms"]))
    return checks


def main(report_only):
    names = [Path(p).stem for p in FROZEN["sets"]]
    if not report_only:
        check_frozen()
        for name in names:
            out = HERE / f"t4-{name}.json"
            if out.exists():
                print(f"{name}: T4 output exists, not running again")
                continue
            subprocess.run([sys.executable, "-I", str(ROOT / "eval/t4/judge_eval.py"), "--set", str(HERE / f"{name}.json"),
                            "--dump", str(HERE / f"lexical-{name}.json"), "--out", str(out)], check=True)
    rows_by_set, judge_ms, lat = {}, [], {}
    for name in names:
        res = json.loads((HERE / f"t4-{name}.json").read_text(encoding="utf-8"))
        rows_by_set[name] = res["rows"]
        judge_ms += [v["ms"] for v in res["verdicts"].values() if v["ms"] > 0]
        lat[name] = res["latency"]
    settings = [r["setting"] for r in next(iter(rows_by_set.values()))]
    pooled = [pool(rows_by_set, s) for s in settings]
    j1 = next(r for r in pooled if r["setting"].startswith("J1"))
    checks = decide(j1, judge_ms)
    verdict = "PASS" if all(ok for _, ok in checks) else "FAIL"
    out = {"frozen": FROZEN["sets"], "pooled": pooled, "latency_per_set": lat,
           "judge_ms": {"calls": len(judge_ms), "median": sorted(judge_ms)[len(judge_ms) // 2], "p95": p95(judge_ms), "max": max(judge_ms)},
           "per_set": {n: [{k: r[k] for k in ("setting", "recall", "langs", "flags", "abstention", "abst_hits", "violations")}
                           for r in rows] for n, rows in rows_by_set.items()},
           "pass_rule": [{"check": c, "ok": ok} for c, ok in checks], "verdict": verdict}
    (HERE / "results.json").write_text(json.dumps(out, ensure_ascii=False, indent=2), encoding="utf-8")
    langs = sorted({k for r in pooled for k in r["lang"]})
    cats = sorted({k for r in pooled for k in r["cat"]})
    print("| Setting | recall@10 | " + " | ".join(langs) + " | " + " | ".join(cats) + " | flags | abstention (hits) | violations |")
    print("|---" * (5 + len(langs) + len(cats)) + "|")
    for r in pooled:
        print(f"| {r['setting']} | {r['recall']:.2f} | " + " | ".join(f"{r['lang'][k]:.2f}" for k in langs) + " | "
              + " | ".join(f"{r['cat'][k]:.2f}" for k in cats) + f" | {r['flags']} | {r['abstention']} ({r['abst_hits']}) | {r['violations']} |")
    print(json.dumps(out["judge_ms"]))
    for c, ok in checks:
        print("ok  " if ok else "FAIL", c)
    print("VERDICT", verdict)
    print("J1 misses:", " ".join(j1["misses"]))


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--report", action="store_true")
    main(ap.parse_args().report)
