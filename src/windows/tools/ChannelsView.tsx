import { Headphones } from "lucide-react";
import { motion } from "motion/react";
import { useState } from "react";
import { spring } from "../../lib/motion";
import { Button, Section } from "../../components/ui";
import type { ToolProps } from "./ToolApp";
import { FileHeader, Sheet } from "./Sheet";

const modes = [
  { value: "mono", label: "Mono", hint: "Mix both sides into one channel" },
  { value: "stereo", label: "Stereo", hint: "Two channels, left and right" },
  { value: "left", label: "Left only", hint: "Play the left side in both ears" },
  { value: "right", label: "Right only", hint: "Play the right side in both ears" },
  { value: "swap", label: "Swap sides", hint: "Left becomes right and right becomes left" },
];

export default function ChannelsView({ session, run }: ToolProps) {
  const [mode, setMode] = useState("mono");
  const [busy, setBusy] = useState(false);
  return (
    <Sheet
      title="Channels"
      footer={
        <Button variant="primary" disabled={busy} onClick={async () => (setBusy(true), await run({ mode }))}>
          <Headphones className="size-4" /> Apply
        </Button>
      }
    >
      <FileHeader paths={session.paths} />
      <Section>
        <div className="flex flex-col gap-1">
          {modes.map((m) => (
            <button key={m.value} onClick={() => setMode(m.value)} className="relative flex items-center gap-3 rounded-xl px-3 py-2.5 text-left">
              {mode === m.value && <motion.span layoutId="channel" className="absolute inset-0 rounded-xl bg-kiwi-100 ring-1 ring-accent/40 dark:bg-kiwi-900/40" transition={spring.snap} />}
              <span className={`relative size-4 rounded-full ring-2 ${mode === m.value ? "bg-accent ring-accent" : "ring-ink-3/50"}`} />
              <span className="relative">
                <span className="block text-[13.5px] font-semibold text-ink">{m.label}</span>
                <span className="block text-xs text-ink-3">{m.hint}</span>
              </span>
            </button>
          ))}
        </div>
      </Section>
    </Sheet>
  );
}
