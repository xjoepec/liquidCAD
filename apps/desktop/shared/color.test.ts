import { describe, expect, it } from "vitest";
import { PALETTE, oklch } from "./color";

describe("oklch", () => {
  it("maps the achromatic axis to greys", () => {
    expect(oklch(0, 0, 0)).toBe("#000000");
    expect(oklch(1, 0, 0)).toBe("#ffffff");
    // OKLab L = 0.5 is sRGB 99 (CSS Color 4 reference value).
    expect(oklch(0.5, 0, 0)).toBe("#636363");
  });

  it("matches CSS Color 4 reference conversions", () => {
    // oklch(0.628 0.2577 29.23) is sRGB red.
    expect(oklch(0.62796, 0.25768, 29.2339)).toBe("#ff0000");
    // oklch(0.452 0.313 264.05) is sRGB blue.
    expect(oklch(0.45201, 0.31321, 264.052)).toBe("#0000ff");
  });

  it("clips out-of-gamut colours instead of wrapping", () => {
    expect(oklch(0.9, 0.4, 145)).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("keeps the chrome blacks neutral", () => {
    for (const hex of [PALETTE.void, PALETTE.canvas]) {
      const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
      expect(Math.max(r!, g!, b!) - Math.min(r!, g!, b!)).toBeLessThanOrEqual(2);
      expect(Math.max(r!, g!, b!)).toBeLessThan(24);
    }
  });
});
