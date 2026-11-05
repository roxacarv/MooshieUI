/**
 * The curated actions between two characters (Anima multi-character). The words
 * each one writes live in the backend (`ACTIONS` in templates/multichar.rs); the
 * ids here must match it. An action is added only after it was measured.
 */

export interface CharacterAction {
  kind: string;
  /** Zero-based character positions. */
  actor: number;
  receiver: number;
}

export interface ActionKind {
  id: string;
  /** Both characters do the same thing: who is listed first does not matter. */
  mutual: boolean;
}

export const ACTION_KINDS: ActionKind[] = [
  { id: "kiss_cheek", mutual: false },
  { id: "hug", mutual: true },
  { id: "hold_hands", mutual: true },
  { id: "lean_on_shoulder", mutual: false },
];

/** Same limit as the backend (`MAX_ACTIONS`). */
export const MAX_ACTIONS = 4;

/** Read actions from untrusted data (saved settings, image metadata); drops anything off-shape. */
export function parseActions(raw: unknown, count: number): CharacterAction[] {
  if (!Array.isArray(raw)) return [];
  const out: CharacterAction[] = [];
  for (const item of raw) {
    const a = item as Partial<CharacterAction> | null;
    if (!a || typeof a.kind !== "string" || !ACTION_KINDS.some((k) => k.id === a.kind)) continue;
    if (!Number.isInteger(a.actor) || !Number.isInteger(a.receiver)) continue;
    const actor = a.actor as number;
    const receiver = a.receiver as number;
    if (actor < 0 || receiver < 0 || actor >= count || receiver >= count || actor === receiver) continue;
    out.push({ kind: a.kind, actor, receiver });
    if (out.length === MAX_ACTIONS) break;
  }
  return out;
}
