import { Gauge } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { api } from "../../lib/ipc";
import { duration } from "../../lib/format";
import { spring } from "../../lib/motion";
import { Button, Section, Toggle } from "../../components/ui";
import type { ToolProps } from "./ToolApp";
import { FileHeader, Sheet } from "./Sheet";

const speeds = [0.25, 0.5, 0.75, 1.25, 1.5, 2, 3, 4];

export default function SpeedView({ session, run }: ToolProps) {
  const [factor, setFactor] = useState(2);
  const [keepPitch, setKeepPitch] = useState(true);
  const [length, setLength] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.mediaInfo(session.paths[0]).then((i) => setLength(i.duration)).catch(() => {});
  }, [session.paths]);

  return (
    <Sheet
      title="Change speed"
      subtitle={length ? `${duration(length)} now, ${duration(length / factor)} after` : undefined}
      footer={
        <Button variant="primary" disabled={busy} onClick={async () => (setBusy(true), await run({ factor, keepPitch }))}>
          <Gauge className="size-4" /> Make it {factor}×
        </Button>
      }
    >
      <FileHeader paths={session.paths} />
      <Section title="Speed">
        <div className="grid grid-cols-4 gap-1.5 pt-1.5">
          {speeds.map((s) => (
            <button
              key={s}
              onClick={() => setFactor(s)}
              className={`relative h-10 rounded-xl text-[13.5px] font-bold ring-1 transition-colors ${factor === s ? "text-accent-ink ring-accent" : "bg-surface text-ink-2 ring-line hover:text-ink"}`}
            >
              {factor === s && <motion.span layoutId="speed" className="absolute inset-0 rounded-xl bg-accent" transition={spring.snap} />}
              <span className="relative">{s}×</span>
            </button>
          ))}
        </div>
      </Section>
      <Section>
        <Toggle label="Keep the voice natural" hint="Changes tempo without raising or lowering pitch" checked={keepPitch} onChange={setKeepPitch} />
      </Section>
    </Sheet>
  );
}
