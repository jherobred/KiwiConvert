import { StrictMode, Suspense, lazy } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles.css";

// Every window loads this bundle and renders the app for its label. Each app is split
// into its own chunk so the always-resident wheel stays small.
const WheelApp = lazy(() => import("./windows/wheel/WheelApp"));
const ActivityApp = lazy(() => import("./windows/activity/ActivityApp"));
const HubApp = lazy(() => import("./windows/hub/HubApp"));
const ToolApp = lazy(() => import("./windows/tools/ToolApp"));

const label = getCurrentWindow().label;
const transparent = label === "wheel" || label === "activity" || label === "hub";
document.body.classList.add(transparent ? "transparent" : "opaque");

// No browser context menu or reload shortcuts: this is an app, not a page.
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!(t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement)) e.preventDefault();
});
window.addEventListener("keydown", (e) => {
  if (e.key === "F5" || (e.ctrlKey && (e.key === "r" || e.key === "R" || e.key === "p" || e.key === "P"))) e.preventDefault();
});

function App() {
  if (label === "wheel") return <WheelApp />;
  if (label === "activity") return <ActivityApp />;
  if (label === "hub") return <HubApp />;
  return <ToolApp />;
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Suspense fallback={null}>
      <App />
    </Suspense>
  </StrictMode>,
);
