"use client";

import { useEffect, useRef, useState } from "react";
import { ConfirmButton } from "./ui";

type Grant = { id: string; name: string; scope: string; expires_at: string; status: string };
type Scope = "workspace:read" | "files:read" | "files:write" | "review:accept" | "grand";

// Roles (migration 0021). Only you, the owner, can give a key a role.
const scopeLabels: Record<Scope, string> = {
  "files:write": "Low: read and edit files, propose, hand off",
  "review:accept": "Medium: also approve other assistants' proposals and handoffs",
  grand: "Grand: everything you can do with project state (not keys or members)",
  "files:read": "Read files and handoffs only",
  "workspace:read": "See the space name only",
};

const roleName = (scope: string) =>
  ({ "files:write": "low", "review:accept": "medium", grand: "grand" } as Record<string, string>)[scope] ?? scope;

export default function AgentAccess({ workspace }: { workspace: { id: string; name: string } }) {
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [name, setName] = useState("");
  const [scope, setScope] = useState<Scope>("files:write");
  const [token, setToken] = useState<string | null>(null);
  const [issuedId, setIssuedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [reload, setReload] = useState(0);
  const pendingId = useRef<string | null>(null);
  const endpoint = `/api/workspaces/${workspace.id}/agents`;

  useEffect(() => {
    const controller = new AbortController();
    fetch(endpoint, { cache: "no-store", signal: controller.signal })
      .then(async (response) => {
        const body = await response.json().catch(() => null);
        if (controller.signal.aborted) return;
        if (response.ok && Array.isArray(body?.grants)) setGrants(body.grants);
        else setError("Access could not be loaded. Only a current space owner can manage keys.");
      })
      .catch(() => { if (!controller.signal.aborted) setError("Access could not be loaded. Try refreshing."); });
    return () => controller.abort();
  }, [endpoint, reload]);

  async function issue() {
    setBusy(true);
    setError(null);
    setToken(null);
    try {
      pendingId.current ??= crypto.randomUUID();
      const response = await fetch(endpoint, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ request_id: pendingId.current, name, scope }),
      });
      const body = await response.json().catch(() => null);
      if (response.status === 201 && body?.token && body?.grant) {
        setToken(body.token);
        setIssuedId(body.grant.id);
        setName("");
        pendingId.current = null;
      } else if (response.status === 409) {
        setError("This request already created a key, but its secret cannot be shown again. Revoke that key in the list before creating a replacement.");
        pendingId.current = null;
      } else if (response.status === 422) {
        setError("Use a name with 1–120 characters and no control characters.");
      } else {
        setError("Creation was not confirmed. Refresh the list or retry the same request. A key may already exist.");
      }
    } catch {
      setError("Creation was not confirmed. Refresh the list or retry the same request. A key may already exist.");
    } finally {
      setBusy(false);
      setReload((n) => n + 1);
    }
  }

  async function revoke(id: string) {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`${endpoint}/${id}`, { method: "DELETE" });
      if (response.status === 204) {
        if (issuedId === id) setToken(null);
        setGrants((items) => items?.map((g) => g.id === id ? { ...g, status: "revoked" } : g) ?? null);
      } else {
        setError("Revocation was not confirmed. Refresh or retry before relying on it.");
      }
    } catch {
      setError("Revocation was not confirmed. Refresh or retry before relying on it.");
    } finally {
      setBusy(false);
      setReload((n) => n + 1);
    }
  }

  return (
    <section className="agent-access" aria-label={`AI access for ${workspace.name}`}>
      <h3>AI access</h3>
      <p>Give an AI assistant (for example Claude Code or Codex over MCP) access to this space. Its changes are recorded under its own name. Decisions it proposes need your approval.</p>
      <form onSubmit={(event) => { event.preventDefault(); issue(); }}>
        <label>Key name<input required maxLength={120} value={name} onChange={(e) => setName(e.target.value)} placeholder="My local assistant" /></label>
        <label>Permission<select value={scope} onChange={(e) => setScope(e.target.value as Scope)}>{(Object.keys(scopeLabels) as Scope[]).map((s) => <option key={s} value={s}>{scopeLabels[s]}</option>)}</select></label>
        <p className="auth-hint">Expires after 30 days. Losing owner access permanently revokes keys you issued.</p>
        <div className="auth-actions">
          <button className="primary-button" type="submit" disabled={busy || token !== null}>{busy ? "Working…" : "Create access key"}</button>
          <button className="secondary-button" type="button" disabled={busy} onClick={() => { setError(null); setReload((n) => n + 1); }}>Refresh access</button>
        </div>
      </form>
      {token && <div className="agent-secret">
        <label>Copy this key now — it will not be shown again<textarea readOnly value={token} spellCheck={false} autoComplete="off" onFocus={(e) => e.target.select()} /></label>
        <p>Keep it private. For the AIDrive MCP server set <code>AIDRIVE_AGENT_TOKEN</code> to this key and <code>AIDRIVE_WORKSPACE_ID</code> to <code>{workspace.id}</code>.</p>
        <button className="secondary-button" type="button" onClick={() => setToken(null)}>I saved it — hide key</button>
      </div>}
      {error && <p className="auth-error" role="alert">{error}</p>}
      {grants === null ? <p aria-busy="true" className="panel-loading">Loading access…</p> : grants.length === 0 ? <p className="empty-line">No keys for this space yet. Create one above and paste it into your assistant’s AIDrive settings.</p> : (
        <ul className="agent-list">{grants.map((g) => <li key={g.id}>
          <div><strong>{g.name}</strong><small>{g.status} · {roleName(g.scope)} · Expires {g.expires_at}</small><small>{g.id}</small></div>
          {g.status !== "revoked" && <ConfirmButton label="Revoke" confirmLabel="Confirm revoke (cannot be undone)" disabled={busy} onConfirm={() => revoke(g.id)}><span className="sr-only"> {g.name}</span></ConfirmButton>}
        </li>)}</ul>
      )}
    </section>
  );
}
