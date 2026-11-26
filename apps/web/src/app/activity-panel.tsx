"use client";

import { useEffect, useState } from "react";

type Event = {
  at: string;
  kind: string;
  actor_email: string | null;
  agent_id: string | null;
  agent_name: string | null;
  subject: string | null;
  revision: number | null;
};

function who(e: Event) {
  if (e.agent_name && !e.kind.startsWith("agent.")) return { name: e.agent_name, detail: `AI · key from ${e.actor_email ?? "owner"}`, ai: true };
  return { name: e.actor_email ?? "Owner or system", detail: "Person", ai: false };
}

function what(e: Event) {
  const s = e.subject ?? "";
  switch (e.kind) {
    case "file.create": return `created ${s} (revision ${e.revision})`;
    case "file.update": return `updated ${s} to revision ${e.revision}`;
    case "file.restore": return `restored an earlier version of ${s} as revision ${e.revision}`;
    case "handoff.proposed": return `proposed a handoff: “${s}”`;
    case "handoff.approved": return `approved the handoff “${s}”`;
    case "handoff.rejected": return `rejected the handoff “${s}”`;
    case "handoff.auto_accepted": return `had a handoff auto-accepted (sources current): “${s}”`;
    case "handoff.kept": return `checked and kept the auto-accepted handoff “${s}”`;
    case "handoff.undone": return `undid the auto-accepted handoff “${s}”`;
    case "knowledge.proposed": return `proposed knowledge: “${s}”`;
    case "knowledge.approved": return `approved knowledge “${s}”`;
    case "knowledge.auto_accepted": return `had knowledge auto-accepted: “${s}”`;
    case "knowledge.rejected": return `rejected knowledge “${s}”`;
    case "knowledge.kept": return `checked and kept auto-accepted knowledge “${s}”`;
    case "knowledge.undone": return `undid auto-accepted knowledge “${s}”`;
    case "knowledge.retired": return `retired knowledge “${s}”`;
    case "agent.issued": return `issued the AI key “${e.agent_name}” (${s})`;
    case "agent.revoked": return `AI key “${e.agent_name}” was revoked`;
    case "message.sent": return `sent a message: “${s}”`;
    default: return e.kind;
  }
}

export default function ActivityPanel({ workspace }: { workspace: { id: string; name: string } }) {
  const [events, setEvents] = useState<Event[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [error, setError] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [reload, setReload] = useState(0);
  const endpoint = `/api/workspaces/${workspace.id}/activity`;

  useEffect(() => {
    const controller = new AbortController();
    fetch(`${endpoint}?limit=30`, { cache: "no-store", signal: controller.signal })
      .then(async (response) => {
        const body = await response.json().catch(() => null);
        if (controller.signal.aborted) return;
        if (response.ok && Array.isArray(body?.events)) { setEvents(body.events); setNext(body.next_before ?? null); setError(false); }
        else setError(true);
      })
      .catch(() => { if (!controller.signal.aborted) setError(true); });
    return () => controller.abort();
  }, [endpoint, reload]);

  async function more() {
    if (!next) return;
    setLoadingMore(true);
    try {
      const response = await fetch(`${endpoint}?limit=30&before=${encodeURIComponent(next)}`, { cache: "no-store" });
      const body = await response.json().catch(() => null);
      if (response.ok && Array.isArray(body?.events)) { setEvents((current) => [...(current ?? []), ...body.events]); setNext(body.next_before ?? null); }
      else setError(true);
    } catch { setError(true); } finally { setLoadingMore(false); }
  }

  return (
    <section className="agent-access activity" aria-label={`Activity in ${workspace.name}`}>
      <div className="panel-head">
        <h3>Activity</h3>
        <button className="secondary-button" type="button" onClick={() => { setError(false); setReload((n) => n + 1); }}>Refresh</button>
      </div>
      <p className="panel-intro">Who did what, newest first. Changes made by an AI show the assistant and the owner whose key it used.</p>
      {error ? <div className="panel-error" role="alert"><p>Activity could not be loaded.</p><button className="secondary-button" type="button" onClick={() => { setError(false); setReload((n) => n + 1); }}>Try again</button></div>
        : events === null ? <p aria-busy="true" className="panel-loading">Loading activity…</p>
        : events.length === 0 ? <p className="empty-line">Nothing has happened in this space yet. Files, handoffs, knowledge and messages will show up here as they happen.</p>
        : (
          <ol className="activity-list">{events.map((e, i) => {
            const actor = who(e);
            return (
              <li key={`${e.at}-${e.kind}-${i}`} className={actor.ai ? "by-ai" : "by-person"}>
                <time dateTime={e.at}>{e.at.replace("T", " ").slice(0, 16)} UTC</time>
                <span className="activity-text"><strong>{actor.name}</strong> <small>{actor.detail}</small> {what(e)}</span>
              </li>
            );
          })}</ol>
        )}
      {next && !error && <div className="list-more"><button className="secondary-button" type="button" disabled={loadingMore} onClick={more}>{loadingMore ? "Loading…" : "Show older activity"}</button></div>}
    </section>
  );
}
