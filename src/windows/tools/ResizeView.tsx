import { Link2, Link2Off, Scaling } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../../lib/ipc";
import { Button, Section, Segmented, Slider } from "../../components/ui";
import type { ToolProps } from "./ToolApp";
import { FileHeader, Sheet } from "./Sheet";

export default function ResizeView({ session, run }: ToolProps) {
  const paths = session.paths;
  const [mode, setMode] = useState<"percent" | "size">("percent");
  const [percent, setPercent] = useState(50);
  const [dims, setDims] = useState<{ w: number; h: number } | null>(null);
  const [width, setWidth] = useState(1920);
  const [height, setHeight] = useState(1080);
  const [locked, setLocked] = useState(true);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.imagePreview(paths[0]).then((p) => {
      if (!p.width) return;
      setDims({ w: p.width, h: p.height });
      setWidth(Math.round(p.width / 2));
      setHeight(Math.round(p.height / 2));
    });
  }, [paths]);

  const ratio = dims ? dims.h / dims.w : 9 / 16;
  const setW = (w: number) => {
    setWidth(w);
    if (locked) setHeight(Math.max(1, Math.round(w * ratio)));
  };
  const setH = (h: number) => {
    setHeight(h);
    if (locked) setWidth(Math.max(1, Math.round(h / ratio)));
  };

  const options = mode === "percent" ? { percent } : { width, height, keepAspect: locked };
  const result = mode === "percent" && dims ? `${Math.round((dims.w * percent) / 100)} × ${Math.round((dims.h * percent) / 100)}` : `${width} × ${height}`;

  const field = (value: number, onChange: (v: number) => void, label: string) => (
    <label className="flex flex-1 flex-col gap-1">
      <span className="text-xs font-medium text-ink-3">{label}</span>
      <input
        type="number"
        min={1}
        value={value}
        onChange={(e) => onChange(Math.max(1, Number(e.target.value) || 1))}
        className="h-9.5 rounded-xl bg-sunken px-3 text-[14px] font-semibold tabular-nums text-ink ring-1 ring-line outline-none focus:ring-2 focus:ring-accent"
      />
    </label>
  );

  return (
    <Sheet
      title="Resize"
      subtitle={dims ? `Now ${dims.w} × ${dims.h}` : undefined}
      footer={
        <Button variant="primary" disabled={busy} onClick={async () => (setBusy(true), await run(options))}>
          <Scaling className="size-4" /> Resize to {result}
        </Button>
      }
    >
      <FileHeader paths={paths} />
      <Section>
        <Segmented value={mode} onChange={setMode} options={[{ value: "percent", label: "By percentage" }, { value: "size", label: "Exact size" }]} />
        {mode === "percent" ? (
          <div className="pt-2">
            <Slider label="Scale" min={5} max={200} value={percent} format={(v) => `${v}%`} onChange={setPercent} />
            <div className="mt-1 flex gap-1.5">
              {[25, 50, 75, 150].map((p) => (
                <button key={p} onClick={() => setPercent(p)} className={`rounded-full px-2.5 py-1 text-xs font-semibold ring-1 ${percent === p ? "bg-accent text-accent-ink ring-accent" : "bg-surface text-ink-2 ring-line"}`}>
                  {p}%
                </button>
              ))}
            </div>
          </div>
        ) : (
          <div className="flex items-end gap-2 pt-3">
            {field(width, setW, "Width")}
            <button
              title={locked ? "Keep proportions" : "Free proportions"}
              onClick={() => setLocked(!locked)}
              className={`mb-1 grid size-8 place-items-center rounded-lg ${locked ? "text-accent" : "text-ink-3"} hover:bg-sunken`}
            >
              {locked ? <Link2 className="size-4" /> : <Link2Off className="size-4" />}
            </button>
            {field(height, setH, "Height")}
          </div>
        )}
      </Section>
    </Sheet>
  );
}
