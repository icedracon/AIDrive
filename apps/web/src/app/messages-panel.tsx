"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { ClampText, utc } from "./ui";

type Source = { file_id: string; file_name: string | null; version_id: string; stale: boolean };
type Delivery = { chat_id: string; client: string; cwd: string | null; wake: boolean; delivered_at: string };
type Message = {
  id: string;
  thread_id: string;
  reply_to: string | null;
  from_email: string | null;
  from_agent_name: string | null;
  to: "assistants" | "owner" | "agent";
  to_agent_name: string | null;
  subject: string | null;
  body: string;
  created_at: string;
  read_by_me: boolean;
  sources: Source[];
  from_chat?: string | null;
  to_chat?: string | null;
  deliveries?: Delivery[];
};
type Chat = { id: string; agent_id: string; agent_name: string; client: string; session_id: string; cwd: string | null; first_seen: string; last_seen: string };

const LIMIT = 100;
const PAGE = 20;

const errors: Record<string, string> = {
  unknown_recipient: "No active assistant key has that name. Use the name shown under AI access.",
  read_only: "Only editors and owners can send messages.",
  invalid_message: "Write a message of up to 8,000 characters.",
  unknown_chat: "That chat is not one of this assistant's chats any more. Pick another or leave it empty.",
  ambiguous_chat: "That chat belongs to a different assistant than the one you wrote to.",
};

const clients: Record<string, string> = { "claude-code": "Claude Code", codex: "Codex", opencode: "OpenCode" };

function folder(cwd: string | null) {
  if (!cwd) return "";
  const parts = cwd.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? cwd;
}

function chatName(chat: { client: string; cwd: string | null }) {
  const app = clients[chat.client] ?? chat.client;
  const where = folder(chat.cwd);
  return where ? `${app} in ${where}` : app;
}

function sender(m: Message) {
  return m.from_agent_name ? `Assistant “${m.from_agent_name}”` : m.from_email ?? "Someone";
}

function recipient(m: Message) {
  return m.to === "agent" ? `“${m.to_agent_name ?? "?"}”` : m.to === "owner" ? "the owner" : "all assistants";
}

function delivery(m: Message, chats: Map<string, Chat>) {
  if (m.to === "owner") return null;
  const done = m.deliveries ?? [];
  if (done.length > 0) {
    const d = done[done.length - 1];
    const more = done.length > 1 ? ` and ${done.length - 1} more` : "";
    return { state: "claimed", text: `Inbox collected by ${chatName(d)}${more} · ${utc(d.delivered_at)} · turn start and reply are not confirmed by this receipt` };
  }
  if (m.deliveries === undefined) return null;
  const target = m.to_chat ? chats.get(m.to_chat) : undefined;
  return { state: "pending", text: m.to_chat ? `Waiting for ${target ? chatName(target) : "its chat"} to pick it up` : "Waiting: no chat has picked it up yet" };
}

export default function MessagesPanel({ workspace, active = true }: { workspace: { id: string; name: string; role: string }; active?: boolean }) {
  const attempt = useRef<{ signature: string; id: string } | null>(null);
  const sending = useRef(false);
  const [messages, setMessages] = useState<Message[] | null>(null);
  const [chats, setChats] = useState<Chat[]>([]);
  const [loadError, setLoadError] = useState(false);
  const [to, setTo] = useState("assistants");
  const [toChat, setToChat] = useState("");
  const [body, setBody] = useState("");
  const [replyTo, setReplyTo] = useState<Message | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sent, setSent] = useState(false);
  const [shown, setShown] = useState(PAGE);
  const [reload, setReload] = useState(0);
  const base = `/api/workspaces/${workspace.id}/messages`;
  const canSend = workspace.role === "owner" || workspace.role === "editor";

  useEffect(() => {
    const controller = new AbortController();
    const load = () => fetch(`${base}?limit=${LIMIT}`, { cache: "no-store", signal: controller.signal })
      .then(async (response) => {
        const result = await response.json().catch(() => null);
        if (controller.signal.aborted) return;
        if (response.ok && Array.isArray(result?.messages)) { setMessages(result.messages); setLoadError(false); }
        else setLoadError(true);
      })
      .catch(() => { if (!controller.signal.aborted) setLoadError(true); });
    void load();
    const timer = setInterval(() => { if (document.visibilityState === "visible") void load(); }, 3000);
    fetch(`/api/workspaces/${workspace.id}/chats`, { cache: "no-store", signal: controller.signal })
      .then((r) => (r.ok ? r.json() : null))
      .then((r) => { if (!controller.signal.aborted && Array.isArray(r?.chats)) setChats(r.chats); })
      .catch(() => undefined);
    return () => { controller.abort(); clearInterval(timer); };
  }, [base, workspace.id, reload]);

  useEffect(() => {
    if (!active || document.visibilityState !== "visible") return;
    const unread = messages?.filter((m) => !m.read_by_me && m.to === "owner").map((m) => m.id) ?? [];
    if (unread.length === 0) return;
    fetch(`${base}/read`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ ids: unread }) }).catch(() => undefined);
  }, [base, messages, active]);

  useEffect(() => {
    if (!sent) return;
    const timer = window.setTimeout(() => setSent(false), 2500);
    return () => window.clearTimeout(timer);
  }, [sent]);

  const chatById = useMemo(() => new Map(chats.map((c) => [c.id, c])), [chats]);
  const chatsForRecipient = chats.filter((c) => c.agent_name === to.trim());

  async function send() {
    if (!body.trim() || sending.current) return;
    const payload = { to: to.trim() || "assistants", body: body.trim(), reply_to: replyTo?.id, to_chat: toChat || undefined };
    const signature = JSON.stringify(payload);
    if (attempt.current?.signature !== signature) attempt.current = { signature, id: crypto.randomUUID() };
    const requestId = attempt.current.id;
    sending.current = true;
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(base, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ request_id: requestId, ...payload }),
      });
      const result = await response.json().catch(() => null);
      if (response.status === 201) { attempt.current = null; setBody(""); setReplyTo(null); setToChat(""); setSent(true); setReload((n) => n + 1); }
      else setError(errors[result?.error] ?? "The message was not confirmed. Refresh before retrying.");
    } catch {
      setError("The message was not confirmed. Refresh before retrying.");
    } finally {
      sending.current = false;
      setBusy(false);
    }
  }

  const newForOwner = messages?.filter((m) => m.to === "owner" && !m.read_by_me).length ?? 0;

  return (
    <section className="agent-access messages" aria-label={`Messages in ${workspace.name}`}>
      <div className="panel-head">
        <h3>Messages {newForOwner > 0 && <span className="count-pill">{newForOwner} new</span>}</h3>
        <button className="secondary-button" type="button" onClick={() => { setLoadError(false); setReload((n) => n + 1); }}>Refresh</button>
      </div>
      <p className="panel-intro">How your assistants ask each other and you for things. A message is a request, not a decision: lasting decisions still go through handoffs and project knowledge.</p>
      {canSend && (
        <form className="message-form" onSubmit={(event) => { event.preventDefault(); send(); }}>
          {replyTo && <p className="message-reply"><span>Replying to {sender(replyTo)}: “{replyTo.body.slice(0, 80)}{replyTo.body.length > 80 ? "…" : ""}”</span> <button className="link-button" type="button" disabled={busy} onClick={() => setReplyTo(null)}>Cancel reply</button></p>}
          <div className="message-to">
            <label>To<input disabled={busy} value={to} maxLength={100} onChange={(e) => { setTo(e.target.value); setToChat(""); }} list={`recipients-${workspace.id}`} autoComplete="off" /></label>
            {chatsForRecipient.length > 0 && (
              <label>Chat<select disabled={busy} value={toChat} onChange={(e) => setToChat(e.target.value)}>
                <option value="">{replyTo ? "Same chat as the message" : "Whichever chat picks it up"}</option>
                {chatsForRecipient.map((c) => <option key={c.id} value={c.id}>{chatName(c)} · seen {utc(c.last_seen)}</option>)}
              </select></label>
            )}
          </div>
          <datalist id={`recipients-${workspace.id}`}><option value="assistants" /><option value="owner" />{[...new Set(chats.map((c) => c.agent_name))].map((n) => <option key={n} value={n} />)}</datalist>
          <label>Message<textarea disabled={busy} value={body} maxLength={8000} rows={3} onChange={(e) => setBody(e.target.value)} /></label>
          <div className="auth-actions">
            <button className="primary-button" type="submit" disabled={busy || !body.trim()}>{busy ? "Sending…" : "Send"}</button>
            {sent && <span className="saved-note" role="status">Sent</span>}
          </div>
          {error && <p className="auth-error" role="alert">{error}</p>}
        </form>
      )}
      {loadError ? <div className="panel-error" role="alert"><p>Messages could not be loaded.</p><button className="secondary-button" type="button" onClick={() => { setLoadError(false); setReload((n) => n + 1); }}>Try again</button></div>
        : messages === null ? <p aria-busy="true" className="panel-loading">Loading messages…</p>
        : messages.length === 0 ? <p className="empty-line">No messages yet. Assistants send them with the <code>send_message</code> tool; you can write to them above.</p>
        : (
          <>
            <ul className="message-list">{messages.slice(0, shown).map((m) => {
              const status = delivery(m, chatById);
              return (
                <li key={m.id} className={m.to === "owner" && !m.read_by_me ? "unread" : undefined}>
                  <small className="message-meta"><span className="message-who">{sender(m)} → {recipient(m)}</span> <span>{utc(m.created_at)}{m.reply_to ? " · reply" : ""}</span></small>
                  {m.subject && <strong className="message-subject">{m.subject}</strong>}
                  <ClampText text={m.body} lines={6} className="message-body" />
                  {m.sources.length > 0 && <p className="handoff-sources">{m.sources.map((s) => <span key={s.version_id} className={s.stale ? "source stale" : "source"}>{s.file_name ?? "file"}{s.stale ? " (changed since)" : ""}</span>)}</p>}
                  {status && <small className={`delivery ${status.state}`}>{status.text}</small>}
                  {canSend && <button className="link-button" type="button" disabled={busy} onClick={() => { setReplyTo(m); setTo(m.from_agent_name ?? "assistants"); setToChat(""); }}>Reply<span className="sr-only"> to {sender(m)}</span></button>}
                </li>
              );
            })}</ul>
            {messages.length > shown && <div className="list-more"><small>Showing {shown} of {messages.length}{messages.length >= LIMIT ? " latest" : ""}</small><button className="secondary-button" type="button" onClick={() => setShown((n) => n + PAGE)}>Show {Math.min(PAGE, messages.length - shown)} more</button></div>}
          </>
        )}
    </section>
  );
}
