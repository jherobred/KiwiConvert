import { Suspense, lazy, useEffect, useState, type ComponentType } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../../lib/ipc";
import { useSettings } from "../../lib/theme";
import { kindOf } from "../../lib/kinds";
import type { Tool, ToolSession } from "../../lib/types";

export interface ToolProps {
  session: ToolSession;
  /** Starts a background job for this tool and closes the window. */
  run: (options: unknown, paths?: string[]) => Promise<void>;
  close: () => void;
}

const views = {
  compress: lazy(() => import("./CompressView")),
  resize: lazy(() => import("./ResizeView")),
  speed: lazy(() => import("./SpeedView")),
  channels: lazy(() => import("./ChannelsView")),
  metadata: lazy(() => import("./MetadataView")),
  image: lazy(() => import("./image/ImageEditor")),
  video: lazy(() => import("./video/VideoEditor")),
  audio: lazy(() => import("./audio/AudioEditor")),
  pdf: lazy(() => import("./pdf/PdfOrganizer")),
  collage: lazy(() => import("./collage/CollageBuilder")),
} satisfies Record<string, ComponentType<ToolProps>>;

function viewFor(tool: Tool, path: string): ComponentType<ToolProps> {
  const kind = kindOf(path);
  switch (tool) {
    case "compress":
      return views.compress;
    case "resize":
      return views.resize;
    case "speed":
      return views.speed;
    case "channels":
      return views.channels;
    case "metadata":
      return views.metadata;
    case "collage":
      return views.collage;
    case "organize-pdf":
    case "split-pdf":
      return views.pdf;
    case "crop":
    case "trim":
      if (kind === "image") return views.image;
      return kind === "audio" ? views.audio : views.video;
    case "adjust":
    case "annotate":
    case "redact":
      return views.image;
    case "split":
    case "snapshot":
      return views.video;
    case "bleep":
    case "normalize":
      return views.audio;
    default:
      return views.compress;
  }
}

export default function ToolApp() {
  useSettings();
  const [session, setSession] = useState<ToolSession | null>(null);

  useEffect(() => {
    api.toolSession().then(setSession);
    const un = getCurrentWindow().onCloseRequested(async (e) => {
      e.preventDefault();
      await api.toolClosed();
    });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !(e.target instanceof HTMLInputElement)) api.toolClosed();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      un.then((f) => f());
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  useEffect(() => {
    // Show the window only after the first frame is painted, so it never flashes white.
    if (session) requestAnimationFrame(() => requestAnimationFrame(() => api.windowReady()));
  }, [session]);

  if (!session) return null;
  const View = viewFor(session.tool, session.paths[0] ?? "");
  const run = async (options: unknown, paths?: string[]) => {
    await api.runTool(session.tool, paths ?? session.paths, options);
    await api.toolClosed();
  };
  return (
    <div className="flex h-full flex-col bg-canvas">
      <Suspense fallback={null}>
        <View session={session} run={run} close={() => api.toolClosed()} />
      </Suspense>
    </div>
  );
}
