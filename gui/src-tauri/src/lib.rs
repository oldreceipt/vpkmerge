use serde::Serialize;
use std::collections::HashMap;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
struct ModInfo {
    path: String,
    name: String,
    file_count: usize,
    size_bytes: u64,
    file_paths: Vec<String>,
}

impl From<vpkmerge_core::ModInfo> for ModInfo {
    fn from(m: vpkmerge_core::ModInfo) -> Self {
        ModInfo {
            path: m.path.to_string_lossy().into_owned(),
            name: m.name,
            file_count: m.file_count,
            size_bytes: m.size_bytes,
            file_paths: m.file_paths,
        }
    }
}

#[derive(Serialize)]
struct Conflict {
    path: String,
    owner_indices: Vec<usize>,
}

impl From<vpkmerge_core::Conflict> for Conflict {
    fn from(c: vpkmerge_core::Conflict) -> Self {
        Conflict {
            path: c.path,
            owner_indices: c.owner_indices,
        }
    }
}

#[derive(Serialize)]
struct MergeReport {
    total_entries: usize,
    overridden: usize,
    inputs: usize,
    output_path: String,
}

impl From<vpkmerge_core::MergeReport> for MergeReport {
    fn from(r: vpkmerge_core::MergeReport) -> Self {
        MergeReport {
            total_entries: r.total_entries,
            overridden: r.overridden_paths,
            inputs: r.inputs,
            output_path: r.output_path.to_string_lossy().into_owned(),
        }
    }
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
    vpkmerge_core::inspect(&path)
        .map(Into::into)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn detect_conflicts(ordered_paths: Vec<String>) -> Result<Vec<Conflict>, String> {
    vpkmerge_core::detect_conflicts(&ordered_paths)
        .map(|cs| cs.into_iter().map(Into::into).collect())
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn merge_vpks(
    ordered_paths: Vec<String>,
    output_path: String,
    policy: Option<String>,
    overrides: Option<HashMap<String, usize>>,
) -> Result<MergeReport, String> {
    let collision_policy = match policy.as_deref() {
        None | Some("last_wins") => vpkmerge_core::CollisionPolicy::LastWins,
        Some("first_wins") => vpkmerge_core::CollisionPolicy::FirstWins,
        Some("strict") => vpkmerge_core::CollisionPolicy::Error,
        Some(other) => return Err(format!("unknown policy: {other}")),
    };
    vpkmerge_core::merge(
        &ordered_paths,
        &output_path,
        &vpkmerge_core::MergeOptions {
            collision_policy,
            overrides: overrides.unwrap_or_default(),
        },
    )
    .map(Into::into)
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

#[tauri::command]
async fn reveal_in_folder(path: String) -> Result<(), String> {
    use std::process::Command;
    let result = if cfg!(target_os = "linux") {
        let p = std::path::Path::new(&path);
        let target = if p.is_file() {
            p.parent().unwrap_or(p)
        } else {
            p
        };
        Command::new("xdg-open").arg(target).spawn()
    } else if cfg!(target_os = "windows") {
        Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
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
            detect_conflicts,
            merge_vpks,
            path_exists,
            reveal_in_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
