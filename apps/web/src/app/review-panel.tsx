"use client";

import { useEffect, useState } from "react";
import { ClampText, ConfirmButton, ShortList, exitDelay, utc } from "./ui";

type AutoItem = { id: string; kind: string; topic: string | null; statement: string; agent_id: string | null; decided_at: string | null; visibility: "private" | "team"; mine: boolean };
type AutoHandoff = { id: string; summary: string; status: string; agent_id: string | null; decided_at: string | null; decisions: string[] };
type Review = { mode: "exceptions" | "strict"; recent_knowledge: AutoItem[]; recent_handoffs: AutoHandoff[] };
type Totals = { owner_approved: number; auto_accepted: number; kept: number; undone: number; owner_rejected: number };
type Metrics = { totals: Totals; owner_actions_per_day: number; undo_rate: number | null };
type Busy = { key: string; action: "keep" | "undo" | "mode" } | null;

const errors: Record<string, string> = {
  not_auto_accepted: "This was already kept, undone or decided. Refresh.",
  owner_required: "Only a space owner can do this.",
};

const kindNames: Record<string, string> = { keystone: "Rule", decision: "Decision", constraint: "Constraint", fact: "Fact", preference: "Preference", gotcha: "Gotcha", attempt: "Attempt" };

const who = (agent: string | null) => (agent ? `Assistant ${agent.slice(0, 8)}` : "Person");

export default function ReviewPanel({ workspace }: { workspace: { id: string; name: string; role: string } }) {
  const [review, setReview] = useState<Review | null>(null);
  const [metrics, setMetrics] = useState<Metrics | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [retried, setRetried] = useState(false);
  const [busy, setBusy] = useState<Busy>(null);
  const [leaving, setLeaving] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const base = `/api/workspaces/${workspace.id}`;
  const owner = workspace.role === "owner";

  useEffect(() => {
    const controller = new AbortController();
    Promise.all([
      fetch(`${base}/review`, { cache: "no-store", signal: controller.signal }),
      fetch(`${base}/review/metrics?days=7`, { cache: "no-store", signal: controller.signal }),
    ])
      .then(async ([r, m]) => {
        const [rb, mb] = await Promise.all([r.json().catch(() => null), m.json().catch(() => null)]);
        if (controller.signal.aborted) return;
        if (r.ok && rb?.mode) { setReview(rb); setMetrics(m.ok ? mb : null); setLoadError(false); }
        else setLoadError(true);
      })
      .catch(() => { if (!controller.signal.aborted) setLoadError(true); });
    return () => controller.abort();
  }, [base, reload]);

  // A slow first compile or a restarting API often fails once; try again by itself.
  useEffect(() => {
    if (!loadError || retried) return;
    const timer = window.setTimeout(() => { setRetried(true); setReload((n) => n + 1); }, 2000);
    return () => window.clearTimeout(timer);
  }, [loadError, retried]);

  const refresh = () => { setError(null); setLoadError(false); setRetried(false); setReload((n) => n + 1); };

  async function post(busyNow: NonNullable<Busy>, path: string, body: unknown, failure: string) {
    setBusy(busyNow);
    setError(null);
    let ok = false;
    try {
      const response = await fetch(`${base}/${path}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const reply = await response.json().catch(() => null);
      ok = response.ok;
      if (!ok) setError(errors[reply?.error] ?? failure);
    } catch {
      setError(failure);
    }
    const finish = () => {
      setBusy(null);
      setLeaving((s) => { const next = new Set(s); next.delete(busyNow.key); return next; });
      setReload((n) => n + 1);
      window.dispatchEvent(new Event("aidrive:review-changed"));
    };
    if (ok && busyNow.action !== "mode") {
      setLeaving((s) => new Set(s).add(busyNow.key));
      window.setTimeout(finish, exitDelay());
    } else finish();
  }

  const pct = (n: number) => `${Math.round(n * 100)}%`;
  const count = review ? review.recent_knowledge.length + review.recent_handoffs.length : 0;

  function actions(kind: "knowledge" | "handoffs", id: string, label: string) {
    const here = busy?.key === id;
    return (
      <div className="auth-actions">
        <button className="secondary-button" type="button" disabled={busy !== null} onClick={() => post({ key: id, action: "keep" }, `${kind}/${id}/keep`, undefined, "Keep was not confirmed. Refresh before retrying.")}>{here && busy?.action === "keep" ? "Keeping…" : "Keep"}<span className="sr-only"> {label}</span></button>
        <ConfirmButton label="Undo" confirmLabel={kind === "knowledge" ? "Confirm undo: assistants stop using it" : "Confirm undo: its decisions stop being current"} busyLabel="Undoing…" busy={here && busy?.action === "undo"} disabled={busy !== null}
          onConfirm={() => post({ key: id, action: "undo" }, `${kind}/${id}/undo`, {}, "Undo was not confirmed. Refresh before retrying.")}><span className="sr-only"> {label}</span></ConfirmButton>
      </div>
    );
  }

  return (
    <section className="agent-access review" id="panel-review" aria-label={`Review settings and recently auto-accepted in ${workspace.name}`}>
      <div className="panel-head">
        <h3>Review</h3>
        <button className="secondary-button" type="button" onClick={refresh}>Refresh</button>
      </div>
      {error && <p className="auth-error" role="alert">{error}</p>}
      {loadError ? (
        <div className="panel-error" role="alert"><p>Review state could not be loaded{retried ? "." : ". Trying again…"}</p><button className="secondary-button" type="button" onClick={refresh}>Try again</button></div>
      ) : review === null ? <p aria-busy="true" className="panel-loading">Loading review state…</p> : (
        <>
          <p className="panel-intro">
            {review.mode === "exceptions"
              ? "Review by exception: proposals with current evidence and no conflict are accepted at once and listed here for you to keep or undo. Conflicts, changed or missing evidence, rules and corrections wait for you under Project knowledge."
              : "Strict review: every proposal and handoff waits for your decision."}
          </p>
          {owner && (
            <div className="auth-actions">
              <ConfirmButton
                label={review.mode === "exceptions" ? "Switch to strict review" : "Switch to review by exception"}
                confirmLabel={review.mode === "exceptions" ? "Confirm: everything waits for you" : "Confirm: accept clean proposals at once"}
                busyLabel="Switching…" busy={busy?.action === "mode"} disabled={busy !== null}
                onConfirm={() => post({ key: "mode", action: "mode" }, "review", { mode: review.mode === "exceptions" ? "strict" : "exceptions" }, "The review mode was not changed. Refresh before retrying.")} />
            </div>
          )}
          {metrics && (
            <p className="metric-line"><small>
              Last 7 days: {metrics.totals.auto_accepted} auto-accepted, {metrics.totals.owner_approved} approved by you, {metrics.totals.kept} kept, {metrics.totals.undone} undone
              {metrics.undo_rate !== null ? ` (undo rate ${pct(metrics.undo_rate)})` : ""} · {metrics.owner_actions_per_day.toFixed(1)} decisions by you per day.
            </small></p>
          )}
          <h4>Recently auto-accepted {count > 0 && <span className="count-pill">{count}</span>}</h4>
          {count === 0 ? <p className="empty-line">No auto-accepted items to check. {review.mode === "exceptions" ? "New proposals with current evidence will appear here." : "In strict review nothing is accepted without you."}</p> : (
            <ul className="handoff-list">
              {review.recent_handoffs.map((h) => (
                <li key={h.id} className={`inbox-item${leaving.has(h.id) ? " is-leaving" : ""}`}>
                  <div className="inbox-item-inner handoff approved">
                    <div className="handoff-head"><strong>Handoff · auto-accepted{h.status === "superseded" ? " · replaced since" : ""}</strong><small>{who(h.agent_id)} · {utc(h.decided_at)}</small></div>
                    <ClampText text={h.summary} />
                    {h.decisions.length > 0 && <ShortList items={h.decisions} />}
                    {owner && actions("handoffs", h.id, `handoff ${h.summary.slice(0, 80)}`)}
                  </div>
                </li>
              ))}
              {review.recent_knowledge.map((k) => (
                <li key={k.id} className={`inbox-item${leaving.has(k.id) ? " is-leaving" : ""}`}>
                  <div className="inbox-item-inner handoff approved">
                    <div className="handoff-head"><strong>{kindNames[k.kind] ?? k.kind}{k.topic ? ` · ${k.topic}` : ""} · auto-accepted</strong><small>{who(k.agent_id)} · {utc(k.decided_at)}</small></div>
                    <ClampText text={k.statement} />
                    {(k.visibility === "private" ? k.mine : owner) && actions("knowledge", k.id, k.statement.slice(0, 80))}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}
