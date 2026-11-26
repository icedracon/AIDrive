"use client";

import { useRef, useState } from "react";
import styles from "./chat-picker.module.css";

export type Capacity = { state: string; observed_at: string | null; reset_at: string | null; notice: string | null; evidence_kind: string | null; requires_recheck: boolean };
export type ChatChoice = { id: string; agent_id: string; agent_name: string; client: string; session_id: string; title?: string | null; capacity?: Capacity; cwd: string | null; first_seen: string; last_seen: string };

export function chatTitle(chat: ChatChoice) {
  return chat.title?.trim() || chat.cwd?.split(/[\\/]/).filter(Boolean).at(-1) || chat.agent_name;
}

function isProcess(chat: ChatChoice) {
  return /^mcp-\d+-\d+$/.test(chat.session_id);
}

function date(value: string) {
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? "Date unavailable" : parsed.toLocaleString();
}

export default function ChatPicker({ chats, selected, disabled, onSelect, workspace }: { chats: ChatChoice[]; selected?: ChatChoice; disabled: boolean; onSelect: (id: string) => void; workspace: { id: string; name: string } }) {
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState("");
  const menu = useRef<HTMLDetailsElement>(null);
  const trigger = useRef<HTMLElement>(null);
  const term = query.trim().toLocaleLowerCase();
  const filtered = chats.filter(chat => [chatTitle(chat), chat.agent_name, chat.session_id, chat.id, chat.cwd ?? ""].some(value => value.toLocaleLowerCase().includes(term)));

  async function copy(value: string, label: string) {
    try { await navigator.clipboard.writeText(value); setNotice(label + " copied."); }
    catch { setNotice("Copy unavailable. Select the ID below and copy it manually."); }
  }

  return <div className={styles.picker}>
    <span className={styles.label}>03 · Session</span>
    <details ref={menu} className={styles.menu} onKeyDown={event => { if (event.key === "Escape" && menu.current) { menu.current.open = false; trigger.current?.focus(); } }}>
      <summary ref={trigger} id={"chat-choice-" + workspace.id} aria-label="Choose a session" aria-disabled={disabled} onClick={event => { if (disabled) event.preventDefault(); }}>
        <span><strong>{selected ? chatTitle(selected) : disabled ? "Sessions unavailable" : "Choose a specific session"}</strong><small>{selected ? selected.agent_name + " · " + date(selected.first_seen) : chats.length + " registered sessions"}</small></span><span aria-hidden="true">⌄</span>
      </summary>
      {!disabled && <div className={styles.options}>
        <label className={styles.search}>Find a session<input type="search" value={query} onChange={event => setQuery(event.target.value)} placeholder="Name, session ID or folder" /></label>
        <p className={styles.count}>{filtered.length} of {chats.length} sessions</p>
        <ul aria-label="Available sessions">{filtered.map(chat => <li key={chat.id}>
          <button type="button" aria-pressed={selected?.id === chat.id} aria-label={"Select session " + chatTitle(chat) + " · " + chat.session_id} onClick={() => { setNotice(""); onSelect(chat.id); if (menu.current) menu.current.open = false; trigger.current?.focus(); }}>
            <span className={styles.name}><strong>{chatTitle(chat)}</strong><small>{chat.agent_name} · Registered {date(chat.first_seen)}</small>{chat.capacity?.state === "limited" && <small>Limit notice captured</small>}{chat.capacity?.state === "reset_unverified" && <small>Limit status needs recheck</small>}</span>
            <span className={styles.number}><small>{isProcess(chat) ? "MCP CONNECTION ID" : "SESSION ID"}</small><code>{chat.session_id}</code></span>
            {chat.cwd && <span className={styles.folder} title={chat.cwd}>{chat.cwd}</span>}
          </button>
        </li>)}</ul>
        {filtered.length === 0 && <p className={styles.count}>No matching sessions. Try a name, full ID or folder.</p>}
      </div>}
    </details>
    {selected && <div className={styles.identity}>
      <div className={styles.capacity} role="status"><strong>{selected.capacity?.state === "limited" ? "Usage limit notice recorded" : selected.capacity?.state === "reset_unverified" ? "Usage limit needs recheck" : "Usage capacity unknown"}</strong>
        {selected.capacity?.observed_at && <span>Observed {date(selected.capacity.observed_at)}</span>}
        {selected.capacity?.reset_at && <span>Reported reset {date(selected.capacity.reset_at)} · availability is not confirmed</span>}
        {selected.capacity?.notice && <details><summary>Captured notice</summary><p>{selected.capacity.notice}</p><p>Evidence from captured session text, not live account quota. Another chat on the same account may share this limit.</p></details>}
      </div>
      <div><label htmlFor={"native-session-" + selected.id}>{isProcess(selected) ? "MCP connection ID" : "Native session ID"}</label><button type="button" onClick={() => void copy(selected.session_id, "Session ID")}>Copy ID</button></div>
      <input id={"native-session-" + selected.id} aria-label={isProcess(selected) ? "MCP connection ID" : "Native session ID"} readOnly value={selected.session_id} onFocus={event => event.target.select()} />
      {isProcess(selected) && <p>This is an older MCP process registration. A native conversation ID was not recorded; this connection may no longer be active.</p>}
      <details><summary>Exact address for an AI assistant</summary><p>Names help you choose. Messages use this exact AIDrive chat ID; the native session ID identifies the conversation in its client.</p><dl><dt>Project</dt><dd>{workspace.name}</dd><dt>AIDrive chat ID · to_chat</dt><dd><code>{selected.id}</code></dd><dt>Assistant key · to</dt><dd>{selected.agent_name}</dd></dl>
        <button type="button" onClick={() => void copy(JSON.stringify({ workspace_id: workspace.id, native_session_id: isProcess(selected) ? null : selected.session_id, connection_id: isProcess(selected) ? selected.session_id : null, client: selected.client, send_message: { to: selected.agent_name, to_chat: selected.id } }, null, 2), "AI address")}>Copy AI address</button>
      </details>
      {notice && <p role="status">{notice}</p>}
    </div>}
  </div>;
}
