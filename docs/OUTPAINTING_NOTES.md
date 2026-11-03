# Extending images (outpainting): findings, not started

Checked 2026-10-31 against this fork (based on v2.3.29). **Nothing is implemented
and no work is planned yet**; multi-character generation comes first.

## What the app has today

- No outpainting or "extend image" feature. The upstream roadmap
  (`ROADMAP-2026-gaps.md`, item 8) lists a dedicated outpainting workflow as a
  proposal. A search of `src/`, `src-tauri/src/` and `comfyui-nodes/` finds no
  `ImagePadForOutpaint`, no extend or expand-canvas code.
- Inpainting with the canvas editor (raster and mask layers, brush, eraser,
  rectangle, lasso, fill, accept or discard).
- Image Edit mode: Qwen Image Edit, Qwen Image Edit Plus, Flux.1 Kontext, Anima
  ReStyler. Asking for "more of the scene" by text might work. Not tested.
- Pause and continue (ComfyUI text-to-image only) can correct a run in progress,
  not a finished image.
- NovelAI: no extend feature documented; its infill is degraded to img2img.

## Why inpainting cannot extend an image now

`canvas.svelte.ts` `initCanvas(width, height)` fixes the canvas to the source
image size and starts the bounding box equal to it. Nothing resizes the canvas
afterwards and `CanvasStage` has no bounding-box control, so the mask can only be
painted inside the image. Untested workaround: pad the image with a blank margin
in an external editor, import it, and mask the margin.

## What a real feature would need

- An "extend" control (left, right, top, bottom in pixels) that grows the canvas.
- A template in `src-tauri/src/templates/` that pads the image (ComfyUI has a
  core `ImagePadForOutpaint` node), builds a feathered mask for the new area and
  runs the inpaint path.
- Canvas store changes, UI, i18n in all 12 locales, and a decision on whether
  MCP should follow (it would point at one image, so `setups::snapshot` clears it).

When this is picked up, first check whether the Image Edit models already cover
the use case well enough, then check the inpaint template's resize and mask
handling (`templates/inpainting.rs`).
