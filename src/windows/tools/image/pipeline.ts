// The photo editor's rendering pipeline:
//   source -> geometry (rotate, flip, straighten) -> adjustments (WebGL) -> redactions -> annotations -> crop
// The same code renders the live preview (reduced size) and the exported file (full size).

export interface Geometry {
  /** Clockwise quarter turns. */
  quarter: number;
  flipH: boolean;
  flipV: boolean;
  /** Straighten angle in degrees (-45..45). The image is scaled so no empty corners show. */
  angle: number;
}

export interface Adjustments {
  exposure: number;
  brightness: number;
  contrast: number;
  highlights: number;
  shadows: number;
  saturation: number;
  warmth: number;
  tint: number;
  vignette: number;
}

export const noAdjustments: Adjustments = {
  exposure: 0,
  brightness: 0,
  contrast: 0,
  highlights: 0,
  shadows: 0,
  saturation: 0,
  warmth: 0,
  tint: 0,
  vignette: 0,
};

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type RedactStyle = "black" | "pixelate" | "blur";

export interface Redaction extends Rect {
  id: number;
  style: RedactStyle;
}

export type Shape =
  | { id: number; kind: "pen" | "highlighter"; points: [number, number][]; color: string; width: number }
  | { id: number; kind: "arrow" | "rect" | "ellipse"; from: [number, number]; to: [number, number]; color: string; width: number }
  | { id: number; kind: "text"; at: [number, number]; text: string; color: string; size: number };

/** Size of the image after quarter turns. Straightening keeps this size. */
export function orientedSize(w: number, h: number, g: Geometry): [number, number] {
  return g.quarter % 2 === 0 ? [w, h] : [h, w];
}

/** Draws `source` with the geometry applied into a canvas of the oriented size times `scale`. */
export function renderGeometry(source: CanvasImageSource, sw: number, sh: number, g: Geometry, scale: number): HTMLCanvasElement {
  const [ow, oh] = orientedSize(sw, sh, g);
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(ow * scale));
  canvas.height = Math.max(1, Math.round(oh * scale));
  const ctx = canvas.getContext("2d")!;
  ctx.imageSmoothingQuality = "high";
  const rad = (g.angle * Math.PI) / 180;
  // Zoom needed so the rotated image still covers the whole frame.
  const cover = Math.abs(Math.cos(rad)) + Math.abs(Math.sin(rad)) * Math.max(ow / oh, oh / ow);
  ctx.translate(canvas.width / 2, canvas.height / 2);
  ctx.rotate(rad);
  ctx.scale(cover * scale, cover * scale);
  ctx.rotate((g.quarter * Math.PI) / 2);
  ctx.scale(g.flipH ? -1 : 1, g.flipV ? -1 : 1);
  ctx.drawImage(source, -sw / 2, -sh / 2, sw, sh);
  return canvas;
}

const VERTEX = `#version 300 es
in vec2 pos;
out vec2 uv;
void main() {
  uv = pos * 0.5 + 0.5;
  gl_Position = vec4(pos, 0.0, 1.0);
}`;

// Adjustments work in linear light where it matters (exposure) and in display space for
// tone controls, which is how photo editors feel to use.
const FRAGMENT = `#version 300 es
precision highp float;
in vec2 uv;
out vec4 color;
uniform sampler2D img;
uniform float exposure, brightness, contrast, highlights, shadows, saturation, warmth, tint, vignette;

vec3 toLinear(vec3 c) { return pow(c, vec3(2.2)); }
vec3 toDisplay(vec3 c) { return pow(max(c, 0.0), vec3(1.0 / 2.2)); }

void main() {
  vec4 src = texture(img, vec2(uv.x, 1.0 - uv.y));
  vec3 c = src.rgb;
  c = toDisplay(toLinear(c) * exp2(exposure * 2.0));
  c += brightness * 0.35;
  float l = dot(c, vec3(0.2126, 0.7152, 0.0722));
  float hw = smoothstep(0.45, 1.0, l);
  float sw = 1.0 - smoothstep(0.0, 0.55, l);
  c += highlights * 0.45 * hw * (highlights < 0.0 ? l : 1.0 - l);
  c += shadows * 0.45 * sw * (shadows > 0.0 ? 1.0 - l : l);
  c = (c - 0.5) * (1.0 + contrast * (contrast > 0.0 ? 1.2 : 0.8)) + 0.5;
  c.r += warmth * 0.12;
  c.b -= warmth * 0.12;
  c.g -= tint * 0.1;
  c.r += tint * 0.04;
  c.b += tint * 0.04;
  float gray = dot(c, vec3(0.2126, 0.7152, 0.0722));
  c = mix(vec3(gray), c, 1.0 + saturation);
  vec2 d = uv - 0.5;
  float v = smoothstep(0.85, 0.2, length(d) * 1.35);
  c *= mix(1.0, v, clamp(vignette, 0.0, 1.0));
  c = mix(c, c + (1.0 - v) * 0.5, clamp(-vignette, 0.0, 1.0));
  color = vec4(clamp(c, 0.0, 1.0), src.a);
}`;

/** Applies adjustments with WebGL2. Reuses one GL context across renders. */
export class Adjuster {
  readonly canvas = document.createElement("canvas");
  private gl: WebGL2RenderingContext;
  private program: WebGLProgram;
  private texture: WebGLTexture;
  private source: HTMLCanvasElement | null = null;

  constructor() {
    const gl = this.canvas.getContext("webgl2", { premultipliedAlpha: false, preserveDrawingBuffer: true });
    if (!gl) throw new Error("WebGL2 is not available");
    this.gl = gl;
    const compile = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? "shader error");
      return s;
    };
    const p = gl.createProgram()!;
    gl.attachShader(p, compile(gl.VERTEX_SHADER, VERTEX));
    gl.attachShader(p, compile(gl.FRAGMENT_SHADER, FRAGMENT));
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? "link error");
    this.program = p;
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(p, "pos");
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    this.texture = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  }

  get maxSize(): number {
    return this.gl.getParameter(this.gl.MAX_TEXTURE_SIZE) as number;
  }

  render(source: HTMLCanvasElement, a: Adjustments): HTMLCanvasElement {
    const gl = this.gl;
    if (this.source !== source) {
      this.canvas.width = source.width;
      this.canvas.height = source.height;
      gl.bindTexture(gl.TEXTURE_2D, this.texture);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
      this.source = source;
    }
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.useProgram(this.program);
    const set = (name: keyof Adjustments) => gl.uniform1f(gl.getUniformLocation(this.program, name), a[name] / 100);
    (Object.keys(noAdjustments) as (keyof Adjustments)[]).forEach(set);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    return this.canvas;
  }
}

/** Paints redactions onto `ctx`, sampling from `image` (both in the same coordinate space). */
export function paintRedactions(ctx: CanvasRenderingContext2D, image: CanvasImageSource, list: Redaction[], scale: number) {
  for (const r of list) {
    const x = r.x * scale;
    const y = r.y * scale;
    const w = Math.max(1, r.w * scale);
    const h = Math.max(1, r.h * scale);
    ctx.save();
    ctx.beginPath();
    ctx.rect(x, y, w, h);
    ctx.clip();
    if (r.style === "black") {
      ctx.fillStyle = "#000";
      ctx.fillRect(x, y, w, h);
    } else if (r.style === "pixelate") {
      const block = Math.max(6, Math.round(Math.max(w, h) / 12));
      const tw = Math.max(1, Math.round(w / block));
      const th = Math.max(1, Math.round(h / block));
      const tiny = document.createElement("canvas");
      tiny.width = tw;
      tiny.height = th;
      tiny.getContext("2d")!.drawImage(image, x, y, w, h, 0, 0, tw, th);
      ctx.imageSmoothingEnabled = false;
      ctx.drawImage(tiny, 0, 0, tw, th, x, y, w, h);
    } else {
      // Blur, then pixelate lightly so the blur can't be reversed by sharpening.
      ctx.filter = `blur(${Math.max(8, Math.min(w, h) / 6)}px)`;
      ctx.drawImage(image, x - 40, y - 40, w + 80, h + 80, x - 40, y - 40, w + 80, h + 80);
      ctx.filter = "none";
    }
    ctx.restore();
  }
}

function arrowHead(ctx: CanvasRenderingContext2D, from: [number, number], to: [number, number], size: number) {
  const angle = Math.atan2(to[1] - from[1], to[0] - from[0]);
  ctx.beginPath();
  ctx.moveTo(to[0], to[1]);
  ctx.lineTo(to[0] - size * Math.cos(angle - 0.45), to[1] - size * Math.sin(angle - 0.45));
  ctx.lineTo(to[0] - size * Math.cos(angle + 0.45), to[1] - size * Math.sin(angle + 0.45));
  ctx.closePath();
  ctx.fill();
}

/** Paints annotation shapes. Coordinates are image pixels, multiplied by `scale`. */
export function paintShapes(ctx: CanvasRenderingContext2D, shapes: Shape[], scale: number) {
  ctx.save();
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (const s of shapes) {
    ctx.globalAlpha = s.kind === "highlighter" ? 0.38 : 1;
    ctx.strokeStyle = s.color;
    ctx.fillStyle = s.color;
    if ("points" in s) {
      ctx.lineWidth = s.width * scale * (s.kind === "highlighter" ? 3 : 1);
      ctx.beginPath();
      s.points.forEach(([x, y], i) => (i ? ctx.lineTo(x * scale, y * scale) : ctx.moveTo(x * scale, y * scale)));
      ctx.stroke();
    } else if (s.kind === "text") {
      ctx.font = `700 ${s.size * scale}px "Segoe UI", sans-serif`;
      ctx.textBaseline = "top";
      ctx.lineWidth = Math.max(2, s.size * scale * 0.12);
      ctx.strokeStyle = s.color === "#ffffff" ? "rgba(0,0,0,0.55)" : "rgba(255,255,255,0.85)";
      ctx.strokeText(s.text, s.at[0] * scale, s.at[1] * scale);
      ctx.fillText(s.text, s.at[0] * scale, s.at[1] * scale);
    } else if ("from" in s) {
      ctx.lineWidth = s.width * scale;
      const [x1, y1] = [s.from[0] * scale, s.from[1] * scale];
      const [x2, y2] = [s.to[0] * scale, s.to[1] * scale];
      ctx.beginPath();
      if (s.kind === "rect") ctx.rect(Math.min(x1, x2), Math.min(y1, y2), Math.abs(x2 - x1), Math.abs(y2 - y1));
      else if (s.kind === "ellipse") ctx.ellipse((x1 + x2) / 2, (y1 + y2) / 2, Math.abs(x2 - x1) / 2, Math.abs(y2 - y1) / 2, 0, 0, Math.PI * 2);
      else {
        const head = Math.max(12, s.width * scale * 3.2);
        const len = Math.hypot(x2 - x1, y2 - y1);
        const k = len > 0 ? (len - head * 0.7) / len : 0;
        ctx.moveTo(x1, y1);
        ctx.lineTo(x1 + (x2 - x1) * k, y1 + (y2 - y1) * k);
      }
      ctx.stroke();
      if (s.kind === "arrow") arrowHead(ctx, [x1, y1], [x2, y2], Math.max(12, s.width * scale * 3.2));
    }
  }
  ctx.restore();
}

/**
 * Renders the finished image at full size and returns its RGBA pixels.
 */
export function exportImage(
  source: CanvasImageSource,
  sw: number,
  sh: number,
  g: Geometry,
  a: Adjustments,
  crop: Rect,
  redactions: Redaction[],
  shapes: Shape[],
  adjuster: Adjuster,
): { pixels: Uint8Array; width: number; height: number } {
  const base = renderGeometry(source, sw, sh, g, 1);
  const hasAdjust = (Object.keys(a) as (keyof Adjustments)[]).some((k) => a[k] !== 0);
  const tooBig = base.width > adjuster.maxSize || base.height > adjuster.maxSize;
  const adjusted = hasAdjust && !tooBig ? adjuster.render(base, a) : base;
  const full = document.createElement("canvas");
  full.width = base.width;
  full.height = base.height;
  const ctx = full.getContext("2d", { willReadFrequently: true })!;
  ctx.drawImage(adjusted, 0, 0);
  paintRedactions(ctx, adjusted, redactions, 1);
  paintShapes(ctx, shapes, 1);
  const x = Math.max(0, Math.round(crop.x));
  const y = Math.max(0, Math.round(crop.y));
  const w = Math.max(1, Math.min(full.width - x, Math.round(crop.w)));
  const h = Math.max(1, Math.min(full.height - y, Math.round(crop.h)));
  const data = ctx.getImageData(x, y, w, h).data;
  return { pixels: new Uint8Array(data.buffer, data.byteOffset, data.byteLength), width: w, height: h };
}
