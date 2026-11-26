"use client";

import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { PaperPlaneTilt, ArrowsClockwise, ChatsCircle, DotsThree, CaretUpDown } from "@phosphor-icons/react";
import ConversationEvidence, { type ConversationEvidenceSource } from "./conversation-evidence";
import AgentScene, { agentLabel, agentProviders } from "./agent-scene";
import ProjectMark from "./project-mark";
import SessionTranscript from "./session-transcript";
import WorkspaceConnection from "./workspace-connection";
import ChatPicker, { chatTitle, type ChatChoice } from "./chat-picker";
import transcriptStyles from "./session-transcript.module.css";
import styles from "./workspace-console.module.css";

type ConversationMessage = { sources?: ConversationEvidenceSource[]; id: string; body: string; created_at: string; from_chat?: string; to_chat?: string; from_agent_name?: string; from_email?: string; to_agent_name?: string };

type Chat = ChatChoice;
type Draft = { body: string; busy: boolean; notice: string; failed: boolean; attempt: { body: string; id: string } | null };
const savedDrafts = new Map<string, Record<string, Draft>>();
const emptyDrafts: Record<string, Draft> = {};
const draftListeners = new Set<() => void>();
function subscribeDrafts(listener: () => void) {
  draftListeners.add(listener);
  return () => { draftListeners.delete(listener); };
}
const emptyDraft: Draft = { body: "", busy: false, notice: "", failed: false, attempt: null };

function isChat(value: unknown): value is Chat {
  if (!value || typeof value !== "object") return false;
  const chat = value as Record<string, unknown>;
  return ["id", "agent_id", "agent_name", "client", "session_id", "first_seen", "last_seen"].every(key => typeof chat[key] === "string") && !!chat.id && !!chat.agent_id && (chat.cwd === null || typeof chat.cwd === "string") && (chat.title == null || typeof chat.title === "string");
}

function seenAt(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "Unavailable" : date.toLocaleString();
}

function messageTime(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "Unavailable" : new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(date);
}

function isMessage(value: unknown): value is ConversationMessage {
  if (!value || typeof value !== "object") return false;
  const message = value as Record<string, unknown>;
  return ["id", "body", "created_at"].every(key => typeof message[key] === "string")
    && (message.sources === undefined || Array.isArray(message.sources));
}

type Project = { id: string; name: string; role: string };
type ConsoleProps = { currentEmail?: string; workspace: Project; projects: Project[]; onProjectChange: (project: Project) => void };

function Console({ workspace, currentEmail, projects, onProjectChange, client, onClientChange }: ConsoleProps & { client: string; onClientChange: (client: string) => void }) {
  const base = "/api/workspaces/" + encodeURIComponent(workspace.id);
  const [chats, setChats] = useState<Chat[] | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [refresh, setRefresh] = useState(0);
  const [target, setTarget] = useState("");
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [messageError, setMessageError] = useState("");
  const [conversationView, setConversationView] = useState("native");
  // Switching assistant reveals the panel's new content from left to right.
  const inspector = useRef<HTMLElement>(null);
  const shownClient = useRef(client);
  useEffect(() => {
    const panel = inspector.current;
    if (!panel || shownClient.current === client) return;
    shownClient.current = client;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    panel.animate(
      reduce
        ? [{ opacity: .6 }, { opacity: 1 }]
        : [{ clipPath: "inset(0 100% 0 0 round 28px)", opacity: .55 }, { clipPath: "inset(0 0 0 0 round 28px)", opacity: 1 }],
      { duration: reduce ? 160 : 340, easing: reduce ? "linear" : "cubic-bezier(0.77, 0, 0.175, 1)" },
    );
  }, [client]);
  const workspaceDraftKey = JSON.stringify([currentEmail, workspace.id]);
  const drafts = useSyncExternalStore(subscribeDrafts, () => savedDrafts.get(workspaceDraftKey) ?? emptyDrafts, () => emptyDrafts);
  const canWrite = ["owner", "editor"].includes(workspace.role);

  function updateDraft(key: string, patch: Partial<Draft>) {
    const stored = savedDrafts.get(workspaceDraftKey) ?? emptyDrafts;
    const next = { ...stored, [key]: { ...(stored[key] ?? emptyDraft), ...patch } };
    savedDrafts.set(workspaceDraftKey, next);
    draftListeners.forEach(listener => listener());
  }

  useEffect(() => {
    const controller = new AbortController();
    async function load() {
      try {
        const response = await fetch(base + "/chats", { signal: controller.signal, cache: "no-store" });
        if (!response.ok) throw Error();
        const result = await response.json();
        if (!Array.isArray(result.chats) || !result.chats.every(isChat)) throw Error();
        if (!controller.signal.aborted) {
          setChats(result.chats);
          setTarget(current => result.chats.some((chat: Chat) => chat.id === current) ? current : "");
          setError("");
        }
      } catch {
        if (!controller.signal.aborted) setError("The chat registry could not be refreshed. Check your connection and try again.");
      } finally {
        if (!controller.signal.aborted) setLoading(false);
      }
    }
    void load();
    return () => controller.abort();
  }, [base, refresh]);

  useEffect(() => {
    const controller = new AbortController();
    let pending = false;
    async function loadMessages() {
      if (pending) return;
      pending = true;
      try {
        const response = await fetch(base + "/messages?limit=50", { signal: controller.signal, cache: "no-store" });
        const result = await response.json();
        if (!response.ok || !Array.isArray(result.messages) || !result.messages.every(isMessage)) throw Error();
        if (!controller.signal.aborted) { setMessages(result.messages); setMessageError(""); }
      } catch { if (!controller.signal.aborted) setMessageError("Messages could not be refreshed. Showing the last loaded messages."); } finally { pending = false; }
    }
    void loadMessages();
    const timer = setInterval(() => { if (document.visibilityState === "visible") void loadMessages(); }, 3000);
    return () => { controller.abort(); clearInterval(timer); };
  }, [base]);

  const availableChats = chats?.filter(chat => chat.client === client) ?? [];
  const selected = availableChats.find(chat => chat.id === target);
  const providers = [...agentProviders, ...Array.from(new Set((chats ?? []).map(chat => chat.client))).filter(value => !agentProviders.some(provider => provider.client === value)).map(value => ({ client: value, name: value }))];
  function chooseClient(value: string) { if (value !== client) { setTarget(""); onClientChange(value); } }
  const draftKey = selected ? JSON.stringify([selected.id, selected.agent_id]) : "";
  const draft = drafts[draftKey] ?? emptyDraft;
  const verified = !loading && !error;

  async function send() {
    if (!selected || !canWrite || !verified) return;
    const current = savedDrafts.get(workspaceDraftKey)?.[draftKey] ?? emptyDraft;
    const value = current.body.trim();
    if (!value || current.busy) return;
    const attempt = current.attempt?.body === value ? current.attempt : { body: value, id: crypto.randomUUID() };
    updateDraft(draftKey, { busy: true, notice: "", failed: false, attempt });
    try {
      const response = await fetch(base + "/messages", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ request_id: attempt.id, to: selected.agent_id, to_chat: selected.id, body: value }),
      });
      if (response.status !== 201) {
        updateDraft(draftKey, { failed: true, notice: response.status === 403 ? "You no longer have permission to send. Your draft is kept." : response.status === 401 ? "Your session ended. Sign in again. Your draft is kept." : "Send was not confirmed. Retry this message." });
        return;
      }
      updateDraft(draftKey, { body: "", attempt: null, failed: false, notice: "Message stored for this chat. Delivery has not been confirmed." });
    } catch {
      updateDraft(draftKey, { failed: true, notice: "Connection lost. Your draft is kept. Retry this message." });
    } finally {
      updateDraft(draftKey, { busy: false });
    }
  }

  const conversation = messages.filter(message => message.from_chat === selected?.id || message.to_chat === selected?.id).slice().reverse();
  const latestAssistantMessage = conversation.filter(message => message.from_chat === selected?.id).at(-1)?.id;
  return <section className={styles.console} aria-label={"Workspace overview for " + workspace.name}>
    <div className={styles.layout}>
      <div className={styles.aurora} aria-hidden="true" />
      <div className={styles.spotlight} aria-hidden="true" data-spotlight />
      <WorkspaceConnection client={client} registryKey={(chats ?? []).map(chat => chat.client).join(",")} />
      <div className={styles.stage}>
        <header className={styles.header}><a className={styles.projectContext} href="#spaces" onClick={() => requestAnimationFrame(() => document.getElementById("main-content")?.focus())} aria-label={"Change project " + workspace.name + " · " + workspace.id.slice(0,8)}><ProjectMark project={workspace} size="small"/><span>PROJECT / {workspace.id.slice(0,8)}</span><CaretUpDown size={14} aria-hidden="true"/></a><h2 title={workspace.name} data-long={workspace.name.length > 40}>{workspace.name}</h2><p>Local developer workspace</p>
          {workspace.name.length > 40 && <details className={styles.projectName}><summary>Full project name</summary><p>{workspace.name}</p></details>}
        </header>
        <AgentScene chats={chats ?? []} client={client} loading={loading} unavailable={!!error} onSelect={chooseClient} />
        <button type="button" className={styles.refresh} disabled={loading} onClick={() => { setLoading(true); setRefresh(v => v + 1); }}><ArrowsClockwise size={15} aria-hidden="true"/>{loading ? "Refreshing…" : "Refresh chats"}</button>
        {error && <p role="alert" className={styles.error}>{error}</p>}
      </div>
      <aside ref={inspector} className={styles.inspector} aria-label="Conversation" data-conversation-panel data-empty={!selected} aria-busy={loading}>
        <div className={styles.destination} aria-label="Message destination">
          <label><span>01 · Assistant</span><select aria-label="Assistant" value={client} onChange={event => chooseClient(event.target.value)}>{providers.map(provider => <option key={provider.client} value={provider.client}>{provider.name}</option>)}</select></label>
          <label><span>02 · Project</span><select aria-label="Project for conversation" value={workspace.id} onChange={event => { const project = projects.find(item => item.id === event.target.value); if (project && project.id !== workspace.id) onProjectChange(project); }}>{projects.map(project => <option key={project.id} value={project.id}>{project.name} · {project.id.slice(0,8)}</option>)}</select></label>
          <ChatPicker key={client} chats={availableChats} selected={selected} disabled={loading || !!error || !availableChats.length} onSelect={setTarget} workspace={workspace} />
        </div>
        {selected ? <>
          <header className={styles.inspectorHeading}><span className={styles.avatar} data-client={selected.client}>{selected.client.slice(0,1).toUpperCase()}</span><div><h3 title={chatTitle(selected)}>{chatTitle(selected)}</h3><p>{workspace.name} · {selected.agent_name}</p></div>
            <details><summary aria-label="Chat details"><DotsThree size={24} aria-hidden="true"/></summary><dl><dt>Folder</dt><dd>{selected.cwd || "Unavailable"}</dd><dt>Chat ID</dt><dd>{selected.id}</dd><dt>Last registered</dt><dd>{seenAt(selected.last_seen)}</dd></dl></details>
          </header>
          <div className={transcriptStyles.views} role="group" aria-label="Conversation view">
            <button type="button" aria-pressed={conversationView === "native"} onClick={() => setConversationView("native")}>Native conversation</button>
            <button type="button" aria-pressed={conversationView === "aidrive"} onClick={() => setConversationView("aidrive")}>AIDrive messages</button>
          </div>
          {conversationView === "native" ? <SessionTranscript key={JSON.stringify([workspace.id, selected.id])} workspaceId={workspace.id} chatId={selected.id} /> : <>
          {messageError && <p role="alert">{messageError}</p>}
          <div className={styles.transcript} aria-label="Messages in selected chat" role="log" aria-live="polite" aria-relevant="additions">
            {!messageError && conversation.length === 0 ? <div className={styles.empty}><h4>No recent messages</h4><p>No messages for this chat appear in the latest 50 workspace messages. Send a request, or open Conversations for the workspace inbox.</p><a href="#panel-messages">Open conversations</a></div> :
              conversation.map(message => <article key={message.id} className={message.from_chat === selected.id ? styles.received : styles.sent}>
                <span className={styles.messageAvatar} data-agent={!!message.from_agent_name}>{(message.from_agent_name || message.from_email || "?").slice(0,1).toUpperCase()}</span><header><strong>{message.from_agent_name || (currentEmail && message.from_email === currentEmail ? "You" : message.from_email || "Unknown sender")}</strong><time dateTime={message.created_at} title={seenAt(message.created_at)}>{messageTime(message.created_at)}</time></header><p>{message.body}</p>{message.sources && message.sources.length > 0 && <ConversationEvidence workspaceId={workspace.id} agentId={selected.agent_id} sources={message.sources} showHandoffs={false} />}{message.id === latestAssistantMessage && <ConversationEvidence workspaceId={workspace.id} agentId={selected.agent_id} />}
              </article>)}
            {!latestAssistantMessage && <ConversationEvidence key={selected.agent_id} workspaceId={workspace.id} agentId={selected.agent_id} />}
          </div>
          </>}
          {canWrite && <form className={styles.composer} onSubmit={event => { event.preventDefault(); void send(); }}>
            <label className={styles.messageLabel} htmlFor={"message-" + workspace.id}>Message to {selected.agent_name}</label>
            <textarea id={"message-" + workspace.id} value={draft.body} rows={2} maxLength={8000} disabled={draft.busy} onChange={event => updateDraft(draftKey, { body: event.target.value })} placeholder={"Ask " + selected.agent_name + "…"} />
            <p className={styles.recipient} title={workspace.name + " · " + selected.id}>To {agentLabel(client)} · {workspace.name} · chat {selected.id.slice(0,8)}</p>
            <div className={styles.composerFooter}><small>Only this chat</small><button type="submit" disabled={!verified || draft.busy || !draft.body.trim()}><PaperPlaneTilt size={19} aria-hidden="true"/>{draft.busy ? "Sending…" : draft.failed ? "Retry message" : "Send to " + selected.agent_name}</button></div>
            {draft.notice && <p role={draft.failed ? "alert" : "status"}>{draft.notice}</p>}
            {draft.notice.includes("Your session ended") && <a href="#account">Renew your session without reloading this page</a>}
          </form>}
        </> : <div className={styles.empty}><span className={styles.emptyIcon}><ChatsCircle size={32} aria-hidden="true"/></span><h3>{loading ? "Loading chats…" : error ? "Chat registry unavailable" : availableChats.length ? "Choose a " + agentLabel(client) + " chat" : "Connect " + agentLabel(client)}</h3><p>{loading ? "Checking the chats registered in this project." : error ? "Refresh the registry to verify your recipient before writing." : availableChats.length ? "Pick the session you want by its name, folder and chat ID above." : "No " + agentLabel(client) + " chat is registered in this project. Choose another project above, or open the assistant in this project and refresh."}</p>
          {availableChats.length && !error ? <button type="button" onClick={() => document.getElementById("chat-choice-" + workspace.id)?.focus()}>Choose a chat</button> : <button type="button" disabled={loading} onClick={() => { setLoading(true); setRefresh(value => value + 1); }}><ArrowsClockwise size={16} aria-hidden="true"/>Refresh chats</button>}
          {workspace.role === "owner" && <a href="#spaces" onClick={() => requestAnimationFrame(() => document.getElementById("main-content")?.focus())}>Project access</a>}
        </div>}
      </aside>
    </div>
  </section>;
}

export default function WorkspaceConsole(props: ConsoleProps) {
  const [client, setClient] = useState("codex");
  return <Console key={props.workspace.id} {...props} client={client} onClientChange={setClient} />;
}
