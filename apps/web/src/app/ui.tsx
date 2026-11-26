"use client";

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import "./panels.css";

// Two-step inline confirm instead of window.confirm: the first press arms the
// button for 3s and says what will happen; the second press acts.
export function ConfirmButton({ label, confirmLabel, busyLabel, busy, disabled, onConfirm, className = "secondary-button", children }: {
  label: string;
  confirmLabel: string;
  busyLabel?: string;
  busy?: boolean;
  disabled?: boolean;
  onConfirm: () => void;
  className?: string;
  children?: React.ReactNode;
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const timer = window.setTimeout(() => setArmed(false), 3000);
    return () => window.clearTimeout(timer);
  }, [armed]);
  return (
    <button
      className={`${className}${armed ? " is-armed" : ""}`}
      type="button"
      disabled={disabled || busy}
      aria-live="polite"
      onBlur={() => setArmed(false)}
      onKeyDown={(e) => { if (e.key === "Escape") setArmed(false); }}
      onClick={() => { if (armed) { setArmed(false); onConfirm(); } else setArmed(true); }}
    >
      {busy ? busyLabel ?? "Working…" : armed ? confirmLabel : label}{children}
    </button>
  );
}

// Long text clamped to a few lines, with a toggle only when it actually overflows.
export function ClampText({ text, lines = 3, className }: { text: string; lines?: number; className?: string }) {
  const ref = useRef<HTMLParagraphElement>(null);
  const [open, setOpen] = useState(false);
  const [overflows, setOverflows] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || open) return;
    const measure = () => setOverflows(el.scrollHeight > el.clientHeight + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [text, open]);
  return (
    <div className={className}>
      <p ref={ref} className={open ? "clamp-text" : "clamp-text is-clamped"} style={{ "--lines": lines } as React.CSSProperties}>{text}</p>
      {(overflows || open) && <button className="link-button" type="button" aria-expanded={open} onClick={() => setOpen(!open)}>{open ? "Show less" : "Show all"}</button>}
    </div>
  );
}

// A list that shows its first few entries and a "+N more" toggle.
export function ShortList({ items, first = 3 }: { items: string[]; first?: number }) {
  const [open, setOpen] = useState(false);
  if (items.length === 0) return null;
  const shown = open ? items : items.slice(0, first);
  const hidden = items.length - shown.length;
  return (
    <>
      <ul>{shown.map((d, i) => <li key={i}>{d}</li>)}</ul>
      {(hidden > 0 || open) && items.length > first && <button className="link-button" type="button" aria-expanded={open} onClick={() => setOpen(!open)}>{open ? "Show fewer" : `+${hidden} more`}</button>}
    </>
  );
}

export function plural(n: number, one: string, many: string) {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

export function utc(t: string | null | undefined) {
  return t ? `${t.replace("T", " ").slice(0, 16)} UTC` : "";
}

// Reduced motion: removal collapses instantly instead of over 200ms.
export function exitDelay() {
  return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 200;
}
