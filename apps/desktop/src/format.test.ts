import { describe, expect, it } from "vitest";
import { checkParameter, feetInches, num, snapParameter, unitSuffix } from "./format";
import type { Parameter } from "./types";

const width: Parameter = {
  name: "width_in",
  label: "Dock width",
  value: 96,
  unit: "inch",
  min: 48,
  max: 240,
  step: 24,
  snap: true,
  group: "plan",
};

describe("parameter checks mirror the Rust validator", () => {
  it("names the nearest valid values like the engine does", () => {
    expect(checkParameter(width, 100)).toBe("Dock width must be a multiple of 24 in (nearest valid: 96 or 120)");
    expect(checkParameter(width, 144)).toBeNull();
    expect(checkParameter(width, 24)).toMatch(/between 48 and 240/);
    expect(checkParameter(width, Number.NaN)).toMatch(/must be a number/);
  });

  it("snaps drags to the module and clamps to the range", () => {
    expect(snapParameter(width, 131)).toBe(120);
    expect(snapParameter(width, 133)).toBe(144);
    expect(snapParameter(width, 10)).toBe(48);
    expect(snapParameter(width, 999)).toBe(240);
    expect(snapParameter({ ...width, snap: false }, 131)).toBe(131);
  });

  it("formats numbers for CAD readouts", () => {
    expect(num(96)).toBe("96");
    expect(num(2.125, 3)).toBe("2.125");
    expect(num(1.5)).toBe("1.5");
    expect(feetInches(100.5)).toBe("8'-4.5\"");
    expect(feetInches(96)).toBe("8'-0\"");
  });
});

describe("unit suffixes", () => {
  it("covers every unit the engine sends", () => {
    expect(unitSuffix({ ...width, unit: "degree" })).toBe("deg");
    expect(unitSuffix({ ...width, unit: "psf" })).toBe("psf");
    expect(unitSuffix(width)).toBe("in");
  });
});
