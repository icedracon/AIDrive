"use client";

import { useEffect, useState } from "react";
import { ArrowRight, CaretDown, GitBranch } from "@phosphor-icons/react";
import styles from "./conversation-evidence.module.css";

export type ConversationEvidenceSource = {
  file_id: string;
  version_id: string;
  file_name?: string | null;
  name?: string;
  current_version_id: string | null;
  stale: boolean;
};

export type ConversationEvidenceProps = {
  workspaceId: string;
  agentId: string;
  sources?: readonly ConversationEvidenceSource[];
  showHandoffs?: boolean;
};

type Handoff = {
  id: string;
  agent_id: string | null;
  status: "proposed" | "approved" | "rejected" | "superseded";
  summary: string;
  decisions: string[];
  next_step: string;
  created_at: string;
  stale: boolean;
  auto_accepted: boolean;
  owner_checked: boolean;
  undone_at: string | null;
  decision_note: string | null;
  review_reasons: string[];
  sources: ConversationEvidenceSource[];
};

type Revision = {
  file_id: string;
  version_id: string;
  name: string;
  revision: number;
  content: string;
  current_version_id: string;
};

type Load<T> = { state: "loading" } | { state: "error"; message: string } | { state: "ready"; value: T };

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isUuid(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
}

function isSource(value: unknown): value is ConversationEvidenceSource {
  return record(value) && isUuid(value.file_id) && isUuid(value.version_id)
    && (value.current_version_id === null || isUuid(value.current_version_id))
    && typeof value.stale === "boolean"
    && (value.file_name == null || typeof value.file_name === "string")
    && (value.name === undefined || typeof value.name === "string");
}

function strings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every(item => typeof item === "string");
}

function isHandoff(value: unknown): value is Handoff {
  return record(value) && typeof value.id === "string"
    && (value.agent_id === null || typeof value.agent_id === "string")
    && ["proposed", "approved", "rejected", "superseded"].includes(String(value.status))
    && ["summary", "next_step", "created_at"].every(key => typeof value[key] === "string")
    && ["stale", "auto_accepted", "owner_checked"].every(key => typeof value[key] === "boolean")
    && (value.undone_at === null || typeof value.undone_at === "string")
    && (value.decision_note === null || typeof value.decision_note === "string")
    && strings(value.decisions) && strings(value.review_reasons)
    && Array.isArray(value.sources) && value.sources.every(isSource);
}

function isRevision(value: unknown): value is Revision {
  return record(value) && ["file_id", "version_id", "name", "content", "current_version_id"].every(key => typeof value[key] === "string")
    && typeof value.revision === "number" && Number.isSafeInteger(value.revision) && value.revision > 0;
}

function loadError(status: number, subject: string) {
  if (status === 401) return "Your session ended. Sign in again to view evidence.";
  if (status === 403) return `You no longer have access to ${subject}.`;
  if (status === 404) return `${subject} is unavailable or has been removed.`;
  return `${subject} could not be loaded. Try again.`;
}

function RevisionContent({ endpoint, source }: { endpoint: string; source: ConversationEvidenceSource }) {
  const [result, setResult] = useState<Load<Revision>>({ state: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    async function load() {
      try {
        const response = await fetch(endpoint, { cache: "no-store", credentials: "same-origin", mode: "same-origin", redirect: "error", signal: controller.signal });
        if (!response.ok) throw new Error(loadError(response.status, "The cited revision"));
        const body: unknown = await response.json();
        if (!isRevision(body) || body.file_id !== source.file_id || body.version_id !== source.version_id) throw new Error("The server did not return the cited revision. Try again.");
        if (!controller.signal.aborted) setResult({ state: "ready", value: body });
      } catch (error) {
        if (!controller.signal.aborted) setResult({ state: "error", message: error instanceof Error ? error.message : "The cited revision could not be loaded." });
      }
    }
    void load();
    return () => controller.abort();
  }, [endpoint, source.file_id, source.version_id, attempt]);

  if (result.state === "loading") return <div className={styles.content} role="status">Loading cited file content…</div>;
  if (result.state === "error") return <div className={styles.content}><p className={styles.error} role="alert">{result.message}</p><button type="button" className={styles.button} onClick={() => { setResult({ state: "loading" }); setAttempt(value => value + 1); }}>Retry source</button></div>;

  const revision = result.value;
  return <div className={styles.content}>
    <div className={styles.metadata}><span title={`Cited version ${revision.version_id}`}>Revision {revision.revision}</span><span className={revision.current_version_id !== revision.version_id ? styles.warning : undefined}>{revision.current_version_id !== revision.version_id ? "File changed since this citation" : "Matched current file when loaded"}</span></div>
    <pre className={styles.code} tabIndex={0} aria-label={`Content of ${revision.name}, revision ${revision.revision}`}><code>{revision.content || "(Empty file)"}</code></pre>
    <a className={styles.download} href={`${endpoint}/download`} title={`Download cited version ${revision.version_id}`}>Download cited revision</a>
  </div>;
}

function SourceCard({ workspaceId, source }: { workspaceId: string; source: ConversationEvidenceSource }) {
  const [open, setOpen] = useState(false);
  if (!isUuid(workspaceId) || !isSource(source)) return <p className={styles.error} role="alert">The source citation is invalid. Content was not requested.</p>;
  const endpoint = `/api/workspaces/${encodeURIComponent(workspaceId)}/files/${encodeURIComponent(source.file_id)}/versions/${encodeURIComponent(source.version_id)}`;
  return <details className={styles.source} onToggle={event => setOpen(event.currentTarget.open)}>
    <summary className={styles.sourceHeading}>
      <GitBranch className={styles.icon} size={18} aria-hidden="true" /><strong>Source revision</strong><span className={styles.fileName}>{source.file_name || source.name || "Cited file"}</span>
      <span className={source.stale ? styles.warning : styles.muted}>{source.stale ? "Changed since cited" : "Cited version"}</span><CaretDown className={styles.chevron} size={14} aria-hidden="true" />
    </summary>
    {open && <RevisionContent endpoint={endpoint} source={source} />}
  </details>;
}

const reasons: Record<string, string> = {
  strict_review: "Strict review requires a decision",
  stale_sources: "A cited file changed",
  no_sources: "No file versions cited",
  stale_evidence: "Cited evidence changed",
  no_evidence: "No evidence provided",
};

function HandoffCard({ handoff, workspaceId }: { handoff: Handoff; workspaceId: string }) {
  const date = new Date(handoff.created_at);
  const status = { proposed: "Proposed", approved: "Approved", rejected: "Rejected", superseded: "Superseded" }[handoff.status];
  return <article className={styles.handoff} aria-label="Assistant handoff">
    <header className={styles.handoffHeading}>
      <ArrowRight className={styles.icon} size={18} aria-hidden="true" /><strong>Handoff</strong><span className={styles.badge}>{status}</span>
      {handoff.auto_accepted && <span className={styles.acceptance}>{handoff.owner_checked ? "Auto-accepted · checked by owner" : "Auto-accepted · not checked by owner"}</span>}
    </header>
    <div className={styles.handoffBody}>
      <span className={handoff.stale ? styles.warning : styles.muted}>{handoff.stale ? "Sources changed since recorded" : handoff.sources.length ? "Cited sources unchanged" : "No cited sources"}</span>
      {handoff.undone_at && <p className={styles.warning}>Acceptance undone</p>}
      <p className={styles.text}>{handoff.next_step}</p>
      <details className={styles.decisionDetails}>
        <summary className={styles.decisionSummary}>Evidence and decision<CaretDown className={styles.chevron} size={14} aria-hidden="true" /></summary>
        <div className={styles.decisionBody}>
          <div className={styles.metadata}><span>Recorded</span><time dateTime={handoff.created_at}>{Number.isNaN(date.getTime()) ? "Date unavailable" : date.toLocaleString()}</time></div>
          <h4 className={styles.label}>Summary</h4><p className={styles.text}>{handoff.summary}</p>
          {handoff.decisions.length > 0 && <><h4 className={styles.label}>Proposed decisions</h4><ul className={styles.decisions}>{handoff.decisions.map((decision, index) => <li key={index}>{decision}</li>)}</ul></>}
          {handoff.status === "proposed" && handoff.review_reasons.length > 0 && <p className={styles.note}>Waiting: {handoff.review_reasons.map(reason => reasons[reason] ?? reason.replaceAll("_", " ")).join("; ")}.</p>}
          {handoff.decision_note && <p className={styles.note}>Decision note: {handoff.decision_note}</p>}
          {handoff.sources.length > 0 && <div className={styles.sources}>{handoff.sources.map(source => <SourceCard key={`${source.file_id}:${source.version_id}`} workspaceId={workspaceId} source={source} />)}</div>}
        </div>
      </details>
    </div>
  </article>;
}

function AssistantHandoffs({ workspaceId, agentId }: { workspaceId: string; agentId: string }) {
  const [result, setResult] = useState<Load<Handoff[]>>({ state: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    let pending = false;
    async function load() {
      if (pending) return;
      pending = true;
      try {
        const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/handoffs`, { cache: "no-store", credentials: "same-origin", mode: "same-origin", redirect: "error", signal: controller.signal });
        if (!response.ok) throw new Error(loadError(response.status, "Handoffs"));
        const body: unknown = await response.json();
        if (!record(body) || !Array.isArray(body.handoffs) || !body.handoffs.every(isHandoff)) throw new Error("The server returned invalid handoff evidence. Try again.");
        if (!controller.signal.aborted) setResult({ state: "ready", value: body.handoffs.filter(handoff => handoff.agent_id === agentId) });
      } catch (error) {
        if (!controller.signal.aborted) setResult({ state: "error", message: error instanceof Error ? error.message : "Handoffs could not be loaded." });
      } finally { pending = false; }
    }
    void load();
    const refresh = () => { if (document.visibilityState === "visible") void load(); };
    const timer = window.setInterval(refresh, 15000);
    window.addEventListener("aidrive:review-changed", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      controller.abort();
      window.clearInterval(timer);
      window.removeEventListener("aidrive:review-changed", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, [workspaceId, agentId, attempt]);

  return <section className={styles.handoffs} aria-label="Selected assistant handoff evidence">
    <div className={styles.sectionHeading}><span>Handoffs</span><button className={styles.button} type="button" onClick={() => { setResult({ state: "loading" }); setAttempt(value => value + 1); }}>Refresh</button></div>
    <p className={styles.scope}>This assistant, across chats · latest 50 workspace handoffs</p>
    {result.state === "loading" ? <p className={styles.note} role="status">Loading handoff evidence…</p>
      : result.state === "error" ? <p className={styles.error} role="alert">{result.message}</p>
      : result.value.length === 0 ? <p className={styles.note}>None by this assistant in these records.</p>
      : result.value.map(handoff => <HandoffCard key={handoff.id} workspaceId={workspaceId} handoff={handoff} />)}
  </section>;
}

export default function ConversationEvidence({ workspaceId, agentId, sources = [], showHandoffs = true }: ConversationEvidenceProps) {
  if (!workspaceId || !agentId) return null;
  if (!isUuid(workspaceId) || !isUuid(agentId)) return <p className={styles.error} role="alert">The evidence selection is invalid. Content was not requested.</p>;
  const uniqueSources = sources.filter((source, index) => sources.findIndex(other => other.file_id === source.file_id && other.version_id === source.version_id) === index);
  return <div className={styles.evidence}>
    {uniqueSources.map(source => <SourceCard key={`${workspaceId}:${agentId}:${source.file_id}:${source.version_id}`} workspaceId={workspaceId} source={source} />)}
    {showHandoffs && <AssistantHandoffs key={`${workspaceId}:${agentId}`} workspaceId={workspaceId} agentId={agentId} />}
  </div>;
}
