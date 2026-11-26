"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { buildGraph, loadMap, type MapData } from "./map-data";
import MapTree from "./map-tree";
import MapGraph from "./map-graph";
import MapTimeline from "./map-timeline";
import MapBrain from "./map-brain";

type Mode = "tree" | "graph" | "timeline" | "brain";
const MODES: { id: Mode; label: string }[] = [{ id: "tree", label: "Tree" }, { id: "graph", label: "Graph" }, { id: "timeline", label: "Timeline" }, { id: "brain", label: "Brain" }];

export default function MapPanel({ workspace }: { workspace: { id: string; name: string; role: string } }) {
  const [mode, setMode] = useState<Mode>("tree");
  const [data, setData] = useState<MapData | null>(null);
  const [error, setError] = useState(false);
  const [reload, setReload] = useState(0);
  const [focus, setFocus] = useState<string | null>(null);
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);

  useEffect(() => {
    const controller = new AbortController();
    loadMap(workspace.id, workspace.role === "owner", controller.signal)
      .then((d) => { if (!controller.signal.aborted) { setData(d); setError(false); } })
      .catch(() => { if (!controller.signal.aborted) setError(true); });
    return () => controller.abort();
  }, [workspace.id, workspace.role, reload]);

  useEffect(() => {
    const refresh = () => setReload((n) => n + 1);
    window.addEventListener("aidrive:review-changed", refresh);
    return () => window.removeEventListener("aidrive:review-changed", refresh);
  }, []);

  const graph = useMemo(() => (data ? buildGraph(data) : null), [data]);

  function onTabKey(e: React.KeyboardEvent, i: number) {
    const next = e.key === "ArrowRight" ? (i + 1) % MODES.length : e.key === "ArrowLeft" ? (i + MODES.length - 1) % MODES.length : e.key === "Home" ? 0 : e.key === "End" ? MODES.length - 1 : -1;
    if (next < 0) return;
    e.preventDefault();
    setMode(MODES[next].id);
    tabs.current[next]?.focus();
  }

  const counts = data && `${data.files.length} files · ${data.knowledge.length} knowledge · ${data.handoffs.length} handoffs · ${data.sessions.length} sessions · ${data.messages.length} messages`;

  return (
    <section className="agent-access map-panel" id="panel-map" aria-labelledby={`map-title-${workspace.id}`}>
      <div className="map-head">
        <div>
          <h3 id={`map-title-${workspace.id}`}>Map · {workspace.name}</h3>
          <p>Everything in this space in one view{counts ? `: ${counts}` : ""}.</p>
        </div>
        <button className="secondary-button" type="button" onClick={() => { setError(false); setData(null); setReload((n) => n + 1); }}>Refresh map</button>
      </div>
      <div className="map-tabs" role="tablist" aria-label="Map mode">
        {MODES.map((m, i) => (
          <button key={m.id} ref={(el) => { tabs.current[i] = el; }} type="button" role="tab" id={`map-tab-${m.id}`} aria-selected={mode === m.id} aria-controls="map-view" tabIndex={mode === m.id ? 0 : -1}
            className={mode === m.id ? "active" : ""} onClick={() => setMode(m.id)} onKeyDown={(e) => onTabKey(e, i)}>{m.label}</button>
        ))}
      </div>
      {data && data.partial.length > 0 && <p className="map-warning" role="status">Some parts could not be loaded and are missing: {data.partial.join(", ")}.</p>}
      <div id="map-view" role="tabpanel" aria-labelledby={`map-tab-${mode}`}>
        {error ? <p role="alert">The map could not be loaded. Refresh to try again.</p>
          : !data || !graph ? <p aria-busy="true" className="map-loading">Loading the map…</p>
          : mode === "tree" ? <MapTree data={data} onShowInGraph={(id) => { setFocus(id); setMode("graph"); requestAnimationFrame(() => document.getElementById("panel-map")?.scrollIntoView({ block: "start" })); }} />
          : mode === "graph" ? <MapGraph nodes={graph.nodes} edges={graph.edges} focus={focus} panelPrefix="panel-" />
          : mode === "timeline" ? <MapTimeline data={data} />
          : <MapBrain data={data} graph={graph} panelPrefix="panel-" />}
      </div>
    </section>
  );
}
