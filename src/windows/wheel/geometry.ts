// Wheel geometry, shared with src-tauri/src/wheel.rs (WIN_W, WIN_H, CX, CY, R_CENTER, R_HIT).
// Rust hit-tests drops with the same numbers, so change both together.

export const WIN_W = 440;
export const WIN_H = 480;
export const CX = 220;
export const CY = 220;
export const R_CENTER = 58;
export const R_HIT = 200;

/** Radii of the drawn kiwi slice. */
export const R_CORE = 58;
export const R_INNER = 64;
export const R_OUTER = 168;
export const R_SKIN = 182;

/** Segment under a point relative to the center; -1 for the center and outside. */
export function segmentAt(dx: number, dy: number, n: number): number {
  const d = Math.hypot(dx, dy);
  if (n === 0 || d < R_CENTER || d > R_HIT) return -1;
  const deg = (((Math.atan2(dy, dx) * 180) / Math.PI + 90) % 360 + 360) % 360;
  const seg = 360 / n;
  return Math.floor(((deg + seg / 2) % 360) / seg) % n;
}

/** Point at `r` along an angle measured clockwise from the top. */
export function polar(r: number, deg: number, cx = CX, cy = CY): [number, number] {
  const a = (deg * Math.PI) / 180;
  return [cx + r * Math.sin(a), cy - r * Math.cos(a)];
}

export function segmentAngle(i: number, n: number): number {
  return (i * 360) / n;
}

/**
 * Annular sector for segment `i` of `n`, leaving a gap of constant width `gapPx` between
 * neighbours (the angular gap is recomputed for each radius so the gap stays parallel).
 */
export function sectorPath(i: number, n: number, r0: number, r1: number, gapPx: number): string {
  const center = segmentAngle(i, n);
  const half = 180 / n;
  const gapAt = (r: number) => ((gapPx / 2 / r) * 180) / Math.PI;
  const [ox0, oy0] = polar(r1, center - half + gapAt(r1));
  const [ox1, oy1] = polar(r1, center + half - gapAt(r1));
  const [ix1, iy1] = polar(r0, center + half - gapAt(r0));
  const [ix0, iy0] = polar(r0, center - half + gapAt(r0));
  const large = 360 / n > 180 ? 1 : 0;
  const f = (v: number) => v.toFixed(2);
  return (
    `M${f(ox0)} ${f(oy0)} A${r1} ${r1} 0 ${large} 1 ${f(ox1)} ${f(oy1)} ` +
    `L${f(ix1)} ${f(iy1)} A${r0} ${r0} 0 ${large} 0 ${f(ix0)} ${f(iy0)} Z`
  );
}

/** Deterministic pseudo-random numbers so seeds sit in the same place every time. */
export function jitter(seed: number): number {
  const x = Math.sin(seed * 12.9898) * 43758.5453;
  return x - Math.floor(x);
}
