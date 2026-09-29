// Mirrors the serde types in src-tauri. Keep the two in sync.

export type Kind =
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "docx"
  | "text"
  | "subtitle"
  | "archive"
  | "folder"
  | "other";

export type Fmt =
  | "jpg" | "png" | "webp" | "heic" | "avif" | "gif" | "bmp" | "tiff" | "ico" | "svg"
  | "pdf" | "docx" | "txt"
  | "mp4" | "mov" | "mkv" | "avi" | "wmv" | "webm"
  | "mp3" | "m4a" | "wav" | "flac" | "ogg" | "opus" | "aiff"
  | "srt" | "vtt" | "zip" | "tar" | "tgz" | "gz" | "extract";

export type Tool =
  | "compress" | "resize" | "crop" | "adjust" | "annotate" | "redact" | "metadata"
  | "read-qr" | "make-pdf" | "collage" | "trim" | "speed" | "split" | "join"
  | "snapshot" | "normalize" | "bleep" | "channels" | "merge-pdf" | "split-pdf"
  | "organize-pdf";

export type Mode = "convert" | "tools";

export type Action = { type: "convert"; to: Fmt } | { type: "tool"; tool: Tool };

export interface WheelOption {
  label: string;
  icon: string | null;
  hint: string;
  action: Action;
}

export interface FileSummary {
  count: number;
  totalBytes: number;
  name: string;
  first: string;
  kind: Kind;
}

export interface WheelPayload {
  generation: number;
  open: boolean;
  mode: Mode;
  clickMode: boolean;
  files: FileSummary | null;
  options: WheelOption[];
  hover: number;
}

export type JobStatus = "queued" | "running" | "done" | "failed" | "cancelled";

export interface JobView {
  id: string;
  title: string;
  detail: string;
  status: JobStatus;
  progress: number;
  stage: string | null;
  outputs: string[];
  text: string | null;
  error: string | null;
  input: string | null;
  startedMs: number;
  finishedMs: number | null;
}

export interface Settings {
  gestureEnabled: boolean;
  anyApp: boolean;
  revealOutputs: boolean;
  outputFolder: string | null;
  jpegQuality: number;
  webpQuality: number;
  avifQuality: number;
  heicQuality: number;
  keepMetadata: boolean;
  pdfDpi: number;
  pageSize: "a4" | "letter" | "fit";
  videoPreset: "fast" | "balanced" | "quality";
  gpuEncoding: boolean;
  theme: "system" | "light" | "dark";
  reduceMotion: boolean;
  onboarded: boolean;
}

export interface ToolSession {
  tool: Tool;
  paths: string[];
}

export interface MediaInfo {
  duration: number;
  width: number;
  height: number;
  fps: number;
  hasVideo: boolean;
  hasAudio: boolean;
  videoCodec: string;
  audioCodec: string;
  channels: number;
}

export interface PageInfo {
  doc: number;
  index: number;
  width: number;
  height: number;
}

export interface MetaReport {
  kind: Kind;
  groups: { name: string; entries: { key: string; value: string }[] }[];
  fields: { key: string; label: string; value: string }[];
  hasLocation: boolean;
  editable: boolean;
}

export interface AppInfo {
  version: string;
  ffmpeg: boolean;
  pdf: boolean;
}
