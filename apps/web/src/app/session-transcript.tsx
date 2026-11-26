"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import styles from "./session-transcript.module.css";

type Cursor = { ordinal: number; key: string };
type Event = Cursor & { role: string; at: string | null; text: string };
type Session = { id: string; client: string; label: string; coverage: string; event_count: number; started_at: string | null; ended_at: string | null };
type Page = { session: Session; events: Event[]; next_cursor: Cursor | null };
type State = { page: Page | null; busy: boolean; missing: boolean; error: string };

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object";
}

function isCursor(value: unknown): value is Cursor {
  return record(value) && Number.isSafeInteger(value.ordinal) && typeof value.ordinal === "number" && value.ordinal >= 0 && typeof value.key === "string" && value.key.length > 0;
}

function nullableText(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isPage(value: unknown): value is Page {
  if (!record(value) || !record(value.session)) return false;
  const session = value.session;
  return ["id", "client", "label", "coverage"].every(key => typeof session[key] === "string")
    && Number.isSafeInteger(session.event_count) && (session.event_count as number) >= 0
    && nullableText(session.started_at) && nullableText(session.ended_at)
    && Array.isArray(value.events) && value.events.every(event => record(event)
      && typeof event.role === "string" && typeof event.text === "string" && nullableText(event.at) && isCursor(event))
    && (value.next_cursor === null || isCursor(value.next_cursor));
}

function when(value: string | null) {
  if (!value) return "Unavailable";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "Unavailable" : date.toLocaleString();
}

export default function SessionTranscript({ workspaceId, chatId }: { workspaceId: string; chatId: string }) {
  const [state, setState] = useState<State>({ page: null, busy: true, missing: false, error: "" });
  const request = useRef<AbortController | null>(null);
  const current = useRef(state);
  const [checkedAt, setCheckedAt] = useState<string | null>(null);
  const base = "/api/workspaces/" + encodeURIComponent(workspaceId) + "/chats/" + encodeURIComponent(chatId) + "/transcript";

  const load = useCallback(async (previous: Page | null) => {
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    const cursor = previous?.next_cursor;
    const query = new URLSearchParams({ limit: "100" });
    if (cursor) {
      query.set("after_ordinal", String(cursor.ordinal));
      query.set("after_key", cursor.key);
    }
    try {
      const response = await fetch(base + "?" + query, { signal: controller.signal, cache: "no-store" });
      if (controller.signal.aborted) return;
      if (response.status === 404) {
        return { page: null, busy: false, missing: true, error: "", signal: controller.signal };
      }
      if (!response.ok) throw Error(response.status === 401 ? "Sign in again to read this capture." : response.status === 403 ? "You do not have permission to read this capture." : "The native conversation could not be loaded. Try again.");
      const result: unknown = await response.json();
      if (controller.signal.aborted) return;
      if (!isPage(result)) throw Error("The transcript response was invalid. Try refreshing.");
      if (previous && result.session.id !== previous.session.id) throw Error("The linked capture changed. Refresh to load it from the beginning.");
      if (cursor && result.next_cursor && (result.next_cursor.ordinal < cursor.ordinal || (result.next_cursor.ordinal === cursor.ordinal && result.next_cursor.key === cursor.key))) throw Error("The transcript cursor did not advance. Refresh to reload the capture.");
      const events = [...new Map([...(previous?.events ?? []), ...result.events].map(event => [event.key, event])).values()];
      return { page: { ...result, events }, busy: false, missing: false, error: "", signal: controller.signal };
    } catch (error) {
      if (!controller.signal.aborted) return { page: previous, busy: false, missing: false, error: error instanceof Error ? error.message : "The native conversation could not be loaded. Try again.", signal: controller.signal };
    }
  }, [base]);

  useEffect(() => {
    void load(null).then(result => { if (result && !result.signal.aborted) setState(result); });
    return () => request.current?.abort();
  }, [load]);

  useEffect(() => { current.current = state; }, [state]);

  useEffect(() => {
    let disposed = false;
    let pending = false;
    async function synchronize() {
      const region = document.getElementById("panel-overview");
      if (pending || current.current.busy || document.visibilityState !== "visible" || region?.hidden) return;
      pending = true;
      const controller = new AbortController();
      request.current?.abort();
      request.current = controller;
      const before = current.current.page;
      try {
        let page: Page | null = null;
        const collected: Event[] = [];
        const desired = Math.max(100, (before?.events.length ?? 0) + 100);
        do {
          if (document.visibilityState !== "visible" || document.getElementById("panel-overview")?.hidden) return;
          const query = new URLSearchParams({ limit: "100" });
          if (page?.next_cursor) {
            query.set("after_ordinal", String(page.next_cursor.ordinal));
            query.set("after_key", page.next_cursor.key);
          }
          const response = await fetch(base + "?" + query, { signal: controller.signal, cache: "no-store" });
          if (controller.signal.aborted || disposed) return;
          if (response.status === 404) { setState({ page: null, busy: false, missing: true, error: "" }); return; }
          if (!response.ok) throw Error("Automatic update failed. Previously loaded history is kept.");
          const result: unknown = await response.json();
          if (controller.signal.aborted || disposed) return;
          if (!isPage(result)) throw Error("Automatic update returned an invalid transcript.");
          if (page && page.session.id !== result.session.id) throw Error("The linked capture changed during update. Try again.");
          if (page?.next_cursor && result.next_cursor && page.next_cursor.ordinal === result.next_cursor.ordinal && page.next_cursor.key === result.next_cursor.key) throw Error("The transcript cursor did not advance.");
          collected.push(...result.events);
          page = { ...result, events: collected };
        } while (page.next_cursor && page.events.length < desired);
        if (!controller.signal.aborted && !disposed) {
          setState({ page, busy: false, missing: false, error: "" });
          setCheckedAt(new Date().toISOString());
        }
      } catch (error) {
        if (!controller.signal.aborted && !disposed) setState(previous => ({ ...previous, error: error instanceof Error ? error.message : "Automatic update failed." }));
      } finally { pending = false; }
    }
    const timer = window.setInterval(() => { void synchronize(); }, 5000);
    const visible = () => { if (document.visibilityState === "visible") void synchronize(); };
    document.addEventListener("visibilitychange", visible);
    return () => { disposed = true; clearInterval(timer); document.removeEventListener("visibilitychange", visible); request.current?.abort(); };
  }, [base]);

  function startLoad(previous: Page | null) {
    setState({ page: previous, busy: true, missing: false, error: "" });
    void load(previous).then(result => { if (result && !result.signal.aborted) setState(result); });
  }

  const { page, busy, missing, error } = state;
  return <section className={styles.transcript} aria-label="Native conversation">
    <div className={styles.toolbar}><strong>Native session capture</strong><span className={styles.scope}>Auto-updates while visible{checkedAt ? " · checked " + when(checkedAt) : ""}</span><button type="button" onClick={() => startLoad(null)}>Refresh transcript</button></div>
    <div className={styles.messages} aria-busy={busy}>
      <details className={styles.scope}><summary>Capture coverage and automatic updates</summary><p>Captured questions and answers are redacted before storage; reasoning and system instructions stay local. Older captures may contain shortened text until the original local transcript is captured again with an updated client. This open view checks for captured updates every five seconds and gradually loads the remaining history. It keeps your reading position and pauses when hidden. It does not wake the assistant.</p></details>
      {page && <header className={styles.metadata}>
        <h4>{page.session.label || "Native session"}</h4>
        <p>{page.session.client} · {page.events.length} of {page.session.event_count} captured events loaded · Capture scope: {page.session.coverage}</p>
        <details><summary>Linked capture details</summary><dl><dt>Session ID</dt><dd>{page.session.id}</dd><dt>Chat ID</dt><dd>{chatId}</dd><dt>Started</dt><dd>{when(page.session.started_at)}</dd><dt>Ended</dt><dd>{when(page.session.ended_at)}</dd></dl></details>
      </header>}
      {missing && <div className={styles.empty} role="status"><h4>No accessible native capture</h4><p>This chat has no accessible linked session capture. Refresh after a capture becomes available.</p></div>}
      {error && <div className={styles.error} role="alert"><p>{error}</p>{page && <p>Previously loaded events remain below.</p>}<button type="button" disabled={busy} onClick={() => startLoad(page)}>Retry {page ? "next messages" : "transcript"}</button></div>}
      {page?.events.map(event => event.role === "user" || event.role === "assistant"
        ? <article key={event.key} className={event.role === "user" ? styles.user : styles.assistant}><header><strong>{event.role === "user" ? "User" : "Assistant"}</strong><time dateTime={event.at ?? undefined}>{when(event.at)}</time></header><p>{event.text || "No captured text."}</p></article>
        : <details key={event.key} className={styles.tool}><summary>{event.role === "tool" ? "Tool event" : event.role + " event"} · {when(event.at)}</summary><pre>{event.text || "No captured text."}</pre></details>)}
      {busy && <p role="status">{page ? "Loading next messages…" : "Loading native conversation…"}</p>}
      {page && !busy && page.events.length === 0 && <p role="status">The linked capture contains no events yet.</p>}
      {page?.next_cursor && <button className={styles.next} type="button" disabled={busy} onClick={() => startLoad(page)}>Load next messages</button>}
      {page && !page.next_cursor && page.events.length > 0 && <p className={styles.scope}>End of currently available capture. New captured events appear automatically while this view is open.</p>}
    </div>
  </section>;
}
