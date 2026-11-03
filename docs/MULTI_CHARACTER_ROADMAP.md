# Multi-character roadmap

Where the experimental Anima multi-character feature is going, in the order that
makes each step testable on its own. The working feature and what was measured
are in [MULTI_CHARACTER.md](MULTI_CHARACTER.md); the `multi-character` skill
keeps the test method and the knowledge log.

Rules for every step: build it behind the existing experimental switch, test it
with fixed seeds against the previous behaviour, write what was measured into
the skill's knowledge log, and decide whether MCP has to follow (see CLAUDE.md).

## Where we are (baseline)

Two characters, left and right columns, prompt and optional LoRA each, shared
scene with a strength control, RS MultiCharacter Sampler (pinned). Hands-on use
gave consistent pairs, interaction written as plain text, and an artist tag
working. Known characters work from their tag alone; a LoRA carrying Anima text
adapter weights is refused per character.

## What NovelAI does that is worth copying (research summary)

Official: one base prompt plus one prompt per character (subject count in the
base), a negative field per character, an optional canvas to place each
character (5x5 grid on V4/V4.5, up to 6 characters on V4 and 22 on V5), the
order of the boxes as the default placement, and `source#` / `target#` /
`mutual#` action tags the docs call "not always reliable". Positions are a
strong hint, and the docs advise reinforcing them in words. Third parties show
the API takes normalised x/y centres per character and has separate "use
coordinates" and "use order" switches. How the model uses them internally is
not published: attention masks per region is the likely guess, nothing more.
Sources are listed in the session notes of the skill's knowledge log.

What the RS pack already offers (pinned revision): point, box or mask per
region, up to 10 regions, a Character Interaction node that delays separation
decay where regions overlap, an OpenPose-to-mask helper, and a detailer that
matches characters. A per-character negative prompt is **not confirmed** in the
pinned version; check before promising it.

## Iterations

### 1. Placement (the NovelAI canvas) - built, see MULTI_CHARACTER.md "Placement"

Goal: the user chooses where each character stands instead of fixed halves.

- Store x, y, width, height per character in `MultiCharacterParams` (serde
  default to today's columns, so old settings and metadata still load).
- A small canvas in the Characters panel: draggable boxes that follow the aspect
  ratio, snap to a coarse grid, and swap order. `multichar.rs` already writes box
  geometry; it only reads fixed columns today.
- Offer an optional "also say it in words" toggle that appends `left side` /
  `right side` style tags, because positions are a hint, not a rule.
- Allow overlap (needed for interaction in step 2).
- Metadata record and remix restore the layout.
- Test: same prompts and seeds with columns, shifted boxes and a swapped order;
  note when the layout is ignored (framing and the right-hand bias seen before).

### 2. Interaction - first version built, see MULTI_CHARACTER.md "Interaction"

Goal: two characters that touch or look at each other, reliably.

- A shared "interaction" text field appended to both characters (what the user
  already does by hand), plus a helper that turns a single action into the
  matching words for each side.
- Wire the pack's Character Interaction node for overlapping boxes.
- Do not copy `source#` / `target#`: they are trained into NovelAI, not Anima.
- Test: hug, hold hands, back to back, across several seeds; count how often
  contact looks right and how often identities merge.

### 3. Per-character controls

- Per-character negative prompt, only if the pinned pack supports it (otherwise
  write that down and stop).
- Per-character framing and facing helpers (the bias seen with the right-hand
  character), possibly a "keep facing the viewer" default that fills the
  character negative instead of the global one.
- Test on the pairs that failed before (back view, cowboy shot).

### 4. More than two characters

- Character count control (3 up to the pack's limit of 10 regions in total).
- Layout presets (row, triangle) feeding step 1.
- Cost grows with each LoRA in precision mode (about 4x for two); measure and
  show an estimate before generating.
- Test crowding: identity merging, extra people, framing.

### 5. Passes after generation

- Per-character LoRAs in the upscale and face fix passes (the pack's detailer
  matches characters to regions; decide whether to use it or to run the app's
  own passes per region).
- Expand prompt chunks and wildcards inside character prompts.
- `collectPrefs` sync for the new settings.

### 6. Saved characters

- Pick a saved character (the app's character library) into a slot, filling its
  tag and, when it has one, its LoRA.
- Show "known to the model" versus "needs a LoRA" using the tag-only result as
  the guide.

### 7. Release readiness

- Review the RS pack's licences (GPL-3.0 and the extra adapter licences it
  ships) and decide: bundled install on demand, or user-supplied.
- Docker image pre-install, remote ComfyUI check, an MCP tool if useful.
- Move the knowledge log into user-facing docs and decide whether any of this
  belongs upstream.

## Open questions

- The problems still seen after the second trial have not been itemised. Ask the
  owner and add them here with seeds.
- Is the Anima-Regional identity/pose split worth borrowing as prompt structure
  only (it cannot hold one LoRA per character)?
- Mobile layout, LAN mode, gallery remix and metadata restore have never been
  exercised in the running app.

## Order of work

1 (placement) first, because 2 (interaction) and 4 (more characters) depend on
free boxes. 3 can run in parallel as small fixes. 5 and 6 follow once the layout
is stable. 7 before anything is offered to anyone else.
