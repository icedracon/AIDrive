"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { ConfirmButton, exitDelay, plural, utc } from "./ui";

type Evidence = { file_name: string | null; session_id: string | null; handoff_id: string | null; quote: string | null; stale: boolean };
type Item = {
  id: string;
  kind: string;
  topic: string | null;
  statement: string;
  outcome: string | null;
  status: string;
  agent_id: string | null;
  decision_note: string | null;
  created_at: string;
  valid_from: string | null;
  invalidation_note: string | null;
  visibility: "private" | "team";
  mine: boolean;
  conflicts: string[];
  disputed_by: string[];
  stale: boolean;
  auto_accepted: boolean;
  owner_checked: boolean;
  review_reasons: string[];
  evidence: Evidence[];
};
type Resolution = "replace" | "keep_both";
type Page = { items: Item[]; total: number | null };
type Busy = { id: string; action: "approve" | "reject" | "retire" } | null;

const PAGE = 20;

const kindNames: Record<string, string> = {
  keystone: "Rule", decision: "Decision", constraint: "Constraint", fact: "Fact", preference: "Preference", gotcha: "Gotcha", attempt: "Attempt",
};

const errors: Record<string, string> = {
  stale_evidence: "A file this proposal cites changed since it was written. Ask the assistant to re-read and propose again.",
  already_decided: "This proposal was already decided. Refresh.",
  owner_required: "Only a space owner can decide on knowledge.",
  conflict_unresolved: "This proposal conflicts with approved knowledge. Choose whether it replaces it or sits next to it.",
  not_current: "This item is no longer current. Refresh.",
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

function label(item: Item) {
  const base = kindNames[item.kind] ?? item.kind;
  const kind = item.kind === "attempt" && item.outcome ? `${base} · ${item.outcome}` : base;
  return item.visibility === "private" ? `${kind} · private` : kind;
}

function sources(item: Item) {
  return item.evidence.map((e, i) => {
    const name = e.file_name ?? (e.session_id ? "session" : e.handoff_id ? "handoff" : "source");
    return <span key={i} className={e.stale ? "source stale" : "source"} title={e.quote ?? undefined}>{name}{e.stale ? " (changed since)" : ""}</span>;
  });
}

// Proposals that compete for the same slot are decided together: the same
// approved items they conflict with, or the same kind and topic.
function slotKey(item: Item) {
  if (item.conflicts.length > 0) return `c:${[...item.conflicts].sort().join(",")}`;
  if (item.topic && item.kind !== "attempt") return `t:${item.kind}:${item.topic.toLowerCase()}`;
  return `i:${item.id}`;
}

async function fetchPage(url: string, signal?: AbortSignal): Promise<Page> {
  const response = await fetch(url, { cache: "no-store", signal });
  const body = await response.json().catch(() => null);
  if (!response.ok || !Array.isArray(body?.items)) throw new Error(body?.error ?? String(response.status));
  return { items: body.items, total: typeof body.total === "number" ? body.total : null };
}

function without<T>(record: Record<string, T>, key: string) {
  const next = { ...record };
  delete next[key];
  return next;
}

function merge(current: Item[], more: Item[]) {
  const seen = new Set(current.map((i) => i.id));
  return [...current, ...more.filter((i) => !seen.has(i.id))];
}

export default function KnowledgePanel({ workspace }: { workspace: { id: string; name: string; role: string } }) {
  const [proposed, setProposed] = useState<Page | null>(null);
  const [approved, setApproved] = useState<Page | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [retried, setRetried] = useState(false);
  const [more, setMore] = useState<"proposed" | "approved" | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [itemErrors, setItemErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<Busy>(null);
  const [leaving, setLeaving] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({});
  const [resolution, setResolution] = useState<Record<string, Resolution>>({});
  const [reload, setReload] = useState(0);
  const base = `/api/workspaces/${workspace.id}/knowledge`;
  const owner = workspace.role === "owner";

  useEffect(() => {
    const controller = new AbortController();
    Promise.all([fetchPage(`${base}?status=approved&limit=${PAGE}`, controller.signal), fetchPage(`${base}?status=proposed&limit=${PAGE}`, controller.signal)])
      .then(([a, p]) => { if (!controller.signal.aborted) { setApproved(a); setProposed(p); setLoadError(false); } })
      .catch(() => { if (!controller.signal.aborted) setLoadError(true); });
    return () => controller.abort();
  }, [base, reload]);

  useEffect(() => {
    if (!loadError || retried) return;
    const timer = window.setTimeout(() => { setRetried(true); setReload((n) => n + 1); }, 2000);
    return () => window.clearTimeout(timer);
  }, [loadError, retried]);

  useEffect(() => {
    const refresh = () => setReload((n) => n + 1);
    window.addEventListener("aidrive:review-changed", refresh);
    return () => window.removeEventListener("aidrive:review-changed", refresh);
  }, []);

  const refreshAll = () => { setLoadError(false); setRetried(false); setItemErrors({}); setReload((n) => n + 1); };

  async function showMore(which: "proposed" | "approved") {
    const page = which === "proposed" ? proposed : approved;
    if (!page) return;
    setMore(which);
    try {
      const next = await fetchPage(`${base}?status=${which}&limit=${PAGE}&offset=${page.items.length}`);
      const set = which === "proposed" ? setProposed : setApproved;
      set((current) => current && { items: merge(current.items, next.items), total: next.total ?? current.total });
    } catch {
      setItemErrors((e) => ({ ...e, [`more-${which}`]: "More items could not be loaded. Try again." }));
    } finally {
      setMore(null);
    }
  }

  const refreshApproved = useCallback(() => {
    fetchPage(`${base}?status=approved&limit=${Math.max(PAGE, Math.min(200, approved?.items.length ?? PAGE))}`)
      .then((a) => setApproved((current) => ({ items: a.items, total: a.total ?? current?.total ?? null })))
      .catch(() => undefined);
  }, [base, approved?.items.length]);

  // Collapse the decided row, then drop it locally so the reader keeps their place.
  function remove(which: "proposed" | "approved", id: string) {
    setLeaving((s) => new Set(s).add(id));
    window.setTimeout(() => {
      const set = which === "proposed" ? setProposed : setApproved;
      set((current) => current && { items: current.items.filter((i) => i.id !== id), total: current.total === null ? null : Math.max(0, current.total - 1) });
      setLeaving((s) => { const next = new Set(s); next.delete(id); return next; });
      setNotes((n) => without(n, id));
      setResolution((r) => without(r, id));
    }, exitDelay());
  }

  async function decide(item: Item, decision: "approved" | "rejected", statement?: string) {
    setBusy({ id: item.id, action: decision === "approved" ? "approve" : "reject" });
    setItemErrors((e) => without(e, item.id));
    try {
      const response = await fetch(`${base}/${item.id}/decision`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ decision, note: notes[item.id]?.trim() || undefined, statement, resolution: decision === "approved" ? resolution[item.id] : undefined }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) setItemErrors((e) => ({ ...e, [item.id]: errors[body?.error] ?? "The decision was not confirmed. Refresh before retrying." }));
      else {
        if (editing === item.id) setEditing(null);
        remove("proposed", item.id);
        if (decision === "approved" || item.conflicts.length > 0) refreshApproved();
      }
    } catch {
      setItemErrors((e) => ({ ...e, [item.id]: "The decision was not confirmed. Refresh before retrying." }));
    } finally {
      setBusy(null);
    }
  }

  async function retire(item: Item) {
    setBusy({ id: item.id, action: "retire" });
    setItemErrors((e) => without(e, item.id));
    try {
      const response = await fetch(`${base}/${item.id}/retire`, { method: "POST", headers: { "content-type": "application/json" }, body: "{}" });
      const body = await response.json().catch(() => null);
      if (!response.ok) setItemErrors((e) => ({ ...e, [item.id]: errors[body?.error] ?? "Retiring was not confirmed. Refresh before retrying." }));
      else remove("approved", item.id);
    } catch {
      setItemErrors((e) => ({ ...e, [item.id]: "Retiring was not confirmed. Refresh before retrying." }));
    } finally {
      setBusy(null);
    }
  }

  const statementOf = (id: string) => approved?.items.find((a) => a.id === id)?.statement ?? `an approved item (${id.slice(0, 8)})`;

  const groups = useMemo(() => {
    const out: { key: string; items: Item[] }[] = [];
    const index = new Map<string, number>();
    for (const item of proposed?.items ?? []) {
      const key = slotKey(item);
      if (!index.has(key)) { index.set(key, out.length); out.push({ key, items: [] }); }
      out[index.get(key)!].items.push(item);
    }
    return out;
  }, [proposed]);

  const pendingTotal = proposed ? proposed.total ?? proposed.items.length : 0;
  const approvedTotal = approved ? approved.total ?? approved.items.length : 0;
  const fewPending = (proposed?.items.length ?? 0) <= 5;
  const canDecide = (item: Item) => (item.visibility === "private" ? item.mine : owner);

  function groupTitle(group: { key: string; items: Item[] }) {
    const first = group.items[0];
    const n = plural(group.items.length, "proposal", "proposals");
    if (first.conflicts.length === 1) {
      const all = approved?.items.find((a) => a.id === first.conflicts[0])?.disputed_by.length ?? 0;
      if (all > group.items.length) return <>{plural(all, "proposal", "proposals")} challenge <q>{statementOf(first.conflicts[0])}</q> <small className="group-loaded">{group.items.length} loaded</small></>;
      return <>{n} challenge <q>{statementOf(first.conflicts[0])}</q></>;
    }
    if (first.conflicts.length > 1) return <>{n} challenge {first.conflicts.length} approved items</>;
    return <>{n} on {(kindNames[first.kind] ?? first.kind).toLowerCase()} <q>{first.topic}</q></>;
  }

  function renderItem(item: Item, inGroup: boolean) {
    const isOpen = open[item.id] ?? (fewPending && !inGroup);
    const mayDecide = canDecide(item);
    const busyHere = busy?.id === item.id;
    const needsChoice = item.conflicts.length > 0 && !resolution[item.id];
    return (
      <li key={item.id} className={`inbox-item${leaving.has(item.id) ? " is-leaving" : ""}`}>
        <div className={`inbox-item-inner handoff proposed${item.stale ? " stale" : ""}${isOpen ? " is-open" : ""}`}>
          <button className="inbox-row" type="button" aria-expanded={isOpen} onClick={() => setOpen((o) => ({ ...o, [item.id]: !isOpen }))}>
            <span className="inbox-kind">{label(item)}{item.topic && !inGroup ? ` · ${item.topic}` : ""}</span>
            <span className="inbox-statement">{item.statement}</span>
            <span className="inbox-chevron" aria-hidden="true" />
          </button>
          {isOpen && (
            <div className="inbox-body">
              <p className="inbox-meta"><small>{item.agent_id ? `Assistant ${item.agent_id.slice(0, 8)}` : "Person"} · {utc(item.created_at)}</small></p>
              {editing === item.id
                ? <label>Edited statement<textarea value={draft} maxLength={2000} onChange={(e) => setDraft(e.target.value)} rows={3} /></label>
                : <p className="inbox-full">{item.statement}</p>}
              {item.evidence.length > 0 && <p className="handoff-sources"><small>Evidence</small> {sources(item)}</p>}
              {item.review_reasons.length > 0 && <p className="reason-line"><small>Waiting because</small> {item.review_reasons.map((r) => reasonNames[r] ?? r).join("; ")}</p>}
              {item.conflicts.length > 0 && (
                <div className="knowledge-conflict" role="group" aria-label="Conflicting approved knowledge">
                  {!inGroup && <><p><strong>Conflicts with approved knowledge:</strong></p><ul>{item.conflicts.map((id) => <li key={id}>{statementOf(id)}</li>)}</ul></>}
                  {mayDecide && (
                    <>
                      <label><input type="radio" name={`resolution-${item.id}`} checked={resolution[item.id] === "replace"} onChange={() => setResolution((r) => ({ ...r, [item.id]: "replace" }))} /> Replace it (the old item becomes history)</label>
                      <label><input type="radio" name={`resolution-${item.id}`} checked={resolution[item.id] === "keep_both"} onChange={() => setResolution((r) => ({ ...r, [item.id]: "keep_both" }))} /> Keep both</label>
                    </>
                  )}
                </div>
              )}
              {itemErrors[item.id] && <p className="auth-error" role="alert">{itemErrors[item.id]}</p>}
              {mayDecide && (
                <div className="auth-actions">
                  {editing === item.id ? (
                    <>
                      <button className="primary-button" type="button" disabled={busy !== null || !draft.trim() || needsChoice} onClick={() => decide(item, "approved", draft.trim())}>{busyHere && busy?.action === "approve" ? "Approving…" : "Approve edited"}<span className="sr-only"> {item.statement}</span></button>
                      <button className="secondary-button" type="button" disabled={busy !== null} onClick={() => setEditing(null)}>Cancel edit</button>
                    </>
                  ) : (
                    <>
                      <button className="primary-button" type="button" disabled={busy !== null || item.stale || needsChoice} title={item.stale ? "A cited file changed; this cannot be approved" : needsChoice ? "Choose replace or keep both first" : undefined} onClick={() => decide(item, "approved")}>{busyHere && busy?.action === "approve" ? "Approving…" : "Approve"}<span className="sr-only"> {item.statement}</span></button>
                      <button className="secondary-button" type="button" disabled={busy !== null || item.stale} onClick={() => { setEditing(item.id); setDraft(item.statement); }}>Edit & approve</button>
                      <button className="secondary-button" type="button" disabled={busy !== null} onClick={() => decide(item, "rejected")}>{busyHere && busy?.action === "reject" ? "Rejecting…" : "Reject"}<span className="sr-only"> {item.statement}</span></button>
                    </>
                  )}
                  <label className="decision-note">Note (optional)<input value={notes[item.id] ?? ""} maxLength={2000} onChange={(e) => setNotes((n) => ({ ...n, [item.id]: e.target.value }))} /></label>
                </div>
              )}
            </div>
          )}
        </div>
      </li>
    );
  }

  return (
    <section className="agent-access knowledge" id="panel-knowledge-inbox" aria-label={`Project knowledge in ${workspace.name}`}>
      <div className="panel-head">
        <h3>Project knowledge</h3>
        <button className="secondary-button" type="button" onClick={refreshAll}>Refresh</button>
      </div>
      <p className="panel-intro">What this project has agreed on. Assistants propose; proposals that need you wait here. Private items are decided by the person they belong to. A changed source marks an item for re-checking.</p>
      {loadError ? (
        <div className="panel-error" role="alert"><p>Knowledge could not be loaded{retried ? "." : ". Trying again…"}</p><button className="secondary-button" type="button" onClick={refreshAll}>Try again</button></div>
      ) : proposed === null || approved === null ? <p aria-busy="true" className="panel-loading">Loading knowledge…</p> : (
        <>
          <h4>Needs your decision {pendingTotal > 0 && <span className="count-pill">{pendingTotal.toLocaleString()}</span>}</h4>
          {pendingTotal === 0 ? <p className="empty-line">Nothing needs your decision.</p> : (
            <>
              <ul className="inbox-list">{groups.map((group) => group.items.length === 1 ? renderItem(group.items[0], false) : (
                <li key={group.key} className="inbox-group">
                  <button className="inbox-group-head" type="button" aria-expanded={openGroups[group.key] ?? group.items.length <= 3} onClick={() => setOpenGroups((g) => ({ ...g, [group.key]: !(g[group.key] ?? group.items.length <= 3) }))}>
                    <span>{groupTitle(group)}</span><span className="inbox-chevron" aria-hidden="true" />
                  </button>
                  {(openGroups[group.key] ?? group.items.length <= 3) && <ul className="inbox-list">{group.items.map((item) => renderItem(item, true))}</ul>}
                </li>
              ))}</ul>
              {proposed.items.length < pendingTotal || (proposed.total === null && proposed.items.length % PAGE === 0 && proposed.items.length > 0) ? (
                <div className="list-more">
                  <small>Showing {proposed.items.length.toLocaleString()}{proposed.total !== null ? ` of ${pendingTotal.toLocaleString()}` : ""}</small>
                  <button className="secondary-button" type="button" disabled={more !== null} onClick={() => showMore("proposed")}>{more === "proposed" ? "Loading…" : `Show ${Math.min(PAGE, Math.max(0, pendingTotal - proposed.items.length)) || PAGE} more`}</button>
                </div>
              ) : null}
              {itemErrors["more-proposed"] && <p className="auth-error" role="alert">{itemErrors["more-proposed"]}</p>}
            </>
          )}
          <h4>Approved knowledge {approvedTotal > 0 && <span className="count-pill muted">{approvedTotal.toLocaleString()}</span>}</h4>
          {approvedTotal === 0 ? <p className="empty-line">No approved knowledge yet. Approving a proposal or a handoff adds it here.</p> : (
            <>
              <ul className="knowledge-list">{approved.items.map((item) => (
                <li key={item.id} className={`${item.stale || item.disputed_by.length > 0 ? "stale" : ""}${leaving.has(item.id) ? " is-leaving" : ""}`}>
                  <strong>{label(item)}{item.topic ? ` · ${item.topic}` : ""}{item.auto_accepted && !item.owner_checked ? " · auto-accepted, not checked by you" : ""}</strong>
                  <span className="knowledge-statement">{item.statement}</span>
                  <small>{item.evidence.length > 0 ? sources(item) : "no evidence"}{item.stale ? " — evidence changed since approval; re-check" : ""}{item.valid_from ? ` · since ${utc(item.valid_from)}` : ""}</small>
                  {item.disputed_by.length > 0 && <small className="disputed">Disputed: {item.disputed_by.length > 1 ? `${item.disputed_by.length} proposals in the review inbox challenge this.` : "a proposal in the review inbox challenges this."}</small>}
                  {itemErrors[item.id] && <p className="auth-error" role="alert">{itemErrors[item.id]}</p>}
                  {canDecide(item) && <ConfirmButton label="Retire" confirmLabel="Confirm retire" busyLabel="Retiring…" busy={busy?.id === item.id} disabled={busy !== null} onConfirm={() => retire(item)}><span className="sr-only"> {item.statement}</span></ConfirmButton>}
                </li>
              ))}</ul>
              {approved.items.length < approvedTotal && (
                <div className="list-more">
                  <small>Showing {approved.items.length.toLocaleString()} of {approvedTotal.toLocaleString()}</small>
                  <button className="secondary-button" type="button" disabled={more !== null} onClick={() => showMore("approved")}>{more === "approved" ? "Loading…" : "Show more"}</button>
                </div>
              )}
            </>
          )}
        </>
      )}
    </section>
  );
}
