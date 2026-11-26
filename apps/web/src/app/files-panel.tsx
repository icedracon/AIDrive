"use client";

import { useEffect, useRef, useState } from "react";

type Summary = { id: string; name: string; current_version_id: string; byte_size: number };
type Version = { file_id: string; name: string; version_id: string; revision: number; content: string; sha256: string; actor_id: string; created_by: string; created_at: string; current_version_id: string; operation: string };
type HistoryItem = { id: string; revision: number; actor_id: string; created_at: string; operation: string };
type Pending = { url: string; method: string; body: string };
const limit = 262144;
const middle = (name: string) => (name.length > 48 ? `${name.slice(0, 30)}…${name.slice(-15)}` : name);
const size = (n: number) => (n < 1024 ? `${n.toLocaleString()} bytes` : `${(n / 1024).toLocaleString(undefined, { maximumFractionDigits: 1 })} KB`);
const unsavedEditors = new Set<symbol>();

export function confirmFileNavigation() {
  return unsavedEditors.size === 0 || window.confirm("You have unsaved text or an unconfirmed save. Leave this editor and discard its local draft?");
}

function useDraftGuard(unsaved: boolean) {
  const id = useRef(Symbol("draft"));
  useEffect(() => {
    if (!unsaved) return;
    const token = id.current;
    unsavedEditors.add(token);
    const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); };
    window.addEventListener("beforeunload", warn);
    return () => { unsavedEditors.delete(token); window.removeEventListener("beforeunload", warn); };
  }, [unsaved]);
}
const messages: Record<string, string> = {
  version_conflict: "Another version was saved first. Your draft is kept. Compare with the latest version before saving again.",
  request_conflict: "This request ID was already used with different content. Refresh before trying again.",
  invalid_filename: "Choose a .txt or .md name without slashes or control characters (up to 255 characters).",
  file_too_large: "The UTF-8 content must be at most 256 KiB.",
  invalid_text: "This file contains a NUL character, which is not supported.",
  read_only: "You no longer have permission to edit this space.",
  not_found: "This file or space is unavailable to your account.",
  unauthenticated: "Your session ended. Sign in again. Your draft is still here.",
};

async function send(pending: Pending): Promise<{ value?: Version; error?: string; ambiguous: boolean }> {
  try {
    const response = await fetch(pending.url, { method: pending.method, headers: { "content-type": "application/json" }, body: pending.body });
    const body = await response.json().catch(() => null);
    if (response.ok && body?.version_id) return { value: body, ambiguous: false };
    const ambiguous = response.status >= 500 || response.ok;
    return { error: ambiguous ? "Save was not confirmed. Retry the same request to find out whether it committed." : messages[body?.error] ?? "The request was rejected. Your text has been kept.", ambiguous };
  } catch {
    return { error: "Connection lost. Save may have committed. Retry the same request; it will not create another version.", ambiguous: true };
  }
}

function FileEditor({ endpoint, file, writable, onSaved }: { endpoint: string; file: Summary; writable: boolean; onSaved: () => void }) {
  const [current, setCurrent] = useState<Version | null>(null);
  const [draft, setDraft] = useState("");
  const [preview, setPreview] = useState<Version | null>(null);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [before, setBefore] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Pending | null>(null);
  const [refresh, setRefresh] = useState(0);
  const fileUrl = `${endpoint}/${file.id}`;
  const dirty = current !== null && draft !== current.content;
  useDraftGuard(dirty || pending !== null);

  useEffect(() => {
    const controller = new AbortController();
    fetch(fileUrl, { cache: "no-store", signal: controller.signal }).then(async (r) => {
      const body = await r.json().catch(() => null);
      if (controller.signal.aborted) return;
      if (!r.ok || !body?.version_id) { setError(messages[body?.error] ?? "File could not be loaded."); return; }
      setCurrent(body); setDraft(body.content);
    }).catch(() => { if (!controller.signal.aborted) setError("File could not be loaded."); });
    return () => controller.abort();
  }, [fileUrl]);

  useEffect(() => {
    const controller = new AbortController();
    fetch(`${fileUrl}/versions`, { cache: "no-store", signal: controller.signal }).then(async (r) => {
      const body = await r.json().catch(() => null);
      if (controller.signal.aborted) return;
      if (r.ok && Array.isArray(body?.versions)) { setHistory(body.versions); setBefore(body.next_before); }
      else setError("Version history could not be loaded.");
    }).catch(() => { if (!controller.signal.aborted) setError("Version history could not be loaded."); });
    return () => controller.abort();
  }, [fileUrl, refresh]);

  async function save(source?: string) {
    if (!current) return;
    if (!pending && !source && new TextEncoder().encode(draft).length > limit) { setError(messages.file_too_large); return; }
    const request = pending ?? { url: source ? `${fileUrl}/restore` : fileUrl, method: source ? "POST" : "PUT", body: JSON.stringify({ request_id: crypto.randomUUID(), base_version_id: current.version_id, ...(source ? { source_version_id: source } : { content: draft }) }) };
    setBusy(true); setError(null); setPending(request);
    const result = await send(request);
    if (!result.ambiguous) setPending(null);
    if (result.value) { setCurrent(result.value); setDraft(result.value.content); setPreview(null); setRefresh((n) => n + 1); onSaved(); }
    else setError(result.error ?? "Save failed.");
    setBusy(false);
  }

  async function inspect(id?: string) {
    setBusy(true); setError(null);
    try {
      const r = await fetch(id ? `${fileUrl}/versions/${id}` : fileUrl, { cache: "no-store" });
      const body = await r.json().catch(() => null);
      if (r.ok && body?.version_id) setPreview(body);
      else setError(messages[body?.error] ?? "Version could not be loaded.");
    } catch { setError("Version could not be loaded. Your draft is kept."); }
    finally { setBusy(false); }
  }

  async function older() {
    setBusy(true);
    try {
      const r = await fetch(`${fileUrl}/versions?before=${before}`, { cache: "no-store" });
      const body = await r.json().catch(() => null);
      if (r.ok && Array.isArray(body?.versions)) { setHistory((items) => [...items, ...body.versions]); setBefore(body.next_before); }
      else setError("Older versions could not be loaded.");
    } catch { setError("Older versions could not be loaded."); }
    finally { setBusy(false); }
  }

  return <section className="file-editor" aria-label={`Edit ${file.name}`}>
    <h3 className="file-title">{file.name}</h3>
    <p>Keep or copy unsaved text before switching files or spaces.</p>
    {error && <p role="alert" className="auth-error">{error}</p>}
    {!current ? <p>Loading file…</p> : <>
      <p className="auth-hint">Version {current.revision} · {current.created_at} · {current.operation}</p>
      {current.version_id !== current.current_version_id && <p role="status">This saved result is an older version. Compare with the latest version before editing.</p>}
      <label>Text<textarea className="file-text" value={draft} readOnly={!writable || pending !== null} disabled={busy} onChange={(e) => setDraft(e.target.value)} spellCheck={false} /></label>
      <p className="auth-hint">{new TextEncoder().encode(draft).length.toLocaleString()} / 262,144 UTF-8 bytes · {dirty ? "Unsaved draft" : "Saved version"}</p>
      <div className="auth-actions">
        {writable && <button className="primary-button" type="button" disabled={busy || (!dirty && !pending)} onClick={() => save()}>{busy ? "Working…" : pending ? "Retry same save" : "Save new version"}</button>}
        <button className="secondary-button" type="button" disabled={busy} onClick={() => inspect()}>Compare with latest</button>
        <a className="secondary-button" href={`${fileUrl}/versions/${current.version_id}/download`}>Download saved version</a>
      </div>
      <details><summary>Review saved text and provenance</summary><pre className="file-preview">{current.content}</pre><p className="file-metadata">Creator: {current.created_by}<br />Version author: {current.actor_id}<br />SHA-256: {current.sha256}</p></details>
      {preview && <div className="file-comparison">
        <h4>Compare version {preview.revision} with your draft</h4>
        <div className="file-compare-grid"><div><strong>Stored version</strong><pre className="file-preview">{preview.content}</pre></div><div><strong>Your draft</strong><pre className="file-preview">{draft}</pre></div></div>
        {writable && <div className="auth-actions">
          <button className="secondary-button" type="button" disabled={busy || !!pending || preview.version_id !== preview.current_version_id} onClick={() => { setCurrent(preview); setPreview(null); }}>Use this base, keep my draft</button>
          <button className="secondary-button" type="button" disabled={busy || !!pending || dirty || preview.version_id === current.version_id} onClick={() => save(preview.version_id)}>Restore this text as a new version</button>
        </div>}
        {dirty && <p>Save your draft before restoring another version.</p>}
      </div>}
      <h4>Version history</h4>
      <ol className="file-history">{history.map((v) => <li key={v.id}><button type="button" className="secondary-button" disabled={busy} onClick={() => inspect(v.id)}>Version {v.revision} · {v.operation}</button><small>{v.created_at} · Author {v.actor_id}</small></li>)}</ol>
      {before !== null && <button className="secondary-button" type="button" disabled={busy} onClick={older}>Older versions</button>}
    </>}
  </section>;
}

export default function FilesPanel({ workspace }: { workspace: { id: string; name: string; role: string } }) {
  const endpoint = `/api/workspaces/${workspace.id}/files`;
  const [items, setItems] = useState<Summary[] | null>(null);
  const [after, setAfter] = useState<string | null>(null);
  const [selected, setSelected] = useState<Summary | null>(null);
  const [name, setName] = useState("notes.md");
  const [content, setContent] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Pending | null>(null);
  const [reload, setReload] = useState(0);
  const upload = useRef<HTMLInputElement>(null);
  const writable = workspace.role !== "viewer";
  useDraftGuard(content !== "" || pending !== null);

  useEffect(() => {
    const controller = new AbortController();
    fetch(endpoint, { cache: "no-store", signal: controller.signal }).then(async (r) => {
      const body = await r.json().catch(() => null);
      if (controller.signal.aborted) return;
      if (r.ok && Array.isArray(body?.files)) { setItems(body.files); setAfter(body.next_after); }
      else setError(messages[body?.error] ?? "Files could not be loaded.");
    }).catch(() => { if (!controller.signal.aborted) setError("Files could not be loaded."); });
    return () => controller.abort();
  }, [endpoint, reload]);

  async function more() {
    setBusy(true);
    try {
      const r = await fetch(`${endpoint}?after=${after}`, { cache: "no-store" });
      const body = await r.json().catch(() => null);
      if (r.ok && Array.isArray(body?.files)) { setItems((rows) => [...(rows ?? []), ...body.files]); setAfter(body.next_after); }
      else setError("More files could not be loaded.");
    } catch { setError("More files could not be loaded."); }
    finally { setBusy(false); }
  }

  async function readUpload(file?: File) {
    if (!file) return;
    setBusy(true); setError(null);
    try {
      if (file.size > limit) { setError(messages.file_too_large); return; }
      const value = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(await file.arrayBuffer());
      if (value.includes("\0")) { setError(messages.invalid_text); return; }
      setName(file.name); setContent(value);
    } catch { setError("Choose a valid UTF-8 text file."); }
    finally { setBusy(false); }
  }

  async function create() {
    if (!pending && new TextEncoder().encode(content).length > limit) { setError(messages.file_too_large); return; }
    const request = pending ?? { url: endpoint, method: "POST", body: JSON.stringify({ request_id: crypto.randomUUID(), name, content }) };
    setBusy(true); setError(null); setPending(request);
    const result = await send(request);
    if (!result.ambiguous) setPending(null);
    if (result.value) {
      const v = result.value;
      setContent(""); setSelected((current) => current ?? { id: v.file_id, name: v.name, current_version_id: v.current_version_id, byte_size: new TextEncoder().encode(v.content).length });
      if (upload.current) upload.current.value = "";
      setReload((n) => n + 1);
    } else setError(result.error ?? "File could not be saved.");
    setBusy(false);
  }

  return <section className="files-panel" aria-label={`Files in ${workspace.name}`}>
    <div className="panel-head">
      <h3>Files {items && items.length > 0 && <span className="count-pill muted">{items.length}{after !== null ? "+" : ""}</span>}</h3>
      <button className="secondary-button" type="button" disabled={busy} onClick={() => { setError(null); setReload((n) => n + 1); }}>Refresh</button>
    </div>
    <p className="panel-intro">UTF-8 .txt and .md files, up to 256 KiB each. Every saved change keeps its previous version.</p>
    {error && <p className="auth-error" role="alert">{error}</p>}
    {items === null ? (error ? null : <p aria-busy="true" className="panel-loading">Loading files…</p>) : items.length === 0 ? <p className="empty-line">No text files yet. Create one below, or let an assistant add one with <code>create_file</code>.</p> : <ul className="file-list">{items.map((f) => <li key={f.id} className={selected?.id === f.id ? "is-selected" : undefined}><button className="file-row" type="button" aria-current={selected?.id === f.id ? "true" : undefined} onClick={() => { if (selected?.id === f.id || confirmFileNavigation()) setSelected(f); }}><span className="file-name" title={f.name.length > 48 ? f.name : undefined}>{middle(f.name)}</span><small>{size(f.byte_size)}</small></button></li>)}</ul>}
    {after !== null && <div className="list-more"><button className="secondary-button" type="button" disabled={busy} onClick={more}>{busy ? "Loading…" : "More files"}</button></div>}
    {writable && <details><summary>Create or upload a text file</summary><form onSubmit={(e) => { e.preventDefault(); create(); }}>
      <label>Choose a UTF-8 file<input ref={upload} type="file" accept=".txt,.md,text/plain,text/markdown" disabled={busy || !!pending} onChange={(e) => readUpload(e.target.files?.[0])} /></label>
      <label>Filename<input required maxLength={255} value={name} disabled={busy || !!pending} onChange={(e) => setName(e.target.value)} /></label>
      <label>Text<textarea className="file-text" value={content} disabled={busy || !!pending} onChange={(e) => setContent(e.target.value)} spellCheck={false} /></label>
      <button className="primary-button" type="submit" disabled={busy}>{busy ? "Working…" : pending ? "Retry same upload" : "Save file"}</button>
    </form></details>}
    {selected && <FileEditor key={selected.id} endpoint={endpoint} file={selected} writable={writable} onSaved={() => setReload((n) => n + 1)} />}
  </section>;
}
