<script setup>
import { computed, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

const heroes = ref([]);
const selectedHero = ref('');
const vpkPath = ref('');
const outputPath = ref('');
const busy = ref(false);
const status = ref({ text: '', kind: '' });
const report = ref(null);

const selectedHeroInfo = computed(() =>
  heroes.value.find((hero) => hero.codename === selectedHero.value),
);

const canBuild = computed(() =>
  !busy.value && selectedHero.value && vpkPath.value.trim() && outputPath.value.trim(),
);

function setStatus(text, kind = '') {
  status.value = { text, kind };
}

function formatHeroMeta(hero) {
  if (!hero) return '';
  const extras = [];
  if (hero.textures) extras.push(`${hero.textures} texture${hero.textures === 1 ? '' : 's'}`);
  if (hero.materials) extras.push(`${hero.materials} material${hero.materials === 1 ? '' : 's'}`);
  if (hero.models) extras.push(`${hero.models} model${hero.models === 1 ? '' : 's'}`);
  return extras.length ? extras.join(' · ') : 'particle-only';
}

async function loadDefaults() {
  try {
    const [options, basePath, addonPath] = await Promise.all([
      invoke('supported_hero_options'),
      invoke('default_deadlock_vpk_path'),
      invoke('default_addon_output_path'),
    ]);
    heroes.value = options || [];
    selectedHero.value = heroes.value.find((hero) => hero.codename === 'yamato')?.codename
      || heroes.value[0]?.codename
      || '';
    vpkPath.value = basePath || '';
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

async function buildPrism() {
  if (!canBuild.value) return;
  busy.value = true;
  report.value = null;
  setStatus('Building prism VPK...');
  try {
    const result = await invoke('build_hero_prism_vpk', {
      vpkPath: vpkPath.value,
      basePath: null,
      hero: selectedHero.value,
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

onMounted(() => {
  loadDefaults();
});
</script>

<template>
  <div class="flex-1 min-h-0 overflow-y-auto">
    <div class="min-h-full p-3 sm:p-4 md:p-8">
      <div class="w-full max-w-3xl mx-auto space-y-5">
        <div class="paper-card rounded-md p-4">
          <div class="flex items-baseline justify-between gap-3 mb-3">
            <h3 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
              Prism
            </h3>
            <span class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300">
              {{ heroes.length }} supported heroes
            </span>
          </div>

          <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(12rem,16rem)]">
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

          <div class="mt-3 grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
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
            <button
              type="button"
              class="btn self-end"
              @click="refreshAddonPath"
            >
              Next pak
            </button>
          </div>

          <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-serif italic text-ink-500 dark:text-ink-300">
            <span v-if="selectedHeroInfo">
              {{ selectedHeroInfo.particles }} particle root{{ selectedHeroInfo.particles === 1 ? '' : 's' }}
            </span>
            <span v-if="selectedHeroInfo">·</span>
            <span>{{ formatHeroMeta(selectedHeroInfo) }}</span>
          </div>
        </div>

        <div class="paper-card rounded-md p-4">
          <div class="flex items-center gap-2 mb-3 min-h-[1.25rem]" aria-live="polite">
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
              {{ status.text || 'Ready' }}
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
            class="merge-button w-full h-12 rounded-md font-medium text-base bg-accent-600 hover:!bg-accent-700 text-surface-0 disabled:opacity-40 disabled:cursor-not-allowed transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-300 focus-visible:ring-offset-2 focus-visible:ring-offset-surface-0 dark:focus-visible:ring-offset-surface-950"
            @click="buildPrism"
          >
            Build Prism VPK
          </button>
        </div>

        <div
          v-if="report"
          class="paper-card rounded-md p-4"
        >
          <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium mb-3">
            Last Build
          </h4>
          <dl class="grid grid-cols-2 md:grid-cols-4 gap-3 text-xs">
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Entries</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.total_entries }}</dd>
            </div>
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Particles</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.particles_recolored }}/{{ report.particles_total }}</dd>
            </div>
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Gradients</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.gradient_fields }}</dd>
            </div>
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Boosted</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.boosted_fields }}</dd>
            </div>
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Textures</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.textures_recolored }}</dd>
            </div>
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Materials</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.materials_recolored }}</dd>
            </div>
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Models</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.models_recolored }}</dd>
            </div>
            <div>
              <dt class="text-ink-500 dark:text-ink-300 font-serif italic">Unpatchable</dt>
              <dd class="font-mono text-ink-800 dark:text-ink-100">{{ report.particles_unpatchable + report.materials_unpatchable }}</dd>
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
