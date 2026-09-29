import { AnimatePresence, motion } from "motion/react";
import { Shrink } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../../lib/ipc";
import { bytes } from "../../lib/format";
import { kindOf } from "../../lib/kinds";
import { Button, Section, Segmented } from "../../components/ui";
import type { ToolProps } from "./ToolApp";
import { FileHeader, Sheet } from "./Sheet";

type Level = "small" | "balanced" | "high";

const levelLabels = [
  { value: "small" as Level, label: "Smallest" },
  { value: "balanced" as Level, label: "Balanced" },
  { value: "high" as Level, label: "Best quality" },
];

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const id = window.setTimeout(() => setV(value), ms);
    return () => window.clearTimeout(id);
  }, [value, ms]);
  return v;
}

export default function CompressView({ session, run }: ToolProps) {
  const paths = session.paths;
  const kind = kindOf(paths[0]);
  const [level, setLevel] = useState<Level>("balanced");
  const [exact, setExact] = useState(false);
  const [target, setTarget] = useState(kind === "video" ? 25 : kind === "image" ? 500 : 5);
  const [maxSide, setMaxSide] = useState<string>("original");
  const [format, setFormat] = useState<"keep" | "jpg" | "webp">("keep");
  const [original, setOriginal] = useState(0);
  const [estimate, setEstimate] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.fileSize(paths[0]).then(setOriginal);
  }, [paths]);

  const options = useMemo(() => {
    const limit = maxSide === "original" ? null : Number(maxSide);
    switch (kind) {
      case "image":
        return { level, targetKb: exact ? target : null, maxSide: limit, format };
      case "video":
        return { level, targetMb: exact ? target : null, maxHeight: limit };
      case "audio":
        return { level, targetMb: exact ? target : null };
      default:
        return { level, targetMb: exact ? target : null };
    }
  }, [kind, level, exact, target, maxSide, format]);

  // Live size estimate for a single image: encode a reduced copy in the background.
  const debounced = useDebounced(options, 250);
  useEffect(() => {
    if (kind !== "image" || paths.length !== 1 || exact) {
      setEstimate(null);
      return;
    }
    let alive = true;
    api
      .compressEstimate(paths[0], debounced)
      .then((n) => alive && setEstimate(n))
      .catch(() => alive && setEstimate(null));
    return () => {
      alive = false;
    };
  }, [debounced, kind, paths, exact]);

  const unit = kind === "image" ? "KB" : "MB";
  const presets = kind === "video" ? [8, 10, 25, 50] : kind === "image" ? [100, 250, 500, 1000] : kind === "audio" ? [2, 5, 10] : [1, 2, 5, 10];
  const saving = estimate !== null && original > 0 ? Math.round((1 - estimate / original) * 100) : null;

  return (
    <Sheet
      title="Compress"
      subtitle="Smaller files, saved as a copy"
      footer={
        <Button
          variant="primary"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            await run(options);
          }}
        >
          <Shrink className="size-4" /> Compress {paths.length > 1 ? `${paths.length} files` : ""}
        </Button>
      }
    >
      <FileHeader paths={paths} />

      <Section title="Aim for">
        <Segmented className="my-1.5" value={exact ? "exact" : "quality"} onChange={(v) => setExact(v === "exact")} options={[{ value: "quality", label: "A quality level" }, { value: "exact", label: "An exact size" }]} />
        <AnimatePresence mode="wait" initial={false}>
          {exact ? (
            <motion.div key="exact" initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} className="pt-1.5">
              <div className="flex items-center gap-2">
                <input
                  type="number"
                  min={1}
                  value={target}
                  onChange={(e) => setTarget(Math.max(1, Number(e.target.value) || 1))}
                  className="h-9.5 w-28 rounded-xl bg-sunken px-3 text-[14px] font-semibold tabular-nums text-ink ring-1 ring-line outline-none focus:ring-2 focus:ring-accent"
                />
                <span className="text-sm font-medium text-ink-2">{unit} or less</span>
              </div>
              <div className="mt-2 flex flex-wrap gap-1.5">
                {presets.map((p) => (
                  <button
                    key={p}
                    onClick={() => setTarget(p)}
                    className={`rounded-full px-2.5 py-1 text-xs font-semibold ring-1 transition-colors ${target === p ? "bg-accent text-accent-ink ring-accent" : "bg-surface text-ink-2 ring-line hover:text-ink"}`}
                  >
                    {p} {unit}
                    {kind === "video" && p === 10 ? " · Discord" : kind === "video" && p === 25 ? " · Email" : ""}
                  </button>
                ))}
              </div>
            </motion.div>
          ) : (
            <motion.div key="level" initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }}>
              <Segmented className="mt-1.5" value={level} onChange={setLevel} options={levelLabels} />
            </motion.div>
          )}
        </AnimatePresence>
      </Section>

      {(kind === "image" || kind === "video") && (
        <Section title={kind === "image" ? "Size and format" : "Resolution"}>
          <Segmented
            className="my-1.5"
            value={maxSide}
            onChange={setMaxSide}
            options={
              kind === "image"
                ? [
                    { value: "original", label: "Original" },
                    { value: "3840", label: "4K" },
                    { value: "1920", label: "1920" },
                    { value: "1280", label: "1280" },
                  ]
                : [
                    { value: "original", label: "Original" },
                    { value: "1080", label: "1080p" },
                    { value: "720", label: "720p" },
                    { value: "480", label: "480p" },
                  ]
            }
          />
          {kind === "image" && (
            <Segmented
              className="mt-2"
              value={format}
              onChange={setFormat}
              options={[
                { value: "keep", label: "Keep format" },
                { value: "jpg", label: "JPG" },
                { value: "webp", label: "WebP" },
              ]}
            />
          )}
        </Section>
      )}

      {kind === "image" && paths.length === 1 && !exact && (
        <div className="flex items-center justify-between rounded-2xl bg-surface px-4 py-3 ring-1 ring-line">
          <span className="text-[12.5px] text-ink-3">Estimated result</span>
          <span className="text-[13.5px] font-semibold tabular-nums text-ink">
            {estimate === null ? "…" : bytes(estimate)}
            {saving !== null && (
              <span className={`ml-2 rounded-full px-2 py-0.5 text-xs ${saving > 0 ? "bg-kiwi-100 text-kiwi-700 dark:bg-kiwi-900/50 dark:text-kiwi-300" : "bg-sunken text-ink-3"}`}>
                {saving > 0 ? `−${saving}%` : "no smaller"}
              </span>
            )}
          </span>
        </div>
      )}
      {kind === "video" && exact && (
        <p className="px-1 text-xs leading-relaxed text-ink-3">Exact sizes encode the video twice: slower, but the result lands on the target.</p>
      )}
    </Sheet>
  );
}
