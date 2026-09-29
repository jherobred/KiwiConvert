import { AnimatePresence, motion } from "motion/react";
import { AudioLines, BellOff, Pause, Play, Scissors } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, fileUrl } from "../../../lib/ipc";
import { baseName, clamp, duration as fmt, extension } from "../../../lib/format";
import { spring } from "../../../lib/motion";
import { Button, Section, Segmented, TitleBar } from "../../../components/ui";
import { Timeline, type Span } from "../Timeline";
import type { ToolProps } from "../ToolApp";

type Tab = "trim" | "bleep" | "normalize";

/** Formats Chromium plays directly; the rest get a preview copy. */
const PLAYABLE = ["mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "opus", "webm", "mp4", "m4v", "mov", "mkv"];

const TARGETS = [
  { value: "-14", label: "Streaming", hint: "Spotify, YouTube, Apple Music (−14 LUFS)" },
  { value: "-16", label: "Podcast", hint: "Spoken word and podcasts (−16 LUFS)" },
  { value: "-23", label: "Broadcast", hint: "TV and radio, EBU R128 (−23 LUFS)" },
];

function Waveform({ peaks, color }: { peaks: number[]; color: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    const draw = () => {
      const dpr = window.devicePixelRatio || 1;
      c.width = c.clientWidth * dpr;
      c.height = c.clientHeight * dpr;
      const ctx = c.getContext("2d")!;
      ctx.clearRect(0, 0, c.width, c.height);
      ctx.fillStyle = color;
      const mid = c.height / 2;
      const bars = Math.floor(c.width / (3 * dpr));
      for (let i = 0; i < bars; i++) {
        const a = Math.floor((i / bars) * peaks.length);
        const b = Math.max(a + 1, Math.floor(((i + 1) / bars) * peaks.length));
        let v = 0;
        for (let j = a; j < b; j++) v = Math.max(v, peaks[j] ?? 0);
        const h = Math.max(1.5 * dpr, v * (c.height - 8 * dpr));
        ctx.beginPath();
        ctx.roundRect(i * 3 * dpr, mid - h / 2, 2 * dpr, h, dpr);
        ctx.fill();
      }
    };
    draw();
    const observer = new ResizeObserver(draw);
    observer.observe(c);
    return () => observer.disconnect();
  }, [peaks, color]);
  return <canvas ref={ref} className="size-full" />;
}

export default function AudioEditor({ session }: ToolProps) {
  const path = session.paths[0];
  const [tab, setTab] = useState<Tab>(session.tool === "bleep" ? "bleep" : session.tool === "normalize" ? "normalize" : "trim");
  const [peaks, setPeaks] = useState<number[]>([]);
  const [dur, setDur] = useState(0);
  const [src, setSrc] = useState<string | null>(null);
  const [time, setTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [range, setRange] = useState({ start: 0, end: 0 });
  const [regions, setRegions] = useState<Span[]>([]);
  const [style, setStyle] = useState<"tone" | "mute">("tone");
  const [target, setTarget] = useState("-14");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const audio = useRef<HTMLAudioElement>(null);

  useEffect(() => {
    api
      .audioPeaks(path, 2400)
      .then((p) => {
        setPeaks(p.peaks);
        setDur(p.duration);
        setRange({ start: 0, end: p.duration });
      })
      .catch((e) => setError(String(e)));
    if (PLAYABLE.includes(extension(path))) setSrc(fileUrl(path));
    else
      api
        .previewProxy(path, false)
        .then((p) => setSrc(fileUrl(p)))
        .catch(() => {});
  }, [path]);

  const seek = useCallback((t: number) => {
    if (audio.current) audio.current.currentTime = t;
    setTime(t);
  }, []);

  const toggle = () => {
    const a = audio.current;
    if (!a) return;
    if (a.paused) {
      if (tab === "trim" && (a.currentTime < range.start || a.currentTime >= range.end - 0.05)) a.currentTime = range.start;
      a.play();
    } else a.pause();
  };

  const onTime = () => {
    const a = audio.current;
    if (!a) return;
    setTime(a.currentTime);
    if (tab === "trim" && !a.paused && a.currentTime >= range.end) {
      a.pause();
      a.currentTime = range.start;
    }
  };

  // Preview bleeps live: the player goes silent inside each marked section.
  useEffect(() => {
    const a = audio.current;
    if (!a) return;
    a.muted = tab === "bleep" && regions.some((r) => time >= r.start && time <= r.end);
  }, [time, regions, tab]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === " ") {
        e.preventDefault();
        toggle();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // The window can switch between tools, so run the one that is showing.
  const save = async () => {
    setBusy(true);
    const options =
      tab === "trim"
        ? { start: range.start, end: range.end, precise: false }
        : tab === "bleep"
          ? { ranges: regions.map((r) => ({ start: r.start, end: r.end })), style }
          : { target: Number(target) };
    await api.runTool(tab, [path], options);
    await api.toolClosed();
  };

  const tabs = [
    { value: "trim" as Tab, label: "Trim", icon: Scissors },
    { value: "bleep" as Tab, label: "Bleep", icon: BellOff },
    { value: "normalize" as Tab, label: "Normalize", icon: AudioLines },
  ];
  const canSave = tab === "trim" ? range.end - range.start < dur - 0.05 : tab === "bleep" ? regions.length > 0 : true;

  return (
    <>
      <TitleBar title="Edit audio" subtitle={`${baseName(path)}${dur ? ` · ${fmt(dur)}` : ""}`} />
      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col justify-center gap-4 px-6">
          {src && <audio ref={audio} src={src} onTimeUpdate={onTime} onPlay={() => setPlaying(true)} onPause={() => setPlaying(false)} preload="auto" />}
          {error && <p className="text-center text-sm text-danger">{error}</p>}
          <Timeline
            duration={dur}
            time={time}
            onSeek={seek}
            height={150}
            range={tab === "trim" ? range : undefined}
            onRange={setRange}
            regions={tab === "bleep" ? regions : undefined}
            onRegions={tab === "bleep" ? setRegions : undefined}
          >
            {peaks.length ? (
              <Waveform peaks={peaks} color="rgb(140 200 60)" />
            ) : (
              <div className="grid size-full place-items-center text-xs text-ink-3">Reading audio…</div>
            )}
          </Timeline>
          <div className="flex items-center justify-center gap-3">
            <motion.button whileTap={{ scale: 0.9 }} className="grid size-12 place-items-center rounded-full bg-accent text-accent-ink shadow" onClick={toggle} title="Play (Space)">
              {playing ? <Pause className="size-5" /> : <Play className="size-5 translate-x-px" />}
            </motion.button>
            <span className="w-32 text-sm tabular-nums text-ink-3">
              {fmt(time, true)} / {fmt(dur)}
            </span>
          </div>
          {tab === "bleep" && <p className="text-center text-xs text-ink-3">Drag across the waveform to mark words to hide. Playback goes silent inside marked sections.</p>}
        </div>

        <aside className="flex w-[290px] shrink-0 flex-col border-l border-line bg-canvas">
          <div className="grid grid-cols-3 gap-1 p-3">
            {tabs.map((t) => (
              <button
                key={t.value}
                onClick={() => setTab(t.value)}
                className={`relative flex flex-col items-center gap-1 rounded-xl py-2 text-[11px] font-semibold ${tab === t.value ? "text-accent-ink" : "text-ink-3 hover:text-ink"}`}
              >
                {tab === t.value && <motion.span layoutId="audio-tab" className="absolute inset-0 rounded-xl bg-accent" transition={spring.snap} />}
                <t.icon className="relative size-4.5" />
                <span className="relative">{t.label}</span>
              </button>
            ))}
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-3">
            <AnimatePresence mode="wait" initial={false}>
              <motion.div key={tab} initial={{ opacity: 0, x: 12 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -12 }} transition={spring.soft} className="flex flex-col gap-2.5">
                {tab === "trim" && (
                  <Section title="Keep">
                    <div className="py-1 text-sm tabular-nums text-ink">
                      {fmt(range.start, true)} – {fmt(range.end, true)}
                    </div>
                    <div className="flex gap-2 pt-1">
                      <Button size="sm" className="flex-1" onClick={() => setRange((r) => ({ start: clamp(time, 0, r.end - 0.05), end: r.end }))}>
                        Start here
                      </Button>
                      <Button size="sm" className="flex-1" onClick={() => setRange((r) => ({ start: r.start, end: clamp(time, r.start + 0.05, dur) }))}>
                        End here
                      </Button>
                    </div>
                  </Section>
                )}
                {tab === "bleep" && (
                  <Section title="Sound">
                    <Segmented className="mt-1" value={style} onChange={setStyle} options={[{ value: "tone", label: "Bleep tone" }, { value: "mute", label: "Silence" }]} />
                    <p className="pt-2 text-xs font-semibold text-ink-2">
                      {regions.length} section{regions.length === 1 ? "" : "s"} marked
                    </p>
                  </Section>
                )}
                {tab === "normalize" && (
                  <Section title="Loudness target">
                    <div className="flex flex-col gap-1 pt-1">
                      {TARGETS.map((t) => (
                        <button key={t.value} onClick={() => setTarget(t.value)} className="relative rounded-xl px-3 py-2 text-left">
                          {target === t.value && <motion.span layoutId="target" className="absolute inset-0 rounded-xl bg-kiwi-100 ring-1 ring-accent/40 dark:bg-kiwi-900/40" transition={spring.snap} />}
                          <span className="relative block text-[13px] font-semibold text-ink">{t.label}</span>
                          <span className="relative block text-xs text-ink-3">{t.hint}</span>
                        </button>
                      ))}
                    </div>
                  </Section>
                )}
              </motion.div>
            </AnimatePresence>
          </div>
          <div className="border-t border-line bg-surface p-3">
            <Button variant="primary" className="w-full" disabled={!canSave || busy} onClick={save}>
              {tab === "trim" ? "Save trimmed copy" : tab === "bleep" ? "Save bleeped copy" : "Normalize loudness"}
            </Button>
          </div>
        </aside>
      </div>
    </>
  );
}
