// Small pose helpers mirroring liquidcad_core::geometry::Pose.
import type { Pose, Vec3 } from "./types";

export function rotate(q: Pose["rotation"], v: Vec3): Vec3 {
  const [w, x, y, z] = q;
  // v' = q v q*, expanded.
  const ix = w * v[0] + y * v[2] - z * v[1];
  const iy = w * v[1] + z * v[0] - x * v[2];
  const iz = w * v[2] + x * v[1] - y * v[0];
  const iw = -x * v[0] - y * v[1] - z * v[2];
  return [
    ix * w + iw * -x + iy * -z - iz * -y,
    iy * w + iw * -y + iz * -x - ix * -z,
    iz * w + iw * -z + ix * -y - iy * -x,
  ];
}

/** World point to the part's local frame. */
export function toLocal(pose: Pose, p: Vec3): Vec3 {
  const [w, x, y, z] = pose.rotation;
  const d: Vec3 = [p[0] - pose.origin[0], p[1] - pose.origin[1], p[2] - pose.origin[2]];
  return rotate([w, -x, -y, -z], d);
}

export function sub(a: Vec3, b: Vec3): Vec3 {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
}
export function dot(a: Vec3, b: Vec3): number {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}
export function norm(a: Vec3): number {
  return Math.sqrt(dot(a, a));
}
export function angleDeg(a: Vec3, b: Vec3): number {
  const c = dot(a, b) / (norm(a) * norm(b) || 1);
  return (Math.acos(Math.max(-1, Math.min(1, c))) * 180) / Math.PI;
}
