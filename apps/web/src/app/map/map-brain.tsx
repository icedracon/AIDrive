"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { agentLabel, clip, nodeId, PANEL_ANCHOR, shortTime, TYPE_LABEL, type GraphEdge, type GraphNode, type MapData, type NodeType } from "./map-data";

type Kind = NodeType | "hub";
type BNode = { id: string; kind: Kind; label: string; detail: string[]; flagged: boolean; hue: number; heat: number };
type BEdge = { a: number; b: number; touch: boolean };
type Pulse = { edge: number; from: number; at: number };
type Brain = { nodes: BNode[]; edges: BEdge[]; pulses: Pulse[]; neighbours: number[][] };
type Palette = Record<NodeType | "edge" | "warn" | "ink" | "muted" | "bg", string> & { dark: boolean };

const HEIGHT = 600;
const FOCAL = 760;
const REPULSION = 2600;
const HUB_REPULSION = 9;
const PULSE_GAP_MS = 380;
const PULSE_MS = 1300;
const RECENT = 60;

function buildBrain(data: MapData, graph: { nodes: GraphNode[]; edges: GraphEdge[] }): Brain {
  const humans = new Map<string, number>();
  for (const e of data.activity) if (!e.agent_id && e.actor_email) humans.set(e.actor_email, (humans.get(e.actor_email) ?? 0) + 1);
  const ownerName = [...humans.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "Owner";

  const nodes: BNode[] = [];
  const index = new Map<string, number>();
  const hubOf = (agentId: string | null) => {
    const id = agentId ? `hub:${agentId}` : "hub:owner";
    if (!index.has(id)) {
      index.set(id, nodes.length);
      nodes.push({ id, kind: "hub", label: agentId ? agentLabel(data, agentId) : ownerName, detail: [agentId ? "Assistant key" : "Owner"], flagged: false, hue: 0, heat: 0 });
    }
    return index.get(id)!;
  };
  hubOf(null);
  for (const id of data.agentNames.keys()) hubOf(id);
  for (const n of graph.nodes) {
    index.set(n.id, nodes.length);
    nodes.push({ id: n.id, kind: n.type, label: n.label, detail: n.detail, flagged: n.flagged, hue: 0, heat: 0 });
  }
  const hubs = nodes.filter((n) => n.kind === "hub");
  hubs.forEach((h, i) => { h.hue = (i * 137.5 + 32) % 360; });

  const edges: BEdge[] = [];
  const seen = new Map<string, number>();
  const link = (a: number | undefined, b: number | undefined, touch: boolean) => {
    if (a === undefined || b === undefined || a === b) return undefined;
    const key = a < b ? `${a}|${b}` : `${b}|${a}`;
    if (!seen.has(key)) { seen.set(key, edges.length); edges.push({ a, b, touch }); }
    return seen.get(key)!;
  };
  for (const e of graph.edges) link(index.get(e.source), index.get(e.target), false);
  for (const f of data.files) for (const v of f.versions) link(hubOf(v.agent_id), index.get(nodeId("file", f.id)), true);
  for (const k of data.knowledge) link(hubOf(k.agent_id), index.get(nodeId("knowledge", k.id)), true);
  for (const h of data.handoffs) link(hubOf(h.agent_id), index.get(nodeId("handoff", h.id)), true);
  for (const s of data.sessions) link(hubOf(s.agent_id), index.get(nodeId("session", s.id)), true);
  for (const m of data.messages) link(hubOf(m.from_agent_id), index.get(nodeId("message", m.id)), true);

  const events: { at: number; hub: number; node: number | undefined }[] = [];
  for (const e of data.activity) {
    const target = e.file_id ? index.get(nodeId("file", e.file_id)) : e.handoff_id ? index.get(nodeId("handoff", e.handoff_id)) : undefined;
    events.push({ at: Date.parse(e.at), hub: hubOf(e.agent_id), node: target });
  }
  for (const k of data.knowledge) events.push({ at: Date.parse(k.created_at), hub: hubOf(k.agent_id), node: index.get(nodeId("knowledge", k.id)) });
  for (const m of data.messages) events.push({ at: Date.parse(m.created_at), hub: hubOf(m.from_agent_id), node: index.get(nodeId("message", m.id)) });
  const recent = events.filter((e) => e.node !== undefined && Number.isFinite(e.at)).sort((a, b) => b.at - a.at).slice(0, RECENT);
  const pulses: Pulse[] = [];
  recent.forEach((e, rank) => {
    const edge = link(e.hub, e.node, true)!;
    pulses.push({ edge, from: e.hub, at: e.at });
    const heat = 1 - rank / RECENT;
    nodes[e.node!].heat = Math.max(nodes[e.node!].heat, heat);
    nodes[e.hub].heat = Math.max(nodes[e.hub].heat, heat);
  });
  pulses.reverse();

  for (const h of hubs) {
    const i = index.get(h.id)!;
    const counts = new Map<string, number>();
    for (const e of edges) if (e.touch && (e.a === i || e.b === i)) {
      const other = nodes[e.a === i ? e.b : e.a];
      counts.set(other.kind, (counts.get(other.kind) ?? 0) + 1);
    }
    h.detail = [...h.detail, ...[...counts.entries()].map(([k, n]) => `${n} ${TYPE_LABEL[k as NodeType].toLowerCase()}`)];
    const last = recent.find((e) => e.hub === i);
    if (last) h.detail.push(`Last active ${shortTime(new Date(last.at).toISOString())}`);
  }

  const neighbours: number[][] = nodes.map(() => []);
  for (const e of edges) { neighbours[e.a].push(e.b); neighbours[e.b].push(e.a); }
  return { nodes, edges, pulses, neighbours };
}

type Sim = { x: Float64Array; y: Float64Array; z: Float64Array; vx: Float64Array; vy: Float64Array; vz: Float64Array; ax: Float64Array; ay: Float64Array; az: Float64Array; alpha: number };

function tick(s: Sim, brain: Brain) {
  const n = brain.nodes.length;
  const { x, y, z, vx, vy, vz, alpha } = s;
  for (let i = 0; i < n; i++) {
    const hi = brain.nodes[i].kind === "hub";
    for (let j = i + 1; j < n; j++) {
      const dx = x[j] - x[i], dy = y[j] - y[i], dz = z[j] - z[i];
      const d2 = dx * dx + dy * dy + dz * dz + 1;
      if (d2 > 360000) continue;
      const f = (REPULSION * alpha * (hi && brain.nodes[j].kind === "hub" ? HUB_REPULSION : 1)) / (d2 * Math.sqrt(d2)) * 40;
      vx[i] -= dx * f; vy[i] -= dy * f; vz[i] -= dz * f;
      vx[j] += dx * f; vy[j] += dy * f; vz[j] += dz * f;
    }
  }
  for (const e of brain.edges) {
    const dx = x[e.b] - x[e.a], dy = y[e.b] - y[e.a], dz = z[e.b] - z[e.a];
    const d = Math.sqrt(dx * dx + dy * dy + dz * dz) || 1;
    const f = ((d - (e.touch ? 120 : 55)) / d) * (e.touch ? 0.012 : 0.05) * alpha;
    vx[e.a] += dx * f; vy[e.a] += dy * f; vz[e.a] += dz * f;
    vx[e.b] -= dx * f; vy[e.b] -= dy * f; vz[e.b] -= dz * f;
  }
  for (let i = 0; i < n; i++) {
    if (brain.nodes[i].kind === "hub") { vx[i] += (s.ax[i] - x[i]) * 0.08; vy[i] += (s.ay[i] - y[i]) * 0.08; vz[i] += (s.az[i] - z[i]) * 0.08; }
    vx[i] = (vx[i] - x[i] * 0.004 * alpha) * 0.6; vy[i] = (vy[i] - y[i] * 0.004 * alpha) * 0.6; vz[i] = (vz[i] - z[i] * 0.004 * alpha) * 0.6;
    x[i] += Math.max(-30, Math.min(30, vx[i])); y[i] += Math.max(-30, Math.min(30, vy[i])); z[i] += Math.max(-30, Math.min(30, vz[i]));
  }
  s.alpha *= 0.985;
}

function readPalette(el: Element): Palette {
  const cs = getComputedStyle(el);
  const v = (name: string) => cs.getPropertyValue(name).trim() || "#888";
  const bg = v("--map-bg");
  const rgb = bg.match(/[0-9a-f]{2}/gi)?.slice(0, 3).map((h) => parseInt(h, 16)) ?? [255, 255, 255];
  return { file: v("--map-file"), knowledge: v("--map-knowledge"), handoff: v("--map-handoff"), session: v("--map-session"), message: v("--map-message"),
    edge: v("--map-edge"), warn: v("--map-warn"), ink: v("--map-ink"), muted: v("--map-muted"), bg, dark: rgb[0] + rgb[1] + rgb[2] < 300 };
}

function colour(n: BNode, p: Palette) {
  return n.kind === "hub" ? `hsl(${n.hue} 70% ${p.dark ? 66 : 42}%)` : p[n.kind];
}

export default function MapBrain({ data, graph, panelPrefix }: { data: MapData; graph: { nodes: GraphNode[]; edges: GraphEdge[] }; panelPrefix: string }) {
  const brain = useMemo(() => buildBrain(data, graph), [data, graph]);
  const [selected, setSelected] = useState<number | null>(null);
  const [spin, setSpin] = useState(true);
  const [pulsing, setPulsing] = useState(true);
  const [frameMs, setFrameMs] = useState<number | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const cam = useRef({ yaw: 0.6, pitch: -0.25, dist: 900 });
  const ui = useRef({ selected: null as number | null, hover: null as number | null, spin: true, pulsing: true, dragging: false, moved: false, sx: 0, sy: 0, dirty: true, autoFit: true });
  const picks = useRef({ px: new Float64Array(0), py: new Float64Array(0), pr: new Float64Array(0) });

  useEffect(() => { Object.assign(ui.current, { selected, spin, pulsing, dirty: true }); }, [selected, spin, pulsing]);

  useEffect(() => {
    const cv = canvas.current, host = wrap.current;
    if (!cv || !host) return;
    const ctx = cv.getContext("2d")!;
    const n = brain.nodes.length;
    const s: Sim = { x: new Float64Array(n), y: new Float64Array(n), z: new Float64Array(n), vx: new Float64Array(n), vy: new Float64Array(n), vz: new Float64Array(n), ax: new Float64Array(n), ay: new Float64Array(n), az: new Float64Array(n), alpha: 1 };
    const hubCount = brain.nodes.filter((x) => x.kind === "hub").length;
    const hubR = 70 + Math.sqrt(n) * 14;
    let h = 0;
    brain.nodes.forEach((node, i) => {
      const hub = node.kind === "hub";
      const k = hub ? h++ : i, total = hub ? hubCount : n;
      const t = Math.acos(1 - (2 * (k + 0.5)) / total), ph = k * 2.39996, r = hub ? hubR : 260;
      s.x[i] = r * Math.sin(t) * Math.cos(ph); s.y[i] = r * Math.cos(t); s.z[i] = r * Math.sin(t) * Math.sin(ph);
      if (hub) { s.ax[i] = s.x[i]; s.ay[i] = s.y[i]; s.az[i] = s.z[i]; }
    });
    for (let i = 0; i < 200; i++) tick(s, brain);
    const radii: number[] = [];
    for (let i = 0; i < n; i++) if (brain.nodes[i].kind !== "hub") radii.push(Math.hypot(s.x[i], s.y[i], s.z[i]));
    radii.sort((a, b) => a - b);
    const shell = Math.max(170, (radii[Math.floor(radii.length * 0.85)] ?? 200) * 1.05);
    for (let i = 0; i < n; i++) if (brain.nodes[i].kind === "hub") {
      const d = Math.hypot(s.ax[i], s.ay[i], s.az[i]) || 1;
      s.ax[i] *= shell / d; s.ay[i] *= shell / d; s.az[i] *= shell / d;
    }
    s.alpha = 0.5;
    for (let i = 0; i < 120; i++) tick(s, brain);
    ui.current.autoFit = true;
    let fitted = false;
    const fitDist = (w: number, h: number) => {
      let mx = 0, my = 0, mz = 0;
      for (let i = 0; i < n; i++) { mx += s.x[i]; my += s.y[i]; mz += s.z[i]; }
      mx /= n || 1; my /= n || 1; mz /= n || 1;
      const c = cam.current, cy = Math.cos(c.yaw), sy0 = Math.sin(c.yaw), cp = Math.cos(c.pitch), sp = Math.sin(c.pitch);
      const pts: [number, number, number, boolean][] = [];
      let R = 60;
      for (let i = 0; i < n; i++) {
        const px0 = s.x[i] - mx, py0 = s.y[i] - my, pz0 = s.z[i] - mz;
        const x1 = px0 * cy - pz0 * sy0, z1 = px0 * sy0 + pz0 * cy;
        pts.push([x1, py0 * cp - z1 * sp, py0 * sp + z1 * cp, brain.nodes[i].kind === "hub"]);
        R = Math.max(R, Math.hypot(px0, py0, pz0));
      }
      const overflow = (D: number) => {
        const r = pts.map(([x, y, z, hub]) => {
          const k = FOCAL / Math.max(20, z + D);
          return [Math.max(Math.abs(x * k) / (w * 0.45), Math.abs(y * k) / (h * 0.44)), hub] as const;
        });
        const hubs = r.filter(([, hub]) => hub).map(([v]) => v);
        const rest = r.filter(([, hub]) => !hub).map(([v]) => v).sort((a, b) => a - b);
        return Math.max(...hubs, rest[Math.floor(rest.length * 0.92)] ?? 0);
      };
      let lo = R * 1.05, hi = R * 40;
      for (let i = 0; i < 24; i++) { const mid = (lo + hi) / 2; if (overflow(mid) > 1) lo = mid; else hi = mid; }
      const sphereCap = R + (FOCAL * R) / (0.75 * Math.min(w, h));
      return Math.max(hi, sphereCap);
    };
    picks.current = { px: new Float64Array(n), py: new Float64Array(n), pr: new Float64Array(n) };

    let palette = readPalette(host);
    const media = matchMedia("(prefers-color-scheme: dark)");
    const onTheme = () => { palette = readPalette(host); ui.current.dirty = true; };
    media.addEventListener("change", onTheme);

    let visible = true;
    const io = new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; });
    io.observe(host);

    const sx = new Float64Array(n), sy = new Float64Array(n), sc = new Float64Array(n), sz = new Float64Array(n);
    const order = Array.from({ length: n }, (_, i) => i);
    let raf = 0, last = performance.now(), frames = 0, work = 0, statAt = last, pulseClock = 0;
    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (!visible || document.hidden) { last = now; return; }
      const dt = Math.min(64, now - last); last = now;
      const u = ui.current, c = cam.current;
      const dpr = window.devicePixelRatio || 1, w = host.clientWidth, h = HEIGHT;
      const resized = cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr);
      const settling = s.alpha > 0.03;
      let fitting = false;
      if (u.autoFit && (!fitted || settling)) {
        const goal = fitDist(w, h);
        if (!fitted) { c.dist = goal; fitted = true; } else c.dist += (goal - c.dist) * 0.12;
        fitting = Math.abs(goal - c.dist) > 1;
      }
      if (!(u.spin || u.pulsing || u.dragging || settling || fitting || u.dirty || resized)) return;
      u.dirty = false;
      const t0 = performance.now();
      if (settling) tick(s, brain);
      if (u.spin && !u.dragging) c.yaw += dt * 0.00012;
      if (u.pulsing) pulseClock += dt;
      if (resized) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr); cv.style.width = `${w}px`; cv.style.height = `${h}px`; }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);

      const cy = Math.cos(c.yaw), sy0 = Math.sin(c.yaw), cp = Math.cos(c.pitch), sp = Math.sin(c.pitch);
      let mx = 0, my = 0, mz = 0;
      for (let i = 0; i < n; i++) { mx += s.x[i]; my += s.y[i]; mz += s.z[i]; }
      mx /= n || 1; my /= n || 1; mz /= n || 1;
      for (let i = 0; i < n; i++) {
        const px0 = s.x[i] - mx, py0 = s.y[i] - my, pz0 = s.z[i] - mz;
        const x1 = px0 * cy - pz0 * sy0, z1 = px0 * sy0 + pz0 * cy;
        const y2 = py0 * cp - z1 * sp, z2 = py0 * sp + z1 * cp + c.dist;
        const k = z2 > 20 ? FOCAL / z2 : 0;
        sx[i] = w / 2 + x1 * k; sy[i] = h / 2 + y2 * k; sc[i] = k; sz[i] = z2;
      }
      const focus = u.hover ?? u.selected;
      const lit = focus !== null ? new Set([focus, ...brain.neighbours[focus]]) : null;
      const depthAlpha = (i: number) => Math.max(0.25, Math.min(1, 1.6 - sz[i] / c.dist));

      ctx.globalCompositeOperation = palette.dark ? "lighter" : "source-over";
      ctx.lineWidth = 1;
      for (const touch of [false, true]) {
        ctx.beginPath();
        for (const e of brain.edges) {
          if (e.touch !== touch || !sc[e.a] || !sc[e.b]) continue;
          if (lit && !(lit.has(e.a) && lit.has(e.b))) continue;
          ctx.moveTo(sx[e.a], sy[e.a]); ctx.lineTo(sx[e.b], sy[e.b]);
        }
        ctx.strokeStyle = touch ? palette.muted : palette.edge;
        ctx.globalAlpha = lit ? 0.75 : touch ? 0.16 : 0.42;
        ctx.stroke();
      }

      if (u.pulsing || pulseClock > 0) {
        const span = brain.pulses.length * PULSE_GAP_MS + PULSE_MS;
        const t = span ? pulseClock % span : 0;
        brain.pulses.forEach((p, i) => {
          const local = (t - i * PULSE_GAP_MS) / PULSE_MS;
          if (local < 0 || local > 1) return;
          const e = brain.edges[p.edge];
          const a = p.from, b = e.a === a ? e.b : e.a;
          if (!sc[a] || !sc[b]) return;
          const px = sx[a] + (sx[b] - sx[a]) * local, py = sy[a] + (sy[b] - sy[a]) * local;
          const r = Math.max(1.5, 3.2 * (sc[a] + (sc[b] - sc[a]) * local)) * (0.6 + (i / brain.pulses.length) * 0.8);
          ctx.fillStyle = colour(brain.nodes[b], palette);
          ctx.globalAlpha = 0.22; ctx.beginPath(); ctx.arc(px, py, r * 3, 0, Math.PI * 2); ctx.fill();
          ctx.globalAlpha = 0.95; ctx.beginPath(); ctx.arc(px, py, r, 0, Math.PI * 2); ctx.fill();
        });
      }

      order.sort((i, j) => sz[j] - sz[i]);
      ctx.globalCompositeOperation = "source-over";
      for (const i of order) {
        if (!sc[i]) { picks.current.pr[i] = 0; continue; }
        const node = brain.nodes[i];
        const hub = node.kind === "hub";
        const r = Math.max(1.5, (hub ? 13 : 4 + Math.min(5, brain.neighbours[i].length * 0.6)) * sc[i]);
        const faded = lit && !lit.has(i);
        const col = colour(node, palette);
        const a = faded ? 0.1 : depthAlpha(i);
        if (node.heat > 0 && !faded) {
          ctx.globalAlpha = a * node.heat * (palette.dark ? 0.35 : 0.22);
          ctx.fillStyle = col; ctx.beginPath(); ctx.arc(sx[i], sy[i], r * (hub ? 2.2 : 2.8), 0, Math.PI * 2); ctx.fill();
        }
        ctx.globalAlpha = a; ctx.fillStyle = col;
        ctx.beginPath(); ctx.arc(sx[i], sy[i], r, 0, Math.PI * 2); ctx.fill();
        if (node.flagged || i === u.selected) {
          ctx.setLineDash(i === u.selected ? [] : [3, 2]); ctx.lineWidth = i === u.selected ? 2.5 : 1.6;
          ctx.strokeStyle = i === u.selected ? palette.ink : palette.warn;
          ctx.beginPath(); ctx.arc(sx[i], sy[i], r + 2.5, 0, Math.PI * 2); ctx.stroke(); ctx.setLineDash([]);
        }
        picks.current.px[i] = sx[i]; picks.current.py[i] = sy[i]; picks.current.pr[i] = r;
      }
      ctx.globalAlpha = 1;
      ctx.textBaseline = "middle";
      const placed: number[][] = [];
      const labelled = order.filter((i) => {
        const node = brain.nodes[i];
        const show = node.kind === "hub" || i === focus || (lit?.has(i) && brain.nodes[focus!].kind !== "hub");
        return show && sc[i] && !(lit && !lit.has(i));
      }).sort((a, b) => Number(b === focus) - Number(a === focus) || Number(brain.nodes[b].kind === "hub") - Number(brain.nodes[a].kind === "hub") || sz[a] - sz[b]);
      for (const i of labelled) {
        const hub = brain.nodes[i].kind === "hub";
        ctx.font = `${hub ? "600 13px" : "11px"} Arial, sans-serif`;
        const text = clip(brain.nodes[i].label, hub ? 28 : 40), tw = ctx.measureText(text).width, th = hub ? 16 : 14;
        const gap = (hub ? 16 : 8) * Math.max(0.6, sc[i]);
        const spots = [[gap, 0], [-gap - tw, 0], [-tw / 2, -gap - 4], [-tw / 2, gap + 4], [gap, -th], [gap, th], [-gap - tw, -th], [-gap - tw, th]];
        const spot = spots.find(([dx, dy]) => {
          const x0 = sx[i] + dx, y0 = sy[i] + dy - th / 2;
          return !placed.some(([a, b, c2, d]) => x0 < c2 && x0 + tw > a && y0 < d && y0 + th > b);
        });
        if (!spot) continue;
        const tx = sx[i] + spot[0], ty = sy[i] + spot[1];
        placed.push([tx - 2, ty - th / 2, tx + tw + 2, ty + th / 2]);
        ctx.lineWidth = 3; ctx.strokeStyle = palette.bg; ctx.strokeText(text, tx, ty);
        ctx.fillStyle = palette.ink; ctx.fillText(text, tx, ty);
      }
      frames++; work += performance.now() - t0;
      if (now - statAt > 1000) { setFrameMs(Math.round((work / Math.max(1, frames)) * 10) / 10); frames = 0; work = 0; statAt = now; }
    };
    raf = requestAnimationFrame(draw);
    return () => { cancelAnimationFrame(raf); media.removeEventListener("change", onTheme); io.disconnect(); };
  }, [brain]);

  useEffect(() => {
    const cv = canvas.current;
    if (!cv) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      cam.current.dist = Math.max(150, Math.min(6000, cam.current.dist * Math.exp(e.deltaY * 0.0012)));
      Object.assign(ui.current, { autoFit: false, dirty: true });
    };
    cv.addEventListener("wheel", onWheel, { passive: false });
    return () => cv.removeEventListener("wheel", onWheel);
  }, []);

  function pick(e: React.PointerEvent) {
    const r = canvas.current!.getBoundingClientRect();
    const mx = e.clientX - r.left, my = e.clientY - r.top;
    const { px, py, pr } = picks.current;
    let best: number | null = null, bestD = Infinity;
    for (let i = 0; i < pr.length; i++) {
      if (!pr[i]) continue;
      const d = Math.hypot(px[i] - mx, py[i] - my);
      if (d < pr[i] + 5 && d < bestD) { best = i; bestD = d; }
    }
    return best;
  }

  function onPointerDown(e: React.PointerEvent) {
    if (e.button !== 0) return;
    canvas.current!.setPointerCapture(e.pointerId);
    Object.assign(ui.current, { dragging: true, moved: false, sx: e.clientX, sy: e.clientY });
  }

  function onPointerMove(e: React.PointerEvent) {
    const u = ui.current;
    if (u.dragging) {
      const dx = e.clientX - u.sx, dy = e.clientY - u.sy;
      if (Math.abs(dx) + Math.abs(dy) > 3) u.moved = true;
      cam.current.yaw += dx * 0.006;
      cam.current.pitch = Math.max(-1.45, Math.min(1.45, cam.current.pitch + dy * 0.006));
      u.sx = e.clientX; u.sy = e.clientY;
    } else {
      const hover = pick(e);
      if (hover !== u.hover) { u.hover = hover; u.dirty = true; }
      canvas.current!.style.cursor = u.hover !== null ? "pointer" : "grab";
    }
  }

  function onPointerUp(e: React.PointerEvent) {
    const u = ui.current;
    const wasClick = u.dragging && !u.moved;
    u.dragging = false;
    if (wasClick) setSelected(pick(e));
  }

  function onKey(e: React.KeyboardEvent) {
    const c = cam.current;
    const keys: Record<string, () => void> = {
      ArrowLeft: () => { c.yaw -= 0.15; }, ArrowRight: () => { c.yaw += 0.15; },
      ArrowUp: () => { c.pitch = Math.max(-1.45, c.pitch - 0.15); }, ArrowDown: () => { c.pitch = Math.min(1.45, c.pitch + 0.15); },
      "+": () => { c.dist = Math.max(150, c.dist * 0.85); }, "=": () => { c.dist = Math.max(150, c.dist * 0.85); },
      "-": () => { c.dist = Math.min(6000, c.dist / 0.85); }, " ": () => setSpin((v) => !v), Escape: () => setSelected(null),
    };
    if (keys[e.key]) {
      e.preventDefault(); keys[e.key]();
      ui.current.dirty = true;
      if ("+=-".includes(e.key)) ui.current.autoFit = false;
    }
  }

  const hubs = brain.nodes.map((n, i) => [n, i] as const).filter(([n]) => n.kind === "hub");
  const sel = selected !== null ? brain.nodes[selected] : null;
  const touches = selected !== null ? brain.neighbours[selected] : [];

  return (
    <div className="map-brain">
      <div className="map-toolbar">
        <div className="map-type-filter" role="group" aria-label="Focus an assistant or the owner">
          {hubs.map(([h, i]) => (
            <button key={h.id} type="button" className="map-chip" aria-pressed={selected === i} onClick={() => setSelected(selected === i ? null : i)}>
              <span className="map-dot" style={{ background: `hsl(${h.hue} 70% 50%)` }} aria-hidden="true" />{h.label} <small>{brain.neighbours[i].length}</small>
            </button>
          ))}
        </div>
      </div>
      <div className="map-toolbar">
        <button type="button" className="secondary-button" aria-pressed={spin} onClick={() => setSpin((v) => !v)}>{spin ? "Pause rotation" : "Rotate"}</button>
        <button type="button" className="secondary-button" aria-pressed={pulsing} onClick={() => setPulsing((v) => !v)}>{pulsing ? "Pause activity" : "Replay activity"}</button>
        <small className="map-hint">{brain.nodes.length} nodes · {brain.edges.length} links · {brain.pulses.length} recent actions replayed{frameMs !== null ? ` · ${frameMs} ms per frame` : ""}</small>
      </div>
      <div className="map-graph-body">
        <div className="map-canvas" ref={wrap}>
          <canvas ref={canvas} height={HEIGHT} tabIndex={0} role="img"
            aria-label={`3D map of ${hubs.length} people and assistants and ${brain.nodes.length - hubs.length} items they touched. Drag or arrow keys rotate, scroll or plus and minus zoom, space pauses rotation; use the buttons above to focus one assistant.`}
            onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerLeave={() => { Object.assign(ui.current, { hover: null, dirty: true }); }} onKeyDown={onKey} />
          <p className="map-hint">Each assistant and the owner is a hub; lines are what they touched and how items cite each other. Moving dots replay the latest {RECENT} actions, brighter halos mark recent work.</p>
        </div>
        <aside className="map-details" aria-live="polite">
          {sel ? (
            <>
              <p className="map-details-type">
                <span className={sel.kind === "hub" ? "map-dot" : `map-dot is-${sel.kind}`} style={sel.kind === "hub" ? { background: `hsl(${sel.hue} 70% 50%)` } : undefined} aria-hidden="true" />
                {sel.kind === "hub" ? "Hub" : TYPE_LABEL[sel.kind]}{sel.flagged ? " · needs attention" : ""}
              </p>
              <h4>{sel.label}</h4>
              {sel.detail.map((line, i) => <p key={i}>{line}</p>)}
              {sel.kind !== "hub" && <p><a href={`#${panelPrefix}${PANEL_ANCHOR[sel.kind]}`}>Open the {TYPE_LABEL[sel.kind].toLowerCase()} panel</a></p>}
              <h5>Connected ({touches.length})</h5>
              <ul>{touches.slice(0, 40).map((i) => {
                const o = brain.nodes[i];
                return <li key={o.id}><button type="button" className="map-link-button" onClick={() => setSelected(i)}>
                  <span className={o.kind === "hub" ? "map-dot" : `map-dot is-${o.kind}`} style={o.kind === "hub" ? { background: `hsl(${o.hue} 70% 50%)` } : undefined} aria-hidden="true" />{clip(o.label, 60)}</button></li>;
              })}</ul>
              {touches.length > 40 && <p className="map-empty">and {touches.length - 40} more</p>}
            </>
          ) : <p className="map-empty">Click a node or pick a hub above to see what it touched. Dashed rings are stale or disputed.</p>}
        </aside>
      </div>
    </div>
  );
}
