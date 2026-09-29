import type { Kind } from "./types";
import { extension } from "./format";

// Mirrors `kind_of` in src-tauri/src/registry.rs for the file types the tool windows handle.
const byExt: Record<string, Kind> = {};
const add = (kind: Kind, exts: string) => exts.split(" ").forEach((e) => (byExt[e] = kind));
add("image", "jpg jpeg jpe jfif png webp heic heif hif avif gif bmp dib tif tiff ico svg tga qoi");
add("video", "mp4 m4v mov qt mkv avi wmv asf webm flv mpg mpeg m2ts mts ts 3gp 3g2 ogv vob");
add("audio", "mp3 m4a aac wav flac ogg oga opus aif aiff aifc wma alac amr ac3 mka");
add("pdf", "pdf");
add("docx", "docx");
add("text", "txt md markdown log csv");
add("subtitle", "srt vtt");
add("archive", "zip tar gz tgz rar");

export function kindOf(path: string): Kind {
  return byExt[extension(path)] ?? "other";
}

/** Image types the WebView can draw directly. Others go through `imagePreview`. */
export function browserImage(path: string): boolean {
  return ["jpg", "jpeg", "jpe", "jfif", "png", "webp", "gif", "bmp", "svg", "avif", "ico"].includes(extension(path));
}
