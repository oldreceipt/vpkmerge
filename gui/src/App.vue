<script setup>
import { ref, computed, onMounted, onBeforeUnmount, watch, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';

const mods = ref([]);
const outputPath = ref('');
const outputExists = ref(false);
const status = ref({ text: '', kind: '' });
const busy = ref(false);
const isDragging = ref(false);
const showConflictsModal = ref(false);
const showMergedModal = ref(false);
const lastReport = ref(null);
const lastFocused = ref(null);
const conflictsModalRef = ref(null);
const mergedModalRef = ref(null);

const conflicts = computed(() => {
  const owners = new Map();
  mods.value.forEach((mod, idx) => {
    for (const p of mod.file_paths) {
      if (!owners.has(p)) owners.set(p, []);
      owners.get(p).push(idx);
    }
  });
  const out = [];
  for (const [path, idxs] of owners) {
    if (idxs.length > 1) {
      out.push({ path, winner: idxs[idxs.length - 1], owners: idxs });
    }
  }
  return out.sort((a, b) => a.path.localeCompare(b.path));
});

const conflictsByMod = computed(() => {
  const counts = new Map();
  for (const c of conflicts.value) {
    for (const idx of c.owners) {
      counts.set(idx, (counts.get(idx) || 0) + 1);
    }
  }
  return counts;
});

const canMerge = computed(() => mods.value.length >= 2 && !busy.value);

function setStatus(text, kind = '') {
  status.value = { text, kind };
}

function defaultOutputPath(firstInputPath) {
  if (!firstInputPath) return '';
  const sepIdx = Math.max(firstInputPath.lastIndexOf('/'), firstInputPath.lastIndexOf('\\'));
  if (sepIdx < 0) return 'combined_dir.vpk';
  const dir = firstInputPath.substring(0, sepIdx);
  const sep = firstInputPath.charAt(sepIdx);
  return `${dir}${sep}combined_dir.vpk`;
}

async function loadPaths(paths) {
  const vpks = paths.filter((p) => p.toLowerCase().endsWith('.vpk'));
  if (vpks.length === 0) {
    setStatus('No .vpk files in that drop', 'error');
    return;
  }
  for (const path of vpks) {
    if (mods.value.some((m) => m.path === path)) continue;
    try {
      const mod = await invoke('add_mod', { path });
      mods.value.push(mod);
    } catch (e) {
      setStatus(`Failed to load ${path}: ${e}`, 'error');
    }
  }
  if (mods.value.length > 0 && !outputPath.value) {
    outputPath.value = defaultOutputPath(mods.value[0].path);
  }
}

async function pickViaDialog() {
  setStatus('');
  let paths;
  try {
    paths = await invoke('pick_vpk_files');
  } catch (e) {
    setStatus(`Picker failed: ${e}`, 'error');
    return;
  }
  if (!paths?.length) return;
  await loadPaths(paths);
}

async function browseOutput() {
  setStatus('');
  try {
    const path = await invoke('pick_output_path');
    if (path) outputPath.value = path;
  } catch (e) {
    setStatus(`Picker failed: ${e}`, 'error');
  }
}

async function doMerge() {
  if (!outputPath.value) {
    setStatus('Add at least one VPK first', 'error');
    return;
  }
  busy.value = true;
  setStatus('Merging...');
  try {
    const report = await invoke('merge_vpks', {
      orderedPaths: mods.value.map((m) => m.path),
      outputPath: outputPath.value,
    });
    lastReport.value = report;
    showMergedModal.value = true;
    setStatus(`Wrote ${report.total_entries} entries`, 'success');
  } catch (e) {
    setStatus(`Merge failed: ${e}`, 'error');
  } finally {
    busy.value = false;
  }
}

function removeMod(idx) {
  mods.value.splice(idx, 1);
  if (mods.value.length === 0) outputPath.value = '';
}

function clearAll() {
  mods.value = [];
  outputPath.value = '';
  setStatus('');
}

async function revealOutput() {
  if (!lastReport.value?.output_path) return;
  try {
    await invoke('reveal_in_folder', { path: lastReport.value.output_path });
  } catch (e) {
    setStatus(`Could not open folder: ${e}`, 'error');
  }
}

const dragSrcIdx = ref(null);
const dragOverIdx = ref(null);
function onDragStart(idx, e) {
  dragSrcIdx.value = idx;
  e.dataTransfer.effectAllowed = 'move';
}
function onDragOver(idx, e) {
  e.preventDefault();
  e.dataTransfer.dropEffect = 'move';
  if (dragOverIdx.value !== idx) dragOverIdx.value = idx;
}
function onDrop(idx) {
  if (dragSrcIdx.value !== null && dragSrcIdx.value !== idx) {
    const [moved] = mods.value.splice(dragSrcIdx.value, 1);
    mods.value.splice(idx, 0, moved);
  }
  dragSrcIdx.value = null;
  dragOverIdx.value = null;
}
function onDragEnd() {
  dragSrcIdx.value = null;
  dragOverIdx.value = null;
}

const isMaximized = ref(false);
async function refreshMaximized() {
  try { isMaximized.value = await getCurrentWindow().isMaximized(); } catch { /* noop */ }
}
async function winMinimize() { await getCurrentWindow().minimize(); }
async function winToggleMax() {
  const w = getCurrentWindow();
  if (await w.isMaximized()) await w.unmaximize();
  else await w.maximize();
  await refreshMaximized();
}
async function winClose() { await getCurrentWindow().close(); }

let outputCheckTimer = null;
watch(outputPath, (p) => {
  if (outputCheckTimer) clearTimeout(outputCheckTimer);
  if (!p) { outputExists.value = false; return; }
  outputCheckTimer = setTimeout(async () => {
    try { outputExists.value = await invoke('path_exists', { path: p }); }
    catch { outputExists.value = false; }
  }, 200);
});

watch(() => mods.value.length, async (n) => {
  const title = n === 0 ? 'vpkmerge' : `vpkmerge - ${n} mod${n === 1 ? '' : 's'} loaded`;
  try { await getCurrentWindow().setTitle(title); } catch { /* noop */ }
});

function onWindowKeydown(e) {
  if (e.key !== 'Escape') return;
  if (showMergedModal.value) { showMergedModal.value = false; e.stopPropagation(); }
  else if (showConflictsModal.value) { showConflictsModal.value = false; e.stopPropagation(); }
}

watch([showConflictsModal, showMergedModal], async ([cv, mv], [pcv, pmv]) => {
  const anyOpen = cv || mv;
  const wasOpen = pcv || pmv;
  if (anyOpen && !wasOpen) {
    lastFocused.value = document.activeElement;
    window.addEventListener('keydown', onWindowKeydown);
    await nextTick();
    const target = mv ? mergedModalRef.value : conflictsModalRef.value;
    target?.focus?.();
  } else if (!anyOpen && wasOpen) {
    window.removeEventListener('keydown', onWindowKeydown);
    const prior = lastFocused.value;
    lastFocused.value = null;
    if (prior && typeof prior.focus === 'function') prior.focus();
  }
});

let unlistenResize = null;
let unlistenDragDrop = null;
onMounted(async () => {
  await refreshMaximized();
  unlistenResize = await getCurrentWindow().onResized(() => refreshMaximized());
  unlistenDragDrop = await getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === 'enter' || event.payload.type === 'over') {
      isDragging.value = true;
    } else if (event.payload.type === 'leave') {
      isDragging.value = false;
    } else if (event.payload.type === 'drop') {
      isDragging.value = false;
      loadPaths(event.payload.paths || []);
    }
  });
});
onBeforeUnmount(() => {
  if (unlistenDragDrop) unlistenDragDrop();
  if (unlistenResize) unlistenResize();
  window.removeEventListener('keydown', onWindowKeydown);
  if (outputCheckTimer) clearTimeout(outputCheckTimer);
});
</script>

<template>
  <div class="w-screen h-screen flex flex-col select-none cursor-default bg-surface-0 dark:bg-surface-950 text-ink-800 dark:text-ink-100">
    <div class="flex flex-col w-full h-full bg-paper overflow-hidden">

      <!-- Custom title bar -->
      <div
        data-tauri-drag-region
        class="select-none flex items-center justify-between h-9 px-3 text-ink-700 dark:text-ink-100 relative z-10"
      >
        <div data-tauri-drag-region class="flex items-center gap-2 text-xs font-serif tracking-wide pointer-events-none">
          <div class="w-2 h-2 rounded-full bg-accent-600 dark:bg-accent-300" />
          <span>vpkmerge</span>
        </div>
        <div class="flex items-center gap-1">
          <button
            type="button"
            class="w-7 h-7 rounded-md inline-flex items-center justify-center hover:bg-accent-700/10 dark:hover:bg-accent-300/10 transition active:scale-95 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-700/60 dark:focus-visible:ring-accent-300/60 focus-visible:ring-inset"
            @click="winMinimize"
            aria-label="Minimize"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor"><rect x="2" y="5.5" width="8" height="1" /></svg>
          </button>
          <button
            type="button"
            class="w-7 h-7 rounded-md inline-flex items-center justify-center hover:bg-accent-700/10 dark:hover:bg-accent-300/10 transition active:scale-95 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-700/60 dark:focus-visible:ring-accent-300/60 focus-visible:ring-inset"
            @click="winToggleMax"
            :aria-label="isMaximized ? 'Restore' : 'Maximize'"
          >
            <svg v-if="!isMaximized" width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1">
              <rect x="2.5" y="2.5" width="7" height="7" />
            </svg>
            <svg v-else width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1">
              <rect x="3.5" y="3.5" width="6" height="6" />
              <path d="M5 3.5 V2 H10 V7 H8.5" />
            </svg>
          </button>
          <button
            type="button"
            class="w-7 h-7 rounded-md inline-flex items-center justify-center hover:bg-red-700 hover:text-ink-50 transition active:scale-95 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red-600 focus-visible:ring-inset"
            @click="winClose"
            aria-label="Close"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2">
              <path d="M2.5 2.5 L9.5 9.5 M9.5 2.5 L2.5 9.5" />
            </svg>
          </button>
        </div>
      </div>

      <!-- Content -->
      <div class="doodle-overlay flex-1 min-h-0 overflow-y-auto">
        <div class="min-h-full flex flex-col p-3 sm:p-4 md:p-8 pb-24">

          <!-- Empty state -->
          <div v-if="mods.length === 0" class="flex-1 w-full flex items-center justify-center px-2 py-4">
            <button
              type="button"
              @click="pickViaDialog"
              class="empty-state paper-card paper-card-pressable w-full max-w-xl flex flex-col items-center justify-center gap-y-3 py-10 px-6 rounded-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-700/45 dark:focus-visible:ring-accent-300/45"
              :class="{ '!border-accent-300 !bg-accent-300/5': isDragging }"
            >
              <div class="relative aspect-square w-[clamp(5rem,24vh,12rem)] flex items-center justify-center pointer-events-none">
                <svg
                  viewBox="-110 -110 220 220"
                  class="absolute inset-0 w-full h-full overflow-visible text-accent-700/45 dark:text-accent-300/40"
                  aria-hidden="true"
                >
                  <circle cx="0" cy="0" r="92" fill="none" stroke="currentColor" stroke-width="1" stroke-dasharray="2.4 4.8" />
                  <circle cx="0" cy="0" r="76" fill="none" stroke="currentColor" stroke-width="0.6" />
                </svg>
                <span class="text-5xl text-accent-700 dark:text-accent-300 font-serif leading-none">⤓</span>
              </div>
              <h3 class="font-serif text-xl sm:text-2xl tracking-wide text-ink-800 dark:text-ink-100 text-center">
                {{ isDragging ? 'Drop to add' : 'Ready to merge' }}
              </h3>
              <p class="text-xs sm:text-sm font-serif italic text-ink-500 dark:text-ink-300 text-center">
                Drop VPK files here or click to browse
              </p>
            </button>
          </div>

          <!-- Non-empty: add control, mod list, output -->
          <div v-else class="w-full max-w-2xl mx-auto space-y-5">

            <button
              type="button"
              @click="pickViaDialog"
              class="paper-card paper-card-pressable w-full py-3 px-4 rounded-md flex items-center justify-center gap-2 text-sm font-serif italic border-accent-700/30 dark:border-accent-300/30 text-accent-700 dark:text-accent-300 hover:!border-accent-700 dark:hover:!border-accent-300"
              :class="{ '!border-accent-300 !bg-accent-300/10': isDragging }"
            >
              <span class="text-base leading-none">+</span>
              {{ isDragging ? 'Drop to add more' : 'Add more VPKs' }}
            </button>

            <div class="flex items-center justify-between px-1">
              <span class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
                {{ mods.length }} {{ mods.length === 1 ? 'mod' : 'mods' }}
              </span>
              <button
                type="button"
                @click="clearAll"
                class="text-xs italic font-serif text-ink-500 dark:text-ink-300 hover:text-red-600 dark:hover:text-red-400 focus-visible:outline-none focus-visible:underline rounded"
              >Clear all</button>
            </div>

            <div class="paper-card rounded-md p-1">
              <ul class="flex flex-col">
                <li
                  v-for="(mod, idx) in mods"
                  :key="mod.path"
                  draggable="true"
                  class="group flex items-center gap-3 px-3 py-2.5 rounded-sm cursor-grab select-none transition-colors hover:bg-surface-100/60 dark:hover:bg-surface-800/60 border-t-2 border-transparent"
                  :class="{
                    'opacity-40': dragSrcIdx === idx,
                    '!border-accent-500 dark:!border-accent-300': dragOverIdx === idx && dragSrcIdx !== null && dragSrcIdx !== idx,
                  }"
                  @dragstart="onDragStart(idx, $event)"
                  @dragover="onDragOver(idx, $event)"
                  @drop="onDrop(idx)"
                  @dragend="onDragEnd"
                >
                  <span class="text-ink-500 dark:text-ink-300 text-lg leading-none opacity-50 group-hover:opacity-100">≡</span>
                  <span class="text-ink-500 dark:text-ink-300 text-xs tabular-nums w-5 text-right font-mono">{{ idx + 1 }}</span>
                  <span class="flex-1 font-serif text-base text-ink-800 dark:text-ink-100 truncate" :title="mod.path">
                    {{ mod.name }}
                  </span>
                  <span
                    v-if="conflictsByMod.get(idx)"
                    class="text-[10px] tracking-wide px-1.5 py-0.5 rounded-full bg-accent-600/15 dark:bg-accent-300/15 text-accent-700 dark:text-accent-300 font-medium tabular-nums whitespace-nowrap"
                    :title="`${conflictsByMod.get(idx)} path${conflictsByMod.get(idx) === 1 ? '' : 's'} conflict with another mod`"
                  >{{ conflictsByMod.get(idx) }} conflict{{ conflictsByMod.get(idx) === 1 ? '' : 's' }}</span>
                  <span class="text-ink-500 dark:text-ink-300 text-xs tabular-nums italic font-serif">{{ mod.file_count }} files</span>
                  <button
                    class="text-ink-500 dark:text-ink-300 hover:text-red-600 dark:hover:text-red-400 text-lg leading-none px-2 py-0.5 rounded opacity-50 group-hover:opacity-100 focus-visible:outline-none focus-visible:opacity-100 focus-visible:ring-1 focus-visible:ring-accent-700/45 dark:focus-visible:ring-accent-300/45"
                    title="Remove"
                    @click.stop="removeMod(idx)"
                  >×</button>
                </li>
              </ul>
            </div>

            <p class="text-xs font-serif italic text-ink-500 dark:text-ink-300 text-center">
              Drag to reorder. Mods lower in the list win on conflict.
            </p>

            <!-- Output -->
            <div class="paper-card rounded-md p-4">
              <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium mb-2">
                Output
              </h4>
              <div class="flex gap-2 items-center">
                <input
                  type="text"
                  v-model="outputPath"
                  spellcheck="false"
                  placeholder="Auto-set from the first VPK added..."
                  class="flex-1 bg-transparent border border-surface-300 dark:border-surface-700 rounded-md px-3 py-2 text-xs font-mono text-ink-800 dark:text-ink-100 placeholder:italic placeholder:font-serif placeholder:text-ink-500 dark:placeholder:text-ink-300 focus:outline-none focus:border-accent-500"
                />
                <button class="btn" @click="browseOutput">Browse</button>
              </div>
              <p
                v-if="outputExists"
                class="text-xs font-serif italic text-accent-700 dark:text-accent-300 mt-2"
              >Will overwrite the existing file at this path.</p>
            </div>
          </div>
        </div>
      </div>

      <!-- Bottom bar -->
      <footer class="border-t border-surface-200 dark:border-surface-800 px-4 sm:px-6 py-3 flex items-center justify-between gap-4">
        <div class="flex items-center gap-3 min-w-0 flex-1">
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
            class="text-xs font-serif italic truncate"
            :class="{
              'text-ink-500 dark:text-ink-300': !status.kind,
              'text-green-700 dark:text-green-400 not-italic font-sans': status.kind === 'success',
              'text-red-700 dark:text-red-400 not-italic font-sans': status.kind === 'error',
            }"
            aria-live="polite"
          >
            {{ status.text || (mods.length ? 'Ready when you are' : 'Drop a VPK to start') }}
          </span>
          <button
            v-if="conflicts.length"
            @click="showConflictsModal = true"
            class="text-xs flex items-center gap-1.5 shrink-0 text-accent-700 dark:text-accent-300 hover:underline italic font-serif focus-visible:outline-none focus-visible:underline rounded"
          >
            <span class="bg-accent-600 text-surface-0 font-bold rounded-full px-2 py-0.5 tracking-wider text-[10px] not-italic font-sans">{{ conflicts.length }}</span>
            view conflicts
          </button>
        </div>
        <button
          :disabled="!canMerge"
          class="btn bg-accent-600 hover:!bg-accent-700 text-surface-0 px-6 py-2 disabled:opacity-40 disabled:cursor-not-allowed shrink-0 font-medium"
          @click="doMerge"
        >Merge VPKs</button>
      </footer>
    </div>

    <!-- Conflicts modal -->
    <Transition name="fx-rise">
      <div
        v-if="showConflictsModal"
        class="fixed inset-0 z-50 bg-black/50 dark:bg-black/70 flex items-center justify-center p-6"
        @click.self="showConflictsModal = false"
      >
        <div
          ref="conflictsModalRef"
          tabindex="-1"
          role="dialog"
          aria-modal="true"
          aria-labelledby="conflicts-modal-title"
          class="paper-card rounded-md w-full max-w-2xl max-h-[80vh] flex flex-col focus:outline-none"
        >
          <header class="flex items-center justify-between px-5 py-4 border-b border-surface-200 dark:border-surface-800">
            <div>
              <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
                Path conflicts
              </h4>
              <h2 id="conflicts-modal-title" class="font-serif text-xl sm:text-2xl text-ink-800 dark:text-ink-100">
                {{ conflicts.length }} {{ conflicts.length === 1 ? 'collision' : 'collisions' }}
              </h2>
            </div>
            <button
              class="text-ink-500 dark:text-ink-300 hover:text-ink-800 dark:hover:text-ink-100 text-2xl leading-none px-2"
              @click="showConflictsModal = false"
            >×</button>
          </header>
          <div class="overflow-y-auto p-5 space-y-3">
            <div
              v-for="c in conflicts"
              :key="c.path"
              class="border border-surface-200 dark:border-surface-800 rounded-md px-3 py-2.5"
            >
              <div class="font-mono text-xs text-ink-800 dark:text-ink-100 break-all mb-1.5">{{ c.path }}</div>
              <div class="flex flex-col gap-0.5">
                <div
                  v-for="idx in c.owners"
                  :key="idx"
                  class="text-xs flex items-center gap-2 font-serif"
                  :class="idx === c.winner
                    ? 'text-accent-700 dark:text-accent-300 font-semibold'
                    : 'text-ink-500 dark:text-ink-300 line-through italic'"
                >
                  <span v-if="idx === c.winner" class="text-accent-700 dark:text-accent-300">✓</span>
                  <span v-else class="text-ink-500 dark:text-ink-300">·</span>
                  {{ mods[idx].name }}
                  <span class="text-ink-500 dark:text-ink-300 not-italic">({{ idx === c.winner ? 'wins' : 'overridden' }})</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>

    <!-- Merged success modal -->
    <Transition name="fx-rise">
      <div
        v-if="showMergedModal && lastReport"
        class="fixed inset-0 z-50 bg-black/50 dark:bg-black/70 flex items-center justify-center p-6"
        @click.self="showMergedModal = false"
      >
        <div
          ref="mergedModalRef"
          tabindex="-1"
          role="dialog"
          aria-modal="true"
          aria-labelledby="merged-modal-title"
          class="paper-card rounded-md w-full max-w-md flex flex-col focus:outline-none"
        >
          <header class="px-5 py-4 border-b border-surface-200 dark:border-surface-800">
            <h4 class="text-[10px] uppercase tracking-[0.18em] text-ink-500 dark:text-ink-300 font-medium">
              Merged
            </h4>
            <h2 id="merged-modal-title" class="font-serif text-xl sm:text-2xl text-ink-800 dark:text-ink-100">
              {{ lastReport.total_entries }} {{ lastReport.total_entries === 1 ? 'entry' : 'entries' }}
            </h2>
          </header>

          <div class="px-5 py-4 space-y-3">
            <p class="text-xs font-serif italic text-ink-500 dark:text-ink-300">
              Wrote to
            </p>
            <code class="block font-mono text-xs text-ink-800 dark:text-ink-100 bg-surface-100 dark:bg-surface-800/60 rounded-sm px-2 py-1.5 break-all border border-surface-200 dark:border-surface-800">
              {{ lastReport.output_path }}
            </code>
            <p class="text-xs font-serif italic text-ink-500 dark:text-ink-300">
              From {{ lastReport.inputs }} {{ lastReport.inputs === 1 ? 'input' : 'inputs' }}<span v-if="lastReport.overridden">, {{ lastReport.overridden }} {{ lastReport.overridden === 1 ? 'path' : 'paths' }} overridden</span>.
            </p>
          </div>

          <footer class="px-5 py-4 border-t border-surface-200 dark:border-surface-800 flex items-center justify-end gap-2">
            <button class="btn" @click="showMergedModal = false">Close</button>
            <button
              class="btn bg-accent-600 hover:!bg-accent-700 text-surface-0 font-medium"
              @click="revealOutput"
            >Open folder</button>
          </footer>
        </div>
      </div>
    </Transition>
  </div>
</template>
