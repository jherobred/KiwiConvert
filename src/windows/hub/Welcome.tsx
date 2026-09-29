import { AnimatePresence, motion } from "motion/react";
import { ImageIcon, Lock, MousePointer2 } from "lucide-react";
import { useEffect, useState } from "react";
import { enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { api } from "../../lib/ipc";
import { spring } from "../../lib/motion";
import type { Settings, WheelOption } from "../../lib/types";
import { Button, Toggle } from "../../components/ui";
import { Wheel } from "../wheel/Wheel";

const demoConvert: WheelOption[] = ["JPG", "PNG", "WEBP", "HEIC", "AVIF", "PDF", "GIF", "TIFF"].map((label) => ({
  label,
  icon: null,
  hint: "",
  action: { type: "convert", to: "jpg" },
}));

const demoTools: WheelOption[] = [
  ["COMPRESS", "shrink"],
  ["CROP", "crop"],
  ["ADJUST", "sliders-horizontal"],
  ["ANNOTATE", "pen-line"],
  ["REDACT", "eye-off"],
  ["METADATA", "tags"],
].map(([label, icon]) => ({ label, icon, hint: "", action: { type: "tool", tool: "compress" } }));

/** A looping animation of the gesture: drag a file, press Shift, drop on a format. */
function GestureDemo({ tools }: { tools: boolean }) {
  const [phase, setPhase] = useState(0);
  useEffect(() => {
    setPhase(0);
    const steps = [900, 900, 1100, 1300];
    let i = 0;
    let id = 0;
    const next = () => {
      i = (i + 1) % steps.length;
      setPhase(i);
      id = window.setTimeout(next, steps[i]);
    };
    id = window.setTimeout(next, steps[0]);
    return () => window.clearTimeout(id);
  }, [tools]);

  const open = phase >= 1;
  return (
    <div className="relative mx-auto h-52 w-full overflow-hidden rounded-2xl bg-sunken ring-1 ring-line">
      <AnimatePresence>
        {open && (
          <motion.div
            key={`wheel-${tools}`}
            className="absolute left-1/2 top-1/2"
            style={{ width: 440, height: 480, marginLeft: -220, marginTop: -220, scale: 0.42 }}
            initial={{ opacity: 0 }}
            animate={{ opacity: phase === 3 ? 0 : 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.25, delay: phase === 3 ? 0.35 : 0 }}
          >
            <Wheel
              options={tools ? demoTools : demoConvert}
              mode={tools ? "tools" : "convert"}
              hover={phase >= 2 ? 0 : -1}
              chosen={phase === 3 ? 0 : null}
              thumb={null}
              count={1}
              sizeLabel="2.4 MB"
            />
          </motion.div>
        )}
      </AnimatePresence>
      <motion.div
        className="absolute flex items-start"
        animate={phase === 0 ? { left: "18%", top: "62%" } : phase === 1 ? { left: "48%", top: "50%" } : { left: "49%", top: "22%" }}
        transition={spring.soft}
      >
        <div className="grid size-9 place-items-center rounded-lg bg-surface shadow-md ring-1 ring-line">
          <ImageIcon className="size-5 text-kiwi-600" />
        </div>
        <MousePointer2 className="-ml-2 mt-5 size-5 fill-white text-ink drop-shadow" />
      </motion.div>
      <div className="absolute bottom-2.5 left-3 flex gap-1.5">
        {(tools ? ["Ctrl", "Shift"] : ["Shift"]).map((k) => (
          <motion.kbd
            key={k}
            animate={{ scale: open ? 0.94 : 1, backgroundColor: open ? "var(--accent)" : "var(--surface)", color: open ? "var(--accent-ink)" : "var(--ink-2)" }}
            transition={spring.snap}
            className="rounded-md px-2 py-0.5 font-sans text-[11px] font-bold shadow-sm ring-1 ring-line"
          >
            {k}
          </motion.kbd>
        ))}
      </div>
    </div>
  );
}

export function Welcome({ settings, onDone }: { settings: Settings; onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [startup, setStartup] = useState(true);

  useEffect(() => {
    isEnabled().then((on) => on && setStartup(true)).catch(() => {});
  }, []);

  const finish = async () => {
    if (startup) await enable().catch(() => {});
    await api.setSettings({ ...settings, onboarded: true });
    onDone();
  };

  const steps = [
    {
      title: "Convert where you already are",
      body: "Start dragging a file in File Explorer, then hold Shift. Drop it on a format and the copy lands right beside the original.",
      art: <GestureDemo tools={false} />,
    },
    {
      title: "Add Ctrl for tools",
      body: "Compress, crop, trim, merge, read metadata and more. Tap Ctrl while the wheel is open to switch.",
      art: <GestureDemo tools />,
    },
    {
      title: "Private by design",
      body: "Every conversion runs on this PC with engines installed alongside KiwiConvert. Your files are never uploaded anywhere.",
      art: (
        <div className="grid h-52 place-items-center rounded-2xl bg-sunken ring-1 ring-line">
          <motion.div initial={{ scale: 0.6, rotate: -12 }} animate={{ scale: 1, rotate: 0 }} transition={spring.bloom} className="grid size-20 place-items-center rounded-3xl bg-accent text-accent-ink shadow-lg">
            <Lock className="size-9" />
          </motion.div>
        </div>
      ),
    },
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col px-4 pb-4">
      <AnimatePresence mode="wait">
        <motion.div
          key={step}
          initial={{ opacity: 0, x: 30 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: -30 }}
          transition={spring.soft}
          className="flex flex-1 flex-col"
        >
          {steps[step].art}
          <h2 className="mt-4 font-display text-[19px] font-bold leading-tight text-ink">{steps[step].title}</h2>
          <p className="mt-1.5 text-[13.5px] leading-relaxed text-ink-2">{steps[step].body}</p>
          {step === 2 && (
            <div className="mt-2">
              <Toggle label="Start KiwiConvert with Windows" hint="It waits quietly in the tray" checked={startup} onChange={setStartup} />
            </div>
          )}
        </motion.div>
      </AnimatePresence>
      <div className="mt-3 flex items-center justify-between">
        <div className="flex gap-1.5">
          {steps.map((_, i) => (
            <motion.span key={i} className="h-1.5 rounded-full bg-ink-3/40" animate={{ width: i === step ? 18 : 6, backgroundColor: i === step ? "var(--accent)" : undefined }} />
          ))}
        </div>
        <div className="flex gap-2">
          {step > 0 && (
            <Button variant="ghost" onClick={() => setStep(step - 1)}>
              Back
            </Button>
          )}
          <Button variant="primary" onClick={() => (step < steps.length - 1 ? setStep(step + 1) : finish())}>
            {step < steps.length - 1 ? "Next" : "Get started"}
          </Button>
        </div>
      </div>
    </div>
  );
}
