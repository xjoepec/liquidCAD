// OKLCH colour. The stylesheet uses oklch() directly; WebGL materials and the
// Electron window background need sRGB hex, so they go through this helper
// from the same OKLCH values.

/** OKLCH (l 0..1, c chroma, h degrees) to an sRGB hex string, gamut-clipped. */
export function oklch(l: number, c: number, h: number): string {
  const rad = (h * Math.PI) / 180;
  const a = c * Math.cos(rad);
  const b = c * Math.sin(rad);
  const l_ = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m_ = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s_ = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const linear = [
    4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
    -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
    -0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_,
  ];
  return (
    "#" +
    linear
      .map((v) => {
        const x = Math.min(1, Math.max(0, v));
        const srgb = x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055;
        return Math.round(srgb * 255)
          .toString(16)
          .padStart(2, "0");
      })
      .join("")
  );
}

/** Colours shared by the 3D scene and the window chrome. Mirrors the tokens in src/styles.css. */
export const PALETTE = {
  void: oklch(0.105, 0.002, 70),
  canvas: oklch(0.145, 0.003, 70),
  gridMajor: oklch(0.29, 0.005, 70),
  gridMinor: oklch(0.2, 0.004, 70),
  ground: oklch(0.2, 0.005, 70),
  sky: oklch(0.94, 0.008, 80),
  fill: oklch(0.86, 0.03, 250),
  accent: oklch(0.79, 0.155, 68),
  accentGlow: oklch(0.27, 0.06, 65),
  hover: oklch(0.4, 0.02, 70),
  edge: oklch(0.1, 0.002, 70),
  pick: oklch(0.82, 0.16, 152),
  pickHover: oklch(0.92, 0.08, 155),
  water: oklch(0.46, 0.055, 228),
  walker: oklch(0.88, 0.14, 88),
  mate: oklch(0.74, 0.14, 305),
  gravity: oklch(0.68, 0.19, 35),
  buoyancy: oklch(0.72, 0.13, 240),
  text: oklch(0.93, 0.004, 85),
} as const;
