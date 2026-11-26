"use client";

import Link from "next/link";
import { useEffect, useState } from "react";

type Workspace = { id: string; name: string; role: string };
type Pending = { client_name: string; status: string; expires_at: string };
type Phase =
  | { kind: "loading" }
  | { kind: "signed-out" }
  | { kind: "missing" }
  | { kind: "error"; message: string }
  | { kind: "ready"; pending: Pending; workspaces: Workspace[] }
  | { kind: "done"; approved: boolean; client: string; workspace?: string };

const scopes: { value: string; label: string }[] = [
  { value: "files:write", label: "Low: read and edit files, propose, hand off" },
  { value: "review:accept", label: "Medium: also approve other assistants' proposals and handoffs" },
  { value: "grand", label: "Grand: everything you can do with project state (not keys or members)" },
  { value: "files:read", label: "Read files and handoffs only" },
];

function formatCode(code: string) {
  const clean = code.toUpperCase().replace(/[^A-Z]/g, "");
  return clean.length === 8 ? `${clean.slice(0, 4)}-${clean.slice(4)}` : clean;
}

export default function PairPage() {
  const [code, setCode] = useState("");
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });
  const [workspace, setWorkspace] = useState("");
  const [scope, setScope] = useState("files:write");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const value = new URLSearchParams(window.location.search).get("code") ?? "";
    (async () => {
      setCode(value);
      if (!value) { setPhase({ kind: "missing" }); return; }
      const session = await fetch("/api/auth/session", { cache: "no-store" }).catch(() => null);
      if (!session || session.status === 401) { setPhase({ kind: "signed-out" }); return; }
      const [pendingResponse, spacesResponse] = await Promise.all([
        fetch(`/api/pairing/${encodeURIComponent(value)}`, { cache: "no-store" }),
        fetch("/api/workspaces", { cache: "no-store" }),
      ]);
      const pending = await pendingResponse.json().catch(() => null);
      const spaces = await spacesResponse.json().catch(() => null);
      if (pendingResponse.status === 429) { setPhase({ kind: "error", message: "Too many incorrect codes. Wait ten minutes and try again." }); return; }
      if (!pendingResponse.ok) { setPhase({ kind: "error", message: "This code is unknown or has expired. Ask your assistant to start connecting again." }); return; }
      if (pending.status !== "pending") { setPhase({ kind: "error", message: `This request was already ${pending.status}.` }); return; }
      const owned: Workspace[] = (spaces?.workspaces ?? []).filter((w: Workspace) => w.role === "owner");
      setWorkspace(owned[0]?.id ?? "");
      setPhase({ kind: "ready", pending, workspaces: owned });
    })().catch(() => setPhase({ kind: "error", message: "The AIDrive API is unreachable." }));
  }, []);

  async function decide(decision: "approved" | "denied") {
    if (phase.kind !== "ready") return;
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/api/pairing/${encodeURIComponent(code)}/decision`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(decision === "approved" ? { decision, workspace_id: workspace, scope } : { decision }),
      });
      const body = await response.json().catch(() => null);
      if (response.ok) {
        const name = phase.workspaces.find((w) => w.id === workspace)?.name;
        setPhase({ kind: "done", approved: body?.status === "approved", client: phase.pending.client_name, workspace: name });
      } else if (response.status === 409) {
        setError("This request was already decided.");
      } else if (response.status === 403) {
        setError("You can only connect assistants to spaces you own.");
      } else {
        setError("The decision was not confirmed. Refresh before trying again.");
      }
    } catch {
      setError("The decision was not confirmed. Refresh before trying again.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="pair-page">
      <section className="auth-card pair-card" aria-labelledby="pair-title">
        <h1 id="pair-title">Connect an AI assistant</h1>
        {phase.kind === "loading" && <p aria-busy="true">Checking the request…</p>}
        {phase.kind === "missing" && <p role="alert">This link has no code. Ask your assistant to start connecting to AIDrive again.</p>}
        {phase.kind === "signed-out" && <p>Sign in on the <Link href="/">AIDrive home page</Link>, then open this link again.</p>}
        {phase.kind === "error" && <p role="alert">{phase.message}</p>}
        {phase.kind === "ready" && (
          <>
            <p><strong>{phase.pending.client_name}</strong> is asking to use your AIDrive files and handoffs.</p>
            <p className="pair-code" aria-label="Pairing code">{formatCode(code)}</p>
            <p className="auth-hint">Only continue if your assistant is showing exactly this code and you started the connection yourself. The request expires at {phase.pending.expires_at.replace("T", " ").replace("Z", " UTC")}.</p>
            {phase.workspaces.length === 0 ? <p role="alert">You do not own a space yet. Create one on the home page first.</p> : (
              <form onSubmit={(event) => { event.preventDefault(); decide("approved"); }}>
                <label>Space<select value={workspace} onChange={(e) => setWorkspace(e.target.value)}>{phase.workspaces.map((w) => <option key={w.id} value={w.id}>{w.name}</option>)}</select></label>
                <label>Permission<select value={scope} onChange={(e) => setScope(e.target.value)}>{scopes.map((s) => <option key={s.value} value={s.value}>{s.label}</option>)}</select></label>
                <p className="auth-hint">Decisions the assistant proposes still need your approval. You can revoke access any time under “Manage AI access”. Access expires after 30 days.</p>
                <div className="auth-actions">
                  <button className="primary-button" type="submit" disabled={busy || !workspace}>{busy ? "Saving…" : "Allow"}</button>
                  <button className="secondary-button" type="button" disabled={busy} onClick={() => decide("denied")}>Deny</button>
                </div>
              </form>
            )}
            {error && <p className="auth-error" role="alert">{error}</p>}
          </>
        )}
        {phase.kind === "done" && (
          phase.approved
            ? <p role="status"><strong>{phase.client}</strong> can now use <strong>{phase.workspace}</strong>. Go back to the assistant; it finishes connecting on its own. Start a new session there to use the AIDrive tools.</p>
            : <p role="status">Request denied. {phase.client} has no access.</p>
        )}
      </section>
    </main>
  );
}
