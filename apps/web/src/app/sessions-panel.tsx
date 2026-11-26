"use client";

import { useEffect, useState } from "react";
import { ConfirmButton } from "./ui";

type Session = {
  id: string;
  client: string;
  label: string;
  capture_origin: string;
  coverage: string;
  started_at: string | null;
  ended_at: string | null;
  event_count: number;
  visibility: "private" | "team";
  mine: boolean;
};

type Hit = { session_id: string; client: string; label: string; ordinal: number; role: string; at: string | null; snippet: string };

const clientNames: Record<string, string> = { "claude-code": "Claude Code", codex: "Codex" };

function when(value: string | null) {
  return value ? `${value.replace("T", " ").slice(0, 16)} UTC` : "unknown time";
}

export default function SessionsPanel({ workspace }: { workspace: { id: string; name: string; role: string } }) {
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Hit[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const base = `/api/workspaces/${workspace.id}/sessions`;
  const owner = workspace.role === "owner";

  useEffect(() => {
    const controller = new AbortController();
    fetch(`${base}?limit=50`, { cache: "no-store", signal: controller.signal })
      .then(async (response) => {
        const body = await response.json().catch(() => null);
        if (controller.signal.aborted) return;
        if (response.ok && Array.isArray(body)) { setSessions(body); setLoadError(false); }
        else setLoadError(true);
      })
      .catch(() => { if (!controller.signal.aborted) setLoadError(true); });
    return () => controller.abort();
  }, [base, reload]);

  async function search() {
    if (!query.trim()) { setHits(null); return; }
    setError(null);
    const response = await fetch(`${base}/search?limit=20&q=${encodeURIComponent(query.trim())}`, { cache: "no-store" }).catch(() => null);
    const body = await response?.json().catch(() => null);
    if (response?.ok && Array.isArray(body)) setHits(body);
    else setError("Search failed. Try different words.");
  }

  async function share(session: Session, visibility: "private" | "team") {
    setBusy(session.id);
    setError(null);
    try {
      const response = await fetch(`${base}/${session.id}/visibility`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ visibility }),
      });
      if (response.status === 204) setSessions((items) => items?.map((s) => (s.id === session.id ? { ...s, visibility } : s)) ?? null);
      else setError("The change was not confirmed. Refresh before retrying.");
    } catch {
      setError("The change was not confirmed. Refresh before retrying.");
    } finally {
      setBusy(null);
    }
  }

  async function remove(session: Session) {
    setBusy(session.id);
    setError(null);
    try {
      const response = await fetch(`${base}/${session.id}`, { method: "DELETE" });
      if (response.status === 204) {
        setSessions((items) => items?.filter((s) => s.id !== session.id) ?? null);
        setHits((items) => items?.filter((h) => h.session_id !== session.id) ?? null);
      } else {
        setError(response.status === 403 ? "Only a space owner or the person who captured it can delete a session." : "Deletion was not confirmed. Refresh before retrying.");
      }
    } catch {
      setError("Deletion was not confirmed. Refresh before retrying.");
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="agent-access sessions" aria-label={`Captured sessions in ${workspace.name}`}>
      <h3>Captured sessions</h3>
      <p>Conversations with Claude Code and Codex in this project, so any assistant can find what was discussed. Only what you and the assistant said and which tools ran is kept; model reasoning and system instructions are not. Secrets are redacted on your computer before upload, but unlabeled secrets can slip through, so review what you capture. This is history, not approved decisions. New captures are private: only you and your assistants see them until you share one with the team.</p>
      <form className="session-search" onSubmit={(event) => { event.preventDefault(); search(); }}>
        <label>Search sessions<input value={query} maxLength={200} onChange={(e) => setQuery(e.target.value)} placeholder="e.g. pricing, migration, հարցազրույց" /></label>
        <button className="secondary-button" type="submit">Search</button>
        <button className="secondary-button" type="button" onClick={() => setReload((n) => n + 1)}>Refresh</button>
      </form>
      {error && <p className="auth-error" role="alert">{error}</p>}
      {hits && (hits.length === 0 ? <p>No captured session mentions that.</p> : (
        <ol className="session-hits" aria-label="Search results">{hits.map((h) => (
          <li key={`${h.session_id}-${h.ordinal}`}>
            <small>{clientNames[h.client] ?? h.client} · {h.label} · {when(h.at)} · {h.role}</small>
            <span>{h.snippet.split(/«|»/).map((part, i) => i % 2 ? <mark key={i}>{part}</mark> : <span key={i}>{part}</span>)}</span>
          </li>
        ))}</ol>
      ))}
      {loadError ? <div className="panel-error" role="alert"><p>Sessions could not be loaded.</p><button className="secondary-button" type="button" onClick={() => setReload((n) => n + 1)}>Try again</button></div>
        : sessions === null ? <p aria-busy="true" className="panel-loading">Loading sessions…</p>
        : sessions.length === 0 ? <p className="empty-line">No sessions captured yet. Run <code>aidrive-mcp-server capture --client codex --project &lt;folder&gt;</code> (or <code>--client claude</code>) on the computer where you work.</p>
        : (
          <ul className="session-list">{sessions.map((s) => (
            <li key={s.id}>
              <div>
                <strong>{clientNames[s.client] ?? s.client} · {s.label}</strong>
                <small>{s.event_count} events · {s.coverage === "complete" ? "complete capture" : "partial capture"} · {s.capture_origin === "hook" ? "captured automatically" : "imported"} · ended {when(s.ended_at)}</small>
                <small className={s.visibility === "team" ? "visibility team" : "visibility"}>{s.visibility === "team" ? "Shared with the team" : s.mine ? "Private: only you and your assistants" : "Private"}</small>
              </div>
              {s.mine && <button className="secondary-button" type="button" disabled={busy !== null} onClick={() => share(s, s.visibility === "team" ? "private" : "team")}>{s.visibility === "team" ? "Make private" : "Share with team"}<span className="sr-only"> session {s.label}</span></button>}
              {(owner || s.mine) && <ConfirmButton label="Delete" confirmLabel={`Delete ${s.event_count} events? The file on your computer stays`} busyLabel="Deleting…" busy={busy === s.id} disabled={busy !== null} onConfirm={() => remove(s)}><span className="sr-only"> session {s.label}</span></ConfirmButton>}
            </li>
          ))}</ul>
        )}
    </section>
  );
}
