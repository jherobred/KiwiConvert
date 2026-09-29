import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, on } from "../../lib/ipc";
import { bytes } from "../../lib/format";
import { spring } from "../../lib/motion";
import { prefersReducedMotion, useSettings } from "../../lib/theme";
import type { WheelPayload } from "../../lib/types";
import { WIN_W } from "./geometry";
import { Wheel } from "./Wheel";

export default function WheelApp() {
  const settings = useSettings();
  const reduceMotion = prefersReducedMotion(settings);
  const [state, setState] = useState<WheelPayload | null>(null);
  const [hover, setHover] = useState(-1);
  const [chosen, setChosen] = useState<number | null>(null);
  const [closing, setClosing] = useState(false);
  const [thumb, setThumb] = useState<string | null>(null);
  const generation = useRef(0);

  const apply = useCallback((p: WheelPayload) => {
    if (p.generation !== generation.current) {
      generation.current = p.generation;
      setChosen(null);
      setClosing(false);
      setThumb(null);
    }
    setState(p);
    setHover(p.hover);
  }, []);

  useEffect(() => {
    api.wheelSnapshot().then(apply);
    const subs = [
      on<WheelPayload>("wheel://state", apply),
      on<number>("wheel://hover", setHover),
      on<{ index: number; generation: number }>("wheel://chosen", (p) => {
        if (p.generation !== generation.current) return;
        setChosen(p.index);
        window.setTimeout(() => api.wheelHidden(p.generation), 580);
      }),
      on<{ reason: string; generation: number }>("wheel://close", (p) => {
        if (p.generation !== generation.current) return;
        setClosing(true);
        window.setTimeout(() => api.wheelHidden(p.generation), 260);
      }),
    ];
    return () => subs.forEach((s) => s.then((f) => f()));
  }, [apply]);

  const first = state?.files?.first;
  useEffect(() => {
    if (!first) return;
    let alive = true;
    api.thumbnail(first, 192).then((t) => alive && setThumb(t)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [first]);

  const clickMode = !!state?.clickMode;
  const options = state?.options ?? [];

  // Keyboard control when the wheel was opened by click (from the hub or "Open with").
  useEffect(() => {
    if (!clickMode || !state?.open) return;
    const onKey = (e: KeyboardEvent) => {
      if (chosen !== null || closing) return;
      if (e.key === "Escape") api.wheelClose("escape");
      else if (e.key === "Tab" || e.key === "Control") {
        e.preventDefault();
        api.wheelToggleMode();
      } else if (e.key === "ArrowRight" || e.key === "ArrowDown") {
        setHover((h) => (options.length ? (h + 1 + options.length) % options.length : -1));
      } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
        setHover((h) => (options.length ? (h - 1 + options.length) % options.length : -1));
      } else if (e.key === "Enter" && hover >= 0) {
        api.wheelChoose(hover);
      }
    };
    // Clicking elsewhere is handled in Rust through window activation.
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [clickMode, state?.open, options.length, hover, chosen, closing]);

  const visible = !!state?.open && !!state.files && !closing;
  const files = state?.files ?? null;
  const hovered = hover >= 0 ? options[hover] : null;
  const title = files ? (files.count > 1 ? `${files.count} files` : files.name) : "";

  return (
    <div className="relative h-full w-full select-none" onContextMenu={(e) => e.preventDefault()}>
      <AnimatePresence>
        {visible && state && (
          <motion.div
            key={state.generation}
            className="absolute inset-0"
            initial={{ opacity: 1 }}
            exit={reduceMotion ? { opacity: 0 } : { opacity: 0, scale: 0.86 }}
            transition={{ duration: 0.2, ease: [0.4, 0, 1, 1] }}
            style={{ transformOrigin: "220px 220px" }}
            onPointerDown={(e) => {
              // A click in the empty corners of the window dismisses a click-mode wheel.
              if (clickMode && e.target === e.currentTarget) api.wheelClose("outside");
            }}
          >
            <Wheel
              options={options}
              mode={state.mode}
              hover={hover}
              chosen={chosen}
              thumb={thumb}
              count={files?.count ?? 1}
              sizeLabel={files ? bytes(files.totalBytes) : null}
              interactive={clickMode}
              onHover={setHover}
              onChoose={(i) => api.wheelChoose(i)}
              reduceMotion={reduceMotion}
            />

            <motion.div
              className="absolute left-1/2 flex -translate-x-1/2 flex-col items-center gap-1.5"
              style={{ top: 404, width: WIN_W - 40 }}
              initial={{ opacity: 0, y: -8 }}
              animate={{ opacity: chosen === null ? 1 : 0, y: 0 }}
              transition={spring.soft}
            >
              <div className="flex max-w-full items-center gap-2 rounded-full bg-[rgb(18_22_14/0.86)] px-3.5 py-1.5 text-[12.5px] font-semibold text-white shadow-lg ring-1 ring-white/10 backdrop-blur">
                <AnimatePresence mode="wait" initial={false}>
                  <motion.span
                    key={hovered ? `h${hover}` : options.length ? "name" : "empty"}
                    className="truncate"
                    initial={{ opacity: 0, y: 4 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -4 }}
                    transition={{ duration: 0.12 }}
                  >
                    {hovered ? (
                      <>
                        <span className={state.mode === "tools" ? "text-gold-300" : "text-kiwi-300"}>{hovered.label}</span>
                        <span className="text-white/60"> · </span>
                        {hovered.hint}
                      </>
                    ) : options.length ? (
                      title
                    ) : state.mode === "tools" ? (
                      "No tools for this selection"
                    ) : (
                      "Nothing to convert this into"
                    )}
                  </motion.span>
                </AnimatePresence>
              </div>
              {clickMode ? (
                <div className="flex items-center gap-1 rounded-full bg-[rgb(18_22_14/0.8)] p-0.5 text-[11px] font-semibold text-white/80 ring-1 ring-white/10">
                  {(["convert", "tools"] as const).map((m) => (
                    <button
                      key={m}
                      className="relative rounded-full px-2.5 py-0.5 capitalize"
                      onClick={() => state.mode !== m && api.wheelToggleMode()}
                    >
                      {state.mode === m && (
                        <motion.span layoutId="mode-pill" className="absolute inset-0 rounded-full bg-white/18" transition={spring.snap} />
                      )}
                      <span className="relative">{m}</span>
                    </button>
                  ))}
                </div>
              ) : (
                <div className="rounded-full bg-[rgb(18_22_14/0.7)] px-2.5 py-0.5 text-[10.5px] font-medium text-white/70">
                  <kbd className="font-sans font-bold text-white/90">Ctrl</kbd> {state.mode === "tools" ? "back to formats" : "for tools"}
                </div>
              )}
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
