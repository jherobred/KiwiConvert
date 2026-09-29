import { AnimatePresence, motion } from "motion/react";
import {
  ArrowUpRight,
  Circle,
  Crop as CropIcon,
  EyeOff,
  FlipHorizontal2,
  FlipVertical2,
  Highlighter,
  Pen,
  PenLine,
  Redo2,
  RotateCcw,
  RotateCw,
  SlidersHorizontal,
  Square,
  Type,
  Undo2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as RPointerEvent } from "react";
import { api, fileUrl } from "../../../lib/ipc";
import { baseName } from "../../../lib/format";
import { spring } from "../../../lib/motion";
import { Button, IconButton, Section, Segmented, Slider, TitleBar } from "../../../components/ui";
import type { ToolProps } from "../ToolApp";
import {
  Adjuster,
  exportImage,
  noAdjustments,
  orientedSize,
  paintRedactions,
  paintShapes,
  renderGeometry,
  type Adjustments,
  type Geometry,
  type Rect,
  type RedactStyle,
  type Redaction,
  type Shape,
} from "./pipeline";

type Tab = "crop" | "adjust" | "annotate" | "redact";
type DrawTool = "pen" | "highlighter" | "arrow" | "rect" | "ellipse" | "text";

const PREVIEW_MAX = 2048;
const COLORS = ["#ef4444", "#facc15", "#22c55e", "#3b82f6", "#ffffff", "#111111"];
const ASPECTS: { label: string; value: number | "original" | null }[] = [
  { label: "Free", value: null },
  { label: "Original", value: "original" },
  { label: "1:1", value: 1 },
  { label: "4:3", value: 4 / 3 },
  { label: "3:2", value: 3 / 2 },
  { label: "16:9", value: 16 / 9 },
  { label: "9:16", value: 9 / 16 },
];
const ADJUST_LABELS: [keyof Adjustments, string][] = [
  ["exposure", "Exposure"],
  ["brightness", "Brightness"],
  ["contrast", "Contrast"],
  ["highlights", "Highlights"],
  ["shadows", "Shadows"],
  ["saturation", "Saturation"],
  ["warmth", "Warmth"],
  ["tint", "Tint"],
  ["vignette", "Vignette"],
];

interface Loaded {
  bitmap: ImageBitmap;
  w: number;
  h: number;
}

interface History {
  shapes: Shape[];
  redactions: Redaction[];
}

let nextId = 1;

function clampRect(r: Rect, w: number, h: number): Rect {
  const x = Math.max(0, Math.min(r.x, w - 1));
  const y = Math.max(0, Math.min(r.y, h - 1));
  return { x, y, w: Math.max(1, Math.min(r.w, w - x)), h: Math.max(1, Math.min(r.h, h - y)) };
}

function normalize(a: [number, number], b: [number, number]): Rect {
  return { x: Math.min(a[0], b[0]), y: Math.min(a[1], b[1]), w: Math.abs(b[0] - a[0]), h: Math.abs(b[1] - a[1]) };
}

export default function ImageEditor({ session, close }: ToolProps) {
  const path = session.paths[0];
  const [tab, setTab] = useState<Tab>(() => (["crop", "adjust", "annotate", "redact"].includes(session.tool) ? (session.tool as Tab) : "adjust"));
  const [img, setImg] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [geometry, setGeometry] = useState<Geometry>({ quarter: 0, flipH: false, flipV: false, angle: 0 });
  const [adjust, setAdjust] = useState<Adjustments>(noAdjustments);
  const [crop, setCrop] = useState<Rect | null>(null);
  const [aspect, setAspect] = useState<number | "original" | null>(null);
  const [history, setHistory] = useState<History[]>([{ shapes: [], redactions: [] }]);
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState<Shape | Redaction | null>(null);
  const [drawTool, setDrawTool] = useState<DrawTool>("arrow");
  const [color, setColor] = useState(COLORS[0]);
  const [strokeScale, setStrokeScale] = useState(1);
  const [redactStyle, setRedactStyle] = useState<RedactStyle>("black");
  const [comparing, setComparing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [textAt, setTextAt] = useState<[number, number] | null>(null);
  const [textValue, setTextValue] = useState("");
  const [box, setBox] = useState({ w: 800, h: 600 });

  const stageRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const adjuster = useMemo(() => new Adjuster(), []);

  // Load the image (HEIC, TIFF and friends come through a temporary PNG).
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const preview = await api.imagePreview(path);
        const blob = await (await fetch(fileUrl(preview.path))).blob();
        const bitmap = await createImageBitmap(blob, { imageOrientation: "from-image" });
        if (alive) setImg({ bitmap, w: bitmap.width, h: bitmap.height });
      } catch (e) {
        if (alive) setError(String(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, [path]);

  useLayoutEffect(() => {
    const el = stageRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBox({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const [ow, oh] = img ? orientedSize(img.w, img.h, geometry) : [1, 1];
  const fullRect: Rect = { x: 0, y: 0, w: ow, h: oh };
  const cropRect = crop ?? fullRect;
  const current = history[step];
  const baseWidth = Math.max(3, Math.round(Math.max(ow, oh) / 260));

  // Geometry at preview size.
  const previewScale = Math.min(1, PREVIEW_MAX / Math.max(ow, oh));
  const base = useMemo(() => (img ? renderGeometry(img.bitmap, img.w, img.h, geometry, previewScale) : null), [img, geometry, previewScale]);

  // Adjustments, redactions and shapes, composed at preview size.
  const composite = useMemo(() => {
    if (!base) return null;
    const adjusted = adjuster.render(base, comparing ? noAdjustments : adjust);
    const out = document.createElement("canvas");
    out.width = base.width;
    out.height = base.height;
    const ctx = out.getContext("2d")!;
    ctx.drawImage(adjusted, 0, 0);
    if (!comparing) {
      const reds = [...current.redactions, ...(draft && "style" in draft ? [draft] : [])];
      paintRedactions(ctx, adjusted, reds, previewScale);
      const shapes = [...current.shapes, ...(draft && "kind" in draft ? [draft] : [])];
      paintShapes(ctx, shapes, previewScale);
    }
    return out;
  }, [base, adjust, comparing, current, draft, adjuster, previewScale]);

  // What part of the image the stage shows, and at what scale.
  const view = tab === "crop" ? fullRect : cropRect;
  const pad = tab === "crop" ? 36 : 20;
  const scale = Math.min((box.w - pad * 2) / view.w, (box.h - pad * 2) / view.h);
  const offX = (box.w - view.w * scale) / 2;
  const offY = (box.h - view.h * scale) / 2;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !composite) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(box.w * dpr);
    canvas.height = Math.round(box.h * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, box.w, box.h);
    ctx.imageSmoothingQuality = "high";
    const k = previewScale;
    ctx.drawImage(composite, view.x * k, view.y * k, view.w * k, view.h * k, offX, offY, view.w * scale, view.h * scale);
  }, [composite, box, view.x, view.y, view.w, view.h, scale, offX, offY, previewScale]);

  const toImage = useCallback(
    (e: { clientX: number; clientY: number }): [number, number] => {
      const r = stageRef.current!.getBoundingClientRect();
      const x = view.x + (e.clientX - r.left - offX) / scale;
      const y = view.y + (e.clientY - r.top - offY) / scale;
      return [Math.max(0, Math.min(ow, x)), Math.max(0, Math.min(oh, y))];
    },
    [view.x, view.y, offX, offY, scale, ow, oh],
  );

  const commit = (next: History) => {
    const list = history.slice(0, step + 1);
    list.push(next);
    setHistory(list);
    setStep(list.length - 1);
  };
  const undo = () => setStep((s) => Math.max(0, s - 1));
  const redo = () => setStep((s) => Math.min(history.length - 1, s + 1));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      if (e.ctrlKey && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      } else if (e.ctrlKey && e.key.toLowerCase() === "y") {
        e.preventDefault();
        redo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // --- pointer handling on the stage -----------------------------------------------------
  const drag = useRef<{ start: [number, number]; mode: string; orig: Rect } | null>(null);

  const aspectValue = aspect === "original" ? (img ? orientedSize(img.w, img.h, geometry)[0] / orientedSize(img.w, img.h, geometry)[1] : null) : aspect;

  const onPointerDown = (e: RPointerEvent) => {
    if (!img || e.button !== 0) return;
    (e.target as Element).setPointerCapture?.(e.pointerId);
    const p = toImage(e);
    if (tab === "crop") {
      const handle = (e.target as HTMLElement).dataset.handle;
      drag.current = { start: p, mode: handle ?? "new", orig: cropRect };
    } else if (tab === "annotate") {
      if (drawTool === "text") {
        setTextAt(p);
        setTextValue("");
        return;
      }
      const width = baseWidth * strokeScale;
      const id = nextId++;
      setDraft(
        drawTool === "pen" || drawTool === "highlighter"
          ? { id, kind: drawTool, points: [p], color, width }
          : { id, kind: drawTool, from: p, to: p, color, width },
      );
      drag.current = { start: p, mode: "draw", orig: cropRect };
    } else if (tab === "redact") {
      setDraft({ id: nextId++, x: p[0], y: p[1], w: 0, h: 0, style: redactStyle });
      drag.current = { start: p, mode: "redact", orig: cropRect };
    }
  };

  const onPointerMove = (e: RPointerEvent) => {
    const d = drag.current;
    if (!d || !img) return;
    const p = toImage(e);
    if (tab === "crop") {
      const dx = p[0] - d.start[0];
      const dy = p[1] - d.start[1];
      let r = { ...d.orig };
      if (d.mode === "move") {
        r.x = Math.max(0, Math.min(ow - r.w, d.orig.x + dx));
        r.y = Math.max(0, Math.min(oh - r.h, d.orig.y + dy));
      } else if (d.mode === "new") {
        r = normalize(d.start, p);
      } else {
        if (d.mode.includes("w")) {
          r.x = d.orig.x + dx;
          r.w = d.orig.w - dx;
        }
        if (d.mode.includes("e")) r.w = d.orig.w + dx;
        if (d.mode.includes("n")) {
          r.y = d.orig.y + dy;
          r.h = d.orig.h - dy;
        }
        if (d.mode.includes("s")) r.h = d.orig.h + dy;
        if (r.w < 0) {
          r.x += r.w;
          r.w = -r.w;
        }
        if (r.h < 0) {
          r.y += r.h;
          r.h = -r.h;
        }
      }
      if (aspectValue && d.mode !== "move") {
        // Keep the ratio by adjusting the height to the width.
        r.h = r.w / aspectValue;
        if (d.mode.includes("n")) r.y = d.orig.y + d.orig.h - r.h;
        if (r.y + r.h > oh) {
          r.h = oh - r.y;
          r.w = r.h * aspectValue;
        }
      }
      setCrop(clampRect({ x: r.x, y: r.y, w: Math.max(8, r.w), h: Math.max(8, r.h) }, ow, oh));
    } else if (draft && "kind" in draft) {
      if ("points" in draft) setDraft({ ...draft, points: [...draft.points, p] });
      else if ("from" in draft) setDraft({ ...draft, to: p });
    } else if (draft && "style" in draft) {
      setDraft({ ...draft, ...normalize(d.start, p) });
    }
  };

  const onPointerUp = () => {
    const d = drag.current;
    drag.current = null;
    if (!d || !draft) return;
    if ("style" in draft) {
      if (draft.w > 3 && draft.h > 3) commit({ ...current, redactions: [...current.redactions, draft] });
    } else if ("kind" in draft) {
      const tiny = "from" in draft && Math.hypot(draft.to[0] - draft.from[0], draft.to[1] - draft.from[1]) < 3;
      if (!tiny) commit({ ...current, shapes: [...current.shapes, draft] });
    }
    setDraft(null);
  };

  const commitText = () => {
    if (textAt && textValue.trim()) {
      const size = Math.max(18, Math.round(Math.max(ow, oh) / 28)) * strokeScale;
      commit({ ...current, shapes: [...current.shapes, { id: nextId++, kind: "text", at: textAt, text: textValue.trim(), color, size }] });
    }
    setTextAt(null);
  };

  // Changing orientation invalidates the crop.
  const turn = (q: number) => {
    setGeometry((g) => ({ ...g, quarter: (g.quarter + q + 4) % 4 }));
    setCrop(null);
  };

  const applyAspect = (value: number | "original" | null) => {
    setAspect(value);
    if (!value || !img) return;
    const ratio = value === "original" ? ow / oh : value;
    let w = ow;
    let h = w / ratio;
    if (h > oh) {
      h = oh;
      w = h * ratio;
    }
    setCrop({ x: (ow - w) / 2, y: (oh - h) / 2, w, h });
  };

  const save = async () => {
    if (!img) return;
    setSaving(true);
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    try {
      const out = exportImage(img.bitmap, img.w, img.h, geometry, adjust, cropRect, current.redactions, current.shapes, new Adjuster());
      const suffix = tab === "redact" || current.redactions.length ? "-redacted" : current.shapes.length ? "-annotated" : "-edited";
      await api.saveRenderedImage(out.pixels, { source: path, width: out.width, height: out.height, suffix, title: baseName(path) });
      close();
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  };

  const changed =
    crop !== null ||
    geometry.quarter !== 0 ||
    geometry.flipH ||
    geometry.flipV ||
    geometry.angle !== 0 ||
    (Object.keys(adjust) as (keyof Adjustments)[]).some((k) => adjust[k] !== 0) ||
    current.shapes.length > 0 ||
    current.redactions.length > 0;

  const tabs: { value: Tab; label: string; icon: typeof CropIcon }[] = [
    { value: "crop", label: "Crop", icon: CropIcon },
    { value: "adjust", label: "Adjust", icon: SlidersHorizontal },
    { value: "annotate", label: "Annotate", icon: PenLine },
    { value: "redact", label: "Redact", icon: EyeOff },
  ];

  // Crop overlay in screen coordinates.
  const sx = (x: number) => offX + (x - view.x) * scale;
  const sy = (y: number) => offY + (y - view.y) * scale;
  const cropScreen = { x: sx(cropRect.x), y: sy(cropRect.y), w: cropRect.w * scale, h: cropRect.h * scale };

  return (
    <>
      <TitleBar title="Edit photo" subtitle={baseName(path)}>
        <div className="mr-2 flex items-center gap-0.5">
          <IconButton label="Undo (Ctrl+Z)" onClick={undo} disabled={step === 0}>
            <Undo2 className="size-4" />
          </IconButton>
          <IconButton label="Redo (Ctrl+Y)" onClick={redo} disabled={step >= history.length - 1}>
            <Redo2 className="size-4" />
          </IconButton>
        </div>
      </TitleBar>
      <div className="flex min-h-0 flex-1">
        <div
          ref={stageRef}
          className="relative min-w-0 flex-1 overflow-hidden bg-[repeating-conic-gradient(var(--sunken)_0_25%,var(--canvas)_0_50%)] bg-[length:22px_22px]"
          style={{ cursor: tab === "crop" ? "crosshair" : tab === "adjust" ? "default" : "crosshair", touchAction: "none" }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
        >
          <canvas ref={canvasRef} className="absolute inset-0 size-full" />
          {!img && !error && <div className="absolute inset-0 grid place-items-center text-sm text-ink-3">Opening photo…</div>}
          {error && <div className="absolute inset-0 grid place-items-center p-8 text-center text-sm text-danger">{error}</div>}

          {tab === "crop" && img && (
            <div className="pointer-events-none absolute inset-0">
              <svg className="absolute inset-0 size-full">
                <path
                  fillRule="evenodd"
                  fill="rgb(0 0 0 / 0.55)"
                  d={`M0 0H${box.w}V${box.h}H0Z M${cropScreen.x} ${cropScreen.y}h${cropScreen.w}v${cropScreen.h}h${-cropScreen.w}Z`}
                />
                {[1, 2].map((i) => (
                  <g key={i} stroke="rgb(255 255 255 / 0.45)" strokeWidth={1}>
                    <line x1={cropScreen.x + (cropScreen.w * i) / 3} x2={cropScreen.x + (cropScreen.w * i) / 3} y1={cropScreen.y} y2={cropScreen.y + cropScreen.h} />
                    <line y1={cropScreen.y + (cropScreen.h * i) / 3} y2={cropScreen.y + (cropScreen.h * i) / 3} x1={cropScreen.x} x2={cropScreen.x + cropScreen.w} />
                  </g>
                ))}
                <rect x={cropScreen.x} y={cropScreen.y} width={cropScreen.w} height={cropScreen.h} fill="none" stroke="white" strokeWidth={2} />
              </svg>
              <div
                data-handle="move"
                className="pointer-events-auto absolute cursor-move"
                style={{ left: cropScreen.x + 12, top: cropScreen.y + 12, width: Math.max(0, cropScreen.w - 24), height: Math.max(0, cropScreen.h - 24) }}
              />
              {(["nw", "n", "ne", "e", "se", "s", "sw", "w"] as const).map((hnd) => {
                const hx = hnd.includes("w") ? 0 : hnd.includes("e") ? 1 : 0.5;
                const hy = hnd.includes("n") ? 0 : hnd.includes("s") ? 1 : 0.5;
                const corner = hnd.length === 2;
                return (
                  <div
                    key={hnd}
                    data-handle={hnd}
                    className="pointer-events-auto absolute rounded-full bg-white shadow-md ring-2 ring-black/20"
                    style={{
                      left: cropScreen.x + cropScreen.w * hx - (corner ? 9 : 7),
                      top: cropScreen.y + cropScreen.h * hy - (corner ? 9 : 7),
                      width: corner ? 18 : 14,
                      height: corner ? 18 : 14,
                      cursor: `${hnd}-resize`,
                    }}
                  />
                );
              })}
            </div>
          )}

          {tab === "redact" &&
            current.redactions.map((r) => (
              <button
                key={r.id}
                title="Remove"
                onPointerDown={(e) => e.stopPropagation()}
                onClick={() => commit({ ...current, redactions: current.redactions.filter((x) => x.id !== r.id) })}
                className="absolute grid size-6 place-items-center rounded-full bg-danger text-white shadow"
                style={{ left: sx(r.x + r.w) - 12, top: sy(r.y) - 12 }}
              >
                <X className="size-3.5" />
              </button>
            ))}

          {textAt && (
            <input
              autoFocus
              value={textValue}
              onChange={(e) => setTextValue(e.target.value)}
              onPointerDown={(e) => e.stopPropagation()}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitText();
                if (e.key === "Escape") setTextAt(null);
              }}
              onBlur={commitText}
              placeholder="Type, then press Enter"
              className="absolute rounded-lg bg-black/60 px-2 py-1 text-lg font-bold text-white outline-none ring-2 ring-accent"
              style={{ left: sx(textAt[0]), top: sy(textAt[1]), color }}
            />
          )}

          <AnimatePresence>
            {saving && (
              <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="absolute inset-0 grid place-items-center bg-black/40 text-sm font-semibold text-white backdrop-blur-sm">
                Rendering full size…
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        <aside className="flex w-[300px] shrink-0 flex-col border-l border-line bg-canvas">
          <div className="grid grid-cols-4 gap-1 p-3">
            {tabs.map((t) => (
              <button
                key={t.value}
                onClick={() => setTab(t.value)}
                className={`relative flex flex-col items-center gap-1 rounded-xl py-2 text-[11px] font-semibold transition-colors ${tab === t.value ? "text-accent-ink" : "text-ink-3 hover:text-ink"}`}
              >
                {tab === t.value && <motion.span layoutId="editor-tab" className="absolute inset-0 rounded-xl bg-accent" transition={spring.snap} />}
                <t.icon className="relative size-4.5" />
                <span className="relative">{t.label}</span>
              </button>
            ))}
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
            <AnimatePresence mode="wait" initial={false}>
              <motion.div key={tab} initial={{ opacity: 0, x: 12 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -12 }} transition={spring.soft} className="flex flex-col gap-2.5">
                {tab === "crop" && (
                  <>
                    <Section title="Aspect ratio">
                      <div className="grid grid-cols-4 gap-1.5 pt-1">
                        {ASPECTS.map((a) => (
                          <button
                            key={a.label}
                            onClick={() => applyAspect(a.value)}
                            className={`rounded-lg py-1.5 text-xs font-semibold ring-1 ${aspect === a.value ? "bg-accent text-accent-ink ring-accent" : "bg-surface text-ink-2 ring-line hover:text-ink"}`}
                          >
                            {a.label}
                          </button>
                        ))}
                      </div>
                    </Section>
                    <Section title="Rotate and flip">
                      <div className="grid grid-cols-4 gap-1.5 pt-1">
                        <IconButton label="Rotate left" className="size-10 bg-surface ring-1 ring-line" onClick={() => turn(-1)}>
                          <RotateCcw className="size-4.5" />
                        </IconButton>
                        <IconButton label="Rotate right" className="size-10 bg-surface ring-1 ring-line" onClick={() => turn(1)}>
                          <RotateCw className="size-4.5" />
                        </IconButton>
                        <IconButton label="Flip horizontally" className="size-10 bg-surface ring-1 ring-line" onClick={() => setGeometry((g) => ({ ...g, flipH: !g.flipH }))}>
                          <FlipHorizontal2 className="size-4.5" />
                        </IconButton>
                        <IconButton label="Flip vertically" className="size-10 bg-surface ring-1 ring-line" onClick={() => setGeometry((g) => ({ ...g, flipV: !g.flipV }))}>
                          <FlipVertical2 className="size-4.5" />
                        </IconButton>
                      </div>
                      <Slider label="Straighten" min={-45} max={45} step={0.5} center={0} value={geometry.angle} format={(v) => `${v}°`} onChange={(v) => setGeometry((g) => ({ ...g, angle: v }))} />
                    </Section>
                    <Button variant="ghost" onClick={() => (setCrop(null), setAspect(null), setGeometry({ quarter: 0, flipH: false, flipV: false, angle: 0 }))}>
                      Reset crop
                    </Button>
                  </>
                )}

                {tab === "adjust" && (
                  <>
                    <Section>
                      {ADJUST_LABELS.map(([k, label]) => (
                        <Slider key={k} label={label} min={-100} max={100} center={0} value={adjust[k]} onChange={(v) => setAdjust((a) => ({ ...a, [k]: v }))} />
                      ))}
                    </Section>
                    <div className="flex gap-2">
                      <Button
                        className="flex-1"
                        onPointerDown={() => setComparing(true)}
                        onPointerUp={() => setComparing(false)}
                        onPointerLeave={() => setComparing(false)}
                      >
                        Hold to compare
                      </Button>
                      <Button variant="ghost" onClick={() => setAdjust(noAdjustments)}>
                        Reset
                      </Button>
                    </div>
                  </>
                )}

                {tab === "annotate" && (
                  <>
                    <Section title="Tool">
                      <div className="grid grid-cols-3 gap-1.5 pt-1">
                        {(
                          [
                            ["arrow", "Arrow", ArrowUpRight],
                            ["pen", "Pen", Pen],
                            ["highlighter", "Marker", Highlighter],
                            ["rect", "Box", Square],
                            ["ellipse", "Circle", Circle],
                            ["text", "Text", Type],
                          ] as const
                        ).map(([value, label, Icon]) => (
                          <button
                            key={value}
                            onClick={() => setDrawTool(value)}
                            className={`flex flex-col items-center gap-1 rounded-xl py-2 text-[11px] font-semibold ring-1 transition-colors ${drawTool === value ? "bg-accent text-accent-ink ring-accent" : "bg-surface text-ink-2 ring-line hover:text-ink"}`}
                          >
                            <Icon className="size-4.5" />
                            {label}
                          </button>
                        ))}
                      </div>
                    </Section>
                    <Section title="Color and size">
                      <div className="flex gap-2 py-1.5">
                        {COLORS.map((c) => (
                          <button
                            key={c}
                            title={c}
                            onClick={() => setColor(c)}
                            className="size-7 rounded-full ring-2 ring-offset-2 ring-offset-surface transition-transform hover:scale-110"
                            style={{ background: c, boxShadow: "inset 0 0 0 1px rgb(0 0 0 / 0.15)", ["--tw-ring-color" as string]: color === c ? "var(--accent)" : "transparent" }}
                          />
                        ))}
                      </div>
                      <Slider label="Size" min={0.4} max={4} step={0.1} value={strokeScale} format={(v) => `${Math.round(v * 100)}%`} onChange={setStrokeScale} />
                    </Section>
                    <p className="px-1 text-xs text-ink-3">Drag on the photo to draw. Text: click where it should go.</p>
                  </>
                )}

                {tab === "redact" && (
                  <>
                    <Section title="Style">
                      <Segmented
                        className="mt-1"
                        value={redactStyle}
                        onChange={setRedactStyle}
                        options={[
                          { value: "black", label: "Black box" },
                          { value: "pixelate", label: "Pixelate" },
                          { value: "blur", label: "Blur" },
                        ]}
                      />
                    </Section>
                    <p className="px-1 text-xs leading-relaxed text-ink-3">
                      Drag over names, faces, or numbers to hide them. Hidden areas are painted into the saved copy; the original stays as it is.
                    </p>
                    <p className="px-1 text-xs font-semibold text-ink-2">
                      {current.redactions.length} area{current.redactions.length === 1 ? "" : "s"} hidden
                    </p>
                  </>
                )}
              </motion.div>
            </AnimatePresence>
          </div>

          <div className="flex gap-2 border-t border-line bg-surface p-3">
            <Button className="flex-1" onClick={close}>
              Cancel
            </Button>
            <Button variant="primary" className="flex-1" disabled={!img || !changed || saving} onClick={save}>
              Save a copy
            </Button>
          </div>
        </aside>
      </div>
    </>
  );
}
