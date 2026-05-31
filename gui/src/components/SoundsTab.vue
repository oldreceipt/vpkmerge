<script setup>
import { computed, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

// Soundevents editor: load a .vsndevts_c (from a VPK or a loose file), tweak
// per-event volume / pitch and swap clip paths, then bake an addon VPK. The
// edit batch is stateless on the Rust side (build_soundevents_vpk applies the
// whole set in one call), so we collect edits here and submit on build.

const fromVpk = ref(true);
const vpkPath = ref('');
const entry = ref('');
const loosePath = ref('');
const outputPath = ref('');

const events = ref(null); // null until loaded; then array of summaries
const edits = ref({});    // event name -> { originalVolume, volume, pitch }
const swaps = ref([]);    // [{ from, to }]
const filter = ref('');

const busy = ref(false);
const status = ref({ text: '', kind: '' });
const report = ref(null);

const sourceReady = computed(() =>
  fromVpk.value ? vpkPath.value.trim() && entry.value.trim() : loosePath.value.trim(),
);

const canBuild = computed(() => !busy.value && events.value && outputPath.value.trim());

const visibleEvents = computed(() => {
  if (!events.value) return [];
  const f = filter.value.trim().toLowerCase();
  if (!f) return events.value;
  return events.value.filter((e) => e.name.toLowerCase().includes(f));
});

function setStatus(text, kind = '') {
  status.value = { text, kind };
}

function sourceArgs() {
  return fromVpk.value
    ? { input: entry.value, fromVpk: vpkPath.value }
    : { input: loosePath.value, fromVpk: null };
}

async function loadDefaults() {
  try {
    const [base, addonPath] = await Promise.all([
      invoke('default_deadlock_vpk_path'),
      invoke('default_addon_output_path'),
    ]);
    vpkPath.value = base || '';
    outputPath.value = addonPath || '';
  } catch (e) {
    setStatus(`Load failed: ${e}`, 'error');
  }
}

async function pickVpk() {
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

async function loadEvents() {
  if (!sourceReady.value) return;
  busy.value = true;
  report.value = null;
  setStatus('Loading soundevents...');
  try {
    const list = await invoke('load_soundevents', sourceArgs());
    events.value = list;
    const next = {};
    for (const e of list) {
      next[e.name] = {
        originalVolume: e.volume,
        volume: e.volume ?? '',
        pitch: '',
      };
    }
    edits.value = next;
    setStatus(`Loaded ${list.length} event${list.length === 1 ? '' : 's'}`, 'success');
  } catch (e) {
    events.value = null;
    setStatus(`Load failed: ${e}`, 'error');
  } finally {
    busy.value = false;
  }
}

function addSwap() {
  swaps.value.push({ from: '', to: '' });
}

function removeSwap(idx) {
  swaps.value.splice(idx, 1);
}

// Build the numeric field-set list from any volume the user changed and any
// pitch they filled in.
function collectSets() {
  const sets = [];
  for (const [event, e] of Object.entries(edits.value)) {
    if (e.volume !== '' && e.volume !== null && Number(e.volume) !== e.originalVolume) {
      sets.push({ event, field: 'volume', value: Number(e.volume) });
    }
    if (e.pitch !== '' && e.pitch !== null) {
      sets.push({ event, field: 'pitch', value: Number(e.pitch) });
    }
  }
  return sets;
}

const editCount = computed(() => {
  const sets = collectSets();
  const cleanSwaps = swaps.value.filter((s) => s.from.trim() && s.to.trim());
  return sets.length + cleanSwaps.length;
});

async function build() {
  if (!canBuild.value) return;
  busy.value = true;
  report.value = null;
  setStatus('Baking soundevents VPK...');
  try {
    const sets = collectSets();
    const cleanSwaps = swaps.value
      .filter((s) => s.from.trim() && s.to.trim())
      .map((s) => ({ from: s.from.trim(), to: s.to.trim() }));
    const args = sourceArgs();
    const result = await invoke('build_soundevents_vpk', {
      input: args.input,
      fromVpk: args.fromVpk,
      vpkEntry: fromVpk.value ? null : entry.value.trim() || null,
      sets,
      swaps: cleanSwaps,
      outputPath: outputPath.value,
    });
    report.value = result;
    setStatus(result.summary, 'success');
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

onMounted(loadDefaults);
</script>

<template>
  <div class="flex-1 min-h-0 overflow-y-auto">
    <div class="min-h-full p-3 sm:p-4 md:p-8">
      <div class="w-full max-w-3xl mx-auto space-y-5">

        <!-- Source -->
        <div class="paper-card rounded-md p-4 space-y-3">
          <div class="flex items-baseline justify-between gap-3">
            <h3 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
              Soundevents
            </h3>
            <span class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300">.vsndevts_c</span>
          </div>

          <div role="radiogroup" aria-label="Source" class="flex gap-1 p-1 bg-surface-100/70 dark:bg-surface-800/40 rounded-md">
            <button
              v-for="opt in [{ k: true, l: 'From VPK' }, { k: false, l: 'Loose file' }]"
              :key="String(opt.k)"
              type="button"
              role="radio"
              :aria-checked="fromVpk === opt.k"
              @click="fromVpk = opt.k"
              class="flex-1 text-xs font-medium py-1.5 px-2 rounded transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-700/45 dark:focus-visible:ring-accent-300/45"
              :class="fromVpk === opt.k ? 'bg-accent-600 text-surface-0' : 'text-ink-700 dark:text-ink-300 hover:bg-surface-200/60 dark:hover:bg-surface-700/40'"
            >{{ opt.l }}</button>
          </div>

          <template v-if="fromVpk">
            <div class="space-y-1.5">
              <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">Source VPK</label>
              <div class="flex gap-2">
                <input v-model="vpkPath" type="text" spellcheck="false" class="flex-1 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500" />
                <button class="btn" type="button" @click="pickVpk">Browse</button>
              </div>
            </div>
            <div class="space-y-1.5">
              <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">Entry path</label>
              <input v-model="entry" type="text" spellcheck="false" placeholder="soundevents/hero/gigawatt.vsndevts_c" class="w-full bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 placeholder:italic placeholder:font-serif placeholder:text-ink-500 dark:placeholder:text-ink-300 focus:outline-none focus:border-accent-500" />
            </div>
          </template>
          <template v-else>
            <div class="space-y-1.5">
              <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">Loose file path</label>
              <input v-model="loosePath" type="text" spellcheck="false" placeholder="/path/to/file.vsndevts_c" class="w-full bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 placeholder:italic placeholder:font-serif placeholder:text-ink-500 dark:placeholder:text-ink-300 focus:outline-none focus:border-accent-500" />
              <p class="text-[11px] font-serif italic text-ink-500 dark:text-ink-300">The addon packs this back at the entry path above; switch to "From VPK" to default it.</p>
            </div>
          </template>

          <button type="button" class="btn w-full" :disabled="!sourceReady || busy" @click="loadEvents">Load events</button>
        </div>

        <!-- Events -->
        <div v-if="events" class="paper-card rounded-md p-4">
          <div class="flex items-baseline justify-between gap-3 mb-2">
            <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
              Events
            </h4>
            <span class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300 tabular-nums">{{ events.length }} total</span>
          </div>
          <input v-model="filter" type="text" spellcheck="false" placeholder="Filter events..." class="w-full mb-3 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-2.5 py-1.5 text-xs font-mono text-ink-800 dark:text-ink-100 placeholder:italic placeholder:font-serif placeholder:text-ink-500 dark:placeholder:text-ink-300 focus:outline-none focus:border-accent-500" />

          <div class="max-h-80 overflow-auto -mx-1 px-1">
            <div v-for="e in visibleEvents" :key="e.name" class="flex items-center gap-2 py-1.5 border-b border-surface-200/60 dark:border-surface-800/60 last:border-0">
              <span class="flex-1 min-w-0 font-mono text-[11px] text-ink-800 dark:text-ink-100 truncate" :title="e.name">{{ e.name }}</span>
              <label class="text-[10px] font-serif italic text-ink-500 dark:text-ink-300">vol</label>
              <input v-model="edits[e.name].volume" type="number" step="0.05" min="0" class="w-16 bg-transparent border border-surface-300 dark:border-surface-700 rounded px-1.5 py-0.5 text-[11px] font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500" />
              <label class="text-[10px] font-serif italic text-ink-500 dark:text-ink-300">pitch</label>
              <input v-model="edits[e.name].pitch" type="number" step="0.05" min="0" placeholder="-" class="w-16 bg-transparent border border-surface-300 dark:border-surface-700 rounded px-1.5 py-0.5 text-[11px] font-mono text-ink-800 dark:text-ink-100 placeholder:text-ink-500/60 focus:outline-none focus:border-accent-500" />
            </div>
            <p v-if="visibleEvents.length === 0" class="text-xs italic font-serif text-ink-500 dark:text-ink-300 text-center py-4">No matches</p>
          </div>
        </div>

        <!-- Clip swaps -->
        <div v-if="events" class="paper-card rounded-md p-4">
          <div class="flex items-baseline justify-between gap-3 mb-2">
            <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">Clip swaps</h4>
            <button type="button" class="text-xs italic font-serif text-accent-700 dark:text-accent-300 hover:underline focus-visible:outline-none focus-visible:underline rounded" @click="addSwap">+ add swap</button>
          </div>
          <p v-if="swaps.length === 0" class="text-[11px] font-serif italic text-ink-500 dark:text-ink-300">Replace a clip path everywhere in the tree (old -&gt; new).</p>
          <div v-for="(s, idx) in swaps" :key="idx" class="flex items-center gap-2 mb-1.5">
            <input v-model="s.from" type="text" spellcheck="false" placeholder="old vsnd path" class="flex-1 min-w-0 bg-transparent border border-surface-300 dark:border-surface-700 rounded px-2 py-1 text-[11px] font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500" />
            <span class="text-ink-500 dark:text-ink-300">&rarr;</span>
            <input v-model="s.to" type="text" spellcheck="false" placeholder="new vsnd path" class="flex-1 min-w-0 bg-transparent border border-surface-300 dark:border-surface-700 rounded px-2 py-1 text-[11px] font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500" />
            <button type="button" class="text-ink-500 dark:text-ink-300 hover:text-red-600 dark:hover:text-red-400 text-lg leading-none px-1" title="Remove" @click="removeSwap(idx)">&times;</button>
          </div>
        </div>

        <!-- Output + build -->
        <div v-if="events" class="paper-card rounded-md p-4 space-y-3">
          <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
            <div class="space-y-1.5">
              <label class="block text-[10px] uppercase tracking-[0.16em] text-ink-500 dark:text-ink-300 font-medium">Output</label>
              <div class="flex gap-2">
                <input v-model="outputPath" type="text" spellcheck="false" class="flex-1 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500" />
                <button class="btn" type="button" @click="pickOutputPath">Browse</button>
              </div>
            </div>
            <button type="button" class="btn self-end" @click="refreshAddonPath">Next pak</button>
          </div>

          <div class="flex items-center gap-2 min-h-[1.25rem]" aria-live="polite">
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
            >{{ status.text || `${editCount} edit${editCount === 1 ? '' : 's'} pending` }}</span>
            <button v-if="report" type="button" class="text-xs italic font-serif text-accent-700 dark:text-accent-300 hover:underline focus-visible:outline-none focus-visible:underline rounded" @click="revealOutput">reveal</button>
          </div>

          <button
            type="button"
            :disabled="!canBuild"
            class="merge-button w-full h-12 rounded-md font-medium text-base bg-accent-600 hover:!bg-accent-700 text-surface-0 disabled:opacity-40 disabled:cursor-not-allowed transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-300 focus-visible:ring-offset-2 focus-visible:ring-offset-surface-0 dark:focus-visible:ring-offset-surface-950"
            @click="build"
          >Build addon VPK</button>
          <p v-if="report" class="text-[11px] font-mono text-ink-500 dark:text-ink-300 break-all">{{ report.output_path }}</p>
        </div>
      </div>
    </div>
  </div>
</template>
