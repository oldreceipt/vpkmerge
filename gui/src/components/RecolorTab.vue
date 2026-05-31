<script setup>
import { computed, onMounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';

// Hero ability-VFX recolor: one tab, two modes.
//   solid   -> recolor_hero_vpk        (one absolute hue, the in-game-confirmed base)
//   rainbow -> build_hero_prism_vpk    (spectrum spread; animated + tuning knobs)
// Both share one hero recipe, one source/base/output, one result readout.

const heroes = ref([]);
const selectedHero = ref('');
const mode = ref('solid'); // 'solid' | 'rainbow'

const vpkPath = ref('');
const basePath = ref('');
const outputPath = ref('');

const hue = ref(0);
const saturation = ref(1.0);
const brightness = ref(1.0);
const hueOffset = ref(0);
const animated = ref(false);

const busy = ref(false);
const status = ref({ text: '', kind: '' });
const report = ref(null);
const reportMode = ref('solid');

const preview = ref({ state: 'idle' }); // idle | loading | ok | none | error
const rainbowScan = ref(null);

const MODES = [
  { key: 'solid', label: 'Solid hue' },
  { key: 'rainbow', label: 'Rainbow' },
];

const SCAN_LABELS = {
  looped: 'Looped (richest): existing looped gradients carry a true moving rainbow.',
  animated: 'Animated: age/lifetime gradients sweep the spectrum over each particle.',
  strong: 'Strong: many static gradients spread the spectrum well.',
  gradient: 'Gradient: some gradients to spread; reads as a static rainbow.',
  static: 'Static: color constants only; the rainbow is a fixed spread, no motion.',
  none: 'None: no patchable color in this hero with the current scalar patcher.',
};

const selectedHeroInfo = computed(() =>
  heroes.value.find((hero) => hero.codename === selectedHero.value),
);

const canBuild = computed(() =>
  !busy.value && selectedHero.value && vpkPath.value.trim() && outputPath.value.trim(),
);

function setStatus(text, kind = '') {
  status.value = { text, kind };
}

function heroMeta(hero) {
  if (!hero) return '';
  const parts = [`${hero.particles} particle root${hero.particles === 1 ? '' : 's'}`];
  if (hero.textures) parts.push(`${hero.textures} tex`);
  if (hero.materials) parts.push(`${hero.materials} mat`);
  if (hero.models) parts.push(`${hero.models} model${hero.models === 1 ? '' : 's'}`);
  return parts.join(' · ');
}

async function loadDefaults() {
  try {
    const [options, base, addonPath] = await Promise.all([
      invoke('supported_hero_options'),
      invoke('default_deadlock_vpk_path'),
      invoke('default_addon_output_path'),
    ]);
    heroes.value = options || [];
    // Prefer a hero with a preview texture so the solid-mode swatch works out
    // of the box (particle-only heroes have no swatch).
    selectedHero.value = heroes.value.find((h) => h.has_preview)?.codename
      || heroes.value[0]?.codename
      || '';
    vpkPath.value = base || '';
    outputPath.value = addonPath || '';
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

async function pickBaseVpk() {
  try {
    const paths = await invoke('pick_vpk_files');
    if (paths?.length) basePath.value = paths[0];
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

async function build() {
  if (!canBuild.value) return;
  busy.value = true;
  report.value = null;
  const base = basePath.value.trim() || null;
  setStatus(mode.value === 'solid' ? 'Baking recolor VPK...' : 'Baking prism VPK...');
  try {
    if (mode.value === 'solid') {
      const r = await invoke('recolor_hero_vpk', {
        vpkPath: vpkPath.value,
        basePath: base,
        hero: selectedHero.value,
        hue: Number(hue.value),
        saturation: Number(saturation.value),
        brightness: Number(brightness.value),
        outputPath: outputPath.value,
      });
      report.value = r;
      reportMode.value = 'solid';
    } else {
      const r = await invoke('build_hero_prism_vpk', {
        vpkPath: vpkPath.value,
        basePath: base,
        hero: selectedHero.value,
        animated: animated.value,
        hueOffset: Number(hueOffset.value),
        saturation: Number(saturation.value),
        brightness: Number(brightness.value),
        outputPath: outputPath.value,
      });
      report.value = r;
      reportMode.value = 'rainbow';
    }
    setStatus(`Wrote ${report.value.total_entries} entries`, 'success');
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

// Live solid-hue swatch (debounced). Only in solid mode, and only for heroes
// whose recipe ships a representative texture (has_preview).
let previewTimer = null;
function schedulePreview() {
  if (previewTimer) clearTimeout(previewTimer);
  if (mode.value !== 'solid' || !selectedHero.value || !vpkPath.value.trim()) {
    preview.value = { state: 'idle' };
    return;
  }
  if (!selectedHeroInfo.value?.has_preview) {
    preview.value = { state: 'none' };
    return;
  }
  preview.value = { state: 'loading' };
  previewTimer = setTimeout(async () => {
    try {
      const url = await invoke('recolor_hero_preview', {
        vpkPath: vpkPath.value,
        basePath: basePath.value.trim() || null,
        hero: selectedHero.value,
        hue: Number(hue.value),
        saturation: Number(saturation.value),
        brightness: Number(brightness.value),
      });
      preview.value = { state: 'ok', url };
    } catch (e) {
      preview.value = { state: 'error', message: String(e) };
    }
  }, 250);
}

// Rainbow suitability scan (debounced). Only in rainbow mode.
let scanTimer = null;
function scheduleScan() {
  if (scanTimer) clearTimeout(scanTimer);
  if (mode.value !== 'rainbow' || !selectedHero.value || !vpkPath.value.trim()) {
    rainbowScan.value = null;
    return;
  }
  rainbowScan.value = { loading: true };
  scanTimer = setTimeout(async () => {
    try {
      rainbowScan.value = await invoke('scan_hero_rainbow', {
        vpkPath: vpkPath.value,
        basePath: basePath.value.trim() || null,
        hero: selectedHero.value,
      });
    } catch (e) {
      rainbowScan.value = { error: String(e) };
    }
  }, 250);
}

watch([mode, selectedHero, vpkPath, basePath, hue, saturation, brightness], () => {
  schedulePreview();
  scheduleScan();
});

onMounted(async () => {
  await loadDefaults();
  schedulePreview();
  scheduleScan();
});
</script>

<template>
  <div class="flex-1 min-h-0 overflow-y-auto">
    <div class="min-h-full p-3 sm:p-4 md:p-8">
      <div class="w-full max-w-3xl mx-auto space-y-5">

        <!-- Mode + hero picker -->
        <div class="paper-card rounded-md p-4">
          <div class="flex items-baseline justify-between gap-3 mb-3">
            <h3 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
              Recolor
            </h3>
            <span class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300">
              {{ heroes.length }} supported heroes
            </span>
          </div>

          <div role="radiogroup" aria-label="Recolor mode" class="flex gap-1 p-1 bg-surface-100/70 dark:bg-surface-800/40 rounded-md mb-4">
            <button
              v-for="m in MODES"
              :key="m.key"
              type="button"
              role="radio"
              :aria-checked="mode === m.key"
              @click="mode = m.key"
              class="flex-1 text-xs font-medium py-1.5 px-2 rounded transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-700/45 dark:focus-visible:ring-accent-300/45"
              :class="mode === m.key
                ? 'bg-accent-600 text-surface-0'
                : 'text-ink-700 dark:text-ink-300 hover:bg-surface-200/60 dark:hover:bg-surface-700/40'"
            >{{ m.label }}</button>
          </div>

          <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium mb-2">
            Hero
          </label>
          <div class="grid gap-2 sm:grid-cols-2">
            <button
              v-for="hero in heroes"
              :key="hero.codename"
              type="button"
              @click="selectedHero = hero.codename"
              class="text-left rounded-md border px-3 py-2 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-700/45 dark:focus-visible:ring-accent-300/45"
              :class="selectedHero === hero.codename
                ? 'border-accent-500 bg-accent-500/10'
                : 'border-surface-200 dark:border-surface-800 hover:bg-surface-100/70 dark:hover:bg-surface-800/50'"
              :aria-pressed="selectedHero === hero.codename"
            >
              <div class="flex items-baseline justify-between gap-2">
                <span class="font-serif text-base text-ink-800 dark:text-ink-100">{{ hero.label }}</span>
                <span class="font-mono text-[10px] text-ink-500 dark:text-ink-300">{{ hero.codename }}</span>
              </div>
              <div class="text-[11px] font-serif italic text-ink-500 dark:text-ink-300 mt-0.5">
                {{ heroMeta(hero) }}
              </div>
            </button>
          </div>
        </div>

        <!-- Inputs -->
        <div class="paper-card rounded-md p-4 space-y-3">
          <div class="space-y-1.5">
            <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
              Source VPK
            </label>
            <div class="flex gap-2">
              <input
                v-model="vpkPath"
                type="text"
                spellcheck="false"
                class="flex-1 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500"
              />
              <button class="btn" type="button" @click="pickSourceVpk">Browse</button>
            </div>
          </div>

          <div class="space-y-1.5">
            <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
              Base VPK <span class="font-serif italic lowercase tracking-normal text-ink-500/80 dark:text-ink-300/80">(optional fallback)</span>
            </label>
            <div class="flex gap-2">
              <input
                v-model="basePath"
                type="text"
                spellcheck="false"
                placeholder="pak01_dir.vpk - fills entries a texture-only skin omits"
                class="flex-1 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 placeholder:italic placeholder:font-serif placeholder:text-ink-500 dark:placeholder:text-ink-300 focus:outline-none focus:border-accent-500"
              />
              <button class="btn" type="button" @click="pickBaseVpk">Browse</button>
            </div>
          </div>

          <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
            <div class="space-y-1.5">
              <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                Output
              </label>
              <div class="flex gap-2">
                <input
                  v-model="outputPath"
                  type="text"
                  spellcheck="false"
                  class="flex-1 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500"
                />
                <button class="btn" type="button" @click="pickOutputPath">Browse</button>
              </div>
            </div>
            <button type="button" class="btn self-end" @click="refreshAddonPath">Next pak</button>
          </div>
        </div>

        <!-- Color controls + preview -->
        <div class="paper-card rounded-md p-4">
          <div class="grid gap-4 md:grid-cols-[minmax(0,1fr)_auto]">
            <div class="space-y-3">
              <div v-if="mode === 'solid'" class="space-y-1.5">
                <label class="flex items-baseline justify-between text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                  <span>Hue</span>
                  <span class="font-mono tracking-normal normal-case text-ink-800 dark:text-ink-100">{{ Math.round(hue) }}&deg;</span>
                </label>
                <input v-model.number="hue" type="range" min="0" max="360" step="1" class="w-full accent-accent-600" />
              </div>
              <div v-else class="space-y-1.5">
                <label class="flex items-baseline justify-between text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">
                  <span>Hue offset</span>
                  <span class="font-mono tracking-normal normal-case text-ink-800 dark:text-ink-100">{{ Math.round(hueOffset) }}&deg;</span>
                </label>
                <input v-model.number="hueOffset" type="range" min="0" max="360" step="1" class="w-full accent-accent-600" />
                <p class="text-[11px] font-serif italic text-ink-500 dark:text-ink-300">
                  Rotates where the spectrum starts; the per-effect spread is unchanged.
                </p>
              </div>

              <div class="grid grid-cols-2 gap-3">
                <div class="space-y-1">
                  <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">Saturation</label>
                  <input v-model.number="saturation" type="number" min="0" max="4" step="0.05" class="w-full bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-2 py-1.5 text-xs font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500" />
                </div>
                <div class="space-y-1">
                  <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">Brightness</label>
                  <input v-model.number="brightness" type="number" min="0" max="4" step="0.05" class="w-full bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-2 py-1.5 text-xs font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500" />
                </div>
              </div>

              <label v-if="mode === 'rainbow'" class="flex items-start gap-2 cursor-pointer select-none">
                <input v-model="animated" type="checkbox" class="mt-0.5 accent-accent-600" />
                <span class="text-xs text-ink-700 dark:text-ink-200">
                  Animated
                  <span class="block text-[11px] font-serif italic text-ink-500 dark:text-ink-300">
                    Sweep the spectrum over each particle's lifetime (glow / beam / trail / slash). Color-only when off.
                  </span>
                </span>
              </label>
            </div>

            <!-- Preview swatch (solid mode) -->
            <div class="flex flex-col items-center justify-start gap-1.5 md:w-32">
              <span class="text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">Preview</span>
              <div class="w-28 h-28 rounded-md border border-surface-200 dark:border-surface-800 bg-surface-100/60 dark:bg-surface-900/60 overflow-hidden flex items-center justify-center text-[10px] text-ink-500 dark:text-ink-300 text-center px-2">
                <template v-if="mode === 'rainbow'">
                  <div class="w-full h-full" style="background: linear-gradient(135deg, #ff004c, #ff7a00, #ffd400, #36d100, #00b3ff, #6a00ff, #ff004c);" aria-hidden="true" />
                </template>
                <img v-else-if="preview.state === 'ok'" :src="preview.url" alt="recolor preview" class="w-full h-full object-contain" style="image-rendering: pixelated;" />
                <span v-else-if="preview.state === 'loading'" class="animate-pulse">decoding...</span>
                <span v-else-if="preview.state === 'none'" class="italic font-serif">particle-only, no swatch</span>
                <span v-else-if="preview.state === 'error'" class="italic font-serif">preview failed</span>
                <span v-else class="italic font-serif">set a source</span>
              </div>
              <span v-if="mode === 'rainbow'" class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300 text-center">illustrative spectrum</span>
            </div>
          </div>
        </div>

        <!-- Rainbow suitability -->
        <div v-if="mode === 'rainbow' && rainbowScan" class="paper-card rounded-md p-4">
          <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium mb-2">
            Rainbow suitability
          </h4>
          <div v-if="rainbowScan.loading" class="text-xs italic font-serif text-ink-500 dark:text-ink-300">Scanning...</div>
          <div v-else-if="rainbowScan.error" class="text-xs font-mono text-red-700 dark:text-red-400 break-all">{{ rainbowScan.error }}</div>
          <template v-else>
            <div class="flex items-center gap-2 mb-2">
              <span class="text-[10px] uppercase tracking-wide px-2 py-0.5 rounded-full bg-accent-600 text-surface-0 font-medium">{{ rainbowScan.mode }}</span>
              <span class="text-[11px] font-serif italic text-ink-500 dark:text-ink-300">{{ SCAN_LABELS[rainbowScan.mode] || '' }}</span>
            </div>
            <dl class="grid grid-cols-3 md:grid-cols-4 gap-2 text-xs">
              <div><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Patchable</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ rainbowScan.particles_patchable }}/{{ rainbowScan.particles_total }}</dd></div>
              <div><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Gradients</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ rainbowScan.gradient_fields }}</dd></div>
              <div><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Looped</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ rainbowScan.looped_gradient_fields }}</dd></div>
              <div><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Age-driven</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ rainbowScan.age_gradient_fields }}</dd></div>
            </dl>
          </template>
        </div>

        <!-- Build -->
        <div class="paper-card rounded-md p-4">
          <div class="flex items-center gap-2 mb-3 min-h-[1.25rem]" aria-live="polite">
            <svg v-if="busy" class="shrink-0 w-4 h-4 animate-spin text-accent-700 dark:text-accent-300" viewBox="0 0 16 16" fill="none" aria-hidden="true">
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
            >{{ status.text || 'Ready' }}</span>
            <button
              v-if="report"
              type="button"
              class="text-xs italic font-serif text-accent-700 dark:text-accent-300 hover:underline focus-visible:outline-none focus-visible:underline rounded"
              @click="revealOutput"
            >reveal</button>
          </div>

          <button
            type="button"
            :disabled="!canBuild"
            class="merge-button w-full h-12 rounded-md font-medium text-base bg-accent-600 hover:!bg-accent-700 text-surface-0 disabled:opacity-40 disabled:cursor-not-allowed transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-300 focus-visible:ring-offset-2 focus-visible:ring-offset-surface-0 dark:focus-visible:ring-offset-surface-950"
            @click="build"
          >{{ mode === 'solid' ? 'Build Recolor VPK' : 'Build Prism VPK' }}</button>
        </div>

        <!-- Result -->
        <div v-if="report" class="paper-card rounded-md p-4">
          <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium mb-3">
            Last build
          </h4>
          <dl class="grid grid-cols-2 md:grid-cols-4 gap-3 text-xs">
            <div><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Entries</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.total_entries }}</dd></div>
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Particles</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">
                <span v-if="reportMode === 'rainbow'">{{ report.particles_recolored }}/{{ report.particles_total }}</span>
                <span v-else>{{ report.particles_recolored }}</span>
              </dd>
            </div>
            <div><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Textures</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.textures_recolored }}</dd></div>
            <div><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Materials</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.materials_recolored }}</dd></div>
            <div><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Models</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.models_recolored }}</dd></div>
            <div v-if="reportMode === 'rainbow'"><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Gradients</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.gradient_fields }}</dd></div>
            <div v-if="reportMode === 'rainbow' && report.particles_animated"><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Animated</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.particles_animated }}</dd></div>
            <div><dt class="text-ink-500 dark:text-ink-300 font-serif italic">Unpatchable</dt><dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.particles_unpatchable + report.materials_unpatchable }}</dd></div>
          </dl>
          <p v-if="report.particles_unpatchable + report.materials_unpatchable > 0" class="mt-2 text-[11px] font-serif italic text-accent-700 dark:text-accent-300">
            Partial: some color-bearing entries could not be patched in place and were left vanilla.
          </p>
          <p class="mt-3 text-[11px] font-mono text-ink-500 dark:text-ink-300 break-all">{{ report.output_path }}</p>
        </div>
      </div>
    </div>
  </div>
</template>
