"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { agentLabel, clip, shortTime, type MapData } from "./map-data";

type Cat = "file" | "handoff" | "decision" | "knowledge" | "message" | "agent";
type Mark = { key: string; at: number; lane: string; cat: Cat; title: string; detail: string };

const CATS: Cat[] = ["file", "handoff", "decision", "knowledge", "message", "agent"];
const CAT_LABEL: Record<Cat, string> = { file: "File changes", handoff: "Handoffs", decision: "Decisions", knowledge: "Knowledge proposals", message: "Messages", agent: "Key changes" };
const LABEL_W = 150;
const LANE_H = 44;
const AXIS_H = 26;
const DAY = 86_400_000;
const STEPS = [3_600_000, 3 * 3_600_000, 6 * 3_600_000, 12 * 3_600_000, DAY, 2 * DAY, 7 * DAY, 14 * DAY, 30 * DAY, 91 * DAY];

function buildMarks(data: MapData): Mark[] {
  const humans = new Map<string, number>();
  for (const e of data.activity) if (!e.agent_id && e.actor_email) humans.set(e.actor_email, (humans.get(e.actor_email) ?? 0) + 1);
  const owner = [...humans.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "Owner";
  const lane = (agentId: string | null, email: string | null) => (agentId ? agentLabel(data, agentId) : email ?? owner);
  const marks: Mark[] = [];
  data.activity.forEach((e, i) => {
    const cat: Cat = e.kind.startsWith("file.") ? "file" : e.kind === "handoff.proposed" ? "handoff" : e.kind.startsWith("handoff.") ? "decision" : "agent";
    const verb = e.kind.split(".")[1];
    const title = cat === "file" ? `${verb} ${e.subject ?? "file"}${e.revision ? ` · r${e.revision}` : ""}` : cat === "agent" ? `Key ${verb}: ${e.subject ?? ""}` : `Handoff ${verb}${e.subject ? `: ${clip(e.subject, 80)}` : ""}`;
    marks.push({ key: `a${i}`, at: Date.parse(e.at), lane: lane(e.agent_id, e.actor_email), cat, title, detail: e.kind });
  });
  for (const k of data.knowledge) {
    marks.push({ key: `kp-${k.id}`, at: Date.parse(k.created_at), lane: lane(k.agent_id, null), cat: "knowledge", title: `Proposed ${k.kind}: ${clip(k.statement, 90)}`, detail: `now ${k.status}${k.stale ? " · stale" : ""}` });
    if (k.decided_at) marks.push({ key: `kd-${k.id}`, at: Date.parse(k.decided_at), lane: owner, cat: "decision", title: `Knowledge ${k.status === "rejected" ? "rejected" : "approved"}: ${clip(k.statement, 90)}`, detail: k.kind });
  }
  for (const m of data.messages) {
    marks.push({ key: `m-${m.id}`, at: Date.parse(m.created_at), lane: lane(m.from_agent_id, m.from_email), cat: "message", title: clip(m.subject || m.body, 100), detail: `to ${m.to === "agent" ? m.to_agent_name ?? "assistant" : m.to}` });
  }
  return marks.filter((m) => Number.isFinite(m.at)).sort((a, b) => a.at - b.at);
}

function dayStart(t: number) { return Math.floor(t / DAY) * DAY; }
function toInput(t: number) { return new Date(t).toISOString().slice(0, 10); }
function tickLabel(t: number, step: number) {
  const iso = new Date(t).toISOString();
  return step < DAY ? iso.slice(5, 16).replace("T", " ") : iso.slice(0, 10);
}

export default function MapTimeline({ data }: { data: MapData }) {
  const marks = useMemo(() => buildMarks(data), [data]);
  const [cats, setCats] = useState<Set<Cat>>(() => new Set(CATS));
  const extent = useMemo<[number, number]>(() => marks.length ? [dayStart(marks[0].at), dayStart(marks[marks.length - 1].at) + DAY] : [0, DAY], [marks]);
  const [range, setRange] = useState<[number, number] | null>(null);
  const [from, to] = range ?? extent;
  const [width, setWidth] = useState(800);
  const [tip, setTip] = useState<{ mark: Mark; x: number; y: number } | null>(null);
  const [brush, setBrush] = useState<[number, number] | null>(null);
  const wrap = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => setWidth(Math.max(360, Math.round(entry.contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const visible = marks.filter((m) => cats.has(m.cat) && m.at >= from && m.at <= to);
  const lanes = useMemo(() => {
    const seen = new Map<string, boolean>();
    for (const m of marks) seen.set(m.lane, m.lane.includes("@") || m.lane === "Owner");
    return [...seen.entries()].sort((a, b) => Number(b[1]) - Number(a[1]) || a[0].localeCompare(b[0])).map(([l]) => l);
  }, [marks]);
  const plotW = width - LABEL_W - 12;
  const x = (t: number) => LABEL_W + ((t - from) / Math.max(1, to - from)) * plotW;
  const tFromX = (px: number) => from + ((px - LABEL_W) / plotW) * (to - from);
  const step = STEPS.find((s) => (to - from) / s <= Math.max(4, Math.floor(plotW / 110))) ?? STEPS[STEPS.length - 1];
  const ticks: number[] = [];
  for (let t = Math.ceil(from / step) * step; t <= to; t += step) ticks.push(t);
  const height = AXIS_H + lanes.length * LANE_H + 8;

  function preset(days: number | null) {
    if (days === null) return setRange(null);
    const end = extent[1];
    setRange([Math.max(extent[0], end - days * DAY), end]);
  }

  function localX(e: React.PointerEvent) {
    return e.clientX - (e.currentTarget as Element).getBoundingClientRect().left;
  }

  return (
    <div className="map-timeline">
      <div className="map-toolbar">
        <label className="map-search">From<input type="date" value={toInput(from)} min={toInput(extent[0])} max={toInput(to)} onChange={(e) => { const t = Date.parse(e.target.value); if (Number.isFinite(t) && t < to) setRange([t, to]); }} /></label>
        <label className="map-search">To<input type="date" value={toInput(to - 1)} min={toInput(from)} max={toInput(extent[1] - 1)} onChange={(e) => { const t = Date.parse(e.target.value); if (Number.isFinite(t) && t + DAY > from) setRange([from, t + DAY]); }} /></label>
        {([["1 day", 1], ["7 days", 7], ["30 days", 30], ["All", null]] as const).map(([label, d]) => <button key={label} type="button" className="secondary-button" onClick={() => preset(d)}>{label}</button>)}
      </div>
      <fieldset className="map-type-filter"><legend className="sr-only">Show event types</legend>
        {CATS.map((c) => (
          <label key={c} className="map-chip">
            <input type="checkbox" checked={cats.has(c)} onChange={() => setCats((s) => { const n = new Set(s); if (n.has(c)) n.delete(c); else n.add(c); return n; })} />
            <svg width="12" height="12" aria-hidden="true"><g transform="translate(6 6)" className={`map-mark is-${c}`}><MarkShape cat={c} /></g></svg>{CAT_LABEL[c]} <small>{marks.filter((m) => m.cat === c).length}</small>
          </label>
        ))}
      </fieldset>
      <div className="map-canvas" ref={wrap}>
        {lanes.length === 0 ? <p className="map-empty">No activity in this space yet.</p> : (
          <svg width={width} height={height} role="group" aria-label={`Timeline from ${toInput(from)} to ${toInput(to - 1)}: ${visible.length} events in ${lanes.length} lanes. Drag across the time axis to zoom into a range.`}
            onPointerDown={(e) => { const px = localX(e); if (px > LABEL_W && !(e.target as Element).closest(".map-mark")) { (e.currentTarget as Element).setPointerCapture(e.pointerId); setBrush([px, px]); } }}
            onPointerMove={(e) => { if (brush) setBrush([brush[0], Math.max(LABEL_W, Math.min(width - 12, localX(e)))]); }}
            onPointerUp={() => { if (brush && Math.abs(brush[1] - brush[0]) > 6) { const a = tFromX(Math.min(...brush)), b = tFromX(Math.max(...brush)); setRange([a, b]); } setBrush(null); }}>
            {ticks.map((t) => (
              <g key={t}>
                <line className="map-grid" x1={x(t)} x2={x(t)} y1={AXIS_H - 6} y2={height} />
                {x(t) < width - 80 && <text className="map-axis" x={x(t) + 3} y={14}>{tickLabel(t, step)}</text>}
              </g>
            ))}
            {lanes.map((lane, li) => {
              const y = AXIS_H + li * LANE_H;
              return (
                <g key={lane}>
                  <rect className={`map-lane ${li % 2 ? "odd" : ""}`} x={0} y={y} width={width} height={LANE_H} />
                  <text className="map-lane-label" x={8} y={y + LANE_H / 2 + 4}>{clip(lane, 20)}</text>
                  {visible.filter((m) => m.lane === lane).map((m) => {
                    const mx = x(m.at), my = y + 8 + CATS.indexOf(m.cat) * ((LANE_H - 16) / (CATS.length - 1));
                    const show = (ev: { currentTarget: Element }) => {
                      const host = wrap.current!.getBoundingClientRect(), r = ev.currentTarget.getBoundingClientRect();
                      setTip({ mark: m, x: r.left - host.left + r.width / 2, y: r.top - host.top });
                    };
                    return (
                      <g key={m.key} className={`map-mark is-${m.cat}`} transform={`translate(${mx} ${my})`} tabIndex={0} role="img"
                        aria-label={`${shortTime(new Date(m.at).toISOString())}, ${lane}: ${m.title}`}
                        onPointerEnter={show} onFocus={show} onPointerLeave={() => setTip(null)} onBlur={() => setTip(null)}>
                        <MarkShape cat={m.cat} />
                      </g>
                    );
                  })}
                </g>
              );
            })}
            {brush && <rect className="map-brush" x={Math.min(...brush)} y={0} width={Math.abs(brush[1] - brush[0])} height={height} />}
          </svg>
        )}
        {tip && (
          <div className="map-tip" role="tooltip" style={{ left: Math.min(Math.max(tip.x, 120), width - 120), top: tip.y }}>
            <strong>{tip.mark.title}</strong>
            <small>{tip.mark.lane} · {shortTime(new Date(tip.mark.at).toISOString())} · {tip.mark.detail}</small>
          </div>
        )}
      </div>
      <p className="map-hint">{visible.length} of {marks.length} events shown{data.activityTruncated ? " · only the latest 500 activity events are loaded" : ""}. Drag across the chart to zoom into a range; choose All to reset.</p>
    </div>
  );
}

function MarkShape({ cat }: { cat: Cat }) {
  switch (cat) {
    case "file": return <circle r={4.5} />;
    case "handoff": return <rect x={-4.5} y={-4.5} width={9} height={9} rx={1.5} />;
    case "decision": return <path d="M0 -6 6 0 0 6 -6 0z" />;
    case "knowledge": return <path d="M0 -5.5 5.5 4.5 -5.5 4.5z" />;
    case "message": return <circle r={4} className="ring" />;
    default: return <path d="M-4 -4 4 4M4 -4 -4 4" className="cross" />;
  }
}
