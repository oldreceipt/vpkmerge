use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
struct ModInfo {
    path: String,
    name: String,
    file_count: usize,
    file_paths: Vec<String>,
}

#[derive(Serialize)]
struct MergeReport {
    total_entries: usize,
    overridden: usize,
    inputs: usize,
    output_path: String,
}

#[tauri::command]
async fn pick_vpk_files(app: AppHandle) -> Vec<String> {
    app.dialog()
        .file()
        .add_filter("VPK files", &["vpk"])
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[tauri::command]
async fn pick_output_path(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("VPK file", &["vpk"])
        .set_file_name("merged_dir.vpk")
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
async fn add_mod(path: String) -> Result<ModInfo, String> {
    let vpk = valve_pak::open(&path).map_err(|e| format!("{e:#}"))?;
    let name = Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let file_paths: Vec<String> = vpk.file_paths().cloned().collect();
    let file_count = file_paths.len();
    Ok(ModInfo { path, name, file_count, file_paths })
}

#[tauri::command]
async fn merge_vpks(ordered_paths: Vec<String>, output_path: String) -> Result<MergeReport, String> {
    if ordered_paths.len() < 2 {
        return Err("Need at least 2 input VPKs to merge".into());
    }

    let vpks: Vec<_> = ordered_paths
        .iter()
        .map(|p| valve_pak::open(p).map_err(|e| format!("Failed to open {p}: {e:#}")))
        .collect::<Result<Vec<_>, _>>()?;

    // Last input wins on collision
    let mut winner: HashMap<String, usize> = HashMap::new();
    let mut collisions = 0usize;
    for (idx, vpk) in vpks.iter().enumerate() {
        for path in vpk.file_paths() {
            if winner.insert(path.clone(), idx).is_some() {
                collisions += 1;
            }
        }
    }

    let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
    for (path, idx) in &winner {
        let mut vf = vpks[*idx]
            .get_file(path)
            .map_err(|e| format!("get_file {path}: {e:#}"))?;
        let bytes = vf
            .read_all()
            .map_err(|e| format!("read_all {path}: {e:#}"))?;
        let dst = tmp.path().join(path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }
        std::fs::write(&dst, &bytes).map_err(|e| format!("write {dst:?}: {e}"))?;
    }

    let merged = valve_pak::from_directory(tmp.path())
        .map_err(|e| format!("from_directory: {e:#}"))?;
    merged
        .save(&output_path)
        .map_err(|e| format!("save: {e:#}"))?;

    Ok(MergeReport {
        total_entries: winner.len(),
        overridden: collisions,
        inputs: ordered_paths.len(),
        output_path,
    })
}

#[tauri::command]
async fn reveal_in_folder(path: String) -> Result<(), String> {
    use std::process::Command;
    let result = if cfg!(target_os = "linux") {
        let p = std::path::Path::new(&path);
        let target = if p.is_file() { p.parent().unwrap_or(p) } else { p };
        Command::new("xdg-open").arg(target).spawn()
    } else if cfg!(target_os = "windows") {
        Command::new("explorer").arg(format!("/select,{}", path)).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").args(["-R", &path]).spawn()
    } else {
        return Err("Unsupported OS".into());
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            pick_vpk_files,
            pick_output_path,
            add_mod,
            merge_vpks,
            reveal_in_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
