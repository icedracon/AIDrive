"use client";

import { useEffect, useState } from "react";
import { ClampText, ShortList, utc } from "./ui";

type Source = { file_id: string; name: string; version_id: string; current_version_id: string | null; stale: boolean };
type Handoff = {
  id: string;
  status: "proposed" | "approved" | "rejected" | "superseded";
  summary: string;
  decisions: string[];
  next_step: string;
  actor_id: string;
  agent_id: string | null;
  created_at: string;
  decided_at: string | null;
  decision_note: string | null;
  stale: boolean;
  auto_accepted: boolean;
  owner_checked: boolean;
  undone_at: string | null;
  review_reasons: string[];
  sources: Source[];
};

const decisionErrors: Record<string, string> = {
  stale_sources: "A file this handoff relied on has changed since. It cannot be approved; ask the assistant to re-read and record a new handoff.",
  already_decided: "This handoff was already decided. Refresh to see its status.",
  owner_required: "Only a space owner can approve or reject handoffs.",
};

const reasonNames: Record<string, string> = {
  strict_review: "strict review is on",
  keystone: "a project rule",
  correction: "it replaces approved knowledge",
  conflict: "it conflicts with approved knowledge",
  stale_evidence: "a cited file changed since",
  no_evidence: "no evidence",
  stale_sources: "a cited file changed since",
  no_sources: "no file versions cited",
  exception_cleared: "it waited as an exception (or before review by exception) and nothing blocks it now",
};

function statusLabel(h: Handoff) {
  const auto = h.auto_accepted && !h.owner_checked;
  if (h.undone_at) return "Undone by you";
  if (h.status === "approved") return `${auto ? "Auto-accepted, not checked by you" : "Approved"} · ${h.stale ? "sources changed since" : "current"}`;
  if (h.status === "superseded" && auto) return "Auto-accepted · replaced by a newer handoff";
  if (h.status === "proposed") return "Proposed · needs your decision";
  return h.status === "rejected" ? "Rejected" : "Superseded by a newer approval";
}

export default function HandoffsPanel({ workspace }: { workspace: { id: string; name: string; role: string } }) {
  const [items, setItems] = useState<Handoff[] | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [busy, setBusy] = useState<{ id: string; decision: "approved" | "rejected" } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [shown, setShown] = useState(10);
  const endpoint = `/api/workspaces/${workspace.id}/handoffs`;
  const owner = workspace.role === "owner";

  useEffect(() => {
    const controller = new AbortController();
    fetch(endpoint, { cache: "no-store", signal: controller.signal })
      .then(async (response) => {
        const body = await response.json().catch(() => null);
        if (controller.signal.aborted) return;
        if (response.ok && Array.isArray(body?.handoffs)) { setItems(body.handoffs); setLoadError(false); }
        else setLoadError(true);
      })
      .catch(() => { if (!controller.signal.aborted) setLoadError(true); });
    return () => controller.abort();
  }, [endpoint, reload]);

  useEffect(() => {
    const refresh = () => setReload((n) => n + 1);
    window.addEventListener("aidrive:review-changed", refresh);
    return () => window.removeEventListener("aidrive:review-changed", refresh);
  }, []);

  async function decide(id: string, decision: "approved" | "rejected") {
    setBusy({ id, decision });
    setError(null);
    try {
      const response = await fetch(`${endpoint}/${id}/decision`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ decision }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) setError(decisionErrors[body?.error] ?? "The decision was not confirmed. Refresh before retrying.");
    } catch {
      setError("The decision was not confirmed. Refresh before retrying.");
    } finally {
      setBusy(null);
      setReload((n) => n + 1);
    }
  }

  return (
    <section className="agent-access handoffs" aria-label={`Handoffs for ${workspace.name}`}>
      <div className="panel-head">
        <h3>Handoffs {items && items.length > 0 && <span className="count-pill muted">{items.length >= 50 ? "latest 50" : items.length}</span>}</h3>
        <button className="secondary-button" type="button" onClick={() => { setError(null); setReload((n) => n + 1); }}>Refresh</button>
      </div>
      <p className="panel-intro">What each assistant did, the decisions it proposes and what comes next. In a space that reviews by exception a handoff whose cited files are unchanged becomes current at once (keep or undo it under Review); others wait for you.</p>
      {error && <p className="auth-error" role="alert">{error}</p>}
      {loadError ? <div className="panel-error" role="alert"><p>Handoffs could not be loaded.</p><button className="secondary-button" type="button" onClick={() => { setLoadError(false); setReload((n) => n + 1); }}>Try again</button></div>
        : items === null ? <p aria-busy="true" className="panel-loading">Loading handoffs…</p>
        : items.length === 0 ? <p className="empty-line">No handoffs yet. An assistant records one with the <code>record_handoff</code> tool when it finishes a piece of work.</p>
        : (
          <>
          <ul className="handoff-list">{items.slice(0, shown).map((h) => (
            <li key={h.id} className={`handoff ${h.status}${h.stale ? " stale" : ""}`}>
              <div className="handoff-head">
                <strong>{statusLabel(h)}</strong>
                <small>{h.agent_id ? `Assistant ${h.agent_id.slice(0, 8)}` : "Person"} · {utc(h.created_at)}</small>
              </div>
              <ClampText text={h.summary} />
              {h.decisions.length > 0 && <><p className="field-label">Proposed decisions</p><ShortList items={h.decisions} /></>}
              <p className="field-label">Next step</p>
              <ClampText text={h.next_step} lines={2} />
              {h.sources.length > 0 && <p className="handoff-sources"><small>Relied on</small> {h.sources.map((s) => (
                <span key={s.file_id} className={s.stale ? "source stale" : "source"}>{s.name}{s.stale ? " (changed since)" : ""}</span>
              ))}</p>}
              {h.status === "proposed" && h.review_reasons.length > 0 && <p className="reason-line"><small>Waiting because</small> {h.review_reasons.map((r) => reasonNames[r] ?? r).join("; ")}</p>}
              {h.decision_note && <p className="reason-line"><small>{h.auto_accepted && !h.undone_at ? "Note" : "Owner note"}</small> {h.decision_note}</p>}
              {owner && h.status === "proposed" && (
                <div className="auth-actions">
                  <button className="primary-button" type="button" disabled={busy !== null || h.stale} title={h.stale ? "A cited file changed; this cannot be approved" : undefined} onClick={() => decide(h.id, "approved")}>{busy?.id === h.id && busy.decision === "approved" ? "Approving…" : "Approve"}<span className="sr-only"> handoff from {h.created_at}</span></button>
                  <button className="secondary-button" type="button" disabled={busy !== null} onClick={() => decide(h.id, "rejected")}>{busy?.id === h.id && busy.decision === "rejected" ? "Rejecting…" : "Reject"}<span className="sr-only"> handoff from {h.created_at}</span></button>
                </div>
              )}
            </li>
          ))}</ul>
          {items.length > shown && <div className="list-more"><small>Showing {shown} of {items.length}</small><button className="secondary-button" type="button" onClick={() => setShown((n) => n + 20)}>Show {Math.min(20, items.length - shown)} more</button></div>}
          </>
        )}
    </section>
  );
}
