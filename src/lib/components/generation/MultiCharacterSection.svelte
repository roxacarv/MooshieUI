<script lang="ts">
  import { onMount } from "svelte";
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { checkNodeAvailable, ensureMultiCharacterNodes, loraChangesTextAdapter } from "../../utils/api.js";
  import { restartComfyuiAndWait } from "../../utils/comfyuiRestart.js";
  import CharacterLayoutCanvas from "./CharacterLayoutCanvas.svelte";
  import { CHARACTER_LOOKS, MAX_CHARACTERS, MIN_CHARACTERS } from "../../utils/characterRegion.js";

  /** The sampler node the whole workflow hangs off (see MULTI_CHARACTER_PACKAGES). */
  const SAMPLER_CLASS = "RSMultiCharacterSampler";

  /** null until checked; false means the pack is missing or not yet loaded. */
  let nodesReady = $state<boolean | null>(null);
  let busy = $state(false);
  let busyMessage = $state("");
  let error = $state<string | null>(null);

  /** LoRA file -> whether it changes Anima's text adapter (cannot be kept per character). */
  let adapterLoras = $state<Record<string, boolean>>({});

  const characters = $derived(generation.multiCharacters);

  /** Aspect ratio each character needs at least; measured: 3 in a square dropped one in 3 of 3 tries, in 2:1 all 3 stayed. */
  const MIN_ASPECT_PER_CHARACTER = 0.5;
  const neededAspect = $derived(characters.length * MIN_ASPECT_PER_CHARACTER);
  const tooNarrow = $derived(
    generation.width > 0 && generation.height > 0 && generation.width / generation.height < neededAspect - 0.01,
  );
  const loraCount = $derived(characters.filter((slot) => slot.loraName).length);
  /** Rough time factor against one image, extrapolated from two LoRAs (about 4x precise, 1.7x normal). */
  const timeFactor = $derived(
    generation.multiCharacterLoraMode === "precision" ? 1 + 1.5 * loraCount : 1 + 0.35 * loraCount,
  );

  $effect(() => {
    for (const slot of characters) {
      const name = slot.loraName;
      if (!name || name in adapterLoras) continue;
      adapterLoras[name] = false;
      void loraChangesTextAdapter(name)
        .then((changes) => {
          adapterLoras[name] = changes;
        })
        .catch(() => {});
    }
  });
  const loraModeOptions = $derived([
    { value: "precision" as const, label: locale.t("generation.multi_character.quality_precision") },
    { value: "normal" as const, label: locale.t("generation.multi_character.quality_normal") },
  ]);

  /** Last path segment, so a long subfolder does not crowd the picker. */
  function loraLabel(name: string): string {
    return name.split(/[\\/]/).pop() ?? name;
  }

  async function refreshNodeState() {
    nodesReady = await checkNodeAvailable(SAMPLER_CLASS).catch(() => false);
  }

  /**
   * Clone the pinned node pack if needed and make ComfyUI load it. ComfyUI only
   * imports custom nodes at startup, so a fresh install ends in a restart, like
   * every other lazy node install.
   */
  async function setUpNodes(): Promise<boolean> {
    if (busy || progress.isGenerating) return false;
    busy = true;
    error = null;
    try {
      busyMessage = locale.t("generation.multi_character.nodes_installing");
      const loaded = await ensureMultiCharacterNodes();
      if (!loaded) {
        busyMessage = locale.t("generation.multi_character.nodes_restarting");
        connection.connected = false;
        await restartComfyuiAndWait(
          locale.t("generation.multi_character.nodes_timeout"),
          locale.t("generation.multi_character.nodes_failed_start"),
        );
        await refreshNodeState();
        if (!nodesReady) throw new Error(locale.t("generation.multi_character.nodes_missing"));
      } else {
        nodesReady = true;
      }
      return true;
    } catch (e) {
      error = locale.t("generation.multi_character.nodes_error", {
        error: e instanceof Error ? e.message : String(e),
      });
      nodesReady = false;
      return false;
    } finally {
      busy = false;
      busyMessage = "";
    }
  }

  async function toggleEnabled(checked: boolean) {
    if (!checked) {
      generation.multiCharacterEnabled = false;
      generation.saveSettings();
      return;
    }
    // Only switch on once the nodes are really there, so Generate never
    // submits a graph ComfyUI would reject.
    if (nodesReady === true || (await setUpNodes())) {
      generation.multiCharacterEnabled = true;
      generation.saveSettings();
    }
  }

  function setPrompt(index: number, value: string) {
    generation.multiCharacters[index].prompt = value;
  }

  function setLora(index: number, value: string) {
    generation.multiCharacters[index].loraName = value || null;
    generation.saveSettings();
  }

  function setLoraStrength(index: number, value: number) {
    if (!Number.isFinite(value)) return;
    generation.multiCharacters[index].loraStrength = Math.min(2, Math.max(0, value));
  }

  onMount(() => {
    if (generation.multiCharacterEnabled) void refreshNodeState();
  });
</script>

<div class="space-y-3 text-xs">
  <label class="flex items-center gap-2 text-neutral-400 select-none cursor-pointer">
    <input
      type="checkbox"
      class="accent-indigo-500"
      checked={generation.multiCharacterEnabled}
      disabled={busy}
      onchange={(e) => toggleEnabled(e.currentTarget.checked)}
    />
    <span>{locale.t("generation.multi_character.enable")}</span>
  </label>

  {#if busy}
    <p class="rounded-lg bg-neutral-800/80 px-3 py-2 text-[11px] text-neutral-300">{busyMessage}</p>
  {/if}

  {#if error}
    <p class="rounded-lg border border-red-600/30 bg-red-600/10 px-3 py-2 text-[11px] text-red-300">{error}</p>
  {/if}

  {#if generation.multiCharacterEnabled}
    <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.intro")}</p>
    <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.known_character_tip")}</p>

    {#if nodesReady === false && !busy}
      <div class="rounded-lg border border-amber-600/30 bg-amber-600/10 px-3 py-2 text-[11px] text-amber-300">
        <p>{locale.t("generation.multi_character.nodes_missing")}</p>
        <button
          class="mt-1 text-indigo-400 transition-colors hover:text-indigo-300 disabled:opacity-50"
          disabled={progress.isGenerating}
          onclick={setUpNodes}
        >
          {locale.t("generation.multi_character.nodes_install")}
        </button>
      </div>
    {/if}

    {#if generation.batchSize !== 1}
      <p class="rounded-lg border border-amber-600/30 bg-amber-600/10 px-3 py-2 text-[11px] text-amber-300">
        {locale.t("generation.multi_character.batch_note")}
      </p>
    {/if}

    {#each characters as slot, index (index)}
      <div class="space-y-2 rounded-lg border border-neutral-800 bg-neutral-900/60 p-2.5">
        <div class="flex items-center justify-between text-[11px]">
          <span class="font-medium text-neutral-300">
            {locale.t("generation.multi_character.character", { n: String(index + 1) })}
          </span>
          <span
            class="h-2.5 w-2.5 rounded-full {CHARACTER_LOOKS[index % CHARACTER_LOOKS.length].dot}"
            aria-hidden="true"
          ></span>
        </div>

        <textarea
          class="h-16 w-full resize-y rounded-lg border border-neutral-700 bg-neutral-800 px-2.5 py-1.5 text-xs text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
          placeholder={locale.t("generation.multi_character.prompt_placeholder")}
          value={slot.prompt}
          oninput={(e) => setPrompt(index, e.currentTarget.value)}
          onchange={() => generation.saveSettings()}
        ></textarea>

        <div class="flex items-center gap-2">
          <span class="w-12 shrink-0 text-neutral-500">{locale.t("generation.multi_character.lora")}</span>
          <select
            class="min-w-0 flex-1 rounded-lg border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-xs text-neutral-100 focus:border-indigo-500 focus:outline-none"
            value={slot.loraName ?? ""}
            onchange={(e) => setLora(index, e.currentTarget.value)}
          >
            <option value="">{locale.t("generation.multi_character.lora_none")}</option>
            {#each models.loras as lora (lora)}
              <option value={lora} title={lora}>{loraLabel(lora)}</option>
            {/each}
          </select>
        </div>

        {#if slot.loraName && adapterLoras[slot.loraName]}
          <p class="rounded-lg border border-amber-600/30 bg-amber-600/10 px-2.5 py-1.5 text-[11px] text-amber-300">
            {locale.t("generation.multi_character.lora_adapter_warning")}
          </p>
        {/if}

        {#if slot.loraName}
          <div class="flex items-center gap-2">
            <span class="w-12 shrink-0 text-neutral-500">{locale.t("generation.multi_character.lora_strength")}</span>
            <input
              type="range"
              min="0"
              max="2"
              step="0.05"
              class="min-w-0 flex-1 accent-indigo-500"
              value={slot.loraStrength}
              oninput={(e) => setLoraStrength(index, e.currentTarget.valueAsNumber)}
              onchange={() => generation.saveSettings()}
            />
            <span class="w-9 shrink-0 text-right tabular-nums text-neutral-400">{slot.loraStrength.toFixed(2)}</span>
          </div>
        {/if}
      </div>
    {/each}

    <div class="flex items-center justify-between text-[11px]">
      <button
        class="text-indigo-400 transition-colors hover:text-indigo-300 disabled:opacity-40"
        disabled={characters.length >= MAX_CHARACTERS}
        onclick={() => generation.setMultiCharacterCount(characters.length + 1)}
      >
        {locale.t("generation.multi_character.add_character")}
      </button>
      <button
        class="text-indigo-400 transition-colors hover:text-indigo-300 disabled:opacity-40"
        disabled={characters.length <= MIN_CHARACTERS}
        onclick={() => generation.setMultiCharacterCount(characters.length - 1)}
      >
        {locale.t("generation.multi_character.remove_character")}
      </button>
    </div>

    {#if tooNarrow}
      <p class="rounded-lg border border-amber-600/30 bg-amber-600/10 px-3 py-2 text-[11px] text-amber-300">
        {locale.t("generation.multi_character.narrow_warning", {
          count: String(characters.length),
          ratio: neededAspect.toFixed(1),
        })}
      </p>
    {/if}
    {#if characters.length >= 3}
      <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.group_phrase_tip")}</p>
    {/if}
    {#if loraCount > 0}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.multi_character.cost_note", {
          n: String(loraCount),
          factor: timeFactor.toFixed(1),
        })}
      </p>
    {/if}

    <div class="space-y-1.5">
      <div class="text-neutral-500">{locale.t("generation.multi_character.interaction")}</div>
      <input
        type="text"
        class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-2.5 py-1.5 text-xs text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
        placeholder={locale.t("generation.multi_character.interaction_placeholder")}
        value={generation.multiCharacterInteraction}
        oninput={(e) => (generation.multiCharacterInteraction = e.currentTarget.value)}
        onchange={() => generation.saveSettings()}
      />
      <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.interaction_hint")}</p>
    </div>

    <CharacterLayoutCanvas />

    <div class="space-y-1.5">
      <label class="flex items-center gap-2 text-neutral-400 select-none cursor-pointer">
        <input
          type="checkbox"
          class="accent-indigo-500"
          checked={generation.multiCharacterSayPosition}
          onchange={(e) => {
            generation.multiCharacterSayPosition = e.currentTarget.checked;
            generation.saveSettings();
          }}
        />
        <span>{locale.t("generation.multi_character.say_position")}</span>
      </label>
      <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.say_position_hint")}</p>
    </div>

    <div class="space-y-1.5">
      <div class="text-neutral-500">{locale.t("generation.multi_character.scene_strength")}</div>
      <input
        type="range"
        min="0"
        max="1"
        step="0.05"
        class="w-full accent-indigo-500"
        value={generation.multiCharacterSceneStrength}
        oninput={(e) => (generation.multiCharacterSceneStrength = e.currentTarget.valueAsNumber)}
        onchange={() => generation.saveSettings()}
      />
      <div class="flex justify-between text-[10px] text-neutral-500">
        <span>{locale.t("generation.multi_character.scene_strength_weak")}</span>
        <span>{locale.t("generation.multi_character.scene_strength_strong")}</span>
      </div>
      <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.scene_strength_hint")}</p>
      <label class="flex items-center gap-2 text-neutral-400 select-none cursor-pointer">
        <input
          type="checkbox"
          class="accent-indigo-500"
          checked={generation.multiCharacterShareScene}
          onchange={(e) => {
            generation.multiCharacterShareScene = e.currentTarget.checked;
            generation.saveSettings();
          }}
        />
        <span>{locale.t("generation.multi_character.share_scene")}</span>
      </label>
      <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.share_scene_hint")}</p>
    </div>

    <div class="space-y-1.5">
      <div class="text-neutral-500">{locale.t("generation.multi_character.quality")}</div>
      <select
        class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-xs text-neutral-100 focus:border-indigo-500 focus:outline-none"
        value={generation.multiCharacterLoraMode}
        onchange={(e) => {
          generation.multiCharacterLoraMode = e.currentTarget.value === "normal" ? "normal" : "precision";
          generation.saveSettings();
        }}
      >
        {#each loraModeOptions as option (option.value)}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
      <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.quality_hint")}</p>
    </div>

    <div class="space-y-1.5">
      <div class="text-neutral-500">{locale.t("generation.multi_character.cohesion")}</div>
      <input
        type="range"
        min="0"
        max="1"
        step="0.05"
        class="w-full accent-indigo-500"
        value={generation.multiCharacterCohesion}
        oninput={(e) => (generation.multiCharacterCohesion = e.currentTarget.valueAsNumber)}
        onchange={() => generation.saveSettings()}
      />
      <div class="flex justify-between text-[10px] text-neutral-500">
        <span>{locale.t("generation.multi_character.cohesion_strict")}</span>
        <span>{locale.t("generation.multi_character.cohesion_scene")}</span>
      </div>
      <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.cohesion_hint")}</p>
    </div>

    <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.back_view_tip")}</p>
    <p class="text-[11px] text-neutral-500">{locale.t("generation.multi_character.unsupported_note")}</p>
  {/if}
</div>
