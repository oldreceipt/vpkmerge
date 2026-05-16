<script setup>
import { ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';

const mods = ref([]);
const outputPath = ref('');
const status = ref({ text: '', kind: '' });
const busy = ref(false);

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

const canMerge = computed(() => mods.value.length >= 2 && !!outputPath.value && !busy.value);

function setStatus(text, kind = '') {
  status.value = { text, kind };
}

async function addMod() {
  setStatus('');
  let paths;
  try {
    paths = await invoke('pick_vpk_files');
  } catch (e) {
    setStatus(`Picker failed: ${e}`, 'error');
    return;
  }
  if (!paths?.length) return;
  for (const path of paths) {
    if (mods.value.some((m) => m.path === path)) continue;
    try {
      const mod = await invoke('add_mod', { path });
      mods.value.push(mod);
    } catch (e) {
      setStatus(`Failed to load ${path}: ${e}`, 'error');
    }
  }
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
  busy.value = true;
  setStatus('Merging...');
  try {
    const report = await invoke('merge_vpks', {
      orderedPaths: mods.value.map((m) => m.path),
      outputPath: outputPath.value,
    });
    setStatus(
      `Done. Wrote ${report.total_entries} entries (${report.overridden} overridden) to ${report.output_path}`,
      'success'
    );
  } catch (e) {
    setStatus(`Merge failed: ${e}`, 'error');
  } finally {
    busy.value = false;
  }
}

function removeMod(idx) {
  mods.value.splice(idx, 1);
}

const dragSrcIdx = ref(null);
function onDragStart(idx, e) {
  dragSrcIdx.value = idx;
  e.dataTransfer.effectAllowed = 'move';
}
function onDragOver(e) {
  e.preventDefault();
  e.dataTransfer.dropEffect = 'move';
}
function onDrop(idx) {
  if (dragSrcIdx.value === null || dragSrcIdx.value === idx) return;
  const [moved] = mods.value.splice(dragSrcIdx.value, 1);
  mods.value.splice(idx, 0, moved);
  dragSrcIdx.value = null;
}
function onDragEnd() {
  dragSrcIdx.value = null;
}
</script>

<template>
  <main class="bg-paper min-h-screen text-ink-700">
    <div class="max-w-3xl mx-auto px-6 pt-8 pb-32 space-y-5">
      <header>
        <h1 class="text-3xl font-bold text-accent-700 tracking-tight">vpkmerge</h1>
        <p class="text-ink-500 mt-1 text-sm">
          Combine Deadlock mod VPKs to bypass the ~100 mount limit.
        </p>
      </header>

      <!-- Mods -->
      <section class="paper-card rounded-xl p-5">
        <div class="flex items-center justify-between mb-2">
          <h2 class="text-xs font-semibold uppercase tracking-wider text-ink-500">Mods</h2>
          <button class="btn bg-accent-600 hover:!bg-accent-700 text-surface-0" @click="addMod">
            + Add VPK
          </button>
        </div>
        <p class="text-xs text-ink-500 mb-3">
          Drag rows to reorder. Mods lower in the list win on conflict.
        </p>

        <ul v-if="mods.length" class="flex flex-col gap-2">
          <li
            v-for="(mod, idx) in mods"
            :key="mod.path"
            draggable="true"
            class="paper-card-pressable flex items-center gap-3 px-3 py-2.5 rounded-lg border border-surface-300/70 bg-surface-0/40 cursor-grab select-none hover:border-accent-500"
            :class="{ 'opacity-40': dragSrcIdx === idx }"
            @dragstart="onDragStart(idx, $event)"
            @dragover="onDragOver"
            @drop="onDrop(idx)"
            @dragend="onDragEnd"
          >
            <span class="text-ink-300 text-lg leading-none">≡</span>
            <span class="text-ink-500 text-xs tabular-nums w-6 text-right">{{ idx + 1 }}.</span>
            <span class="flex-1 font-medium text-ink-700 truncate" :title="mod.path">{{ mod.name }}</span>
            <span class="text-ink-500 text-xs tabular-nums">{{ mod.file_count }} files</span>
            <button
              class="text-ink-300 hover:text-red-700 text-lg leading-none px-2 py-1 rounded"
              title="Remove"
              @click.stop="removeMod(idx)"
            >×</button>
          </li>
        </ul>
        <p v-else class="text-center text-ink-500 text-sm py-4">No mods added yet.</p>
      </section>

      <!-- Conflicts -->
      <section class="paper-card rounded-xl p-5">
        <div class="flex items-center gap-2 mb-3">
          <h2 class="text-xs font-semibold uppercase tracking-wider text-ink-500">Conflicts</h2>
          <span
            v-if="conflicts.length"
            class="bg-accent-600 text-surface-0 text-[10px] font-bold rounded-full px-2 py-0.5 tracking-wider"
          >{{ conflicts.length }}</span>
        </div>

        <ul v-if="conflicts.length" class="flex flex-col gap-2 max-h-96 overflow-auto">
          <li
            v-for="c in conflicts"
            :key="c.path"
            class="border border-surface-300/70 bg-surface-0/40 rounded-lg px-3 py-2.5"
          >
            <div class="font-mono text-xs text-ink-700 break-all mb-1.5">{{ c.path }}</div>
            <div class="flex flex-col gap-0.5">
              <div
                v-for="idx in c.owners"
                :key="idx"
                class="text-xs flex items-center gap-2"
                :class="idx === c.winner
                  ? 'text-accent-700 font-semibold'
                  : 'text-ink-500 line-through'"
              >
                <span class="text-accent-600" v-if="idx === c.winner">✓</span>
                <span class="text-ink-300" v-else>·</span>
                {{ mods[idx].name }}
                <span class="text-ink-300 not-italic">({{ idx === c.winner ? 'wins' : 'overridden' }})</span>
              </div>
            </div>
          </li>
        </ul>
        <p v-else class="text-center text-ink-500 text-sm py-4">No conflicts.</p>
      </section>

      <!-- Output -->
      <section class="paper-card rounded-xl p-5">
        <h2 class="text-xs font-semibold uppercase tracking-wider text-ink-500 mb-3">Output</h2>
        <div class="flex gap-2">
          <input
            type="text"
            readonly
            :value="outputPath"
            placeholder="Choose output VPK file..."
            class="flex-1 bg-surface-0/60 border border-surface-300/70 rounded-md px-3 py-2 text-sm text-ink-700 focus:outline-none focus:border-accent-500"
          />
          <button class="btn" @click="browseOutput">Browse</button>
        </div>
      </section>
    </div>

    <footer
      class="fixed bottom-0 left-0 right-0 bg-paper border-t border-surface-300/70 px-6 py-3 flex items-center justify-between"
    >
      <div
        class="text-sm"
        :class="{
          'text-ink-500': !status.kind,
          'text-green-700': status.kind === 'success',
          'text-red-700': status.kind === 'error',
        }"
      >{{ status.text }}</div>
      <button
        :disabled="!canMerge"
        class="btn bg-accent-600 hover:!bg-accent-700 text-surface-0 px-6 disabled:opacity-40 disabled:cursor-not-allowed"
        @click="doMerge"
      >Merge VPKs</button>
    </footer>
  </main>
</template>
