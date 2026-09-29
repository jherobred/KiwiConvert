import {
  AudioLines,
  BellOff,
  Camera,
  Crop,
  EyeOff,
  FileStack,
  Files,
  Gauge,
  Headphones,
  LayoutGrid,
  LayoutPanelLeft,
  Merge,
  PenLine,
  QrCode,
  Scaling,
  Scissors,
  Shrink,
  SlidersHorizontal,
  Split,
  Tags,
  type LucideIcon,
} from "lucide-react";

// Names come from `Tool::icon` in src-tauri/src/registry.rs.
const icons: Record<string, LucideIcon> = {
  shrink: Shrink,
  scaling: Scaling,
  crop: Crop,
  "sliders-horizontal": SlidersHorizontal,
  "pen-line": PenLine,
  "eye-off": EyeOff,
  tags: Tags,
  "qr-code": QrCode,
  "file-stack": FileStack,
  "layout-grid": LayoutGrid,
  scissors: Scissors,
  gauge: Gauge,
  split: Split,
  merge: Merge,
  camera: Camera,
  "audio-lines": AudioLines,
  "bell-off": BellOff,
  headphones: Headphones,
  files: Files,
  "layout-panel-left": LayoutPanelLeft,
};

export function toolIcon(name: string | null | undefined): LucideIcon | null {
  return name ? (icons[name] ?? null) : null;
}
