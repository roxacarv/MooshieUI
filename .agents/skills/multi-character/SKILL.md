---
name: multi-character
description: >-
  Work on MooshieUI's experimental Anima multi-character generation (two
  characters with their own prompts and LoRAs, RS MultiCharacter Sampler). Use
  when changing, testing, debugging or extending that feature, or for
  /multi-character. Carries what was measured and learned; append new findings.
model: sonnet
---

# Multi-character generation (Anima)

Status: **experimental, iteration one** (exactly two characters, side by side, no
interaction). Not for release until the node pack's licence is reviewed. Human
doc: `docs/MULTI_CHARACTER.md` (keep it in step with this file).

## Read first

1. `docs/MULTI_CHARACTER.md` for behaviour, controls, measurements, gaps; `docs/MULTI_CHARACTER_ROADMAP.md` for the ordered iterations (placement first).
2. `src-tauri/src/templates/multichar.rs` for the graph (12+ unit tests).
3. `tests/` (user's own material): `tests/workflows/Anima-Regional-0.4.7-Global-Mix-Adaptive-Focus.json` (a vanilla ComfyUI Anima-Regional workflow the user says works well, including interaction), `tests/manual generation results/` (a result image and the tags used).

## Where the code lives

| Piece | File |
|-------|------|
| Params (`MultiCharacterParams`, `all_loras()`) | `src-tauri/src/comfyui/types.rs` |
| Graph, validation | `src-tauri/src/templates/multichar.rs`, hooked in `templates/mod.rs` (`validate`, early return in `build_workflow`) |
| Node pack install/verify (pinned rev) | `src-tauri/src/comfyui/nodes.rs` (`MULTI_CHARACTER_PACKAGES`, `ensure_/verify_required_multi_character_nodes`, error kind `missing_multi_character_nodes`) |
| Commands | `commands/api.rs` (`ensure_multi_character_nodes[_inner]`), `commands/workflow.rs`, `lib.rs`, `webserver.rs` (dispatch, staff list, generate arm) |
| MCP | `mcp/setups.rs` `snapshot()` clears `multi_character` (no MCP tool yet) |
| Placement canvas | `components/generation/CharacterLayoutCanvas.svelte`, `utils/characterRegion.ts`; backend `CharacterRegion`/`MultiCharacter.region`/`say_position` in `types.rs`, `region_for`/`position_words` in `multichar.rs` |
| Store | `src/lib/stores/generation.svelte.ts` (`multiCharacter*`, `multiCharacterParams()`, `restoreMultiCharacter()`) |
| UI | `src/lib/components/generation/MultiCharacterSection.svelte`, registered in `GenerationPage.svelte` |
| Metadata | `src/App.svelte` writes/restores `mooshie_multi_character` |
| i18n | `generation.multi_character.*`, all 12 locales |

## How the graph works

`PrimitiveStringMultiline x2 -> RSMultiCharacterPrompt -> [RSMultiCharacterLoRA] -> RSMultiCharacterPosition -> RSMultiCharacterFinalize -> RSMultiCharacterSampler -> finish_workflow`.

- Pack: `raspie10032/ComfyUI_RS_MultiCharacter_Sampler`, pinned `d62788806d2a95877a0af9e976076fb628784b51`, GPL-3.0, alpha, changes almost daily. Bump the pin deliberately.
- Prompt, LoRA and Position nodes require **all ten slots** (`characterN_enable/_kind`, `_lora/_lora_strength/_lora_clip_strength`, `_geometry/_x/_y/_width/_height`). Character prompts must be STRING connections.
- The sampler decodes the image itself; `finish_workflow` still runs upscale/facefix/segment after it and needs a positive and negative, so two plain `CLIPTextEncode` nodes are added (nothing else connects to them).
- TeaCache is forced off. `autosave` is false (the app saves through `MooshieSaveImage`, which streams over the websocket and writes nothing to disk).
- Boxes: a character's own `region` (centre x/y and width/height as fractions), else the column: centre x=(i+.5)/n, y=.5, width 1/n, height 1.0. Validation: sides >= 0.1 and inside the image. `interaction` (text appended to every character) switches characters without a box to `overlapping_box` (0.35/0.65, width 0.65); frontend twin `overlapRegion` in `characterRegion.ts` (keep both in step). `say_position` appends `left side`/`right side` by centre x (<0.4, >0.6).
- LoRA mode `precision` (independent prediction per LoRA, about 4x a plain run) vs `normal` (about 1.7x, may drift identity).
- Sampler tuning the app sets: `initial_spatial_separation` 0.9 to 0.5 and `initial_character_gain` 1.7 to 1.3 (cohesion); `initial_common_prior` 0.05 to 0.8 and `common_after_step3_boost` 0.1 to 0.43 (scene strength, default 0.6 = 0.5/0.3).
- `share_scene` (default on) appends the main prompt to each character's prompt.

## What is known (evidence, not opinion)

- Two character LoRAs on a plain KSampler: both characters take the first one's look in every seed. The RS graph kept two identities.
- **Main prompt is nearly ignored by default**: common weight stays at `initial_common_prior` (0.05) for the first 40% of denoising, rising to 1 by 75%. Backgrounds from the main prompt stayed flat or white (0 of 3 seeds). 0.5/0.3 gave an interior on the right in 3 of 3 and the left in 1 of 3; 0.8/0.5 gave 2 of 3 and turned the second character's light hair black once; 0.5/0.3 plus the scene text in each character gave both halves in 3 of 3.
- **Back view** (character facing away despite `facing viewer`) comes from the LoRA/prompt, not the method: one LoRA alone on a plain KSampler was from behind in 3 of 3 seeds. Negative `from behind, back, facing away` fixes most seeds, not all (a precision run still gave a short-haired back view).
- **Framing tags are weak**: `cowboy shot` in the main prompt worked for the left character in 3 of 3 seeds, not for the right one (LoRA bias to full body; also true alone). Columns are 512x1024 at 1024x1024, which also pulls to full body.
- Timings (RTX 3060 12 GB): 1344x768 plain 52 s, normal 82 to 86 s, precision 204 to 210 s. 1024x1024 normal 81 to 93 s, precision 153 to 165 s.
- Normal mode lost the second character's hair colour in 1 of 3 seeds (earlier test). A step-distillation LoRA leaked style across characters.
- A LoRA's effect also depends on the seed: in one seed the second character's LoRA barely took.
- Evidence base is thin: one pair of LoRAs, 2 to 3 seeds, judged by eye. Say so when reporting.
- Not yet compared: the user's Anima-Regional workflow vs RS on the same characters. Whether it works with per-character LoRAs is **untested** (user: "podemos ver isso depois").

## Alternative to evaluate next: ComfyUI-Anima-Regional (An1X3R, MIT)

Separate Identity and Pose prompts per character, a shared scene prompt, an interaction text compiler, `global_mix_v1`. Depends on Anima Artist Mixer nodes (AnimaArtistPack, AnimaArtistAdapterMixer, AnimaFlowCorrectiveSampler). The user's JSON uses it (hug example, 896x1320, no LoRA nodes). Relevant for the interaction iteration. Open questions: per-character LoRA support, speed, licence of its dependencies.

## Testing safely (how it was done)

- Never touch the user's main install data dir (`~/.local/share/com.mooshieui.desktop`). Use the test data dir `~/mooshie-test2` (own ComfyUI at `~/mooshie-test2/comfyui`, venv at `~/mooshie-test2/venv`, models incl. `anima_baseV10` in `checkpoints/`, VAE, text encoders and gacha LoRAs). Do not touch `.part-*` downloads there.
- App against it: `MOOSHIEUI_DATA_DIR=$HOME/mooshie-test2`. The AppImage segfaulted on the main data dir (cause unknown), so keep tests in the test dir.
- Standalone ComfyUI for experiments: `~/mooshie-test2/venv/bin/python main.py --listen 127.0.0.1 --port 18299 --disable-auto-launch --extra-model-paths-config <yaml mapping diffusion_models to checkpoints/>`. Stop it when finished.
- Reproduce the user's runs exactly from the image: read the PNG `parameters` chunk (SwarmUI-style `sui_image_params` plus `mooshie_extra`) for prompt, negative, sampler, scheduler, steps, cfg, size, seed. Real case: `er_sde`, `sgm_uniform`, 30 steps, CFG 4, 1024x1024.
- Vary the seed between runs: ComfyUI caches identical graphs and a repeat reports a fake 2 s.
- Python stdout is buffered when redirected; use `python -u`. Pillow lives in the test venv, not system Python.
- Do not write background waiters with `pgrep -f "<pattern>"` where the pattern is in the waiter's own command line: it matches itself and never ends. Run the work with `run_in_background` and use the completion notification.
- Do not kill the user's processes (a ComfyUI from the main install may be on 8188).

## Build and check (Linux, Linuxbrew)

Put brew's node/cargo/rust/patchelf first via a dir of symlinks, then `/usr/local/bin:/usr/bin:/bin` (brew `pkg-config` breaks the AppImage GTK plugin; system Node 18 is too old, `npm run tauri dev` fails with `styleText`). Checks for any change here:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml          # ~1200 pass, 8 ignored
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server
cargo clippy --manifest-path src-tauri/Cargo.toml        # look for multichar/setups/types
npx svelte-check --threshold error && npm run build
node scripts/check-i18n-parity.mjs                        # every key in all 12 locales
```

## Rules for changes

- Adding a field: Rust `types.rs` with a `serde(default)` (old saved settings), TS type, store state + load/save + `multiCharacterParams()`, `restoreMultiCharacter()`, UI, i18n x12, `setups::snapshot` if per image, tests. Fix the `MultiCharacterParams` literals in `multichar.rs` and `mcp/setups.rs` tests.
- Keep `validate()` rejecting what is unsupported (batch>1, ControlNet, style transfer/ref, regional prompts, prompt scheduling, pause/continue) instead of silently dropping it.
- Decide whether MCP must follow and say so (see `docs/MCP.md`).
- Project rules still apply: no `Co-Authored-By`, owner author `Mooshieblob <blob@mooshieblob.com>`, no `claude/` branches, no em dashes in text posted as the user. The user does not want commits unless asked.
- Untracked files `.ai-jail`, `apply.py`, `jail.toml`, `run_jailed.sh` belong to the user's sandbox, not to the project.

## LoRA rules (since 2026-11)

- Per-character LoRA is optional; known characters work from tags (see knowledge log).
- LoRAs with Anima text adapter weights are refused per character: `header_has_text_adapter` / `validate_multi_character_loras` in `commands/api.rs` (called from `commands/workflow.rs` and the `generate` arm in `webserver.rs`), UI warning via `lora_changes_text_adapter` command + `loraChangesTextAdapter()`; local ComfyUI only. Keys `known_character_tip`, `lora_adapter_warning`.

## Known gaps / backlog

- UI not exercised in the running app beyond the user's first session; mobile layout, LAN server, gallery remix and metadata restore are only built and type-checked.
- Per-character LoRAs are not applied in upscale/facefix passes; prompt chunks/wildcards are not expanded in character prompts; `collectPrefs` sync not updated.
- No positioning UI (fixed left/right columns); no framing control per character; no identity/pose split per character; no interaction.
- Licensing review of the RS pack (GPL-3.0 plus further licences) before any release; the user said they would look at licences after tests.
- Docker image does not pre-install the pack; remote ComfyUI is only checked, never cloned.

## Knowledge log (append, newest last; keep entries short, with evidence)

- 2026-10: first iteration built and integrated; user trial showed three problems (framing, back view, global prompt ignored). Causes found as above; added scene strength, shared scene, metadata recording and a back-view tip. Precision check on the user's seed: interior backgrounds in 2 of 2, one back view remained.
- 2026-11: Anima-Regional 0.4.7 tried on the same pair (Marciana + Remielle LoRAs, 1024x1024, seeds 101-103; own simplified graph: UNET/CLIP/VAE loaders, LoraLoader chain, its Layout/Compiler/PromptPack/Apply nodes, KSampler euler/simple cfg 4, 28 steps; the Artist/FlowCorrective nodes of the user's workflow come from other packs and were not needed). The pack has no LoRA concept: it patches cross-attention of ONE model, so LoRAs are global. With both LoRAs loaded: outfits split correctly by side in 3/3, shared scene (window, sunlight) in 3/3, no back view in 6/6 runs (negative included), but Remielle's silver hair never appeared (right character brown in 3/3, also with only her LoRA loaded) and a ponytail leaked across; cowboy shot ignored. About 70 s per image. RS precision on the user's seed kept silver hair on the right character. Thin evidence (3 seeds, by eye). Test pack is in `~/mooshie-test2/comfyui/custom_nodes/ComfyUI-Anima-Regional` (test dir only).
- 2026-11 (round 2, Yor + Uzaki; seeds 201-203, 1024x1024, euler/simple cfg 4, 28 steps; RS precision, scene strength 0.6, cohesion 0): **characters the model already knows need no LoRA**: tags `yor briar` / `uzaki tsuki` alone gave two distinct, correct characters with RS in 3/3 seeds (about 70 s). Anima-Regional with the same tags gave two Uzaki in 3/3 (Yor lost); with both LoRAs loaded globally, two identical Uzaki in 3/3 (Yor absent); with only Uzaki's LoRA global and Yor by tag, two Uzaki in 3/3. RS with Yor by tag and Uzaki by per-character LoRA (the "optional LoRA" case): both correct in 3/3 (about 143 s). **RS refuses a LoRA that carries Anima text-adapter weights** (`Sysdeep Yor` has 180 adapter keys; error "this LoRA changes Anima's precomputed text adapter... Use it in a global LoRA loader"); detect with safetensors keys containing `adapter`. Per-character LoRAs must be optional; the app should explain that error. Regional timings were erratic (72 s up to 2015 s per image, 72 s per step when it hit memory pressure with a LoRA); RS stable. Prompts had no NSFW negative, some images came out nude; not a method issue.
- 2026-11 (user trial of the build with the round-2 changes): the user generated many consistent images with two characters, **including interaction between them**, and also with an artist tag; "much better than the first iteration", some problems remain (not itemised yet; ask). First-hand report, not measured by us. Treat the current RS iteration as the working baseline.
- 2026-11 NovelAI research (docs.novelai.net multi-character page, V4 launch post, RS NAI API pack README, an SDK issue): official facts are base prompt + per-character prompts and negatives, 5x5 position grid on V4/V4.5, 6 characters on V4 and 22 on V5, box order as default placement, `source#`/`target#`/`mutual#` tags ("not always reliable"), positions advised to be reinforced in words. API takes normalised x/y centres with separate use-coordinates and use-order switches. Internal mechanism unpublished (per-region attention is only a guess). The pinned RS pack supports point/box/mask regions, up to 10 regions, a Character Interaction node and a detailer; a per-character negative was not found in the pinned version. Roadmap written from this.
- 2026-11 placement test (RS, tags only, Yor + Uzaki, seeds 201-203, 1024x1024): custom boxes (Yor centre x .30, Uzaki .72, both 0.5 wide and overlapping, y shifted a little) gave both characters correct in 3/3, with small visible change from the columns (composition and framing stayed close); swapping the order and the boxes (Uzaki left, Yor right) gave both correct in 3/3 (no identity stuck to a slot). About 62 to 70 s each. Only small shifts were tried; big moves (small box low in the frame, foreground/background) are untested.
- 2026-11 user trial of the placement canvas: character placement worked in all of the owner's tests with no leakage, also together with an extra global (artist) LoRA the owner trained. First-hand report. Placement iteration counted as a success within the known limits.
- 2026-11 interaction test (RS, tags only, Yor + Uzaki, seeds 201-203, "hugging each other, arms around each other, close together" in both character prompts): plain columns gave four people (each column its own hugging pair) in 3/3; overlapping boxes (0.65 wide at 0.35/0.65) gave a correct hugging pair in 2/3 and some feature mixing in the third; `RSMultiCharacterInteraction` delay3/delay6 on top of overlap: nearly identical to no node in two seeds and a third head in the other, so not exposed. About 62 s each. Some outputs were nude (no NSFW negative); not a method issue. Not yet tried: other actions (holding hands, back to back), `source#`-style per-side wording.
- 2026-11 per-character negative (iteration 3): checked the pinned pack (`d627888`) by reading its source. The current builder nodes (`RSMultiCharacterPrompt`, `LoRA`, `Position`) have no UC input; the prompt node's description says "No per-character UC". Only the deprecated all-in-one `RSMultiCharacterPrompts` has `character{i}_uc` (the sampler merges it with the common negative through regional masks, `pipe_compat.regional_negatives`). We build the graph from the current nodes, so there is no per-character negative; do not switch to the deprecated node just for this (it is flagged DEPRECATED and is not the path the interaction node uses). Revisit when bumping the pin: search `character_builder.py` for a `uc` field on a builder node.
- 2026-11 more than two characters (iteration 4; RS, tags only, Yor / Uzaki / Miku / Rem, seeds 301-303, columns): with "group photo, standing side by side" in the main prompt, 3 characters at 1024x1024 gave 0/3 clean (one image Yor plus two Mikus, one with four people, one with two merged), 4 at 1024x1024 gave 1/3 clean, and 3 at 1536x768 gave 5 to 6 people (each character twice); overlapping boxes at 1536x768 were no better. **Removing the group phrase fixed it**: 3 at 1536x768 gave 3/3 exactly right, in order; 4 at 1792x832 gave 3/3 with all four right (one Miku seen from behind). Without the phrase, 3 at 1024x1024 still dropped a character in 3/3 (too narrow: about 341 px of width per column). Rules: no group phrases in the main prompt; give each character about half its height in width (aspect ratio >= 0.5 x count; the UI warns). Times without LoRA about 63 to 75 s for 3 and 4 characters (about as for 2); with LoRAs from the earlier measurement, two LoRAs cost about 4x in precision mode and about 2x in normal (queue overlap made later numbers unreliable; the UI factor 1 + 1.5 L / 1 + 0.35 L is extrapolated). A Yor LoRA cannot be used per character (text adapter). Bug found and fixed on the way: rounding the box centre to two decimals pushed the fourth column's edge to 1.005 and the pack refused it; boxes are now rounded to four decimals and shrunk to stay inside (`axis_inside`). The same bug could hit free-dragged boxes with two characters. Limits: 2 to 6 characters (UI and backend); five and six not measured.
- 2026-11 resolution error (user, 16:9 on 1024 = 1368x768): the sampler refuses sides that are not multiples of 16 ("needs dimensions divisible by 16, from 64 to 4096") while the app allows multiples of 8. Fixed in `multichar.rs` (`sampler_side`): sides snap to the nearest multiple of 16, so the image can be up to 8 px off the size shown. Not an RS limit on aspect ratio.
- 2026-11 directional action test (RS, tags only, Yor + Uzaki, seeds 401-403, 1024x1024, no group phrase): Yor's prompt `kissing another girl's cheek, leaning toward her`, Uzaki's `being kissed on the cheek by another girl`. With overlapping boxes: Yor kisses Uzaki's cheek in 3/3 (direction correct). Same text in plain columns: no kiss in 3/3 (hands on cheeks, a back view, nothing). Same verb in both prompts (`cheek kiss, close together`) with overlap: a kiss in 3/3 but who kisses whom is not controlled. So the working recipe is: actor and receiver each get their own wording, and the boxes must overlap. Overlap currently switches on only when the Interaction field is filled, so per-character action text alone stays in columns. The pack has no source/target concept; this wording split is the closest emulation (about 3 seeds, one pair, one action).
