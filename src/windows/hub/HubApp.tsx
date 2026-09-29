import { AnimatePresence, motion } from "motion/react";
import { ArrowLeft, Lock, Pause, Settings as SettingsIcon, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, on } from "../../lib/ipc";
import { spring } from "../../lib/motion";
import { useSettings } from "../../lib/theme";
import { IconButton, KiwiMark } from "../../components/ui";
import { Home } from "./Home";
import { SettingsView } from "./SettingsView";
import { Welcome } from "./Welcome";

export type View = "home" | "settings" | "welcome";

export default function HubApp() {
  const settings = useSettings();
  const [view, setView] = useState<View>("home");
  const [paused, setPaused] = useState(false);
  const [shownAt, setShownAt] = useState(0);

  useEffect(() => {
    if (settings && !settings.onboarded) setView("welcome");
  }, [settings]);

  useEffect(() => {
    // Hiding when another app is activated is handled in Rust.
    const subs = [
      on<void>("hub://settings", () => setView("settings")),
      on<void>("hub://shown", () => setShownAt(Date.now())),
      on<boolean>("app://paused", setPaused),
    ];
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && api.hubHide();
    window.addEventListener("keydown", onKey);
    return () => {
      subs.forEach((s) => s.then((f) => f()));
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  return (
    <div className="h-full w-full p-2.5">
      <motion.div
        key={shownAt}
        initial={{ opacity: 0, y: 18, scale: 0.97 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={spring.soft}
        className="panel flex h-full flex-col overflow-hidden rounded-[20px] bg-canvas"
      >
        <header data-tauri-drag-region className="flex h-13 shrink-0 items-center gap-2.5 px-4">
          <AnimatePresence mode="wait" initial={false}>
            {view === "settings" ? (
              <motion.div key="back" initial={{ opacity: 0, x: -6 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0 }}>
                <IconButton label="Back" onClick={() => setView("home")}>
                  <ArrowLeft className="size-4.5" />
                </IconButton>
              </motion.div>
            ) : (
              <motion.div key="mark" initial={{ opacity: 0, scale: 0.8 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0 }}>
                <KiwiMark className="size-7" />
              </motion.div>
            )}
          </AnimatePresence>
          <div data-tauri-drag-region className="flex-1 font-display text-[15px] font-bold tracking-tight text-ink">
            {view === "settings" ? "Settings" : "KiwiConvert"}
          </div>
          {paused && (
            <span className="flex items-center gap-1 rounded-full bg-gold-100 px-2 py-0.5 text-[11px] font-semibold text-gold-700">
              <Pause className="size-3" /> Paused
            </span>
          )}
          {view === "home" && (
            <IconButton label="Settings" onClick={() => setView("settings")}>
              <SettingsIcon className="size-4.5" />
            </IconButton>
          )}
          <IconButton label="Close" onClick={() => api.hubHide()}>
            <X className="size-4.5" />
          </IconButton>
        </header>

        <div className="relative min-h-0 flex-1">
          <AnimatePresence mode="popLayout" initial={false}>
            <motion.div
              key={view}
              className="absolute inset-0 flex flex-col"
              initial={{ opacity: 0, x: view === "home" ? -24 : 24 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: view === "home" ? -24 : 24 }}
              transition={spring.soft}
            >
              {view === "home" && <Home />}
              {view === "settings" && settings && <SettingsView settings={settings} />}
              {view === "welcome" && settings && <Welcome settings={settings} onDone={() => setView("home")} />}
            </motion.div>
          </AnimatePresence>
        </div>

        {view === "home" && (
          <footer className="flex h-10 shrink-0 items-center justify-center gap-1.5 border-t border-line text-[11.5px] text-ink-3">
            <Lock className="size-3.5" /> Files never leave this PC
          </footer>
        )}
      </motion.div>
    </div>
  );
}
