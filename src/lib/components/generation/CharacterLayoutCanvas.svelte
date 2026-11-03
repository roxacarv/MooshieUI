<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import {
    GRID_STEP,
    MIN_SIDE,
    clampRegion,
    columnRegion,
    snap,
    type CharacterRegion,
  } from "../../utils/characterRegion.js";

  type Corner = "nw" | "ne" | "sw" | "se";

  /** Per-character look on the canvas; the same order as the cards above it. */
  const LOOKS = [
    "border-amber-400 bg-amber-400/20 text-amber-200",
    "border-sky-400 bg-sky-400/20 text-sky-200",
  ];
  const CORNERS: { id: Corner; classes: string }[] = [
    { id: "nw", classes: "-left-1.5 -top-1.5 cursor-nwse-resize" },
    { id: "ne", classes: "-right-1.5 -top-1.5 cursor-nesw-resize" },
    { id: "sw", classes: "-left-1.5 -bottom-1.5 cursor-nesw-resize" },
    { id: "se", classes: "-right-1.5 -bottom-1.5 cursor-nwse-resize" },
  ];

  let canvas = $state<HTMLDivElement | null>(null);
  let snapToGrid = $state(true);
  let active = $state(0);

  const slots = $derived(generation.multiCharacters);
  const aspect = $derived(
    generation.width > 0 && generation.height > 0 ? generation.width / generation.height : 1,
  );

  function effective(index: number): CharacterRegion {
    return slots[index].region ?? columnRegion(index, slots.length);
  }

  function setRegion(index: number, region: CharacterRegion) {
    generation.multiCharacters[index].region = clampRegion(region);
  }

  function snapped(value: number): number {
    return snapToGrid ? snap(value, GRID_STEP) : value;
  }

  /** Drag the box (`move`) or one of its corners. */
  function startDrag(event: PointerEvent, index: number, mode: "move" | Corner) {
    if (!canvas) return;
    event.preventDefault();
    event.stopPropagation();
    active = index;
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const bounds = canvas.getBoundingClientRect();
    const start = effective(index);
    const startX = event.clientX;
    const startY = event.clientY;
    const edges = {
      left: start.x - start.width / 2,
      right: start.x + start.width / 2,
      top: start.y - start.height / 2,
      bottom: start.y + start.height / 2,
    };

    const onMove = (e: PointerEvent) => {
      const dx = (e.clientX - startX) / bounds.width;
      const dy = (e.clientY - startY) / bounds.height;
      if (mode === "move") {
        setRegion(index, {
          ...start,
          x: snapped(start.x + dx),
          y: snapped(start.y + dy),
        });
        return;
      }
      let { left, right, top, bottom } = edges;
      if (mode.includes("w")) left = Math.min(snapped(edges.left + dx), right - MIN_SIDE);
      if (mode.includes("e")) right = Math.max(snapped(edges.right + dx), left + MIN_SIDE);
      if (mode.startsWith("n")) top = Math.min(snapped(edges.top + dy), bottom - MIN_SIDE);
      if (mode.startsWith("s")) bottom = Math.max(snapped(edges.bottom + dy), top + MIN_SIDE);
      setRegion(index, {
        x: (left + right) / 2,
        y: (top + bottom) / 2,
        width: right - left,
        height: bottom - top,
      });
    };
    const onUp = () => {
      target.removeEventListener("pointermove", onMove);
      target.removeEventListener("pointerup", onUp);
      target.removeEventListener("pointercancel", onUp);
      generation.saveSettings();
    };
    target.addEventListener("pointermove", onMove);
    target.addEventListener("pointerup", onUp);
    target.addEventListener("pointercancel", onUp);
  }

  /** Arrow keys nudge the focused box, so the layout works without a pointer. */
  function onKey(event: KeyboardEvent, index: number) {
    const step = event.shiftKey ? 0.1 : GRID_STEP;
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const move = moves[event.key];
    if (!move) return;
    event.preventDefault();
    const r = effective(index);
    setRegion(index, { ...r, x: r.x + move[0], y: r.y + move[1] });
    generation.saveSettings();
  }

  function swap() {
    const [a, b] = [effective(0), effective(1)];
    setRegion(0, b);
    setRegion(1, a);
    generation.saveSettings();
  }

  function reset() {
    for (let i = 0; i < slots.length; i++) generation.multiCharacters[i].region = null;
    generation.saveSettings();
  }

  const isDefault = $derived(slots.every((slot) => slot.region === null));

  function pct(value: number): string {
    return `${(value * 100).toFixed(2)}%`;
  }
</script>

<div class="space-y-1.5">
  <div class="flex items-center justify-between">
    <span class="text-neutral-500">{locale.t("generation.multi_character.layout_title")}</span>
    <div class="flex items-center gap-3 text-[11px]">
      <button class="text-indigo-400 transition-colors hover:text-indigo-300" onclick={swap}>
        {locale.t("generation.multi_character.layout_swap")}
      </button>
      <button
        class="text-indigo-400 transition-colors hover:text-indigo-300 disabled:opacity-40"
        disabled={isDefault}
        onclick={reset}
      >
        {locale.t("generation.multi_character.layout_reset")}
      </button>
    </div>
  </div>

  <div
    bind:this={canvas}
    class="relative mx-auto w-full max-w-full select-none overflow-hidden rounded-lg border border-neutral-700 bg-neutral-900"
    style="aspect-ratio: {aspect}; max-height: 18rem;"
  >
    {#each slots as _slot, index (index)}
      {@const r = effective(index)}
      <div
        role="button"
        tabindex="0"
        aria-label={locale.t("generation.multi_character.layout_box", { n: String(index + 1) })}
        class="absolute cursor-move touch-none rounded border-2 outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 {LOOKS[index % LOOKS.length]}"
        style="left: {pct(r.x - r.width / 2)}; top: {pct(r.y - r.height / 2)}; width: {pct(r.width)}; height: {pct(r.height)}; z-index: {active === index ? 2 : 1};"
        onpointerdown={(e) => startDrag(e, index, "move")}
        onkeydown={(e) => onKey(e, index)}
        onfocus={() => (active = index)}
      >
        <span class="pointer-events-none absolute left-1 top-0.5 text-[11px] font-semibold">{index + 1}</span>
        {#each CORNERS as corner (corner.id)}
          <span
            role="presentation"
            class="absolute h-3 w-3 touch-none rounded-sm border border-neutral-900 bg-neutral-100 {corner.classes}"
            onpointerdown={(e) => startDrag(e, index, corner.id)}
          ></span>
        {/each}
      </div>
    {/each}
  </div>

  <div class="flex items-center justify-between gap-2">
    <label class="flex items-center gap-2 text-neutral-400 select-none cursor-pointer">
      <input type="checkbox" class="accent-indigo-500" bind:checked={snapToGrid} />
      <span>{locale.t("generation.multi_character.layout_snap")}</span>
    </label>
  </div>
  <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.layout_hint")}</p>
</div>
