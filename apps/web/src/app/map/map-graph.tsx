"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { NODE_TYPES, PANEL_ANCHOR, TYPE_LABEL, type GraphEdge, type GraphNode, type NodeType } from "./map-data";

type Sim = { x: Float64Array; y: Float64Array; vx: Float64Array; vy: Float64Array; pinned: Uint8Array; alpha: number };
type View = { k: number; tx: number; ty: number };

const HEIGHT = 560;
const LINK_LENGTH = 70;
const REPULSION = 650;
const CLUSTER_PULL = 0.03;
const DAMPING = 0.55;
const MAX_STEP = 40;

function anchors(types: NodeType[]) {
  const out = new Map<NodeType, [number, number]>();
  types.forEach((t, i) => {
    const a = (i / types.length) * Math.PI * 2 - Math.PI / 2;
    out.set(t, [Math.cos(a) * 170, Math.sin(a) * 120]);
  });
  return out;
}

function tick(sim: Sim, nodes: GraphNode[], links: [number, number][], anchor: Map<NodeType, [number, number]>) {
  const n = nodes.length;
  const { x, y, vx, vy, pinned, alpha } = sim;
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      let dx = x[j] - x[i], dy = y[j] - y[i];
      let d2 = dx * dx + dy * dy;
      if (d2 < 0.01) { dx = (i % 7) - 3 + 0.1; dy = (j % 5) - 2 + 0.1; d2 = dx * dx + dy * dy; }
      if (d2 > 160000) continue;
      const f = (REPULSION * alpha) / d2;
      vx[i] -= dx * f; vy[i] -= dy * f;
      vx[j] += dx * f; vy[j] += dy * f;
    }
  }
  for (const [a, b] of links) {
    const dx = x[b] - x[a], dy = y[b] - y[a];
    const d = Math.sqrt(dx * dx + dy * dy) || 1;
    const f = ((d - LINK_LENGTH) / d) * 0.08 * alpha;
    vx[a] += dx * f; vy[a] += dy * f;
    vx[b] -= dx * f; vy[b] -= dy * f;
  }
  for (let i = 0; i < n; i++) {
    const [ax, ay] = anchor.get(nodes[i].type)!;
    vx[i] += (ax - x[i]) * CLUSTER_PULL * alpha;
    vy[i] += (ay - y[i]) * CLUSTER_PULL * alpha;
    if (pinned[i]) { vx[i] = 0; vy[i] = 0; continue; }
    vx[i] *= DAMPING; vy[i] *= DAMPING;
    x[i] += Math.max(-MAX_STEP, Math.min(MAX_STEP, vx[i]));
    y[i] += Math.max(-MAX_STEP, Math.min(MAX_STEP, vy[i]));
  }
  sim.alpha *= 0.975;
}

type Frame = { nodes: GraphNode[]; x: Float64Array; y: Float64Array; pinned: Uint8Array };

function bounds(sim: Sim, n: number) {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (let i = 0; i < n; i++) { x0 = Math.min(x0, sim.x[i]); x1 = Math.max(x1, sim.x[i]); y0 = Math.min(y0, sim.y[i]); y1 = Math.max(y1, sim.y[i]); }
  return { x0, y0, x1, y1 };
}

function fitView(sim: Sim, n: number, width: number): View {
  if (!n) return { k: 1, tx: width / 2, ty: HEIGHT / 2 };
  const { x0, y0, x1, y1 } = bounds(sim, n);
  const k = Math.max(0.2, Math.min(2, Math.min((width - 80) / (x1 - x0 || 1), (HEIGHT - 80) / (y1 - y0 || 1))));
  return { k, tx: width / 2 - ((x0 + x1) / 2) * k, ty: HEIGHT / 2 - ((y0 + y1) / 2) * k };
}

export default function MapGraph({ nodes: allNodes, edges: allEdges, focus, panelPrefix }: { nodes: GraphNode[]; edges: GraphEdge[]; focus: string | null; panelPrefix: string }) {
  const [types, setTypes] = useState<Set<NodeType>>(() => new Set(NODE_TYPES));
  const [hideHistory, setHideHistory] = useState(false);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<string | null>(focus);
  const [hover, setHover] = useState<string | null>(null);
  const [view, setView] = useState<View>({ k: 1, tx: 0, ty: 0 });
  const [width, setWidth] = useState(800);
  const [frame, setFrame] = useState<Frame | null>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const svg = useRef<SVGSVGElement>(null);
  const simRef = useRef<Sim | null>(null);
  const dirty = useRef(false);
  const focusRef = useRef(focus);
  const autoFit = useRef(true);
  const drag = useRef<{ mode: "node" | "pan"; index: number; sx: number; sy: number; tx: number; ty: number; moved: boolean } | null>(null);

  const nodes = useMemo(() => allNodes.filter((n) => types.has(n.type) && !(hideHistory && n.dim)), [allNodes, types, hideHistory]);
  const index = useMemo(() => new Map(nodes.map((n, i) => [n.id, i])), [nodes]);
  const edges = useMemo(() => allEdges.filter((e) => index.has(e.source) && index.has(e.target)), [allEdges, index]);
  const links = useMemo(() => edges.map((e) => [index.get(e.source)!, index.get(e.target)!] as [number, number]), [edges, index]);
  const degree = useMemo(() => { const d = new Uint16Array(nodes.length); for (const [a, b] of links) { d[a]++; d[b]++; } return d; }, [links, nodes.length]);
  const hubDegree = useMemo(() => Math.max(5, [...degree].sort((a, b) => b - a)[11] ?? 0), [degree]);
  const neighbours = useMemo(() => {
    const m = new Map<string, Set<string>>();
    for (const e of edges) {
      if (!m.has(e.source)) m.set(e.source, new Set());
      if (!m.has(e.target)) m.set(e.target, new Set());
      m.get(e.source)!.add(e.target); m.get(e.target)!.add(e.source);
    }
    return m;
  }, [edges]);

  useEffect(() => {
    const anchor = anchors(NODE_TYPES.filter((t) => nodes.some((n) => n.type === t)));
    const n = nodes.length;
    const s: Sim = { x: new Float64Array(n), y: new Float64Array(n), vx: new Float64Array(n), vy: new Float64Array(n), pinned: new Uint8Array(n), alpha: 1 };
    const perType = new Map<NodeType, number>();
    nodes.forEach((node, i) => {
      const k = perType.get(node.type) ?? 0;
      perType.set(node.type, k + 1);
      const [ax, ay] = anchor.get(node.type)!;
      const r = 12 * Math.sqrt(k + 1), a = k * 2.39996;
      s.x[i] = ax + Math.cos(a) * r; s.y[i] = ay + Math.sin(a) * r;
    });
    for (let i = 0; i < 160; i++) tick(s, nodes, links, anchor);
    simRef.current = s;
    let first = true;
    let settled = false;
    let raf = 0;
    const loop = () => {
      const moving = s.alpha > 0.02;
      if (moving) tick(s, nodes, links, anchor);
      if (!moving && !settled) {
        settled = true;
        if (autoFit.current) setView(fitView(s, n, wrap.current?.clientWidth ?? 800));
      }
      if (moving || dirty.current || first) {
        dirty.current = false;
        setFrame({ nodes, x: s.x.slice(), y: s.y.slice(), pinned: s.pinned.slice() });
      }
      if (first) {
        first = false;
        const w = wrap.current?.clientWidth ?? 800;
        const fi = focusRef.current ? nodes.findIndex((x) => x.id === focusRef.current) : -1;
        focusRef.current = null;
        if (fi >= 0) autoFit.current = false;
        setView(fi >= 0 ? { k: 1.3, tx: w / 2 - s.x[fi] * 1.3, ty: HEIGHT / 2 - s.y[fi] * 1.3 } : fitView(s, n, w));
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [nodes, links]);

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => setWidth(Math.max(320, Math.round(entry.contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const el = svg.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      autoFit.current = false;
      const r = el.getBoundingClientRect();
      const px = e.clientX - r.left, py = e.clientY - r.top;
      setView((v) => {
        const k = Math.max(0.15, Math.min(5, v.k * Math.exp(-e.deltaY * 0.0015)));
        return { k, tx: px - ((px - v.tx) / v.k) * k, ty: py - ((py - v.ty) / v.k) * k };
      });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  function fit() {
    if (simRef.current) setView(fitView(simRef.current, nodes.length, width));
  }

  function reheat(alpha: number) {
    const s = simRef.current;
    if (s) { s.alpha = Math.max(s.alpha, alpha); dirty.current = true; }
  }

  function onPointerDown(e: React.PointerEvent, nodeIndex = -1) {
    if (e.button !== 0) return;
    e.stopPropagation();
    svg.current?.setPointerCapture(e.pointerId);
    autoFit.current = false;
    drag.current = { mode: nodeIndex >= 0 ? "node" : "pan", index: nodeIndex, sx: e.clientX, sy: e.clientY, tx: view.tx, ty: view.ty, moved: false };
  }

  function onPointerMove(e: React.PointerEvent) {
    const d = drag.current;
    const s = simRef.current;
    if (!d || !s) return;
    if (Math.abs(e.clientX - d.sx) + Math.abs(e.clientY - d.sy) > 3) d.moved = true;
    if (d.mode === "pan") setView((v) => ({ ...v, tx: d.tx + e.clientX - d.sx, ty: d.ty + e.clientY - d.sy }));
    else if (d.moved) {
      const r = svg.current!.getBoundingClientRect();
      s.x[d.index] = (e.clientX - r.left - view.tx) / view.k;
      s.y[d.index] = (e.clientY - r.top - view.ty) / view.k;
      s.pinned[d.index] = 1;
      reheat(0.25);
    }
  }

  function onPointerUp() {
    const d = drag.current;
    drag.current = null;
    if (!d || d.moved) return;
    setSelected(d.mode === "node" ? nodes[d.index].id : null);
  }

  function unpin(i: number) {
    if (simRef.current) simRef.current.pinned[i] = 0;
    reheat(0.3);
  }

  function unpinAll() {
    simRef.current?.pinned.fill(0);
    reheat(0.6);
  }

  function zoomAt(v: View, f: number): View {
    const k = Math.max(0.15, Math.min(5, v.k * f));
    return { k, tx: width / 2 - ((width / 2 - v.tx) / v.k) * k, ty: HEIGHT / 2 - ((HEIGHT / 2 - v.ty) / v.k) * k };
  }

  function onKey(e: React.KeyboardEvent) {
    if (e.target !== svg.current) return;
    autoFit.current = false;
    const step = 60;
    const moves: Record<string, () => void> = {
      ArrowLeft: () => setView((v) => ({ ...v, tx: v.tx + step })),
      ArrowRight: () => setView((v) => ({ ...v, tx: v.tx - step })),
      ArrowUp: () => setView((v) => ({ ...v, ty: v.ty + step })),
      ArrowDown: () => setView((v) => ({ ...v, ty: v.ty - step })),
      "+": () => setView((v) => zoomAt(v, 1.25)), "=": () => setView((v) => zoomAt(v, 1.25)),
      "-": () => setView((v) => zoomAt(v, 0.8)), "0": fit, Escape: () => setSelected(null),
    };
    if (moves[e.key]) { e.preventDefault(); moves[e.key](); }
  }

  const ready = frame !== null && frame.nodes === nodes;
  const focusId = hover ?? selected;
  const lit = focusId ? new Set([focusId, ...(neighbours.get(focusId) ?? [])]) : null;
  const q = query.trim().toLowerCase();
  const matches = q ? new Set(nodes.filter((n) => n.label.toLowerCase().includes(q) || n.detail.join(" ").toLowerCase().includes(q)).map((n) => n.id)) : null;
  const selIndex = selected ? index.get(selected) : undefined;
  const sel = selIndex !== undefined ? nodes[selIndex] : undefined;
  const grow = view.k < 1 ? Math.pow(view.k, -0.75) : 1;
  const radius = (i: number) => (5 + Math.min(9, Math.sqrt(degree[i]) * 2.2)) * grow;

  return (
    <div className="map-graph">
      <fieldset className="map-type-filter"><legend className="sr-only">Show node types</legend>
        {NODE_TYPES.map((t) => (
          <label key={t} className="map-chip">
            <input type="checkbox" checked={types.has(t)} onChange={() => setTypes((s) => { const n = new Set(s); if (n.has(t)) n.delete(t); else n.add(t); return n; })} />
            <span className={`map-dot is-${t}`} aria-hidden="true" />{TYPE_LABEL[t]} <small>{allNodes.filter((n) => n.type === t).length}</small>
          </label>
        ))}
        <label className="map-chip"><input type="checkbox" checked={hideHistory} onChange={(e) => setHideHistory(e.target.checked)} />Hide superseded / rejected</label>
      </fieldset>
      <div className="map-toolbar">
        <label className="map-search">Find<input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="highlight nodes…" /></label>
        <button type="button" className="secondary-button" onClick={fit}>Fit</button>
        <button type="button" className="secondary-button" onClick={unpinAll}>Unpin all</button>
      </div>
      <div className="map-graph-body">
        <div className="map-canvas" ref={wrap}>
          <svg ref={svg} width={width} height={HEIGHT} role="application" tabIndex={0}
            aria-label={`Graph of ${nodes.length} items and ${edges.length} links. Arrow keys pan, plus and minus zoom, 0 fits, Tab moves between nodes, Enter selects.`}
            onKeyDown={onKey} onPointerDown={(e) => onPointerDown(e)} onPointerMove={onPointerMove} onPointerUp={onPointerUp}>
            <defs>
              <marker id="map-arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0 0 10 5 0 10z" className="map-arrowhead" /></marker>
            </defs>
            {ready && (
              <g transform={`translate(${view.tx} ${view.ty}) scale(${view.k})`}>
                {edges.map((e, i) => {
                  const [a, b] = links[i];
                  const on = lit ? lit.has(e.source) && lit.has(e.target) : true;
                  const dx = frame.x[b] - frame.x[a], dy = frame.y[b] - frame.y[a];
                  const d = Math.sqrt(dx * dx + dy * dy) || 1;
                  const rb = radius(b) + 2;
                  return <line key={`${e.source}-${e.target}`} className={`map-edge ${e.kind}${on ? "" : " faded"}`} x1={frame.x[a]} y1={frame.y[a]} x2={frame.x[b] - (dx / d) * rb} y2={frame.y[b] - (dy / d) * rb} markerEnd="url(#map-arrow)" />;
                })}
                {nodes.map((n, i) => {
                  const r = radius(i);
                  const faded = (lit && !lit.has(n.id)) || (matches && !matches.has(n.id));
                  const showLabel = view.k > 1.5 || degree[i] >= hubDegree || (lit?.has(n.id) ?? false) || (matches?.has(n.id) ?? false);
                  return (
                    <g key={n.id} className={`map-node is-${n.type}${n.flagged ? " flagged" : ""}${n.dim ? " history" : ""}${faded ? " faded" : ""}${n.id === selected ? " selected" : ""}`}
                      transform={`translate(${frame.x[i]} ${frame.y[i]})`} tabIndex={0} role="button" aria-label={`${TYPE_LABEL[n.type]}: ${n.label}${n.flagged ? " (stale or disputed)" : ""}`}
                      onPointerDown={(e) => onPointerDown(e, i)} onPointerEnter={() => setHover(n.id)} onPointerLeave={() => setHover(null)}
                      onDoubleClick={() => unpin(i)} onFocus={() => setHover(n.id)} onBlur={() => setHover(null)}
                      onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setSelected(n.id); } }}>
                      <circle r={r} />
                      {frame.pinned[i] ? <circle r={r + 3.5} className="map-pin" /> : null}
                      {showLabel && <text x={r + 4} y={4} style={{ fontSize: `${11 / view.k}px`, strokeWidth: `${3 / view.k}px` }}>{n.label}</text>}
                    </g>
                  );
                })}
              </g>
            )}
          </svg>
          {nodes.length === 0 && <p className="map-empty map-overlay">No nodes for the selected types.</p>}
          <p className="map-hint">Drag a node to pin it, double-click to release. Drag the background to pan, scroll to zoom.</p>
        </div>
        <aside className="map-details" aria-live="polite">
          {sel && selIndex !== undefined ? (
            <>
              <p className="map-details-type"><span className={`map-dot is-${sel.type}`} aria-hidden="true" />{TYPE_LABEL[sel.type]}{sel.flagged ? " · needs attention" : ""}</p>
              <h4>{sel.label}</h4>
              {sel.detail.map((line, i) => <p key={i}>{line}</p>)}
              <p><a href={`#${panelPrefix}${PANEL_ANCHOR[sel.type]}`}>Open the {TYPE_LABEL[sel.type].toLowerCase()} panel</a>{ready && frame.pinned[selIndex] ? <> · <button type="button" className="map-link-button" onClick={() => unpin(selIndex)}>Unpin</button></> : null}</p>
              <h5>Connected ({neighbours.get(sel.id)?.size ?? 0})</h5>
              <ul>{[...(neighbours.get(sel.id) ?? [])].map((id) => {
                const other = nodes[index.get(id)!];
                const e = edges.find((x) => (x.source === sel.id && x.target === id) || (x.target === sel.id && x.source === id))!;
                return <li key={id}><button type="button" className="map-link-button" onClick={() => setSelected(id)}><span className={`map-dot is-${other.type}`} aria-hidden="true" />{other.label}</button> <small>{e.source === sel.id ? e.kind : `${e.kind} (from)`}</small></li>;
              })}</ul>
            </>
          ) : <p className="map-empty">Select a node to see its details and connections. Nodes with a dashed outline are stale or disputed.</p>}
          <div className="map-legend">
            <span><i className="map-legend-line evidence" />evidence</span>
            <span><i className="map-legend-line supersedes" />supersedes</span>
            <span><i className="map-legend-line source" />relied on</span>
            <span><i className="map-legend-line reply" />reply</span>
          </div>
        </aside>
      </div>
    </div>
  );
}
