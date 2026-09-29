import { useEffect, useState } from "react";
import { api, on } from "./ipc";
import type { Settings } from "./types";

const media = window.matchMedia("(prefers-color-scheme: dark)");

function apply(settings: Settings | null) {
  const pref = settings?.theme ?? "system";
  const dark = pref === "dark" || (pref === "system" && media.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  document.documentElement.dataset.reduceMotion = settings?.reduceMotion ? "true" : "false";
}

/** Keeps the document theme in sync with Settings and the system; returns the settings. */
export function useSettings(): Settings | null {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    let alive = true;
    api.getSettings().then((s) => {
      if (!alive) return;
      setSettings(s);
      apply(s);
    });
    const un = on<Settings>("settings://changed", (s) => {
      setSettings(s);
      apply(s);
    });
    const onSystem = () => setSettings((s) => (apply(s), s));
    media.addEventListener("change", onSystem);
    return () => {
      alive = false;
      un.then((f) => f());
      media.removeEventListener("change", onSystem);
    };
  }, []);
  return settings;
}

export function prefersReducedMotion(settings: Settings | null): boolean {
  return !!settings?.reduceMotion || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}
