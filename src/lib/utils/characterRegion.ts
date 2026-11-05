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
/** How many characters one image may have; must match the backend (templates/multichar.rs). */
export const MIN_CHARACTERS = 2;
export const MAX_CHARACTERS = 6;
/** Position snaps to this step when the grid is on (a 20x20 grid, fine enough to be free but tidy). */
export const GRID_STEP = 0.05;

/** The equal full-height column a slot owns when it has no box of its own. */
export function columnRegion(index: number, count: number): CharacterRegion {
  const width = 1 / count;
  return { x: (index + 0.5) * width, y: 0.5, width, height: 1 };
}

/**
 * The layout used while the characters interact: two overlapping boxes. Measured
 * with two characters, plain columns turn each half into its own hugging pair.
 * Must match `overlapping_box` in the backend (templates/multichar.rs).
 */
export function overlapRegion(index: number, count: number): CharacterRegion {
  if (count === 2) return { x: index === 0 ? 0.35 : 0.65, y: 0.5, width: 0.65, height: 1 };
  // More characters: boxes 1.3 columns wide, spread edge to edge.
  const width = Math.min(1, 1.3 / count);
  const step = (1 - width) / (count - 1);
  return { x: width / 2 + index * step, y: 0.5, width, height: 1 };
}

export type LayoutPreset = "row" | "grid" | "pyramid";
export const LAYOUT_PRESETS: LayoutPreset[] = ["row", "grid", "pyramid"];

/** Characters per row, top to bottom, for a preset. */
function presetRows(kind: LayoutPreset, count: number): number[] {
  if (kind === "row") return [count];
  if (kind === "grid") {
    const first = count === 3 ? 2 : Math.ceil(count / 2);
    return [first, count - first];
  }
  const top = Math.max(1, Math.floor((count - 1) / 2));
  return [top, count - top];
}

/**
 * A ready-made arrangement: one box per character, in reading order (left to
 * right, top to bottom). "row" is the default columns, so it returns nulls (no
 * custom box). Rows overlap a little so nobody is cut off at the seam.
 */
export function presetRegions(kind: LayoutPreset, count: number): (CharacterRegion | null)[] {
  if (kind === "row") return Array.from({ length: count }, () => null);
  const rows = presetRows(kind, count);
  const widest = Math.max(...rows);
  const width = 1 / widest;
  const height = rows.length === 1 ? 1 : 0.6;
  const out: CharacterRegion[] = [];
  rows.forEach((perRow, row) => {
    const y = rows.length === 1 ? 0.5 : 0.3 + (row * 0.4) / (rows.length - 1);
    for (let i = 0; i < perRow; i++) {
      out.push(clampRegion({ x: 0.5 + (i - (perRow - 1) / 2) * width, y, width, height }));
    }
  });
  return out;
}

/** Marker colours per character, shared by the cards and the canvas boxes. */
export const CHARACTER_LOOKS = [
  { dot: "bg-amber-400", box: "border-amber-400 bg-amber-400/20 text-amber-200" },
  { dot: "bg-sky-400", box: "border-sky-400 bg-sky-400/20 text-sky-200" },
  { dot: "bg-emerald-400", box: "border-emerald-400 bg-emerald-400/20 text-emerald-200" },
  { dot: "bg-rose-400", box: "border-rose-400 bg-rose-400/20 text-rose-200" },
  { dot: "bg-violet-400", box: "border-violet-400 bg-violet-400/20 text-violet-200" },
  { dot: "bg-orange-400", box: "border-orange-400 bg-orange-400/20 text-orange-200" },
];

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
