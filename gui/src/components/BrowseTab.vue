<script setup>
import { ref, computed, onMounted, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';

// Browse tab: auto-loads the local Deadlock pak01_dir.vpk and lets the user
// walk the VPK's file tree. Selecting a .vtex_c entry renders a live preview
// via the existing `preview_texture` Tauri command.

const state = ref({ kind: 'loading' });
//  Possible shapes:
//   { kind: 'loading' }
//   { kind: 'no-install' }
//   { kind: 'error', message }
//   { kind: 'ready', vpkPath, name, fileCount, tree }

const expanded = ref(new Set()); // folder paths that are currently expanded
const selected = ref(null);      // entry path of the selected file
const filter = ref('');
const previewState = ref(null);  // null | { kind: 'loading' } | { kind: 'ok', ... } | { kind: 'error', message }

const PREVIEW_MAX_DIM = 512;
// pak01_dir.vpk has ~130k entries; an unbounded filter would crater the DOM.
// Cap to a workable visible count and tell the user there's more behind it.
const FILTER_MAX_RESULTS = 500;
const filterTotal = ref(0);

onMounted(async () => {
  try {
    const path = await invoke('default_deadlock_vpk_path');
    if (!path) {
      state.value = { kind: 'no-install' };
      return;
    }
    await loadVpk(path);
  } catch (e) {
    state.value = { kind: 'error', message: String(e) };
  }
});

async function loadVpk(path) {
  state.value = { kind: 'loading' };
  try {
    const mod = await invoke('add_mod', { path });
    const tree = buildTree(mod.file_paths);
    state.value = {
      kind: 'ready',
      vpkPath: mod.path,
      name: mod.name,
      fileCount: mod.file_count,
      tree,
    };
    expanded.value = new Set(); // start fully collapsed
    selected.value = null;
    previewState.value = null;
  } catch (e) {
    state.value = { kind: 'error', message: String(e) };
  }
}

// Build a nested tree from a flat list of "a/b/c.ext" paths.
// Each node: { name, path, children: Map<string, node>, files: Array<{name, path}> }.
// Folders sort case-insensitively; files inside each folder also.
function buildTree(paths) {
  const root = { name: '', path: '', children: new Map(), files: [] };
  for (const p of paths) {
    const parts = p.split('/');
    const filename = parts.pop();
    let node = root;
    let accum = '';
    for (const part of parts) {
      accum = accum ? `${accum}/${part}` : part;
      let next = node.children.get(part);
      if (!next) {
        next = { name: part, path: accum, children: new Map(), files: [] };
        node.children.set(part, next);
      }
      node = next;
    }
    node.files.push({ name: filename, path: p });
  }
  sortTree(root);
  return root;
}

function sortTree(node) {
  // Sort children map by key, case-insensitive.
  const entries = Array.from(node.children.entries());
  entries.sort(([a], [b]) => a.localeCompare(b, undefined, { sensitivity: 'base' }));
  node.children = new Map(entries);
  node.files.sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }));
  for (const child of node.children.values()) sortTree(child);
}

// Flatten the tree into the visible rows for the current expansion + filter
// state. Each row is { type: 'folder' | 'file', name, path, depth, expanded? }.
// When a filter is active we ignore tree shape and emit matching files only.
const visibleRows = computed(() => {
  if (state.value.kind !== 'ready') return [];
  const f = filter.value.trim().toLowerCase();
  if (f) return filteredRows(state.value.tree, f);
  return treeRows(state.value.tree, 0);
});

function treeRows(node, depth) {
  const out = [];
  // depth 0 is the synthetic root; skip its row.
  if (depth > 0) {
    out.push({
      type: 'folder',
      name: node.name,
      path: node.path,
      depth: depth - 1,
      expanded: expanded.value.has(node.path),
    });
    if (!expanded.value.has(node.path)) return out;
  }
  for (const child of node.children.values()) {
    out.push(...treeRows(child, depth + 1));
  }
  if (depth > 0 || node.path === '') {
    for (const file of node.files) {
      out.push({
        type: 'file',
        name: file.name,
        path: file.path,
        depth: depth, // files sit one level deeper than their folder
      });
    }
  }
  return out;
}

function filteredRows(root, needle) {
  const out = [];
  let total = 0;
  walkFiles(root, (file) => {
    if (file.path.toLowerCase().includes(needle)) {
      total += 1;
      if (out.length < FILTER_MAX_RESULTS) {
        out.push({ type: 'file', name: file.path, path: file.path, depth: 0 });
      }
    }
  });
  filterTotal.value = total;
  return out;
}

function walkFiles(node, visit) {
  for (const file of node.files) visit(file);
  for (const child of node.children.values()) walkFiles(child, visit);
}

function toggleFolder(path) {
  const next = new Set(expanded.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  expanded.value = next;
}

function selectFile(path) {
  selected.value = path;
}

function isPreviewable(path) {
  return typeof path === 'string' && path.toLowerCase().endsWith('.vtex_c');
}

watch(selected, async (path) => {
  if (!path) {
    previewState.value = null;
    return;
  }
  if (!isPreviewable(path)) {
    previewState.value = { kind: 'unsupported' };
    return;
  }
  if (state.value.kind !== 'ready') return;
  previewState.value = { kind: 'loading' };
  const vpkPath = state.value.vpkPath;
  try {
    const p = await invoke('preview_texture', {
      vpkPath,
      entry: path,
      maxDim: PREVIEW_MAX_DIM,
    });
    // Race guard: another selection may have replaced this one mid-await.
    if (selected.value !== path) return;
    previewState.value = {
      kind: 'ok',
      dataUrl: p.data_url,
      width: p.width,
      height: p.height,
      origWidth: p.orig_width,
      origHeight: p.orig_height,
      format: p.format,
      mipCount: p.mip_count,
      isCubemap: p.is_cubemap,
    };
  } catch (e) {
    if (selected.value !== path) return;
    previewState.value = { kind: 'error', message: String(e) };
  }
});

async function pickVpkManually() {
  try {
    const paths = await invoke('pick_vpk_files');
    if (paths?.length) await loadVpk(paths[0]);
  } catch (e) {
    state.value = { kind: 'error', message: String(e) };
  }
}

const fileCount = computed(() =>
  state.value.kind === 'ready' ? state.value.fileCount : 0,
);

// --- Contextual recolor of the selected entry ---------------------------
// A .vtex_c gets a hue/sat/brightness recolor with a live thumbnail; a
// .vmdl_c gets its color-buffer list + a recolor-to-addon action. Both pack a
// standalone addon VPK that overrides the entry in place.
const addonOut = ref('');
const buildBusy = ref(false);
const buildStatus = ref({ text: '', kind: '' });
const addonResult = ref(null);

const hue = ref(0);
const saturation = ref(1.0);
const brightness = ref(1.0);

const texPreview = ref({ state: 'idle' }); // idle | loading | ok | error
const modelBuffers = ref(null);            // null | [] | [...] | { error }

function isModelPath(path) {
  return typeof path === 'string' && path.toLowerCase().endsWith('.vmdl_c');
}
const isRecolorableTexture = computed(() => isPreviewable(selected.value));
const isRecolorableModel = computed(() => isModelPath(selected.value));

function setBuildStatus(text, kind = '') {
  buildStatus.value = { text, kind };
}

async function ensureAddonOut() {
  if (addonOut.value) return;
  try { addonOut.value = (await invoke('default_addon_output_path')) || ''; }
  catch { /* noop */ }
}
async function pickAddonOut() {
  try { const p = await invoke('pick_output_path'); if (p) addonOut.value = p; }
  catch (e) { setBuildStatus(`Picker failed: ${e}`, 'error'); }
}
async function nextPak() {
  try { const p = await invoke('default_addon_output_path'); if (p) addonOut.value = p; }
  catch (e) { setBuildStatus(`Could not resolve addon path: ${e}`, 'error'); }
}

let texTimer = null;
function scheduleTexPreview() {
  if (texTimer) clearTimeout(texTimer);
  if (!isRecolorableTexture.value || state.value.kind !== 'ready') {
    texPreview.value = { state: 'idle' };
    return;
  }
  texPreview.value = { state: 'loading' };
  const path = selected.value;
  const vpkPath = state.value.vpkPath;
  texTimer = setTimeout(async () => {
    try {
      const url = await invoke('preview_texture_recolor', {
        vpkPath,
        entry: path,
        hue: Number(hue.value),
        saturation: Number(saturation.value),
        brightness: Number(brightness.value),
        maxDim: 256,
      });
      if (selected.value !== path) return;
      texPreview.value = { state: 'ok', url };
    } catch (e) {
      if (selected.value !== path) return;
      texPreview.value = { state: 'error', message: String(e) };
    }
  }, 250);
}

async function loadModelBuffers(path) {
  modelBuffers.value = null;
  if (state.value.kind !== 'ready') return;
  try {
    const list = await invoke('list_model_colors', {
      vpkPath: state.value.vpkPath,
      entry: path,
      basePath: null,
    });
    if (selected.value !== path) return;
    modelBuffers.value = list;
  } catch (e) {
    if (selected.value !== path) return;
    modelBuffers.value = { error: String(e) };
  }
}

async function saveTextureAddon() {
  if (state.value.kind !== 'ready') return;
  await ensureAddonOut();
  if (!addonOut.value) { setBuildStatus('Set an output path', 'error'); return; }
  buildBusy.value = true;
  addonResult.value = null;
  setBuildStatus('Baking addon...');
  try {
    const r = await invoke('recolor_texture_to_addon', {
      vpkPath: state.value.vpkPath,
      entry: selected.value,
      hue: Number(hue.value),
      saturation: Number(saturation.value),
      brightness: Number(brightness.value),
      outputPath: addonOut.value,
    });
    addonResult.value = r;
    setBuildStatus(r.summary, 'success');
  } catch (e) {
    setBuildStatus(`Build failed: ${e}`, 'error');
  } finally {
    buildBusy.value = false;
  }
}

async function saveModelAddon() {
  if (state.value.kind !== 'ready') return;
  await ensureAddonOut();
  if (!addonOut.value) { setBuildStatus('Set an output path', 'error'); return; }
  buildBusy.value = true;
  addonResult.value = null;
  setBuildStatus('Baking addon...');
  try {
    const r = await invoke('recolor_model_to_addon', {
      vpkPath: state.value.vpkPath,
      entry: selected.value,
      basePath: null,
      hue: Number(hue.value),
      saturation: Number(saturation.value),
      brightness: Number(brightness.value),
      outputPath: addonOut.value,
    });
    addonResult.value = r;
    setBuildStatus(r.summary, 'success');
  } catch (e) {
    setBuildStatus(`Build failed: ${e}`, 'error');
  } finally {
    buildBusy.value = false;
  }
}

async function revealAddon() {
  if (!addonResult.value?.output_path) return;
  try { await invoke('reveal_in_folder', { path: addonResult.value.output_path }); }
  catch { /* noop */ }
}

// Reset recolor state on selection change; preload model color buffers and the
// live texture thumbnail. (The original preview watcher above is untouched.)
watch(selected, (path) => {
  addonResult.value = null;
  setBuildStatus('');
  modelBuffers.value = null;
  texPreview.value = { state: 'idle' };
  if (isModelPath(path)) loadModelBuffers(path);
  scheduleTexPreview();
});
watch([hue, saturation, brightness], scheduleTexPreview);
onMounted(ensureAddonOut);
</script>

<template>
  <div class="flex flex-col h-full min-h-0">
    <!-- Loading / error / no-install banners -->
    <div
      v-if="state.kind !== 'ready'"
      class="flex-1 flex items-center justify-center p-6"
    >
      <div class="paper-card rounded-md max-w-md w-full p-6 text-center space-y-3">
        <h3 class="font-serif text-lg text-ink-800 dark:text-ink-100">
          <span v-if="state.kind === 'loading'">Loading Deadlock VPK...</span>
          <span v-else-if="state.kind === 'no-install'">No local Deadlock install found</span>
          <span v-else>Couldn't open VPK</span>
        </h3>
        <p
          v-if="state.kind === 'no-install'"
          class="text-xs font-serif italic text-ink-500 dark:text-ink-300"
        >Checked the standard Steam paths. Point to <code class="font-mono not-italic">pak01_dir.vpk</code> manually.</p>
        <p
          v-else-if="state.kind === 'error'"
          class="text-xs font-mono text-red-700 dark:text-red-400 break-all"
        >{{ state.message }}</p>
        <button
          v-if="state.kind !== 'loading'"
          class="btn"
          @click="pickVpkManually"
        >Pick a VPK</button>
      </div>
    </div>

    <!-- Ready: header + two-pane (tree | preview) -->
    <div v-else class="flex-1 min-h-0 flex flex-col">
      <header class="flex items-baseline justify-between gap-4 px-4 py-2 border-b border-surface-200 dark:border-surface-800 shrink-0">
        <div class="min-w-0">
          <h3 class="font-serif text-base text-ink-800 dark:text-ink-100 truncate" :title="state.vpkPath">
            {{ state.name }}
          </h3>
          <p class="text-[11px] font-serif italic text-ink-500 dark:text-ink-300 tabular-nums">
            {{ fileCount.toLocaleString() }} entries
          </p>
        </div>
        <button
          class="text-xs italic font-serif text-ink-500 dark:text-ink-300 hover:text-accent-700 dark:hover:text-accent-300 focus-visible:outline-none focus-visible:underline rounded shrink-0"
          @click="pickVpkManually"
        >Open different VPK...</button>
      </header>

      <div class="flex-1 min-h-0 flex">
        <!-- Left pane: filter + file tree -->
        <div class="flex flex-col w-1/2 min-w-0 border-r border-surface-200 dark:border-surface-800">
          <div class="px-3 py-2 border-b border-surface-200 dark:border-surface-800 shrink-0">
            <input
              type="text"
              v-model="filter"
              spellcheck="false"
              placeholder="Filter by path..."
              class="w-full bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-2.5 py-1.5 text-xs font-mono text-ink-800 dark:text-ink-100 placeholder:italic placeholder:font-serif placeholder:text-ink-500 dark:placeholder:text-ink-300 focus:outline-none focus:border-accent-500"
            />
            <p
              v-if="filter.trim()"
              class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300 mt-1 tabular-nums"
            >
              <span v-if="filterTotal > FILTER_MAX_RESULTS">
                Showing {{ visibleRows.length.toLocaleString() }} of {{ filterTotal.toLocaleString() }} matches (narrow the filter)
              </span>
              <span v-else>{{ visibleRows.length }} match{{ visibleRows.length === 1 ? '' : 'es' }}</span>
            </p>
          </div>
          <div class="flex-1 min-h-0 overflow-auto">
            <ul class="py-1 min-w-max">
              <li
                v-for="row in visibleRows"
                :key="`${row.type}:${row.path}`"
                class="font-mono text-xs leading-snug whitespace-nowrap"
              >
                <button
                  v-if="row.type === 'folder'"
                  type="button"
                  @click="toggleFolder(row.path)"
                  class="w-full flex items-center gap-1.5 text-left pr-3 py-1 hover:bg-surface-100/70 dark:hover:bg-surface-800/50 focus-visible:outline-none focus-visible:bg-surface-100/70 dark:focus-visible:bg-surface-800/50 rounded-sm text-ink-800 dark:text-ink-100 font-medium"
                  :style="{ paddingLeft: `${0.5 + row.depth * 0.75}rem` }"
                >
                  <span class="text-ink-700 dark:text-ink-200 w-3 inline-block tabular-nums">{{ row.expanded ? '▾' : '▸' }}</span>
                  <span class="text-accent-700/80 dark:text-accent-300/80">{{ row.name }}</span><span class="text-ink-500 dark:text-ink-300">/</span>
                </button>
                <button
                  v-else
                  type="button"
                  @click="selectFile(row.path)"
                  class="w-full flex items-center gap-1.5 text-left pr-3 py-1 rounded-sm focus-visible:outline-none"
                  :class="selected === row.path
                    ? 'bg-accent-600/15 dark:bg-accent-300/15 text-accent-700 dark:text-accent-300 font-medium'
                    : 'text-ink-800 dark:text-ink-100 hover:bg-surface-100/70 dark:hover:bg-surface-800/50'"
                  :style="{ paddingLeft: `${0.5 + (row.depth + 1) * 0.75}rem` }"
                  :title="row.path"
                >
                  <span
                    class="w-3 inline-block"
                    :class="selected === row.path ? 'text-accent-700 dark:text-accent-300' : 'text-ink-500 dark:text-ink-300'"
                  >·</span>
                  <span>{{ row.name }}</span>
                </button>
              </li>
            </ul>
            <p
              v-if="visibleRows.length === 0"
              class="text-xs italic font-serif text-ink-500 dark:text-ink-300 text-center py-6"
            >No matches</p>
          </div>
        </div>

        <!-- Right pane: selected entry preview -->
        <div class="flex-1 min-w-0 flex flex-col">
          <div v-if="!selected" class="flex-1 flex items-center justify-center p-6">
            <p class="text-xs italic font-serif text-ink-500 dark:text-ink-300 text-center">
              Select an entry to preview.
            </p>
          </div>
          <div v-else class="flex-1 min-h-0 flex flex-col">
            <div class="px-4 py-2 border-b border-surface-200 dark:border-surface-800 shrink-0">
              <p class="font-mono text-[11px] text-ink-800 dark:text-ink-100 break-all" :title="selected">
                {{ selected }}
              </p>
              <p
                v-if="previewState?.kind === 'ok'"
                class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300 mt-0.5 tabular-nums"
              >
                {{ previewState.format }} · {{ previewState.origWidth }}x{{ previewState.origHeight }}
                <span v-if="previewState.mipCount > 1"> · {{ previewState.mipCount }} mips</span>
                <span v-if="previewState.isCubemap"> · cubemap</span>
              </p>
            </div>
            <div class="flex-1 min-h-0 overflow-auto flex items-center justify-center p-4">
              <div v-if="previewState?.kind === 'loading'" class="text-xs italic font-serif text-ink-500 dark:text-ink-300">
                Decoding...
              </div>
              <div
                v-else-if="previewState?.kind === 'unsupported'"
                class="max-w-sm text-xs italic font-serif text-ink-500 dark:text-ink-300 text-center"
              >Preview only supported for <code class="font-mono not-italic">.vtex_c</code> entries.</div>
              <div
                v-else-if="previewState?.kind === 'error'"
                class="max-w-sm text-xs font-mono text-red-700 dark:text-red-400 break-all text-center"
              >{{ previewState.message }}</div>
              <img
                v-else-if="previewState?.kind === 'ok'"
                :src="previewState.dataUrl"
                :alt="selected"
                class="max-w-full max-h-full object-contain"
                style="image-rendering: pixelated;"
              />
            </div>

            <!-- Contextual recolor panel: .vtex_c or .vmdl_c -->
            <div
              v-if="isRecolorableTexture || isRecolorableModel"
              class="border-t border-surface-200 dark:border-surface-800 p-4 space-y-2.5 shrink-0"
            >
              <div class="flex items-baseline justify-between">
                <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">Recolor</h4>
                <span
                  v-if="isRecolorableModel && Array.isArray(modelBuffers)"
                  class="text-[10px] italic font-serif text-ink-500 dark:text-ink-300"
                >{{ modelBuffers.length }} color buffer{{ modelBuffers.length === 1 ? '' : 's' }}</span>
              </div>

              <p
                v-if="isRecolorableModel && Array.isArray(modelBuffers) && modelBuffers.length === 0"
                class="text-[11px] font-serif italic text-ink-500 dark:text-ink-300"
              >No baked vertex colors in this model (nothing to recolor).</p>
              <p
                v-else-if="isRecolorableModel && modelBuffers === null"
                class="text-[11px] font-serif italic text-ink-500 dark:text-ink-300"
              >Reading model...</p>
              <p
                v-else-if="isRecolorableModel && modelBuffers && modelBuffers.error"
                class="text-[11px] font-mono text-red-700 dark:text-red-400 break-all"
              >{{ modelBuffers.error }}</p>

              <template v-else>
                <div class="flex items-center gap-3">
                  <input v-model.number="hue" type="range" min="0" max="360" step="1" class="flex-1 accent-accent-600" aria-label="Hue" />
                  <span class="font-mono text-[11px] text-ink-800 dark:text-ink-100 w-10 text-right">{{ Math.round(hue) }}&deg;</span>
                  <span
                    v-if="isRecolorableTexture"
                    class="w-12 h-12 shrink-0 rounded border border-surface-200 dark:border-surface-800 bg-surface-100/60 dark:bg-surface-900/60 overflow-hidden flex items-center justify-center text-[9px] text-ink-500 dark:text-ink-300"
                    title="recolored preview"
                  >
                    <img v-if="texPreview.state === 'ok'" :src="texPreview.url" class="w-full h-full object-contain" style="image-rendering: pixelated;" alt="recolored preview" />
                    <span v-else-if="texPreview.state === 'loading'" class="animate-pulse">...</span>
                    <span v-else-if="texPreview.state === 'error'">?</span>
                  </span>
                </div>
                <div class="flex items-center gap-2 text-[11px]">
                  <label class="font-serif italic text-ink-500 dark:text-ink-300">sat</label>
                  <input v-model.number="saturation" type="number" min="0" max="4" step="0.05" class="w-16 bg-transparent border border-surface-300 dark:border-surface-700 rounded px-1.5 py-0.5 font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500" />
                  <label class="font-serif italic text-ink-500 dark:text-ink-300">bright</label>
                  <input v-model.number="brightness" type="number" min="0" max="4" step="0.05" class="w-16 bg-transparent border border-surface-300 dark:border-surface-700 rounded px-1.5 py-0.5 font-mono text-ink-800 dark:text-ink-100 focus:outline-none focus:border-accent-500" />
                </div>
                <div class="flex gap-2 items-center">
                  <input v-model="addonOut" type="text" spellcheck="false" placeholder="addon output path" class="flex-1 min-w-0 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-2.5 py-1.5 text-[11px] font-mono text-ink-800 dark:text-ink-100 placeholder:italic placeholder:font-serif placeholder:text-ink-500 dark:placeholder:text-ink-300 focus:outline-none focus:border-accent-500" />
                  <button class="btn" type="button" @click="pickAddonOut">Browse</button>
                  <button class="btn" type="button" @click="nextPak">Next pak</button>
                </div>
                <div class="flex items-center gap-2 min-h-[1.1rem]" aria-live="polite">
                  <span
                    class="text-[11px] font-serif italic truncate flex-1"
                    :class="{
                      'text-ink-500 dark:text-ink-300': !buildStatus.kind,
                      'text-green-700 dark:text-green-400 not-italic font-sans': buildStatus.kind === 'success',
                      'text-red-700 dark:text-red-400 not-italic font-sans': buildStatus.kind === 'error',
                    }"
                  >{{ buildStatus.text }}</span>
                  <button v-if="addonResult" type="button" class="text-[11px] italic font-serif text-accent-700 dark:text-accent-300 hover:underline focus-visible:outline-none focus-visible:underline rounded" @click="revealAddon">reveal</button>
                </div>
                <button
                  type="button"
                  :disabled="buildBusy"
                  class="btn w-full bg-accent-600 hover:!bg-accent-700 text-surface-0 font-medium disabled:opacity-40 disabled:cursor-not-allowed"
                  @click="isRecolorableTexture ? saveTextureAddon() : saveModelAddon()"
                >Save as addon VPK</button>
              </template>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
