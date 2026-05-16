const { invoke } = window.__TAURI__.core;

const state = {
  mods: [],
  outputPath: "",
};

const $ = (id) => document.getElementById(id);
const modList = $("mod-list");
const modEmpty = $("mod-empty");
const conflictList = $("conflict-list");
const conflictEmpty = $("conflict-empty");
const conflictCount = $("conflict-count");
const outputInput = $("output-path");
const mergeBtn = $("merge");
const statusEl = $("status");

$("add-mod").addEventListener("click", onAddMod);
$("browse-output").addEventListener("click", onBrowseOutput);
mergeBtn.addEventListener("click", onMerge);

async function onAddMod() {
  setStatus("");
  let paths;
  try {
    paths = await invoke("pick_vpk_files");
  } catch (e) {
    setStatus(`Picker failed: ${e}`, "error");
    return;
  }
  if (!paths || paths.length === 0) return;
  for (const path of paths) {
    if (state.mods.some((m) => m.path === path)) continue;
    try {
      const mod = await invoke("add_mod", { path });
      state.mods.push(mod);
    } catch (e) {
      setStatus(`Failed to load ${path}: ${e}`, "error");
    }
  }
  render();
}

async function onBrowseOutput() {
  setStatus("");
  try {
    const path = await invoke("pick_output_path");
    if (path) {
      state.outputPath = path;
      outputInput.value = path;
      updateMergeButton();
    }
  } catch (e) {
    setStatus(`Picker failed: ${e}`, "error");
  }
}

async function onMerge() {
  setStatus("Merging...");
  mergeBtn.disabled = true;
  try {
    const report = await invoke("merge_vpks", {
      orderedPaths: state.mods.map((m) => m.path),
      outputPath: state.outputPath,
    });
    setStatus(
      `Done. Wrote ${report.total_entries} entries (${report.overridden} overridden) to ${report.output_path}`,
      "success"
    );
  } catch (e) {
    setStatus(`Merge failed: ${e}`, "error");
  } finally {
    updateMergeButton();
  }
}

function removeMod(idx) {
  state.mods.splice(idx, 1);
  render();
}

function render() {
  renderMods();
  renderConflicts();
  updateMergeButton();
}

function renderMods() {
  modList.innerHTML = "";
  if (state.mods.length === 0) {
    modEmpty.classList.remove("hidden");
    return;
  }
  modEmpty.classList.add("hidden");
  state.mods.forEach((mod, idx) => {
    const li = document.createElement("li");
    li.className = "mod-row";
    li.draggable = true;
    li.dataset.idx = idx;
    li.innerHTML = `
      <span class="grip">≡</span>
      <span class="priority">${idx + 1}.</span>
      <span class="name" title="${escapeAttr(mod.path)}">${escapeHtml(mod.name)}</span>
      <span class="meta">${mod.file_count} files</span>
      <button class="remove" title="Remove">×</button>
    `;
    li.querySelector(".remove").addEventListener("click", (e) => {
      e.stopPropagation();
      removeMod(idx);
    });
    attachDragHandlers(li, idx);
    modList.appendChild(li);
  });
}

function renderConflicts() {
  const conflicts = computeConflicts();
  conflictList.innerHTML = "";
  if (conflicts.length === 0) {
    conflictEmpty.classList.remove("hidden");
    conflictCount.classList.add("hidden");
    return;
  }
  conflictEmpty.classList.add("hidden");
  conflictCount.classList.remove("hidden");
  conflictCount.textContent = conflicts.length;

  for (const c of conflicts) {
    const li = document.createElement("li");
    li.className = "conflict";
    const ownersHtml = c.owners
      .map((idx) => {
        const winner = idx === c.winner;
        const cls = winner ? "winner" : "loser";
        const label = winner ? "wins" : "overridden";
        return `<span class="owner ${cls}">${escapeHtml(state.mods[idx].name)} <small>(${label})</small></span>`;
      })
      .join("");
    li.innerHTML = `
      <div class="path">${escapeHtml(c.path)}</div>
      <div class="owners">${ownersHtml}</div>
    `;
    conflictList.appendChild(li);
  }
}

function computeConflicts() {
  const owners = new Map();
  state.mods.forEach((mod, idx) => {
    for (const p of mod.file_paths) {
      if (!owners.has(p)) owners.set(p, []);
      owners.get(p).push(idx);
    }
  });
  const conflicts = [];
  for (const [path, idxs] of owners) {
    if (idxs.length > 1) {
      conflicts.push({ path, winner: idxs[idxs.length - 1], owners: idxs });
    }
  }
  conflicts.sort((a, b) => a.path.localeCompare(b.path));
  return conflicts;
}

function updateMergeButton() {
  mergeBtn.disabled =
    state.mods.length < 2 || !state.outputPath;
}

function setStatus(text, kind = "") {
  statusEl.textContent = text;
  statusEl.className = "status" + (kind ? ` ${kind}` : "");
}

let dragSrcIdx = null;
function attachDragHandlers(el, idx) {
  el.addEventListener("dragstart", (e) => {
    dragSrcIdx = idx;
    el.classList.add("dragging");
    e.dataTransfer.effectAllowed = "move";
  });
  el.addEventListener("dragend", () => {
    el.classList.remove("dragging");
    document.querySelectorAll(".mod-row.drop-target").forEach((r) => r.classList.remove("drop-target"));
    dragSrcIdx = null;
  });
  el.addEventListener("dragover", (e) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    el.classList.add("drop-target");
  });
  el.addEventListener("dragleave", () => el.classList.remove("drop-target"));
  el.addEventListener("drop", (e) => {
    e.preventDefault();
    el.classList.remove("drop-target");
    if (dragSrcIdx === null || dragSrcIdx === idx) return;
    const [moved] = state.mods.splice(dragSrcIdx, 1);
    state.mods.splice(idx, 0, moved);
    render();
  });
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({"&":"&amp;","<":"&lt;",">":"&gt;","\"":"&quot;","'":"&#39;"}[c]));
}
function escapeAttr(s) { return escapeHtml(s); }

render();
