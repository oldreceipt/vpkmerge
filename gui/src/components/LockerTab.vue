<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';

const heroes = ref([]);
const styles = ref([]);
const animations = ref([]);
const selectedHero = ref('');
const vpkPath = ref('');
const outputPath = ref('');
const recipeParts = ref(null);
const style = ref('holo');
const animationStyle = ref('cycle');
const intensity = ref(1);
const phase = ref(0);
const scroll = ref(1);
const animationIntensity = ref(1);
const includeBody = ref(true);
const includeSkinWeapons = ref(true);
const includeAbilities = ref(true);
const includeVfxWeapons = ref(true);
const busy = ref(false);
const previewBusy = ref(false);
const status = ref({ text: '', kind: '' });
const previewStatus = ref({ text: '', kind: '' });
const report = ref(null);
const previewFrames = ref([]);
const frameIndex = ref(0);

let previewTimer = null;
let previewDebounce = null;
let previewRequest = 0;

const selectedHeroInfo = computed(() =>
  heroes.value.find((hero) => hero.codename === selectedHero.value),
);

const hasTargets = computed(() =>
  includeBody.value || includeSkinWeapons.value || includeAbilities.value || includeVfxWeapons.value,
);

const canBuild = computed(() =>
  !busy.value
    && selectedHero.value
    && vpkPath.value.trim()
    && outputPath.value.trim()
    && hasTargets.value,
);

const previewSrc = computed(() => previewFrames.value[frameIndex.value] || '');

const partRows = computed(() => [
  {
    key: 'body',
    label: 'Body skin',
    checked: includeBody.value,
    count: selectedHeroInfo.value ? 'discover' : '',
  },
  {
    key: 'skin_weapons',
    label: 'Weapon skin',
    checked: includeSkinWeapons.value,
    count: selectedHeroInfo.value ? 'discover' : '',
  },
  {
    key: 'abilities',
    label: 'Ability FX',
    checked: includeAbilities.value,
    count: recipeParts.value
      ? recipeParts.value.particle_prefixes.length
        + recipeParts.value.texture_entries.length
        + recipeParts.value.material_entries.length
        + recipeParts.value.model_entries.length
      : 0,
  },
  {
    key: 'vfx_weapons',
    label: 'Weapon FX',
    checked: includeVfxWeapons.value,
    count: recipeParts.value?.particle_prefixes.filter((p) => p.includes('/weapon_fx/')).length || 0,
  },
]);

const recipeBuckets = computed(() => {
  if (!recipeParts.value) return [];
  return [
    { label: 'Particle roots', items: recipeParts.value.particle_prefixes },
    { label: 'Textures', items: recipeParts.value.texture_entries },
    { label: 'Materials', items: recipeParts.value.material_entries },
    { label: 'Models', items: recipeParts.value.model_entries },
  ].filter((bucket) => bucket.items.length);
});

function setStatus(text, kind = '') {
  status.value = { text, kind };
}

function setPreviewStatus(text, kind = '') {
  previewStatus.value = { text, kind };
}

function partValue(key) {
  if (key === 'body') return includeBody.value;
  if (key === 'skin_weapons') return includeSkinWeapons.value;
  if (key === 'abilities') return includeAbilities.value;
  return includeVfxWeapons.value;
}

function setPartValue(key, value) {
  if (key === 'body') includeBody.value = value;
  else if (key === 'skin_weapons') includeSkinWeapons.value = value;
  else if (key === 'abilities') includeAbilities.value = value;
  else includeVfxWeapons.value = value;
}

function countLabel(value) {
  return value === 'discover' ? 'scan' : String(value);
}

function stopFrameLoop() {
  if (previewTimer) clearInterval(previewTimer);
  previewTimer = null;
}

function restartFrameLoop(frameMs = 90) {
  stopFrameLoop();
  frameIndex.value = 0;
  if (previewFrames.value.length <= 1 || animationStyle.value === 'off') return;
  previewTimer = setInterval(() => {
    frameIndex.value = (frameIndex.value + 1) % previewFrames.value.length;
  }, frameMs);
}

function previewScroll() {
  if (animationStyle.value === 'off') return 0;
  return Math.max(0, Number(scroll.value) * Number(animationIntensity.value));
}

async function refreshPreview() {
  const request = ++previewRequest;
  previewBusy.value = true;
  setPreviewStatus('Rendering...');
  try {
    const result = await invoke('trippy_preview', {
      style: style.value,
      phase: Number(phase.value),
      scroll: previewScroll(),
      intensity: Number(intensity.value),
      frames: animationStyle.value === 'off' ? 1 : 18,
      size: 224,
    });
    if (request !== previewRequest) return;
    previewFrames.value = result.frames || [];
    restartFrameLoop(result.frame_ms || 90);
    setPreviewStatus('');
  } catch (e) {
    if (request !== previewRequest) return;
    previewFrames.value = [];
    stopFrameLoop();
    setPreviewStatus(`Preview failed: ${e}`, 'error');
  } finally {
    if (request === previewRequest) previewBusy.value = false;
  }
}

function schedulePreview() {
  if (previewDebounce) clearTimeout(previewDebounce);
  previewDebounce = setTimeout(() => {
    refreshPreview();
  }, 120);
}

async function loadRecipeParts() {
  if (!selectedHero.value) return;
  try {
    recipeParts.value = await invoke('hero_recipe_parts', { hero: selectedHero.value });
  } catch (e) {
    recipeParts.value = null;
    setStatus(`Recipe failed: ${e}`, 'error');
  }
}

async function loadDefaults() {
  try {
    const [options, styleOptions, animationOptions, basePath, addonPath] = await Promise.all([
      invoke('supported_hero_options'),
      invoke('trippy_style_options'),
      invoke('trippy_animation_options'),
      invoke('default_deadlock_vpk_path'),
      invoke('default_addon_output_path'),
    ]);
    heroes.value = options || [];
    styles.value = styleOptions || [];
    animations.value = animationOptions || [];
    selectedHero.value = heroes.value.find((hero) => hero.codename === 'pocket')?.codename
      || heroes.value.find((hero) => hero.codename === 'chrono')?.codename
      || heroes.value[0]?.codename
      || '';
    vpkPath.value = basePath || '';
    outputPath.value = addonPath || '';
    await loadRecipeParts();
    refreshPreview();
  } catch (e) {
    setStatus(`Load failed: ${e}`, 'error');
  }
}

async function pickSourceVpk() {
  try {
    const paths = await invoke('pick_vpk_files');
    if (paths?.length) vpkPath.value = paths[0];
  } catch (e) {
    setStatus(`Picker failed: ${e}`, 'error');
  }
}

async function pickOutputPath() {
  try {
    const path = await invoke('pick_output_path');
    if (path) outputPath.value = path;
  } catch (e) {
    setStatus(`Picker failed: ${e}`, 'error');
  }
}

async function refreshAddonPath() {
  try {
    const path = await invoke('default_addon_output_path');
    if (path) outputPath.value = path;
  } catch (e) {
    setStatus(`Could not resolve addon path: ${e}`, 'error');
  }
}

async function buildLocker() {
  if (!canBuild.value) return;
  busy.value = true;
  report.value = null;
  setStatus('Building trippy addon...');
  try {
    const result = await invoke('build_trippy_addon', {
      vpkPath: vpkPath.value,
      basePath: null,
      hero: selectedHero.value,
      style: style.value,
      intensity: Number(intensity.value),
      phase: Number(phase.value),
      scroll: Number(scroll.value),
      animationStyle: animationStyle.value,
      animationIntensity: Number(animationIntensity.value),
      includeBody: includeBody.value,
      includeSkinWeapons: includeSkinWeapons.value,
      includeAbilities: includeAbilities.value,
      includeVfxWeapons: includeVfxWeapons.value,
      outputPath: outputPath.value,
    });
    report.value = result;
    setStatus(`Wrote ${result.total_entries} entries`, 'success');
  } catch (e) {
    setStatus(`Build failed: ${e}`, 'error');
  } finally {
    busy.value = false;
  }
}

async function revealOutput() {
  if (!report.value?.output_path) return;
  try {
    await invoke('reveal_in_folder', { path: report.value.output_path });
  } catch (e) {
    setStatus(`Could not open folder: ${e}`, 'error');
  }
}

watch(selectedHero, () => {
  loadRecipeParts();
});

watch(
  [style, animationStyle, intensity, phase, scroll, animationIntensity],
  () => {
    schedulePreview();
  },
);

onMounted(() => {
  loadDefaults();
});

onBeforeUnmount(() => {
  stopFrameLoop();
  if (previewDebounce) clearTimeout(previewDebounce);
});
</script>

<template>
  <div class="flex-1 min-h-0 overflow-y-auto">
    <div class="min-h-full p-3 sm:p-4 md:p-8">
      <div class="w-full max-w-6xl mx-auto space-y-5">
        <div class="paper-card rounded-md p-4">
          <div class="flex items-baseline justify-between gap-3 mb-3">
            <h3 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
              Locker
            </h3>
            <span class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300">
              {{ heroes.length }} heroes
            </span>
          </div>

          <div class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(12rem,16rem)]">
            <div class="space-y-1.5">
              <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                Source VPK
              </label>
              <div class="flex gap-2">
                <input
                  v-model="vpkPath"
                  type="text"
                  spellcheck="false"
                  class="flex-1 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500 min-w-0"
                />
                <button class="btn" type="button" @click="pickSourceVpk">Browse</button>
              </div>
            </div>

            <div class="space-y-1.5">
              <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                Hero
              </label>
              <select
                v-model="selectedHero"
                class="w-full bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500"
              >
                <option
                  v-for="hero in heroes"
                  :key="hero.codename"
                  :value="hero.codename"
                >
                  {{ hero.label }} · {{ hero.codename }}
                </option>
              </select>
            </div>
          </div>

          <div class="mt-3 grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto]">
            <div class="space-y-1.5">
              <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                Output
              </label>
              <div class="flex gap-2">
                <input
                  v-model="outputPath"
                  type="text"
                  spellcheck="false"
                  class="flex-1 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500 min-w-0"
                />
                <button class="btn" type="button" @click="pickOutputPath">Browse</button>
              </div>
            </div>
            <button
              type="button"
              class="btn self-end"
              @click="refreshAddonPath"
            >
              Next pak
            </button>
          </div>
        </div>

        <div class="grid gap-5 xl:grid-cols-[minmax(14rem,18rem)_minmax(18rem,1fr)_minmax(18rem,24rem)]">
          <div class="paper-card rounded-md p-4">
            <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium mb-3">
              Parts
            </h4>
            <div class="space-y-2">
              <label
                v-for="row in partRows"
                :key="row.key"
                class="flex items-center justify-between gap-3 cursor-pointer select-none rounded-sm px-1 py-1 hover:bg-surface-100/60 dark:hover:bg-surface-800/50"
              >
                <span class="flex items-center gap-2 min-w-0">
                  <input
                    type="checkbox"
                    class="checkbox"
                    :checked="partValue(row.key)"
                    @change="setPartValue(row.key, $event.target.checked)"
                  />
                  <span class="text-xs text-ink-800 dark:text-ink-100 truncate">{{ row.label }}</span>
                </span>
                <span class="text-[10px] font-mono text-ink-500 dark:text-ink-300 shrink-0">{{ countLabel(row.count) }}</span>
              </label>
            </div>

            <div v-if="recipeBuckets.length" class="mt-4 space-y-2">
              <details
                v-for="bucket in recipeBuckets"
                :key="bucket.label"
                class="border-t border-surface-200 dark:border-surface-800 pt-2"
              >
                <summary class="cursor-pointer text-[11px] font-serif italic text-ink-500 dark:text-ink-300">
                  {{ bucket.label }} · {{ bucket.items.length }}
                </summary>
                <ul class="mt-2 max-h-32 overflow-y-auto space-y-1 pr-1">
                  <li
                    v-for="item in bucket.items"
                    :key="item"
                    class="text-[10px] font-mono text-ink-600 dark:text-ink-300 break-all"
                  >
                    {{ item }}
                  </li>
                </ul>
              </details>
            </div>
          </div>

          <div class="paper-card rounded-md p-4">
            <div class="flex items-baseline justify-between gap-3 mb-3">
              <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
                Treatment
              </h4>
              <span class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300">
                {{ selectedHeroInfo?.label || selectedHero }}
              </span>
            </div>

            <div class="grid gap-3 md:grid-cols-2">
              <div class="space-y-1.5">
                <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                  Style
                </label>
                <select
                  v-model="style"
                  class="w-full bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500"
                >
                  <option v-for="opt in styles" :key="opt.key" :value="opt.key">
                    {{ opt.label }}
                  </option>
                </select>
              </div>

              <div class="space-y-1.5">
                <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                  Animation
                </label>
                <select
                  v-model="animationStyle"
                  class="w-full bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500"
                >
                  <option v-for="opt in animations" :key="opt.key" :value="opt.key">
                    {{ opt.label }}
                  </option>
                </select>
              </div>
            </div>

            <div class="mt-4 grid gap-4 md:grid-cols-2">
              <label class="space-y-1.5">
                <span class="flex justify-between text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                  <span>Intensity</span>
                  <span class="font-mono tracking-normal">{{ intensity.toFixed(2) }}</span>
                </span>
                <input v-model.number="intensity" type="range" min="0" max="1" step="0.01" class="w-full accent-accent-600" />
              </label>

              <label class="space-y-1.5">
                <span class="flex justify-between text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                  <span>Phase</span>
                  <span class="font-mono tracking-normal">{{ phase.toFixed(2) }}</span>
                </span>
                <input v-model.number="phase" type="range" min="0" max="1" step="0.01" class="w-full accent-accent-600" />
              </label>

              <label class="space-y-1.5">
                <span class="flex justify-between text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                  <span>Scroll</span>
                  <span class="font-mono tracking-normal">{{ scroll.toFixed(2) }}</span>
                </span>
                <input v-model.number="scroll" type="range" min="0" max="2.5" step="0.05" class="w-full accent-accent-600" />
              </label>

              <label class="space-y-1.5">
                <span class="flex justify-between text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                  <span>Anim depth</span>
                  <span class="font-mono tracking-normal">{{ animationIntensity.toFixed(2) }}</span>
                </span>
                <input v-model.number="animationIntensity" type="range" min="0" max="2" step="0.05" class="w-full accent-accent-600" />
              </label>
            </div>

            <div class="mt-4 flex items-center gap-2 min-h-[1.25rem]" aria-live="polite">
              <svg
                v-if="busy"
                class="shrink-0 w-4 h-4 animate-spin text-accent-700 dark:text-accent-300"
                viewBox="0 0 16 16"
                fill="none"
                aria-hidden="true"
              >
                <circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.5" stroke-opacity="0.25" />
                <path d="M14 8a6 6 0 0 0-6-6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
              </svg>
              <span
                class="text-xs font-serif italic truncate flex-1"
                :class="{
                  'text-ink-500 dark:text-ink-300': !status.kind,
                  'text-green-700 dark:text-green-400 not-italic font-sans': status.kind === 'success',
                  'text-red-700 dark:text-red-400 not-italic font-sans': status.kind === 'error',
                }"
              >
                {{ status.text || (hasTargets ? 'Ready' : 'Select a target') }}
              </span>
              <button
                v-if="report"
                type="button"
                class="text-xs italic font-serif text-accent-700 dark:text-accent-300 hover:underline focus-visible:outline-none focus-visible:underline rounded"
                @click="revealOutput"
              >
                reveal
              </button>
            </div>

            <button
              type="button"
              :disabled="!canBuild"
              class="merge-button mt-3 w-full h-12 rounded-md font-medium text-base bg-accent-600 hover:!bg-accent-700 text-surface-0 disabled:opacity-40 disabled:cursor-not-allowed transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-300 focus-visible:ring-offset-2 focus-visible:ring-offset-surface-0 dark:focus-visible:ring-offset-surface-950"
              @click="buildLocker"
            >
              Bake Locker VPK
            </button>
          </div>

          <div class="paper-card rounded-md p-4">
            <div class="flex items-baseline justify-between gap-3 mb-3">
              <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
                Live swatch
              </h4>
              <span class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300">
                {{ style }} · {{ animationStyle }}
              </span>
            </div>

            <div class="locker-swatch aspect-square w-full rounded-md border border-surface-200 dark:border-surface-800 overflow-hidden flex items-center justify-center">
              <img
                v-if="previewSrc"
                :src="previewSrc"
                alt="Trippy preview"
                class="w-full h-full object-cover"
                draggable="false"
              />
              <span v-else class="text-xs font-serif italic text-ink-500 dark:text-ink-300">
                No preview
              </span>
            </div>

            <div class="mt-3 flex items-center gap-2 min-h-[1.25rem]" aria-live="polite">
              <svg
                v-if="previewBusy"
                class="shrink-0 w-4 h-4 animate-spin text-accent-700 dark:text-accent-300"
                viewBox="0 0 16 16"
                fill="none"
                aria-hidden="true"
              >
                <circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.5" stroke-opacity="0.25" />
                <path d="M14 8a6 6 0 0 0-6-6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
              </svg>
              <span
                class="text-xs font-serif italic truncate"
                :class="previewStatus.kind === 'error'
                  ? 'text-red-700 dark:text-red-400 not-italic font-sans'
                  : 'text-ink-500 dark:text-ink-300'"
              >
                {{ previewStatus.text || `${previewFrames.length || 0} frame${previewFrames.length === 1 ? '' : 's'}` }}
              </span>
            </div>
          </div>
        </div>

        <div
          v-if="report"
          class="paper-card rounded-md p-4"
        >
          <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium mb-3">
            Last Bake
          </h4>
          <dl class="grid grid-cols-2 md:grid-cols-4 xl:grid-cols-6 gap-3 text-xs">
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Entries</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.total_entries }}</dd>
            </div>
            <div v-if="report.skin">
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Body tex</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.skin.body_textures }}</dd>
            </div>
            <div v-if="report.skin">
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Weapon tex</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.skin.weapon_textures }}</dd>
            </div>
            <div v-if="report.ability">
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Particles</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.ability.particles_recolored }}/{{ report.ability.particles_total }}</dd>
            </div>
            <div v-if="report.ability">
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">VFX tex</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.ability.textures_painted }}</dd>
            </div>
            <div v-if="report.ability">
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Animated</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.ability.particles_animated }}</dd>
            </div>
          </dl>
          <p class="mt-3 text-[11px] font-mono text-ink-500 dark:text-ink-300 break-all">
            {{ report.output_path }}
          </p>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.locker-swatch {
  background-color: rgba(20, 20, 24, 0.92);
  background-image:
    linear-gradient(45deg, rgba(255, 255, 255, 0.04) 25%, transparent 25%),
    linear-gradient(-45deg, rgba(255, 255, 255, 0.04) 25%, transparent 25%),
    linear-gradient(45deg, transparent 75%, rgba(255, 255, 255, 0.04) 75%),
    linear-gradient(-45deg, transparent 75%, rgba(255, 255, 255, 0.04) 75%);
  background-position: 0 0, 0 8px, 8px -8px, -8px 0;
  background-size: 16px 16px;
}
</style>
