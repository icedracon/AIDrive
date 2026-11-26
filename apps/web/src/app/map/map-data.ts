export type FileSummary = { id: string; name: string; current_version_id: string; byte_size: number };
export type FileVersionItem = { id: string; revision: number; actor_id: string; agent_id: string | null; created_at: string; operation: string };
export type MapFile = FileSummary & { versions: FileVersionItem[]; moreVersions: boolean; versionsFailed: boolean };
export type Evidence = { file_id: string | null; file_name: string | null; version_id: string | null; session_id: string | null; handoff_id: string | null; quote: string | null; stale: boolean };
export type Knowledge = {
  id: string; kind: string; topic: string | null; statement: string; outcome: string | null; status: string;
  supersedes: string | null; superseded_by: string | null; actor_id: string; agent_id: string | null;
  created_at: string; decided_at: string | null; visibility: "private" | "team";
  conflicts: string[]; disputed_by: string[]; stale: boolean; evidence: Evidence[];
};
export type HandoffSource = { file_id: string; name: string; version_id: string; stale: boolean };
export type Handoff = { id: string; status: string; summary: string; next_step: string; agent_id: string | null; created_at: string; decided_at: string | null; stale: boolean; sources: HandoffSource[] };
export type Session = { id: string; client: string; label: string; agent_id: string | null; started_at: string | null; ended_at: string | null; event_count: number; visibility: "private" | "team"; mine: boolean };
export type MessageSource = { file_id: string; file_name: string | null; version_id: string; stale: boolean };
export type Message = {
  id: string; thread_id: string; reply_to: string | null; from_email: string | null; from_agent_id: string | null; from_agent_name: string | null;
  to: string; to_agent_name: string | null; subject: string | null; body: string; created_at: string; sources: MessageSource[];
};
export type ActivityEvent = { at: string; kind: string; actor_email: string | null; agent_id: string | null; agent_name: string | null; subject: string | null; file_id: string | null; revision: number | null; handoff_id: string | null };

export type MapData = {
  files: MapFile[];
  filesTruncated: boolean;
  knowledge: Knowledge[];
  handoffs: Handoff[];
  currentHandoffId: string | null;
  sessions: Session[];
  messages: Message[];
  activity: ActivityEvent[];
  activityTruncated: boolean;
  agentNames: Map<string, string>;
  partial: string[];
};

const FILE_PAGES = 10;
const ACTIVITY_PAGES = 5;
const VERSION_CONCURRENCY = 6;

async function getJson(url: string, signal: AbortSignal) {
  const response = await fetch(url, { cache: "no-store", signal });
  const body = await response.json().catch(() => null);
  if (!response.ok || body === null) throw new Error(`${url}: ${body?.error ?? response.status}`);
  return body;
}

async function pool<T, R>(items: T[], limit: number, run: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (next < items.length) { const i = next++; out[i] = await run(items[i]); }
  }));
  return out;
}

export async function loadMap(workspaceId: string, owner: boolean, signal: AbortSignal): Promise<MapData> {
  const base = `/api/workspaces/${workspaceId}`;
  const partial: string[] = [];
  const optional = <T,>(label: string, p: Promise<T>, fallback: T) => p.catch((err) => {
    if (signal.aborted) throw err;
    partial.push(label);
    return fallback;
  });

  const filesP = (async () => {
    const files: FileSummary[] = [];
    let after: string | null = null;
    for (let page = 0; page < FILE_PAGES; page++) {
      const body = await getJson(after ? `${base}/files?after=${after}` : `${base}/files`, signal);
      files.push(...body.files);
      after = body.next_after ?? null;
      if (!after) break;
    }
    return { files, truncated: after !== null };
  })();

  const activityP = (async () => {
    const events: ActivityEvent[] = [];
    let before: string | null = null;
    for (let page = 0; page < ACTIVITY_PAGES; page++) {
      const body = await getJson(`${base}/activity?limit=100${before ? `&before=${encodeURIComponent(before)}` : ""}`, signal);
      events.push(...body.events);
      before = body.next_before ?? null;
      if (!before) break;
    }
    return { events, truncated: before !== null };
  })();

  const [filesRes, knowledge, handoffs, latest, sessions, messages, activity, grants] = await Promise.all([
    filesP,
    optional("knowledge", getJson(`${base}/knowledge?status=all&limit=200`, signal).then((b) => b.items as Knowledge[]), []),
    optional("handoffs", getJson(`${base}/handoffs`, signal).then((b) => b.handoffs as Handoff[]), []),
    optional("latest handoff", getJson(`${base}/handoffs/latest`, signal).then((b) => (b.approved?.id as string) ?? null), null),
    optional("sessions", getJson(`${base}/sessions?limit=100`, signal).then((b) => (Array.isArray(b) ? b : b.sessions) as Session[]), []),
    optional("messages", getJson(`${base}/messages?limit=100`, signal).then((b) => b.messages as Message[]), []),
    optional("activity", activityP, { events: [] as ActivityEvent[], truncated: false }),
    owner ? getJson(`${base}/agents`, signal).then((b) => (b.grants ?? []) as { id: string; name: string }[]).catch(() => []) : Promise.resolve([]),
  ]);

  let versionFailures = 0;
  const files = await pool(filesRes.files, VERSION_CONCURRENCY, async (f): Promise<MapFile> => {
    try {
      const body = await getJson(`${base}/files/${f.id}/versions`, signal);
      return { ...f, versions: body.versions, moreVersions: body.next_before !== null, versionsFailed: false };
    } catch (err) {
      if (signal.aborted) throw err;
      versionFailures++;
      return { ...f, versions: [], moreVersions: false, versionsFailed: true };
    }
  });
  if (versionFailures) partial.push(`version history of ${versionFailures} file${versionFailures === 1 ? "" : "s"}`);

  const agentNames = new Map<string, string>();
  for (const g of grants) agentNames.set(g.id, g.name);
  for (const e of activity.events) if (e.agent_id && e.agent_name) agentNames.set(e.agent_id, e.agent_name);
  for (const m of messages) if (m.from_agent_id && m.from_agent_name) agentNames.set(m.from_agent_id, m.from_agent_name);

  return {
    files, filesTruncated: filesRes.truncated, knowledge, handoffs, currentHandoffId: latest, sessions, messages,
    activity: activity.events, activityTruncated: activity.truncated, agentNames, partial,
  };
}

export function agentLabel(data: MapData, agentId: string | null, fallback = "Person") {
  if (!agentId) return fallback;
  return data.agentNames.get(agentId) ?? `Assistant ${agentId.slice(0, 8)}`;
}

export function shortTime(iso: string | null) {
  return iso ? `${iso.replace("T", " ").slice(0, 16)} UTC` : "unknown time";
}

export function clip(text: string, n: number) {
  const one = text.replace(/\s+/g, " ").trim();
  return one.length > n ? `${one.slice(0, n - 1)}…` : one;
}

export type NodeType = "file" | "knowledge" | "handoff" | "session" | "message";
export const NODE_TYPES: NodeType[] = ["file", "knowledge", "handoff", "session", "message"];
export const TYPE_LABEL: Record<NodeType, string> = { file: "Files", knowledge: "Knowledge", handoff: "Handoffs", session: "Sessions", message: "Messages" };
export const PANEL_ANCHOR: Record<NodeType, string> = { file: "files", knowledge: "knowledge", handoff: "handoffs", session: "sessions", message: "messages" };

export type GraphNode = { id: string; type: NodeType; label: string; detail: string[]; flagged: boolean; dim: boolean };
export type GraphEdge = { source: string; target: string; kind: "evidence" | "supersedes" | "source" | "reply" };

export function nodeId(type: NodeType, id: string) {
  return `${type[0]}:${id}`;
}

export function buildGraph(data: MapData): { nodes: GraphNode[]; edges: GraphEdge[] } {
  const nodes: GraphNode[] = [];
  const known = new Set<string>();
  const add = (n: GraphNode) => { nodes.push(n); known.add(n.id); };

  for (const f of data.files) {
    const latest = f.versions[0];
    add({ id: nodeId("file", f.id), type: "file", label: f.name, flagged: false, dim: false, detail: [
      `${f.versions.length}${f.moreVersions ? "+" : ""} versions · ${f.byte_size} bytes`,
      latest ? `Latest r${latest.revision} by ${agentLabel(data, latest.agent_id)} · ${shortTime(latest.created_at)}` : "No history loaded",
    ] });
  }
  for (const k of data.knowledge) {
    const disputed = k.disputed_by.length > 0;
    add({ id: nodeId("knowledge", k.id), type: "knowledge", label: clip(k.topic ? `${k.topic}: ${k.statement}` : k.statement, 48),
      flagged: k.stale || disputed, dim: k.status !== "approved" && k.status !== "proposed", detail: [
        `${k.kind} · ${k.status}${k.stale ? " · stale" : ""}${disputed ? " · disputed" : ""} · ${k.visibility}`,
        clip(k.statement, 220),
        `By ${agentLabel(data, k.agent_id)} · ${shortTime(k.created_at)}`,
      ] });
  }
  for (const h of data.handoffs) {
    add({ id: nodeId("handoff", h.id), type: "handoff", label: clip(h.summary, 48), flagged: h.stale,
      dim: h.status === "rejected" || h.status === "superseded", detail: [
        `${h.status}${h.id === data.currentHandoffId ? " · current" : ""}${h.stale ? " · sources changed" : ""}`,
        clip(h.summary, 220), `Next: ${clip(h.next_step, 160)}`, `By ${agentLabel(data, h.agent_id)} · ${shortTime(h.created_at)}`,
      ] });
  }
  for (const s of data.sessions) {
    add({ id: nodeId("session", s.id), type: "session", label: clip(s.label || s.client, 40), flagged: false, dim: false, detail: [
      `${s.client} · ${s.event_count} events · ${s.visibility}`, `Started ${shortTime(s.started_at)}`,
    ] });
  }
  for (const m of data.messages) {
    add({ id: nodeId("message", m.id), type: "message", label: clip(m.subject || m.body, 40), flagged: m.sources.some((s) => s.stale), dim: false, detail: [
      `${m.from_agent_name ?? m.from_email ?? "Unknown"} → ${m.to === "agent" ? m.to_agent_name ?? "assistant" : m.to}`,
      clip(m.body, 220), shortTime(m.created_at),
    ] });
  }

  const edges: GraphEdge[] = [];
  const seen = new Set<string>();
  const link = (source: string, target: string, kind: GraphEdge["kind"]) => {
    const key = `${source}|${target}`;
    if (source === target || !known.has(source) || !known.has(target) || seen.has(key)) return;
    seen.add(key);
    edges.push({ source, target, kind });
  };
  for (const k of data.knowledge) {
    const from = nodeId("knowledge", k.id);
    for (const e of k.evidence) {
      if (e.file_id) link(from, nodeId("file", e.file_id), "evidence");
      if (e.session_id) link(from, nodeId("session", e.session_id), "evidence");
      if (e.handoff_id) link(from, nodeId("handoff", e.handoff_id), "evidence");
    }
    if (k.supersedes) link(from, nodeId("knowledge", k.supersedes), "supersedes");
  }
  for (const h of data.handoffs) for (const s of h.sources) link(nodeId("handoff", h.id), nodeId("file", s.file_id), "source");
  for (const m of data.messages) {
    if (m.reply_to) link(nodeId("message", m.id), nodeId("message", m.reply_to), "reply");
    for (const s of m.sources) link(nodeId("message", m.id), nodeId("file", s.file_id), "source");
  }
  return { nodes, edges };
}
