import type { Parameter } from "./types";

export function num(value: number, digits = 2): string {
  if (!Number.isFinite(value)) return "-";
  const fixed = value.toFixed(digits);
  return fixed.includes(".") ? fixed.replace(/\.?0+$/, "") : fixed;
}

export function inches(value: number, digits = 2): string {
  return `${num(value, digits)} in`;
}

export function feetInches(value: number): string {
  const feet = Math.floor(value / 12 + 1e-9);
  const rest = value - feet * 12;
  return `${feet}'-${num(rest, 2)}"`;
}

export function pounds(value: number): string {
  return `${Math.round(value).toLocaleString("en-US")} lb`;
}

const UNIT_SUFFIX: Record<Parameter["unit"], string> = { inch: "in", millimeter: "mm", psf: "psf", degree: "deg" };

export function unitSuffix(p: Parameter): string {
  return UNIT_SUFFIX[p.unit] ?? p.unit;
}

/** Nearest allowed value for a snapped parameter, clamped to its range. */
export function snapParameter(p: Parameter, value: number): number {
  const clamped = Math.min(p.max, Math.max(p.min, value));
  if (!p.snap || p.step <= 0) return clamped;
  const steps = Math.round((clamped - p.min) / p.step);
  return Math.min(p.max, p.min + steps * p.step);
}

/** Client-side pre-check mirroring Parameter::validate in Rust. */
export function checkParameter(p: Parameter, value: number): string | null {
  if (!Number.isFinite(value)) return `${p.label} must be a number`;
  if (value < p.min - 1e-9 || value > p.max + 1e-9) return `${p.label} must be between ${num(p.min)} and ${num(p.max)} ${unitSuffix(p)}`;
  if (p.snap && p.step > 0) {
    const steps = (value - p.min) / p.step;
    if (Math.abs(steps - Math.round(steps)) > 1e-6) {
      const lo = p.min + Math.floor(steps) * p.step;
      return `${p.label} must be a multiple of ${num(p.step)} ${unitSuffix(p)} (nearest valid: ${num(lo)} or ${num(lo + p.step)})`;
    }
  }
  return null;
}

export function rgb(c: [number, number, number]): string {
  return `rgb(${Math.round(c[0] * 255)}, ${Math.round(c[1] * 255)}, ${Math.round(c[2] * 255)})`;
}
