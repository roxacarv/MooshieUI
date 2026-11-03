# PROJECT_STRUCTURE.md

Map of the MooshieUI repository: what it is, how the pieces fit together, and where to find things. Version at time of writing: **2.3.29** (AGPL-3.0). For rules and commands see [`CLAUDE.md`](CLAUDE.md) / [`AGENTS.md`](AGENTS.md).

## 1. What MooshieUI is

A beginner-friendly front end for **ComfyUI** that hides the node graph behind a guided UI. It generates **images** (txt2img, img2img, inpainting, Image Edit, upscaling, face fix, ControlNet, style references), **video** (MiniMax H3 text/frame/reference to video, RIFE/GMFSS interpolation), and **music** (YuE2 songs, covers, score tools). It can also generate images through the **NovelAI API** (user's own key) instead of ComfyUI.

MooshieUI installs and manages ComfyUI itself (uv, Python, PyTorch, custom nodes) through a setup wizard, or connects to an existing/remote ComfyUI.

## 2. Instances (ways the code runs)

One Rust crate (`comfyui-desktop`, lib name `comfyui_desktop_lib`) and one Svelte frontend produce several runtime shapes:

| Instance | How it is built/run | Notes |
|----------|--------------------|-------|
| **Desktop app (Tauri v2)** | `default = ["desktop"]` feature; binary `comfyui-desktop` (`src-tauri/src/main.rs`) | Webview (WebKitGTK on Linux). Frontend talks to Rust over Tauri IPC. Also starts the embedded axum web server on demand. |
| **Browser mode** | Same desktop binary serves the built frontend with axum (`webserver.rs`, default `127.0.0.1`, optional LAN mode with mDNS `mooshieui.local`, UPnP, auth, roles, per-user galleries) | `window.__MOOSHIE_BROWSER_MODE__` is injected into the HTML; IPC becomes REST POST + SSE. Dedicated mobile layout (`components/mobile/`). A 120s idle heartbeat watchdog stops it unless LAN mode is on. |
| **Headless server** | `cargo build --no-default-features --features server`; binary `mooshieui-server` (`src-tauri/src/server_main.rs`) | No Tauri, no GUI. Used by Docker (`deploy/Dockerfile`, `docker-compose.yml`) and Kubernetes (`deploy/k8s/`). Admin seeded from `MOOSHIEUI_ADMIN_USER` / `MOOSHIEUI_ADMIN_PASS`. |
| **Managed ComfyUI** | Child process spawned by `src-tauri/src/comfyui/process.rs` / `managed_process.rs` | Port 18288 by default. `keep_alive` lets it outlive the app. Custom Python nodes from `comfyui-nodes/` are installed into it. |
| **Remote/external ComfyUI** | "Remote" connection mode | App only talks HTTP/WebSocket to it. |
| **MCP server** | `src-tauri/src/mcp/` (opt-in, localhost, token protected, optional Cloudflare quick tunnel) | Lets AI assistants generate, check the queue and search the gallery. See `docs/MCP.md`. |
| **report-proxy** | `report-proxy/` (separate small Rust crate + Dockerfile) | Backend for the in-app error-report feature. Not part of the app build. |
| **Nix** | `flake.nix` | Packaging expression (its `version` field is stale, see file). |

Feature-gating rule: modules compiled in both builds (`api.rs`, `video_export.rs`, `video_interpolate.rs`, `webserver.rs`) need per-item `#[cfg(feature = "desktop")]` gates around anything that touches `tauri`.

## 3. Technology stack

**Frontend**: Svelte 5 (runes), TypeScript 6, Vite 6, Tailwind CSS 4, `@lucide/svelte` icons, Konva + `svelte-konva` (canvas/mask editor), `marked` + DOMPurify (markdown, sanitizing), SortableJS, `@jsquash/avif`, Tauri JS plugins (store, dialog, fs, shell, clipboard, notification, process, updater).

**Backend (Rust 2021)**: Tauri 2 (optional), axum 0.8 + axum-server (rustls) + tower-http, tokio, reqwest, tokio-tungstenite (ComfyUI WebSocket), rusqlite (bundled SQLite gallery index), `image`/`png`/`jxl-oxide`/`jxl-encoder` (JPEG XL storage), `ort` (ONNX Runtime, load-dynamic, for interrogator/detectors), argon2 + chacha20poly1305 (auth, encrypted secrets), sysinfo, rust-embed, mdns-sd, igd-next, webkit2gtk (Linux desktop only).

**ComfyUI side**: Python custom nodes in `comfyui-nodes/` plus `src-tauri/src/comfyui/mooshie_nodes.py`; workflows are built in Rust as JSON and posted to ComfyUI's `/prompt`.

**Tooling/CI**: GitHub Actions (`release`, `rust-checks`, `pr-guardrails`, `glassworm-scan`, `macos-native`, `comfyui-compat`, `media-tools`), Docker/GHCR, Node scripts in `scripts/`.

## 4. Request flow

1. User edits settings in Svelte components; state lives in rune stores (`src/lib/stores/*.svelte.ts`).
2. `generation.toParams()` converts camelCase UI state to snake_case `GenerationParams`.
3. `ipcInvoke()` (`src/lib/utils/ipc.ts`) sends to Rust: Tauri IPC on desktop, `POST` in browser mode. `ipcListen()` receives events via Tauri events or SSE.
4. Rust command (`src-tauri/src/commands/`) builds a ComfyUI workflow from `src-tauri/src/templates/*.rs` (or a NovelAI request from `novelai/`).
5. Workflow goes to ComfyUI `/prompt`; a WebSocket (`comfyui/websocket.rs`) streams progress and previews back as `comfyui:*` events. A fair-queue (`state.rs` `PromptQueue`) submits one prompt per user at a time.
6. Outputs are saved to the gallery as JPEG XL, indexed in SQLite (`gallery_index.rs`), and shown through `gallery://` / `thumbnail://` URI schemes or `loadGalleryImageDisplay()`.

## 5. Frontend (`src/`)

```
index.html            Vite entry
src/main.ts           installs console log interceptor, mounts App.svelte
src/App.svelte        shell: setup wizard, top nav, page switch, global events, $effects between stores
src/app.css           Tailwind + --theme-accent-* tokens
src/lib/
  components/         UI by area (see below)
  stores/             ~50 class-singleton stores with $state (*.svelte.ts)
  utils/              api.ts (typed IPC wrappers), ipc.ts, theme, prompt/music/NovelAI helpers, portal.ts
  types/              shared TS types (index.ts, music.ts, scene.ts)
  locales/            en, de, es, fr, it, ja, ko, pl, pt, ru, zh, zh-tw
  errors/             error catalog and reporting
  config/             controlnet presets
  animadex/           Animadex character database client + components
  artist-gallery/     artist style browser (own README.md)
  assets/             logo, tag/artist JSON
```

`PrimaryPage` values in `App.svelte`: `generate`, `music`, `scene`, `gallery`, `modelhub`, `artists`, `characters`, `settings`. On narrow/touch devices `useMobileLayout` swaps in `components/mobile/MobileApp.svelte`.

Component folders in `src/lib/components/`: `generation/` (prompt inputs, sampler, LoRA/checkpoint galleries, NovelAI panels, ControlNet, upscale, face fix, style tools, Compare Grid), `canvas/` (Konva editor, layers, staging), `gallery/`, `video/`, `music/`, `scene/`, `modelhub/`, `settings/` (incl. GPU, MCP, LLM providers, cloud providers), `setup/` (wizard), `progress/`, `downloads/`, `errors/`, `updater/`, `ui/`, `mobile/`.

Conventions that matter: Svelte 5 runes only (no `svelte/store`), Tailwind only (no `<style>` blocks), `onclick` not `on:click`, all strings via `locale.t('key')` (keys must exist in every locale file), every `fixed` overlay uses `use:portal`.

## 6. Backend (`src-tauri/`)

```
Cargo.toml            features: desktop (default) | server; bin mooshieui-server needs `server`
tauri.conf.json       window, CSP, bundle (targets "all"), updater endpoint + pubkey
build.rs              tauri-build (desktop only)
capabilities/         Tauri permission sets
runtime/              pinned ComfyUI source + macOS runtime constraints
src/main.rs           desktop entry
src/server_main.rs    headless server entry
src/lib.rs            module list, Tauri builder, generate_handler![] registration, URI schemes
src/state.rs          AppState, PromptQueue, BroadcastEvent
src/config.rs         AppConfig, config.json in the app data dir, gallery path
src/webserver.rs      axum routes, SSE, auth, static frontend (rust-embed)
src/auth.rs, user_prefs.rs, user_secrets.rs, output_owners.rs   multi-user support
src/gallery_index.rs, jxl.rs, metadata/, temp_images.rs          gallery and image formats
src/setup.rs, comfyui_version.rs, media_tools.rs                 installers (uv/Python/ComfyUI/ffmpeg tools)
src/lan_*.rs          LAN discovery, firewall, UPnP
src/comfyui/          client, websocket, process management, GPU manager, node requirements (nodes.rs)
src/commands/         Tauri/REST command handlers (api, config, server, workflow, music*, video*, novelai, ...)
src/templates/        ComfyUI workflow builders: txt2img, img2img, inpainting, image_edit, controlnet,
                      upscale, facefix, style_ref/transfer, segment_detail, multichar (Anima
                      multi-character, see docs/MULTI_CHARACTER.md), video*, rife, music
src/novelai/          NovelAI client, payload, params, models, face detail, augment, metadata
src/prompt_assistant/ local llama.cpp / external LLM prompt help, OAuth sign-in, vision, grounding
src/cloud/            ElevenLabs voice, scene/video cloud providers
src/mcp/              MCP tools, jobs, setups snapshot/overrides, music, tunnel
src/interrogator.rs, kaloscope.rs, attention.rs   image tagging and detector models (ONNX)
```

Errors: commands return `Result<T, AppError>` (`error.rs`). Shared HTTP client: `state.http_client`. Event names: `comfyui:{type}`, `setup:{type}`.

## 7. Other top-level directories

| Path | Contents |
|------|----------|
| `comfyui-nodes/` | Python ComfyUI nodes installed at setup (tiled diffusion, guidance, SDXL/Flux2 VAE adapter, H3 video helpers, GGUF compat, Nanosaur support) and `install.sh`. Not build output. |
| `deploy/` | Server Dockerfile, entrypoint, Kubernetes manifests. |
| `docs/` | Technical docs: `MCP.md`, `MUSIC.md`, `VIDEO.md`, `NOVELAI.md`, `STYLE_REFERENCE.md`, `MACOS.md`, `RELEASE_NOTES.md`, bot triage, roadmap, `README.md` index. |
| `scripts/` | Build helpers (`tauri-wrapper.mjs`, `ensure-linuxdeploy.mjs`, `linuxdeploy-wrapper.c`), CI checks (`check-i18n-parity.mjs`, `check-portals.mjs`, `verify-release-version.mjs`), data fetchers, smoke tests, macOS scripts. |
| `.github/` | Workflows, CONTRIBUTING, Copilot instructions, agents, `tauri-no-updater-artifacts.json`. |
| `.agents/` | Canonical agent skills and rules (mirrored into `.claude/skills/` and `.claude/commands/`). |
| `.githooks/` | Pre-commit hook (GlassWorm scan). Enable with `scripts/setup-hooks.sh`. |
| `public/` | Static assets served by Vite. |
| `error-logs/` | Git-ignored scratch space for large logs. |
| `jail.toml`, `apply.py`, `run_jailed.sh`, `.ai-jail` | Local, untracked sandbox setup for running AI agents. Not part of the project. |

## 8. Features present (summary)

- **Multi-character (experimental)**: two Anima characters with their own prompts and LoRAs, via the RS MultiCharacter Sampler node pack installed on demand (`docs/MULTI_CHARACTER.md`).
- **Image**: txt2img, img2img, inpainting with canvas/mask editor, Image Edit (Qwen Image Edit, Flux Kontext, Anima ReStyler), pause and continue sampling, queue management, Compare Grid (XYZ), regional prompts, prompt scheduling, wildcards/random syntax, Prompt Chunks, autocomplete, Style Creator, artist gallery, Animadex characters.
- **Refinement**: MultiDiffusion/SpotDiffusion tiled upscaling, SeedVR2, face detailing, ControlNet, IP-Adapter/Flux Redux/Anima character references, sampler guidance, SDXL DMD2 preset.
- **Models**: 20+ architectures auto-detected (hash, metadata, tensors, filename), GGUF, INT8-Fast, Model Hub with downloads, Civitai integration, LoRA presets.
- **NovelAI**: V4/V4.5/V5, characters and positioning, references, Anlas estimates, Enhance/Upscale/Variations, Director Tools, face detailer.
- **Video**: MiniMax H3 with model stacks, shot timeline, drafts and refinement, RIFE/GMFSS interpolation, player, MP4/animated export; Scene page with cloud voice/video.
- **Music**: YuE2 songs, SheetSage2 covers, score review/edit, MIDI export, playlists, style profiles from reference audio.
- **Prompt Assistant**: local (llama.cpp) or external LLM providers, image interrogation.
- **Gallery**: JXL storage, SQLite index, metadata import (SwarmUI, A1111, NovelAI), A/B compare, ratings, refresh.
- **Platform**: setup wizard (NVIDIA/AMD/Intel detection), auto-updater, 12 languages, accessibility options, LAN/multi-user server, Docker/K8s, MCP server, mobile layout.

## 9. Build outputs (Linux)

| Command | Output |
|---------|--------|
| `npm run build` | `dist/` (frontend) |
| `npx tauri build --bundles deb --config .github/tauri-no-updater-artifacts.json` | `src-tauri/target/release/bundle/deb/*.deb` |
| `npx tauri build --bundles appimage --config .github/tauri-no-updater-artifacts.json` | `src-tauri/target/release/bundle/appimage/*.AppImage` |
| `cargo build --release --no-default-features --features server` (in `src-tauri/`) | `src-tauri/target/release/mooshieui-server` |

Full steps and prerequisites are in the "Linux build" section of [`CLAUDE.md`](CLAUDE.md).
