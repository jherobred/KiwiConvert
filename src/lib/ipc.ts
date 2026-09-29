import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppInfo,
  Fmt,
  JobView,
  MediaInfo,
  MetaReport,
  Mode,
  PageInfo,
  Settings,
  Tool,
  ToolSession,
  WheelOption,
  WheelPayload,
} from "./types";

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  appInfo: () => invoke<AppInfo>("app_info"),
  setPaused: (paused: boolean) => invoke<void>("set_paused", { paused }),
  quit: () => invoke<void>("quit"),

  wheelSnapshot: () => invoke<WheelPayload>("wheel_snapshot"),
  wheelChoose: (index: number) => invoke<void>("wheel_choose", { index }),
  wheelClose: (reason: string) => invoke<void>("wheel_close", { reason }),
  wheelHidden: (generation: number) => invoke<void>("wheel_hidden", { generation }),
  wheelToggleMode: () => invoke<void>("wheel_toggle_mode"),
  openWheel: (paths: string[], x: number, y: number) => invoke<void>("open_wheel", { paths, x, y }),
  wheelOptions: (paths: string[], mode: Mode) => invoke<WheelOption[]>("wheel_options", { paths, mode }),
  thumbnail: (path: string, size = 192) => invoke<string | null>("thumbnail", { path, size }),

  jobsActive: () => invoke<JobView[]>("jobs_active"),
  jobsHistory: () => invoke<JobView[]>("jobs_history"),
  jobCancel: (id: string) => invoke<void>("job_cancel", { id }),
  jobDismiss: (id: string) => invoke<void>("job_dismiss", { id }),
  historyClear: () => invoke<void>("history_clear"),
  runConvert: (paths: string[], to: Fmt) => invoke<string>("run_convert", { paths, to }),
  runTool: (tool: Tool, paths: string[], options: unknown = null) => invoke<string>("run_tool", { tool, paths, options }),
  openTool: (tool: Tool, paths: string[]) => invoke<void>("open_tool", { tool, paths }),

  activityResize: (height: number) => invoke<void>("activity_resize", { height }),
  activityHide: () => invoke<void>("activity_hide"),
  hubHide: () => invoke<void>("hub_hide"),
  windowReady: () => invoke<void>("window_ready"),
  toolSession: () => invoke<ToolSession | null>("tool_session"),
  toolClosed: () => invoke<void>("tool_closed"),
  reveal: (paths: string[]) => invoke<void>("reveal", { paths }),
  openFile: (path: string) => invoke<void>("open_file", { path }),
  openLink: (url: string) => invoke<void>("open_link", { url }),
  allowFile: (path: string) => invoke<void>("allow_file", { path }),

  mediaInfo: (path: string) => invoke<MediaInfo>("media_info", { path }),
  videoFrame: (path: string, time: number, width = 640) => invoke<string>("video_frame", { path, time, width }),
  videoStrip: (path: string, count: number, width = 160) => invoke<string[]>("video_strip", { path, count, width }),
  audioPeaks: (path: string, buckets: number) => invoke<{ peaks: number[]; duration: number }>("audio_peaks", { path, buckets }),
  previewProxy: (path: string, video: boolean) => invoke<string>("preview_proxy", { path, video }),
  imagePreview: (path: string) => invoke<{ path: string; width: number; height: number }>("image_preview", { path }),
  pdfPages: (paths: string[]) => invoke<PageInfo[]>("pdf_pages", { paths }),
  pdfThumb: (path: string, index: number, size = 220) => invoke<string>("pdf_thumb", { path, index, size }),
  metadataRead: (path: string) => invoke<MetaReport>("metadata_read", { path }),
  compressEstimate: (path: string, options: unknown) => invoke<number>("compress_estimate", { path, options }),
  fileSize: (path: string) => invoke<number>("file_size", { path }),

  /** Sends raw RGBA pixels to be encoded and saved beside `source`. */
  saveRenderedImage: (
    pixels: Uint8Array,
    meta: { source: string; width: number; height: number; suffix?: string; name?: string; format?: string; title?: string },
  ) =>
    invoke<string>("save_rendered_image", pixels, {
      headers: { "x-kiwi-meta": encodeURIComponent(JSON.stringify(meta)) },
    }),
};

/** URL a window can load a local file from (the path must be allowed first). */
export const fileUrl = (path: string) => convertFileSrc(path);

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}
