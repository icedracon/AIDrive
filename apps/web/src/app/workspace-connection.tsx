"use client";

import { useEffect, useRef } from "react";
import styles from "./workspace-connection.module.css";

// The wire from the selected assistant to the conversation panel. It is
// redrawn from the assistant to the panel (left to right) when the selection
// changes, bends gently toward the pointer, and drives the light spot that
// follows the pointer over the workspace. Pointer effects run only with a fine
// pointer and without reduced motion; the loop sleeps when nothing moves.
const EASE_OUT = "cubic-bezier(0.23, 1, 0.32, 1)";
const FOLLOW = 0.14; // share of the remaining distance covered per frame
const PULL = 0.18; // how far the curve's handles lean toward the pointer
const MAX_PULL = 64; // px

type Geometry = { x1: number; y1: number; x2: number; y2: number; bend: number; mobile: boolean };

export default function WorkspaceConnection({ client, registryKey }: { client: string; registryKey: string }) {
  const svg = useRef<SVGSVGElement>(null);
  const drawn = useRef<string | null>(null);
  useEffect(() => {
    const element = svg.current;
    const layout = element?.parentElement;
    if (!element || !layout) return;
    const source = layout.querySelector<HTMLButtonElement>('[data-agent-client="' + CSS.escape(client) + '"]');
    const destination = layout.querySelector<HTMLElement>("[data-conversation-panel]");
    if (!source || !destination) return;
    const paths = element.querySelectorAll("path");
    const terminals = element.querySelectorAll("circle");
    const spot = layout.querySelector<HTMLElement>("[data-spotlight]");
    const lively = window.matchMedia("(hover: hover) and (pointer: fine)").matches
      && !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let geometry: Geometry | null = null;
    const pointer = { x: 0, y: 0, tx: 0, ty: 0, inside: false };
    let frame = 0;

    function draw() {
      if (!element || !geometry) return;
      const { x1, y1, x2, y2, bend, mobile } = geometry;
      let dx = 0, dy = 0;
      if (lively && !mobile && pointer.inside) {
        const clamp = (v: number) => Math.max(-MAX_PULL, Math.min(MAX_PULL, v));
        dx = clamp((pointer.x - (x1 + x2) / 2) * PULL);
        dy = clamp((pointer.y - (y1 + y2) / 2) * PULL);
      }
      const d = mobile
        ? `M ${x1} ${y1} C ${x1} ${y1 + bend}, ${x2} ${y2 - bend}, ${x2} ${y2}`
        : `M ${x1} ${y1} C ${x1 + bend + dx} ${y1 + dy}, ${x2 - bend + dx} ${y2 + dy}, ${x2} ${y2}`;
      paths.forEach(path => path.setAttribute("d", d));
      terminals[0]?.setAttribute("cx", String(x1)); terminals[0]?.setAttribute("cy", String(y1));
      terminals[1]?.setAttribute("cx", String(x2)); terminals[1]?.setAttribute("cy", String(y2));
    }

    function measure() {
      if (!element || !layout || !source || !destination) return;
      const bounds = layout.getBoundingClientRect();
      const from = source.getBoundingClientRect();
      const to = destination.getBoundingClientRect();
      const mobile = window.matchMedia("(max-width: 760px)").matches;
      const x1 = (mobile ? from.left + from.width / 2 : from.right) - bounds.left;
      const y1 = (mobile ? from.bottom : from.top + from.height / 2) - bounds.top;
      const x2 = (mobile ? to.left + to.width / 2 : to.left) - bounds.left;
      const y2 = (mobile ? to.top : to.top + to.height * .46) - bounds.top;
      geometry = { x1, y1, x2, y2, mobile, bend: Math.max(20, Math.abs(mobile ? y2 - y1 : x2 - x1) * .55) };
      element.setAttribute("viewBox", `0 0 ${bounds.width} ${bounds.height}`);
      draw();
    }

    // Draw the new wire from the assistant toward the panel, once per selection.
    function reveal() {
      if (!lively || drawn.current === client) return;
      const first = drawn.current === null;
      drawn.current = client;
      if (first) return; // the page's own entrance covers the first paint
      [paths[0], paths[1]].forEach(path => {
        if (!path) return;
        const length = path.getTotalLength();
        path.animate(
          [{ strokeDasharray: `${length} ${length}`, strokeDashoffset: length }, { strokeDasharray: `${length} ${length}`, strokeDashoffset: 0 }],
          { duration: 420, easing: EASE_OUT },
        );
      });
    }

    function tick() {
      pointer.x += (pointer.tx - pointer.x) * FOLLOW;
      pointer.y += (pointer.ty - pointer.y) * FOLLOW;
      draw();
      if (spot) spot.style.transform = `translate3d(${pointer.x}px, ${pointer.y}px, 0) translate(-50%, -50%)`;
      frame = Math.abs(pointer.tx - pointer.x) + Math.abs(pointer.ty - pointer.y) > .4 ? requestAnimationFrame(tick) : 0;
    }
    function wake() { if (!frame) frame = requestAnimationFrame(tick); }
    function move(event: PointerEvent) {
      if (!layout) return;
      const bounds = layout.getBoundingClientRect();
      pointer.tx = event.clientX - bounds.left;
      pointer.ty = event.clientY - bounds.top;
      if (!pointer.inside) { pointer.inside = true; pointer.x = pointer.tx; pointer.y = pointer.ty; spot?.setAttribute("data-active", "true"); }
      wake();
    }
    function leave() {
      pointer.inside = false;
      spot?.removeAttribute("data-active");
      draw();
    }

    const observer = new ResizeObserver(measure);
    [layout, source, destination].forEach(node => observer.observe(node));
    window.addEventListener("resize", measure);
    if (lively) { layout.addEventListener("pointermove", move); layout.addEventListener("pointerleave", leave); }
    measure();
    reveal();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
      layout.removeEventListener("pointermove", move);
      layout.removeEventListener("pointerleave", leave);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [client, registryKey]);
  return <svg ref={svg} className={styles.connection} aria-hidden="true" focusable="false" data-workspace-connection>
    <path className={styles.glow} /><path className={styles.wire} /><path className={styles.sheen} /><path className={styles.pulse} pathLength={100} />
    <circle r="3" /><circle r="4" />
  </svg>;
}
