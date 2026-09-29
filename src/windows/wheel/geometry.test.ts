import { describe, expect, it } from "vitest";
import { segmentAt, sectorPath } from "./geometry";

// Same cases as the Rust tests in src-tauri/src/wheel.rs, so the two stay in agreement.
describe("segmentAt", () => {
  it("puts segment zero at the top and goes clockwise", () => {
    expect(segmentAt(0, -120, 8)).toBe(0);
    expect(segmentAt(120, 0, 8)).toBe(2);
    expect(segmentAt(0, 120, 8)).toBe(4);
    expect(segmentAt(-120, 0, 8)).toBe(6);
    expect(segmentAt(-30, -120, 8)).toBe(0);
  });

  it("ignores the center and the outside", () => {
    expect(segmentAt(10, 10, 8)).toBe(-1);
    expect(segmentAt(0, -260, 8)).toBe(-1);
    expect(segmentAt(0, -120, 0)).toBe(-1);
  });
});

describe("sectorPath", () => {
  it("draws a closed path with two arcs", () => {
    const d = sectorPath(0, 8, 64, 168, 3);
    expect(d.startsWith("M")).toBe(true);
    expect(d.match(/A/g)?.length).toBe(2);
    expect(d.endsWith("Z")).toBe(true);
  });
});
