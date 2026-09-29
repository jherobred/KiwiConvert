import { AnimatePresence, motion } from "motion/react";
import { Camera, Crop as CropIcon, Pause, Play, Scissors, SkipBack, SkipForward, Split } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as RPointerEvent } from "react";
import { api, fileUrl } from "../../../lib/ipc";
import { baseName, clamp, duration as fmt } from "../../../lib/format";
import { spring } from "../../../lib/motion";
import type { MediaInfo } from "../../../lib/types";
import { Button, Section, Toggle, TitleBar } from "../../../components/ui";
import { Timeline } from "../Timeline";
import type { ToolProps } from "../ToolApp";

type Tab = "trim" | "crop" | "split" | "snapshot";

interface CropRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

const ASPECTS: { label: string; value: number | null }[] = [
  { label: "Free", value: null },
  { label: "16:9", value: 16 / 9 },
  { label: "9:16", value: 9 / 16 },
  { label: "1:1", value: 1 },
  { label: "4:5", value: 4 / 5 },
  { label: "4:3", value: 4 / 3 },
];

export default function VideoEditor({ session }: ToolProps) {
  const path = session.paths[0];
  const initial: Tab = session.tool === "crop" ? "crop" : session.tool === "split" ? "split" : session.tool === "snapshot" ? "snapshot" : "trim";
  const [tab, setTab] = useState<Tab>(initial);
  const [info, setInfo] = useState<MediaInfo | null>(null);
  const [src, setSrc] = useState<string | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [time, setTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [range, setRange] = useState({ start: 0, end: 0 });
  const [precise, setPrecise] = useState(false);
  const [markers, setMarkers] = useState<number[]>([]);
  const [strip, setStrip] = useState<string[]>([]);
  const [crop, setCrop] = useState<CropRect | null>(null);
  const [aspect, setAspect] = useState<number | null>(null);
  const [flash, setFlash] = useState(0);
  const [saved, setSaved] = useState<number>(0);
  const [busy, setBusy] = useState(false);
  const [box, setBox] = useState({ w: 800, h: 450 });
  const video = useRef<HTMLVideoElement>(null);
  const stage = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api
      .mediaInfo(path)
      .then((i) => {
        setInfo(i);
        setRange({ start: 0, end: i.duration });
      })
      .catch((e) => setError(String(e)));
    setSrc(fileUrl(path));
    api.videoStrip(path, 14, 160).then(setStrip).catch(() => {});
  }, [path]);

  useLayoutEffect(() => {
    const el = stage.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBox({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // WebView2 can't play every codec (HEVC without the extension, WMV, ProRes...). Fall back
  // to a quick preview copy; edits are always made from the original.
  const triedProxy = useRef(false);
  const onVideoError = async () => {
    if (triedProxy.current) {
      setError("This video can't be previewed.");
      return;
    }
    triedProxy.current = true;
    setPreparing(true);
    try {
      const proxy = await api.previewProxy(path, true);
      setSrc(fileUrl(proxy));
    } catch (e) {
      setError(`This video can't be previewed: ${e}`);
    } finally {
      setPreparing(false);
    }
  };

  const dur = info?.duration ?? 0;
  const fps = info?.fps && info.fps > 0 ? info.fps : 30;
  const seek = useCallback((t: number) => {
    const v = video.current;
    if (v) v.currentTime = clamp(t, 0, v.duration || t);
    setTime(t);
  }, []);

  const toggle = () => {
    const v = video.current;
    if (!v) return;
    if (v.paused) {
      if (tab === "trim" && (v.currentTime < range.start || v.currentTime >= range.end - 0.05)) v.currentTime = range.start;
      v.play();
    } else v.pause();
  };

  // Keep playback inside the trim selection.
  const onTimeUpdate = () => {
    const v = video.current;
    if (!v) return;
    setTime(v.currentTime);
    if (tab === "trim" && !v.paused && v.currentTime >= range.end) {
      v.pause();
      v.currentTime = range.start;
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      if (e.key === " ") {
        e.preventDefault();
        toggle();
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        const step = e.shiftKey ? 1 : 1 / fps;
        seek(time + (e.key === "ArrowLeft" ? -step : step));
      } else if (e.key.toLowerCase() === "i") setRange((r) => ({ start: Math.min(time, r.end - 0.05), end: r.end }));
      else if (e.key.toLowerCase() === "o") setRange((r) => ({ start: r.start, end: Math.max(time, r.start + 0.05) }));
      else if (e.key.toLowerCase() === "s" && tab === "split") setMarkers((m) => [...m, time].sort((a, b) => a - b));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // Where the video sits inside the stage (letterboxed).
  const vw = info?.width || 16;
  const vh = info?.height || 9;
  const scale = Math.min(box.w / vw, box.h / vh);
  const dispW = vw * scale;
  const dispH = vh * scale;
  const dx = (box.w - dispW) / 2;
  const dy = (box.h - dispH) / 2;
  const rect = crop ?? { x: 0, y: 0, w: vw, h: vh };

  const cropDrag = useRef<{ mode: string; start: [number, number]; orig: CropRect } | null>(null);
  const toVideo = (e: { clientX: number; clientY: number }): [number, number] => {
    const r = stage.current!.getBoundingClientRect();
    return [clamp((e.clientX - r.left - dx) / scale, 0, vw), clamp((e.clientY - r.top - dy) / scale, 0, vh)];
  };
  const cropDown = (e: RPointerEvent) => {
    if (tab !== "crop") return;
    (e.target as Element).setPointerCapture(e.pointerId);
    cropDrag.current = { mode: (e.target as HTMLElement).dataset.handle ?? "new", start: toVideo(e), orig: rect };
  };
  const cropMove = (e: RPointerEvent) => {
    const d = cropDrag.current;
    if (!d) return;
    const p = toVideo(e);
    let r = { ...d.orig };
    const ddx = p[0] - d.start[0];
    const ddy = p[1] - d.start[1];
    if (d.mode === "move") {
      r.x = clamp(d.orig.x + ddx, 0, vw - r.w);
      r.y = clamp(d.orig.y + ddy, 0, vh - r.h);
    } else if (d.mode === "new") {
      r = { x: Math.min(d.start[0], p[0]), y: Math.min(d.start[1], p[1]), w: Math.abs(p[0] - d.start[0]), h: Math.abs(p[1] - d.start[1]) };
    } else {
      if (d.mode.includes("w")) {
        r.x = d.orig.x + ddx;
        r.w = d.orig.w - ddx;
      }
      if (d.mode.includes("e")) r.w = d.orig.w + ddx;
      if (d.mode.includes("n")) {
        r.y = d.orig.y + ddy;
        r.h = d.orig.h - ddy;
      }
      if (d.mode.includes("s")) r.h = d.orig.h + ddy;
    }
    if (aspect && d.mode !== "move") r.h = r.w / aspect;
    r.w = clamp(r.w, 16, vw - Math.max(0, r.x));
    r.h = clamp(r.h, 16, vh - Math.max(0, r.y));
    setCrop({ x: Math.max(0, r.x), y: Math.max(0, r.y), w: r.w, h: r.h });
  };

  const applyAspect = (a: number | null) => {
    setAspect(a);
    if (!a) return;
    let w = vw;
    let h = w / a;
    if (h > vh) {
      h = vh;
      w = h * a;
    }
    setCrop({ x: (vw - w) / 2, y: (vh - h) / 2, w, h });
  };

  const snapshot = async () => {
    setFlash((f) => f + 1);
    await api.runTool("snapshot", [path], { points: [time] });
    setSaved((n) => n + 1);
  };

  // The window can switch between tools, so run the one that is showing.
  const save = async () => {
    setBusy(true);
    const options =
      tab === "trim"
        ? { start: range.start, end: range.end, precise }
        : tab === "crop"
          ? { rect: { x: Math.round(rect.x), y: Math.round(rect.y), w: Math.round(rect.w), h: Math.round(rect.h) } }
          : { points: markers };
    await api.runTool(tab, [path], options);
    await api.toolClosed();
  };

  const tabs = [
    { value: "trim" as Tab, label: "Trim", icon: Scissors },
    { value: "crop" as Tab, label: "Crop", icon: CropIcon },
    { value: "split" as Tab, label: "Split", icon: Split },
    { value: "snapshot" as Tab, label: "Snapshot", icon: Camera },
  ];
  const canSave = tab === "trim" ? range.end - range.start < dur - 0.05 : tab === "crop" ? crop !== null : tab === "split" ? markers.length > 0 : false;

  return (
    <>
      <TitleBar title="Edit video" subtitle={info ? `${baseName(path)} · ${info.width}×${info.height} · ${fmt(dur)}` : baseName(path)} />
      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col bg-black">
          <div ref={stage} className="relative min-h-0 flex-1" onPointerDown={cropDown} onPointerMove={cropMove} onPointerUp={() => (cropDrag.current = null)}>
            {src && (
              <video
                ref={video}
                src={src}
                className="absolute inset-0 size-full object-contain"
                onLoadedMetadata={() => setTime(video.current?.currentTime ?? 0)}
                onTimeUpdate={onTimeUpdate}
                onPlay={() => setPlaying(true)}
                onPause={() => setPlaying(false)}
                onError={onVideoError}
                onClick={() => tab !== "crop" && toggle()}
                preload="auto"
              />
            )}
            {tab === "crop" && info && (
              <div className="pointer-events-none absolute inset-0">
                <svg className="absolute inset-0 size-full">
                  <path
                    fillRule="evenodd"
                    fill="rgb(0 0 0 / 0.6)"
                    d={`M${dx} ${dy}h${dispW}v${dispH}h${-dispW}Z M${dx + rect.x * scale} ${dy + rect.y * scale}h${rect.w * scale}v${rect.h * scale}h${-rect.w * scale}Z`}
                  />
                  <rect x={dx + rect.x * scale} y={dy + rect.y * scale} width={rect.w * scale} height={rect.h * scale} fill="none" stroke="white" strokeWidth={2} />
                </svg>
                <div data-handle="move" className="pointer-events-auto absolute cursor-move" style={{ left: dx + rect.x * scale + 10, top: dy + rect.y * scale + 10, width: Math.max(0, rect.w * scale - 20), height: Math.max(0, rect.h * scale - 20) }} />
                {(["nw", "ne", "se", "sw"] as const).map((hnd) => (
                  <div
                    key={hnd}
                    data-handle={hnd}
                    className="pointer-events-auto absolute size-4 rounded-full bg-white shadow ring-2 ring-black/30"
                    style={{
                      left: dx + (rect.x + (hnd.includes("e") ? rect.w : 0)) * scale - 8,
                      top: dy + (rect.y + (hnd.includes("s") ? rect.h : 0)) * scale - 8,
                      cursor: `${hnd}-resize`,
                    }}
                  />
                ))}
                <div className="absolute rounded bg-black/70 px-1.5 py-0.5 text-[11px] font-semibold tabular-nums text-white" style={{ left: dx + rect.x * scale + 6, top: dy + rect.y * scale + 6 }}>
                  {Math.round(rect.w)} × {Math.round(rect.h)}
                </div>
              </div>
            )}
            <AnimatePresence>
              {flash > 0 && (
                <motion.div key={flash} className="pointer-events-none absolute inset-0 bg-white" initial={{ opacity: 0.8 }} animate={{ opacity: 0 }} transition={{ duration: 0.35 }} />
              )}
            </AnimatePresence>
            {(preparing || error) && (
              <div className="absolute inset-0 grid place-items-center bg-black/70 p-8 text-center text-sm font-medium text-white">
                {error ?? "Preparing a preview for this video…"}
              </div>
            )}
          </div>

          <div className="border-t border-white/10 bg-canvas px-4 pb-3 pt-1">
            <Timeline
              duration={dur}
              time={time}
              onSeek={seek}
              range={tab === "trim" ? range : undefined}
              onRange={setRange}
              markers={tab === "split" ? markers : undefined}
              onMarkers={(m) => setMarkers([...m].sort((a, b) => a - b))}
            >
              <div className="flex size-full">
                {strip.map((s, i) => (
                  <img key={i} src={s} alt="" draggable={false} className="h-full min-w-0 flex-1 object-cover opacity-90" />
                ))}
              </div>
            </Timeline>
            <div className="mt-2 flex items-center justify-center gap-2">
              <button className="grid size-9 place-items-center rounded-full text-ink-2 hover:bg-sunken" title="Back one frame" onClick={() => seek(time - 1 / fps)}>
                <SkipBack className="size-4" />
              </button>
              <motion.button whileTap={{ scale: 0.9 }} className="grid size-11 place-items-center rounded-full bg-accent text-accent-ink shadow" title="Play (Space)" onClick={toggle}>
                {playing ? <Pause className="size-5" /> : <Play className="size-5 translate-x-px" />}
              </motion.button>
              <button className="grid size-9 place-items-center rounded-full text-ink-2 hover:bg-sunken" title="Forward one frame" onClick={() => seek(time + 1 / fps)}>
                <SkipForward className="size-4" />
              </button>
              <span className="ml-2 w-28 text-xs tabular-nums text-ink-3">
                {fmt(time, true)} / {fmt(dur)}
              </span>
            </div>
          </div>
        </div>

        <aside className="flex w-[290px] shrink-0 flex-col border-l border-line bg-canvas">
          <div className="grid grid-cols-4 gap-1 p-3">
            {tabs.map((t) => (
              <button
                key={t.value}
                onClick={() => setTab(t.value)}
                className={`relative flex flex-col items-center gap-1 rounded-xl py-2 text-[11px] font-semibold ${tab === t.value ? "text-accent-ink" : "text-ink-3 hover:text-ink"}`}
              >
                {tab === t.value && <motion.span layoutId="video-tab" className="absolute inset-0 rounded-xl bg-accent" transition={spring.snap} />}
                <t.icon className="relative size-4.5" />
                <span className="relative">{t.label}</span>
              </button>
            ))}
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-3">
            <AnimatePresence mode="wait" initial={false}>
              <motion.div key={tab} initial={{ opacity: 0, x: 12 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -12 }} transition={spring.soft} className="flex flex-col gap-2.5">
                {tab === "trim" && (
                  <>
                    <Section title="Keep">
                      <div className="flex items-baseline justify-between py-1 text-sm">
                        <span className="tabular-nums text-ink">
                          {fmt(range.start, true)} – {fmt(range.end, true)}
                        </span>
                        <span className="text-xs text-ink-3">{fmt(range.end - range.start, true)} long</span>
                      </div>
                      <div className="flex gap-2 pt-1">
                        <Button size="sm" className="flex-1" onClick={() => setRange((r) => ({ start: Math.min(time, r.end - 0.05), end: r.end }))}>
                          Start here (I)
                        </Button>
                        <Button size="sm" className="flex-1" onClick={() => setRange((r) => ({ start: r.start, end: Math.max(time, r.start + 0.05) }))}>
                          End here (O)
                        </Button>
                      </div>
                    </Section>
                    <Section>
                      <Toggle label="Frame-accurate cut" hint="Re-encodes the clip. Off is instant but cuts at the nearest keyframe." checked={precise} onChange={setPrecise} />
                    </Section>
                  </>
                )}
                {tab === "crop" && (
                  <Section title="Aspect ratio">
                    <div className="grid grid-cols-3 gap-1.5 pt-1">
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
                    <p className="pt-2 text-xs text-ink-3">Drag on the video to choose the area to keep.</p>
                  </Section>
                )}
                {tab === "split" && (
                  <Section title="Split points">
                    <Button size="sm" className="mt-1 w-full" onClick={() => setMarkers((m) => [...m, time].sort((a, b) => a - b))}>
                      <Split className="size-4" /> Split at {fmt(time, true)} (S)
                    </Button>
                    <ul className="mt-2 flex flex-col gap-1">
                      {markers.map((m, i) => (
                        <li key={i} className="flex items-center justify-between rounded-lg bg-sunken px-2.5 py-1.5 text-xs tabular-nums text-ink-2">
                          Cut at {fmt(m, true)}
                          <button className="text-ink-3 hover:text-danger" onClick={() => setMarkers(markers.filter((_, j) => j !== i))}>
                            Remove
                          </button>
                        </li>
                      ))}
                    </ul>
                    {markers.length > 0 && <p className="pt-2 text-xs text-ink-3">Makes {markers.length + 1} clips without re-encoding.</p>}
                  </Section>
                )}
                {tab === "snapshot" && (
                  <Section title="Snapshot">
                    <p className="py-1 text-xs text-ink-3">Pause on the frame you want, then save it as a full-resolution PNG.</p>
                    <Button variant="primary" className="mt-1 w-full" onClick={snapshot}>
                      <Camera className="size-4" /> Save this frame
                    </Button>
                    {saved > 0 && <p className="pt-2 text-xs font-semibold text-accent">{saved} saved beside the video</p>}
                  </Section>
                )}
              </motion.div>
            </AnimatePresence>
          </div>
          {tab !== "snapshot" && (
            <div className="border-t border-line bg-surface p-3">
              <Button variant="primary" className="w-full" disabled={!canSave || busy} onClick={save}>
                {tab === "trim" ? "Save trimmed copy" : tab === "crop" ? "Save cropped copy" : "Split into clips"}
              </Button>
            </div>
          )}
        </aside>
      </div>
    </>
  );
}
