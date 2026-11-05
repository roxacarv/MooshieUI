# Multi-character generation (Anima)

Describe two characters separately and have the app keep each one's look, and
each one's LoRA, with the right character. Status: **experimental, first
iteration**. Exactly two characters, side by side, no interaction between them.

## What it does

The **Characters** section on the generation page (Anima models, text to image,
ComfyUI backend) takes, for each character:

- a description (what they look like and are doing),
- an optional LoRA with its own strength.

The scene and style stay in the main prompt. Characters are placed left to
right in the order given: character 1 on the left half, character 2 on the right.

Four controls shape the result:

| Control | Meaning |
|---------|---------|
| LoRA mode | **Best identity** (`precision`, default) keeps an independent prediction per LoRA. **Faster** (`normal`) shares layer work and can drift identity. |
| Scene strength | How much the main prompt (setting, lighting, background) counts from the first steps. Maps to the sampler's `initial_common_prior` 0.05 to 0.8 and `common_after_step3_boost` 0.1 to 0.43. Default 0.6 (0.5 / 0.3). |
| Repeat the main prompt in each character | On by default. Appends the main prompt to each character's own prompt so both halves agree on the setting. |
| Scene cohesion | 0 keeps characters strictly apart (the node's defaults). 1 trades some separation for one continuous background. Maps to the sampler's `initial_spatial_separation` 0.9 to 0.5 and `initial_character_gain` 1.7 to 1.3. |

## How it works

`templates/multichar.rs` builds the graph around the **RS MultiCharacter Sampler**
node pack (`raspie10032/ComfyUI_RS_MultiCharacter_Sampler`):

```
loaders (+ global LoRAs) ──────────────────────────────┐
PrimitiveStringMultiline x2 → RSMultiCharacterPrompt   │
  → [RSMultiCharacterLoRA] → RSMultiCharacterPosition  │
  → RSMultiCharacterFinalize → RSMultiCharacterSampler ┘ → finish_workflow
```

- The sampler decodes the image itself, so the template hands `finish_workflow`
  its IMAGE output. Upscale, face fix and segment passes still run after it.
- Those passes need a positive and a negative, which the sampler does not
  expose. The template adds two plain `CLIPTextEncode` nodes (the whole scene,
  and the negative). Nodes nothing connects to never run.
- Global LoRAs (the normal LoRA list) load before the sampler and affect every
  character. Per-character LoRAs are applied to that character's region only.
- Anima TeaCache is switched off for this path (it keeps a step counter inside
  the model patch).
- The node's own `autosave` is off; the app saves through `MooshieSaveImage`.

### Not supported together (rejected with a message)

Batch size above 1, ControlNet, style transfer, style or character reference,
regional prompts, prompt scheduling, pause and continue. Img2img, inpainting
and Image Edit never use this path.

## Installing the node pack

The pack is **not** installed at setup. Switching the panel on calls
`ensure_multi_character_nodes`, which clones it at the pinned revision into
`custom_nodes/`, then restarts ComfyUI so it loads (as every lazy node install
does). Generating verifies the five node classes first and answers with an
actionable error instead of ComfyUI's raw `missing_node_type`.

- Pin: `MULTI_CHARACTER_PACKAGES` in `src-tauri/src/comfyui/nodes.rs`
  (`d62788806d2a95877a0af9e976076fb628784b51`, the revision this was tested
  on). The pack is alpha and changes almost daily, so bump it deliberately and
  re-run the checks below.
- A **remote** ComfyUI has its own nodes: the app only checks, never clones.
- The Docker image does not pre-install the pack.

## MCP

Character descriptions belong to one image, like the prompt, so
`mcp::setups::snapshot` clears `multi_character`. An assistant generating through
MCP gets the owner's plain model setup, never a stale pair of characters. There
is no MCP tool for multi-character generation yet.

### Why the main prompt needed its own control

The node keeps the common (main) prompt at `initial_common_prior` (0.05) for the
first 40% of denoising and only raises it to 1 by 75%, while each character's
prior is always 1. Composition and background are decided early, so with the
defaults a background asked for in the main prompt (`indoors, window`) is
ignored: it stayed flat or white in real use. Scene strength raises that prior.

### Other things seen in real use

- A character facing away despite `facing viewer` came from the LoRA and the
  prompt, not from the method: one character LoRA alone gave a back view in 3 of
  3 seeds on a plain KSampler. Adding `from behind, back, facing away` to the
  negative prompt fixed most seeds, not all (the panel shows this as a tip).
- Framing tags (`cowboy shot`) are weak. In the main prompt they worked for the
  left character in 3 of 3 seeds, but the right character's LoRA kept a full
  body. The same LoRA does this on its own. The columns are 512x1024 at
  1024x1024, so shape also pulls toward full body.
- The metadata of the saved image now records the characters, their LoRAs and
  these settings (`mooshie_multi_character`), and "use these settings" restores
  the panel. `smart_guidance` is not recorded for these images, because the RS
  sampler never sees that patch.

## What was measured

Tested on an RTX 3060 12 GB, 1344x768, 30 steps, CFG 4, `euler_ancestral` /
`sgm_uniform`, directly in ComfyUI (not through the app) with the pinned pack.
Judged by eye, three seeds, one pair of character LoRAs: treat these as
directional, not as a benchmark.

| Run | Time |
|-----|------|
| Plain KSampler, 2 LoRAs global | 50 to 52 s |
| RS, per-character LoRAs, Faster | 82 to 86 s |
| RS, per-character LoRAs, Best identity | 204 to 210 s |
| RS, no LoRA | 64 to 66 s (plain KSampler: 52 to 54 s) |

- With two character LoRAs on one KSampler, **both characters took on the first
  character's hair and outfit** in every seed. The RS graph kept two distinct
  identities in all three, with a short-haired second character in one seed.
- Faster mode lost the second character's hair colour in one of three seeds
  that Best identity kept.
- A LoRA applied to one character leaked style into the other in a test with a
  step-distillation LoRA (the worst case for isolation).
- Backgrounds often differ between the two halves. Raising cohesion helped in
  most seeds and cost identity in one.
- Asking for a wider shot in the prompt did not reliably change framing.

Scene strength on the user's own prompt (1024x1024, Faster mode, 3 seeds, back
view added to the negative prompt):

| Setting | Interior background in a seed |
|---------|-------------------------------|
| Node defaults (0.05 / 0.1) | 0 of 3: sky, sea or white |
| 0.5 / 0.3 | right half in 3 of 3, left half in 1 of 3 |
| 0.8 / 0.5 | 2 of 3, and the second character's light hair turned black in 1 |
| 0.5 / 0.3 plus the scene text in each character | both halves in 3 of 3 |

That last row is what the panel does by default. One of those seeds still gave a
back view even with the negative.

## Known gaps

- Not verified: "remix" from the gallery for these images (metadata is
  recorded and restored, but this was only built and type-checked), the mobile
  layout, upscale or face fix after a multi-character image, the LAN web
  server end to end.
- Character prompts go to the node as typed: prompt chunks, wildcards and
  random syntax are not expanded inside them.
- Per-character LoRAs are not applied in the upscale or face fix passes.
- Characters are fixed to left and right columns; there is no positioning UI.
- Licensing of the node pack has not been reviewed (it is GPL-3.0 and ships
  further model and code licences). Do this before any release.
- ComfyUI must be running a version that supports the Anima architecture and
  `PrimitiveStringMultiline` (tested on 0.41.1).

## Placement

Each character can have its own box on a canvas in the Characters panel
(`CharacterLayoutCanvas.svelte`): drag to move, drag a corner to resize, arrow
keys to nudge, optional snap to a 5 percent grid, Swap and Reset to columns.
A box is stored as centre and size as fractions of the image
(`MultiCharacter.region`, `utils/characterRegion.ts`). A character without a
box keeps its equal full-height column, so older settings and images behave as
before. The backend refuses boxes smaller than 0.1 or leaving the image.
"Also say the position in words" appends `left side` or `right side` (by box
centre; nothing in the middle third) to that character's prompt, because a box
is a strong hint, not a rule. The layout is saved with the image metadata and
restored on remix. In the owner's own use, placement held in every test with no
leakage between characters, also with an extra global (artist) LoRA.

## Interaction

One text field, "Interaction (both characters)", is appended to every
character's prompt (`MultiCharacterParams.interaction`). While it is filled,
characters without a box of their own use an overlapping layout (boxes 0.65 wide
centred at 0.35 and 0.65) instead of two columns; their own boxes always win.
Measured (Yor and Uzaki by tag, seeds 201 to 203, "hugging each other, arms
around each other, close together"): with plain columns each half became its own
hugging pair, four people (0 of 3); with overlapping boxes the pair hugged with
the right identities in 2 of 3, and in the third some features mixed. The
pack's Character Interaction node (delay3 and delay6) changed little in two
seeds and added a third head in the other, so it is not exposed.

## Status after the second trial

In the owner's own use of the app after the scene-strength, shared-scene and
optional-LoRA changes, many images came out consistent with two characters,
including interaction between them and an artist tag. Some problems remain and
have not been itemised. This is a hands-on report, not a measurement.

## LoRAs are optional

Characters the model already knows (for example `yor briar`, `uzaki tsuki`) come
out right from their tag alone, so a LoRA per character is optional and meant
for characters the model does not know. The Characters panel says so.

A LoRA that carries Anima text adapter weights (its safetensors header lists
`adapter` keys; `Sysdeep Yor` is one) cannot be kept on one character: the
sampler refuses it. The panel warns as soon as such a LoRA is picked, and a
generation with it is refused with a message that says to use it in the main
LoRA list or to describe the character by its tag. The check reads the file
header, so it only runs for a ComfyUI the app launched itself.

## More than two characters

Two to six characters. Add or remove them below the cards; the canvas has Row, Grid and Pyramid presets. What measured well: 3 characters at 1536x768 and 4 at 1792x832 (all right, in order, 3 of 3 seeds each). Two things matter: keep group phrases such as `group photo`, `standing side by side` and `3girls` out of the main prompt (they made every character appear twice), and give each character about half its height in width (the panel warns when the image is narrower than 0.5 x count). Five and six characters are not measured. Time without LoRAs stays close to the two-character time; each character LoRA adds a lot in high quality mode.

## Per-character negative

Not supported. The pinned RS pack's builder nodes take no negative per character; only its deprecated all-in-one prompt node does. Put exclusions in the main negative prompt, or state the wanted trait positively in the character's own prompt. Checked again whenever the pin is bumped.

## Next steps

- Compare against `ComfyUI-Anima-Regional` (MIT), using the vanilla workflow in
  `tests/workflows/Anima-Regional-0.4.7-Global-Mix-Adaptive-Focus.json`. It splits
  Identity and Pose per character, shares one scene prompt and compiles
  interaction text, which is what the interaction iteration needs. Whether it
  works with a LoRA per character is untested.
- Ideas not built: a framing control per character, a positioning UI, splitting
  identity from pose, per-character LoRAs in the upscale and face fix passes.
- Review the node pack's licences before any release.

`tests/manual generation results/` holds one real result and the tags behind it,
useful for reproducing the three problems found in the first trial (framing,
back view, ignored main prompt).

The `multi-character` skill (`.agents/skills/multi-character/SKILL.md`) keeps the
working knowledge and test method, and is where new findings are appended.

## Checks when bumping the pin

1. `cargo test --manifest-path src-tauri/Cargo.toml multichar nodes`
2. Run the pack's `examples/two-characters-anima-api.json` once to confirm the
   node inputs the template sets (all ten slots of Prompt, LoRA and Position are
   required) have not changed.
3. Generate one pair with a LoRA on each character and compare with a previous
   result.
