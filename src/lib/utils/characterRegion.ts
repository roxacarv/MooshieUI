/**
 * Boxes for the Characters panel (Anima multi-character). A box is the centre
 * (`x`, `y`) and size (`width`, `height`) as fractions of the image, which is
 * how the sampler reads it. The backend refuses boxes that leave the image or
 * have a side under MIN_SIDE, so every edit here is clamped to a valid box.
 */

export interface CharacterRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const MIN_SIDE = 0.1;
/** Position snaps to this step when the grid is on (a 20x20 grid, fine enough to be free but tidy). */
export const GRID_STEP = 0.05;

/** The equal full-height column a slot owns when it has no box of its own. */
export function columnRegion(index: number, count: number): CharacterRegion {
  const width = 1 / count;
  return { x: (index + 0.5) * width, y: 0.5, width, height: 1 };
}

const round3 = (v: number) => Math.round(v * 1000) / 1000;

/** Clamp a box to a valid one: sides within [MIN_SIDE, 1], fully inside the image. */
export function clampRegion(r: CharacterRegion): CharacterRegion {
  const width = Math.min(1, Math.max(MIN_SIDE, r.width));
  const height = Math.min(1, Math.max(MIN_SIDE, r.height));
  return {
    x: round3(Math.min(1 - width / 2, Math.max(width / 2, r.x))),
    y: round3(Math.min(1 - height / 2, Math.max(height / 2, r.y))),
    width: round3(width),
    height: round3(height),
  };
}

/** Read a box from untrusted data (saved settings, image metadata); null when it is off-shape. */
export function parseRegion(raw: unknown): CharacterRegion | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Partial<CharacterRegion>;
  const values = [r.x, r.y, r.width, r.height];
  if (!values.every((v) => typeof v === "number" && Number.isFinite(v))) return null;
  return clampRegion(r as CharacterRegion);
}

export function snap(value: number, step = GRID_STEP): number {
  return Math.round(value / step) * step;
}
