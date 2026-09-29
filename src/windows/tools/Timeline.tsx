import { motion } from "motion/react";
import { X } from "lucide-react";
import { useRef, type PointerEvent as RPointerEvent, type ReactNode } from "react";
import { clamp, duration as fmt } from "../../lib/format";

export interface Span {
  id: number;
  start: number;
  end: number;
}

interface TimelineProps {
  duration: number;
  time: number;
  onSeek: (t: number) => void;
  /** Drawn under everything: thumbnails or a waveform. */
  children: ReactNode;
  /** A selection with draggable ends (trim). */
  range?: { start: number; end: number };
  onRange?: (r: { start: number; end: number }) => void;
  /** Cut points (split). */
  markers?: number[];
  onMarkers?: (m: number[]) => void;
  /** Regions added by dragging (bleep). */
  regions?: Span[];
  onRegions?: (r: Span[]) => void;
  regionColor?: string;
  height?: number;
}

let nextSpanId = 1;

export function Timeline(p: TimelineProps) {
  const ref = useRef<HTMLDivElement>(null);
  const drag = useRef<{ kind: string; index?: number; anchor?: number } | null>(null);
  const toTime = (clientX: number) => {
    const r = ref.current!.getBoundingClientRect();
    return clamp(((clientX - r.left) / r.width) * p.duration, 0, p.duration);
  };
  const pct = (t: number) => `${(t / Math.max(p.duration, 0.001)) * 100}%`;

  const down = (e: RPointerEvent, kind: string, index?: number) => {
    e.stopPropagation();
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    const t = toTime(e.clientX);
    if (kind === "track") {
      if (p.onRegions) {
        const span = { id: nextSpanId++, start: t, end: t };
        p.onRegions([...(p.regions ?? []), span]);
        drag.current = { kind: "region-new", index: (p.regions ?? []).length, anchor: t };
      } else {
        drag.current = { kind: "seek" };
        p.onSeek(t);
      }
      return;
    }
    drag.current = { kind, index };
  };

  const move = (e: RPointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const t = toTime(e.clientX);
    if (d.kind === "seek") p.onSeek(t);
    else if (d.kind === "start" && p.range) p.onRange?.({ start: Math.min(t, p.range.end - 0.05), end: p.range.end });
    else if (d.kind === "end" && p.range) p.onRange?.({ start: p.range.start, end: Math.max(t, p.range.start + 0.05) });
    else if (d.kind === "marker" && p.markers && d.index !== undefined) {
      const next = p.markers.slice();
      next[d.index] = t;
      p.onMarkers?.(next);
    } else if (d.kind === "region-new" && p.regions && d.index !== undefined && d.anchor !== undefined) {
      const next = p.regions.slice();
      next[d.index] = { ...next[d.index], start: Math.min(d.anchor, t), end: Math.max(d.anchor, t) };
      p.onRegions?.(next);
    }
  };

  const up = () => {
    const d = drag.current;
    drag.current = null;
    // Drop regions that were clicks rather than drags.
    if (d?.kind === "region-new" && p.regions) p.onRegions?.(p.regions.filter((r) => r.end - r.start > 0.05));
  };

  const h = p.height ?? 72;
  const range = p.range;
  return (
    <div className="select-none px-1 pt-5 pb-1">
      <div
        ref={ref}
        className="relative w-full cursor-pointer overflow-visible rounded-xl bg-sunken ring-1 ring-line"
        style={{ height: h }}
        onPointerDown={(e) => down(e, "track")}
        onPointerMove={move}
        onPointerUp={up}
      >
        <div className="absolute inset-0 overflow-hidden rounded-xl">{p.children}</div>

        {range && (
          <>
            <div className="pointer-events-none absolute inset-y-0 left-0 rounded-l-xl bg-black/55" style={{ width: pct(range.start) }} />
            <div className="pointer-events-none absolute inset-y-0 right-0 rounded-r-xl bg-black/55" style={{ left: pct(range.end) }} />
            <div className="pointer-events-none absolute inset-y-0 rounded-md ring-2 ring-accent" style={{ left: pct(range.start), width: pct(range.end - range.start) }} />
            {(["start", "end"] as const).map((k) => (
              <div
                key={k}
                onPointerDown={(e) => down(e, k)}
                onPointerMove={move}
                onPointerUp={up}
                className="absolute inset-y-[-6px] z-10 flex w-4 -translate-x-1/2 cursor-ew-resize items-center justify-center rounded-md bg-accent shadow-md"
                style={{ left: pct(range[k]) }}
              >
                <div className="h-6 w-0.5 rounded-full bg-accent-ink/60" />
              </div>
            ))}
          </>
        )}

        {p.regions?.map((r, i) => (
          <div key={r.id} className="group absolute inset-y-0 rounded-md" style={{ left: pct(r.start), width: pct(r.end - r.start), background: p.regionColor ?? "rgb(239 68 68 / 0.45)" }}>
            <button
              title="Remove"
              onPointerDown={(e) => e.stopPropagation()}
              onClick={() => p.onRegions?.((p.regions ?? []).filter((_, j) => j !== i))}
              className="absolute -top-3 left-1/2 hidden size-5 -translate-x-1/2 place-items-center rounded-full bg-danger text-white shadow group-hover:grid"
            >
              <X className="size-3" />
            </button>
          </div>
        ))}

        {p.markers?.map((m, i) => (
          <div
            key={i}
            onPointerDown={(e) => down(e, "marker", i)}
            onPointerMove={move}
            onPointerUp={up}
            onDoubleClick={() => p.onMarkers?.((p.markers ?? []).filter((_, j) => j !== i))}
            title="Drag to move, double-click to remove"
            className="absolute inset-y-[-8px] z-10 w-3 -translate-x-1/2 cursor-ew-resize"
            style={{ left: pct(m) }}
          >
            <div className="mx-auto h-full w-0.5 bg-gold-400 shadow" />
            <div className="absolute -top-1 left-1/2 size-3 -translate-x-1/2 rotate-45 rounded-sm bg-gold-400" />
          </div>
        ))}

        <motion.div className="pointer-events-none absolute inset-y-[-10px] z-20 w-0.5 bg-white shadow-[0_0_0_1px_rgb(0_0_0/0.3)]" style={{ left: pct(p.time) }}>
          <div className="absolute -top-5 left-1/2 -translate-x-1/2 rounded bg-ink px-1.5 py-0.5 text-[10px] font-semibold tabular-nums text-canvas">{fmt(p.time, true)}</div>
        </motion.div>
      </div>
    </div>
  );
}
