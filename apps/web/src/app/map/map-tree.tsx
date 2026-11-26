"use client";

import { useMemo, useState } from "react";
import { agentLabel, clip, nodeId, PANEL_ANCHOR, shortTime, type MapData, type NodeType } from "./map-data";

type Badge = { text: string; tone: "ok" | "wait" | "warn" | "muted" | "info" };
type Leaf = { key: string; node: string | null; type: NodeType; title: string; meta: string; badges: Badge[] };
type Group = { key: string; title: string; type: NodeType | null; note?: string; leaves: Leaf[]; groups?: Group[] };

const KIND_ORDER = ["keystone", "constraint", "decision", "fact", "preference", "gotcha", "attempt"];
const KIND_LABEL: Record<string, string> = { keystone: "Rules", constraint: "Constraints", decision: "Decisions", fact: "Facts", preference: "Preferences", gotcha: "Gotchas", attempt: "Attempts" };
const STATUS_TONE: Record<string, Badge["tone"]> = { approved: "ok", proposed: "wait", superseded: "muted", retired: "muted", rejected: "muted" };

function buildTree(data: MapData): Group[] {
  const files: Leaf[] = [...data.files].sort((a, b) => a.name.localeCompare(b.name)).map((f) => {
    const latest = f.versions[0];
    return {
      key: f.id, node: nodeId("file", f.id), type: "file", title: f.name,
      meta: f.versionsFailed ? "history unavailable" : latest ? `r${latest.revision} · ${agentLabel(data, latest.agent_id)} · ${shortTime(latest.created_at)}` : "",
      badges: [{ text: `${f.versions.length}${f.moreVersions ? "+" : ""} version${f.versions.length === 1 && !f.moreVersions ? "" : "s"}`, tone: "info" }],
    };
  });

  const kinds = new Map<string, Leaf[]>();
  for (const k of data.knowledge) {
    const badges: Badge[] = [{ text: k.status, tone: STATUS_TONE[k.status] ?? "muted" }];
    if (k.stale) badges.push({ text: "stale", tone: "warn" });
    if (k.disputed_by.length) badges.push({ text: "disputed", tone: "warn" });
    if (k.conflicts.length) badges.push({ text: "conflicts", tone: "warn" });
    if (k.visibility === "private") badges.push({ text: "private", tone: "muted" });
    if (k.outcome) badges.push({ text: k.outcome, tone: "info" });
    const list = kinds.get(k.kind) ?? [];
    list.push({ key: k.id, node: nodeId("knowledge", k.id), type: "knowledge", title: clip(k.topic ? `${k.topic}: ${k.statement}` : k.statement, 120),
      meta: `${agentLabel(data, k.agent_id)} · ${shortTime(k.created_at)} · ${k.evidence.length} evidence`, badges });
    kinds.set(k.kind, list);
  }
  const kindGroups: Group[] = [...kinds.keys()]
    .sort((a, b) => (KIND_ORDER.indexOf(a) + 99) % 99 - (KIND_ORDER.indexOf(b) + 99) % 99)
    .map((kind) => ({ key: `k-${kind}`, title: KIND_LABEL[kind] ?? kind, type: "knowledge", leaves: kinds.get(kind)! }));

  const handoffs: Leaf[] = data.handoffs.map((h) => {
    const badges: Badge[] = [{ text: h.status, tone: STATUS_TONE[h.status] ?? "muted" }];
    if (h.id === data.currentHandoffId) badges.push({ text: "current", tone: "ok" });
    if (h.stale) badges.push({ text: "stale", tone: "warn" });
    return { key: h.id, node: nodeId("handoff", h.id), type: "handoff", title: clip(h.summary, 120),
      meta: `${agentLabel(data, h.agent_id)} · ${shortTime(h.created_at)} · ${h.sources.length} source${h.sources.length === 1 ? "" : "s"}`, badges };
  });

  const sessions: Leaf[] = data.sessions.map((s) => ({
    key: s.id, node: nodeId("session", s.id), type: "session", title: s.label || "(no label)",
    meta: `${s.client} · ${s.event_count} events · ${shortTime(s.started_at)}`,
    badges: [{ text: s.visibility === "team" ? "team" : "private", tone: s.visibility === "team" ? "ok" : "muted" }, { text: s.client, tone: "info" }],
  }));

  const threads = new Map<string, typeof data.messages>();
  for (const m of [...data.messages].reverse()) {
    const list = threads.get(m.thread_id) ?? [];
    list.push(m);
    threads.set(m.thread_id, list);
  }
  const threadGroups: Group[] = [...threads.entries()].map(([id, list]) => ({
    key: `t-${id}`, title: clip(list[0].subject || list[0].body, 80), type: "message",
    leaves: list.map((m) => ({
      key: m.id, node: nodeId("message", m.id), type: "message" as const, title: clip(m.body, 120),
      meta: `${m.from_agent_name ?? m.from_email ?? "Unknown"} → ${m.to === "agent" ? m.to_agent_name ?? "assistant" : m.to} · ${shortTime(m.created_at)}`,
      badges: m.sources.some((s) => s.stale) ? [{ text: "source changed", tone: "warn" as const }] : [],
    })),
  }));

  return [
    { key: "files", title: "Files", type: "file", note: data.filesTruncated ? "first 1,000 files" : undefined, leaves: files },
    // REGISTERED FILES: the file index (migration 0020, /artifacts routes) is being built in another session.
    // When it lands, load it in map-data.ts and fill this group's leaves (type "file", node null until the graph knows them).
    { key: "registered", title: "Registered files", type: null, note: "the file index is not available yet", leaves: [] },
    { key: "knowledge", title: "Knowledge", type: "knowledge", leaves: [], groups: kindGroups },
    { key: "handoffs", title: "Handoffs", type: "handoff", leaves: handoffs },
    { key: "sessions", title: "Sessions", type: "session", leaves: sessions },
    { key: "messages", title: "Messages", type: "message", note: `${threads.size} threads`, leaves: [], groups: threadGroups },
  ];
}

function count(g: Group): number {
  return g.leaves.length + (g.groups ?? []).reduce((n, c) => n + count(c), 0);
}

function filterGroup(g: Group, q: string): Group {
  const leaves = g.leaves.filter((l) => `${l.title} ${l.meta} ${l.badges.map((b) => b.text).join(" ")}`.toLowerCase().includes(q));
  const groups = g.groups?.map((c) => filterGroup(c, q)).filter((c) => count(c) > 0 || c.title.toLowerCase().includes(q));
  return { ...g, leaves, groups };
}

export default function MapTree({ data, onShowInGraph }: { data: MapData; onShowInGraph: (node: string) => void }) {
  const [query, setQuery] = useState("");
  const [openAll, setOpenAll] = useState<{ open: boolean; gen: number }>({ open: false, gen: 0 });
  const tree = useMemo(() => buildTree(data), [data]);
  const q = query.trim().toLowerCase();
  const shown = q ? tree.map((g) => filterGroup(g, q)) : tree;

  function renderGroup(g: Group, depth: number) {
    const total = count(g);
    const forced = q ? total > 0 : openAll.open;
    return (
      <li key={`${g.key}-${openAll.gen}-${q ? "q" : ""}`} className={`map-tree-group depth-${depth}`}>
        <details open={forced || (depth === 0 && !openAll.gen && g.type !== null && total <= 30)}>
          <summary>
            <span className={`map-dot is-${g.type ?? "none"}`} aria-hidden="true" />
            <strong>{g.title}</strong>
            <span className="map-count">{total}</span>
            {g.note && <small>{g.note}</small>}
            {depth === 0 && g.type && <a className="map-panel-link" href={`#panel-${PANEL_ANCHOR[g.type]}`} onClick={(e) => e.stopPropagation()}>Open panel</a>}
          </summary>
          {g.key === "registered" ? <p className="map-empty">Files registered in the file index will appear here once that index ships.</p>
            : total === 0 ? <p className="map-empty">Nothing here yet.</p>
            : (
              <ul>
                {g.groups?.map((c) => renderGroup(c, depth + 1))}
                {g.leaves.map((l) => (
                  <li key={l.key} className="map-leaf">
                    <div className="map-leaf-main">
                      <span className="map-leaf-title">{l.title}</span>
                      <span className="map-badges">{l.badges.map((b, i) => <span key={i} className={`map-badge ${b.tone}`}>{b.text}</span>)}</span>
                    </div>
                    <div className="map-leaf-meta">
                      <small>{l.meta}</small>
                      <a href={`#panel-${PANEL_ANCHOR[l.type]}`}>Panel</a>
                      {l.node && <button type="button" className="map-link-button" onClick={() => onShowInGraph(l.node!)}>Graph<span className="sr-only">: {l.title}</span></button>}
                    </div>
                  </li>
                ))}
              </ul>
            )}
        </details>
      </li>
    );
  }

  return (
    <div className="map-tree">
      <div className="map-toolbar">
        <label className="map-search">Filter<input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="name, status, assistant…" /></label>
        <button type="button" className="secondary-button" onClick={() => setOpenAll((s) => ({ open: true, gen: s.gen + 1 }))}>Expand all</button>
        <button type="button" className="secondary-button" onClick={() => setOpenAll((s) => ({ open: false, gen: s.gen + 1 }))}>Collapse all</button>
      </div>
      <ul className="map-tree-root">{shown.map((g) => renderGroup(g, 0))}</ul>
    </div>
  );
}
