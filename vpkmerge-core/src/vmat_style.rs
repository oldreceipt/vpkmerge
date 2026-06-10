//! Per-material `.vmat_c` shader-parameter styling.
//!
//! Deadlock's hero shader (`pbr.vfx`) exposes a large per-material vocabulary:
//! the NPR/toon controls (solid outlines, unlit) and the PBR/specular side
//! (sheen, glass, translucency) are all plain `m_intParams` / `m_floatParams` /
//! `m_vectorParams` data in the compiled material. This module sets or inserts
//! those params byte-faithfully (no full KV3 re-encode; same discipline as the
//! particle and scroll patches) and packs the result into an addon VPK.
//!
//! `TextureXxx1` vector params double as flat constant fallbacks when no
//! texture is bound to the matching sampler, so looks like sheen can be enabled
//! with constants alone: Valve's own `xmas_vindicta_dress.vmat_c` is the model
//! for the gem preset. Survey + background: `docs/spike-npr-toon-shading.md`.

use std::path::Path;

use anyhow::{Context, Result};
use morphic::kv3::{Seg, Value};

use crate::trippy::{hero_path_match, is_weapon_path, open_vpks, read_entry};

/// One set-or-insert edit against a compiled material's parameter tables.
#[derive(Debug, Clone, PartialEq)]
pub enum VmatEdit {
    /// `m_intParams` entry (feature flags like `F_SHEEN` and int/bool knobs).
    Int { name: String, value: i64 },
    /// `m_floatParams` entry.
    Float { name: String, value: f64 },
    /// `m_vectorParams` entry (RGBA; pass 0 for unused lanes).
    Vector { name: String, value: [f64; 4] },
}

impl VmatEdit {
    fn table(&self) -> &'static str {
        match self {
            Self::Int { .. } => "m_intParams",
            Self::Float { .. } => "m_floatParams",
            Self::Vector { .. } => "m_vectorParams",
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Int { name, .. } | Self::Float { name, .. } | Self::Vector { name, .. } => name,
        }
    }

    fn as_object(&self) -> Value {
        let (key, value) = match self {
            Self::Int { value, .. } => ("m_nValue", Value::Int(*value)),
            Self::Float { value, .. } => ("m_flValue", Value::Double(*value)),
            Self::Vector { value, .. } => (
                "m_value",
                Value::Array(value.iter().map(|&c| Value::Double(c)).collect()),
            ),
        };
        Value::Object(vec![
            ("m_name".to_string(), Value::String(self.name().to_string())),
            (key.to_string(), value),
        ])
    }
}

/// Per-material outcome counters for [`patch_vmat_params`].
#[derive(Debug, Clone, Default)]
pub struct VmatPatchStats {
    /// Params that already existed and were value-patched.
    pub set: usize,
    /// Params inserted into their table.
    pub inserted: usize,
    /// Param names that could not be applied (e.g. blob-section materials
    /// refuse structural inserts).
    pub failed: Vec<String>,
}

fn param_index(root: &Value, table: &str, name: &str) -> Option<usize> {
    root.get(table)?
        .as_array()?
        .iter()
        .position(|p| p.get("m_name").and_then(Value::as_str) == Some(name))
}

/// Does the decoded param at `table[i]` already hold this edit's value?
fn already_applied(root: &Value, i: usize, edit: &VmatEdit) -> bool {
    let Some(param) = root
        .get(edit.table())
        .and_then(Value::as_array)
        .and_then(|a| a.get(i))
    else {
        return false;
    };
    match edit {
        VmatEdit::Int { value, .. } => {
            param.get("m_nValue").and_then(Value::as_int) == Some(*value)
        }
        VmatEdit::Float { value, .. } => {
            param.get("m_flValue").and_then(Value::as_f64) == Some(*value)
        }
        VmatEdit::Vector { value, .. } => param
            .get("m_value")
            .and_then(Value::as_array)
            .is_some_and(|a| {
                a.len() == 4 && a.iter().zip(value).all(|(c, w)| c.as_f64() == Some(*w))
            }),
    }
}

/// Applies one edit byte-faithfully: in-place set when the param exists,
/// structural insert when it does not.
fn apply_in_place(
    working: &[u8],
    root: &Value,
    edit: &VmatEdit,
) -> std::result::Result<(Vec<u8>, bool), morphic::DecodeError> {
    let table = edit.table();
    if let Some(i) = param_index(root, table, edit.name()) {
        let bytes = match edit {
            VmatEdit::Int { value, .. } => morphic::patch_kv3_resource_scalars(
                working,
                &[(
                    vec![
                        Seg::Key(table.to_string()),
                        Seg::Index(i),
                        Seg::Key("m_nValue".to_string()),
                    ],
                    *value,
                )],
            )?,
            VmatEdit::Float { value, .. } => morphic::patch_kv3_resource_doubles(
                working,
                &[(
                    vec![
                        Seg::Key(table.to_string()),
                        Seg::Index(i),
                        Seg::Key("m_flValue".to_string()),
                    ],
                    *value,
                )],
            )?,
            VmatEdit::Vector { value, .. } => {
                let edits: Vec<(Vec<Seg>, f64)> = value
                    .iter()
                    .enumerate()
                    .map(|(k, &c)| {
                        (
                            vec![
                                Seg::Key(table.to_string()),
                                Seg::Index(i),
                                Seg::Key("m_value".to_string()),
                                Seg::Index(k),
                            ],
                            c,
                        )
                    })
                    .collect();
                morphic::patch_kv3_resource_doubles(working, &edits)?
            }
        };
        Ok((bytes, false))
    } else {
        let bytes = morphic::patch_kv3_resource_array_insert(
            working,
            &[Seg::Key(table.to_string())],
            0,
            &edit.as_object(),
        )?;
        Ok((bytes, true))
    }
}

/// Applies one edit to the decoded tree (the re-encode fallback path).
fn apply_to_tree(tree: &mut Value, edit: &VmatEdit) -> bool {
    let exists = param_index(tree, edit.table(), edit.name());
    let Some(Value::Array(params)) = tree.get_mut(edit.table()) else {
        return false;
    };
    match exists {
        Some(i) => {
            let value_key = match edit {
                VmatEdit::Int { .. } => "m_nValue",
                VmatEdit::Float { .. } => "m_flValue",
                VmatEdit::Vector { .. } => "m_value",
            };
            let Some(slot) = params[i].get_mut(value_key) else {
                return false;
            };
            *slot = match edit {
                VmatEdit::Int { value, .. } => Value::Int(*value),
                VmatEdit::Float { value, .. } => Value::Double(*value),
                VmatEdit::Vector { value, .. } => {
                    Value::Array(value.iter().map(|&c| Value::Double(c)).collect())
                }
            };
        }
        None => params.push(edit.as_object()),
    }
    true
}

/// Applies `edits` to a compiled `.vmat_c`, setting existing params and
/// inserting missing ones.
///
/// Byte-faithful in-place patching is tried first. A tagless stored value (a
/// 0/1 encoded with no data bytes) cannot be patched in place; in that case a
/// non-blobbed material is fully re-encoded ([`morphic::encode_kv3_resource`],
/// which preserves the texture dependency blocks) with all edits applied to
/// the tree. The same discipline as `hero_recolor`'s tint stamping. A blobbed
/// material that needs the fallback reports the edit in
/// [`VmatPatchStats::failed`] instead (a re-encode would mangle its blob
/// framing).
///
/// # Errors
/// Fails only if the bytes are not a decodable KV3 resource.
pub fn patch_vmat_params(bytes: &[u8], edits: &[VmatEdit]) -> Result<(Vec<u8>, VmatPatchStats)> {
    let mut working = bytes.to_vec();
    let mut stats = VmatPatchStats::default();
    let mut needs_reencode = Vec::new();

    for edit in edits {
        let root = morphic::decode_kv3_resource(&working)
            .context("decoding material KV3 for parameter lookup")?;
        if let Some(i) = param_index(&root, edit.table(), edit.name()) {
            if already_applied(&root, i, edit) {
                stats.set += 1;
                continue;
            }
        }
        match apply_in_place(&working, &root, edit) {
            Ok((bytes, was_insert)) => {
                working = bytes;
                if was_insert {
                    stats.inserted += 1;
                } else {
                    stats.set += 1;
                }
            }
            Err(_) => needs_reencode.push(edit.clone()),
        }
    }

    if !needs_reencode.is_empty() {
        if morphic::kv3_resource_has_blobs(&working).unwrap_or(true) {
            stats
                .failed
                .extend(needs_reencode.iter().map(|e| e.name().to_string()));
        } else {
            let mut tree = morphic::decode_kv3_resource(&working)
                .context("decoding material KV3 for re-encode fallback")?;
            for edit in &needs_reencode {
                if apply_to_tree(&mut tree, edit) {
                    stats.set += 1;
                } else {
                    stats.failed.push(edit.name().to_string());
                }
            }
            working = morphic::encode_kv3_resource(&working, &tree)
                .context("re-encoding material to promote tagless params")?;
        }
    }

    Ok((working, stats))
}

/// Curated parameter bundles for common looks. All values are modeled on
/// shipped Valve materials so the engine demonstrably accepts them (see the
/// spike doc for the survey).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmatPreset {
    /// Gemstone sheen: `F_SHEEN` with constant sheen color + low sheen
    /// roughness (recipe: `xmas_vindicta_dress.vmat_c`). `tint` colors the
    /// sheen lobe.
    Gem,
    /// Glassy specular coat: `F_GLASS` with a full constant mask (recipe:
    /// `viscous_body.vmat_c`, minus its bound mask texture).
    Glass,
    /// Drop the hero NPR lighting path entirely: full PBR response, real
    /// environment reflections.
    Pbr,
    /// Fully unlit (lighting ignored; albedo as authored).
    Unlit,
    /// Thick solid-color outline (`tint` is the ink color).
    Ink,
}

impl VmatPreset {
    /// Parse a CLI preset name.
    ///
    /// # Errors
    /// Fails on an unknown name, listing the valid ones.
    pub fn from_name(name: &str) -> Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "gem" | "sheen" => Ok(Self::Gem),
            "glass" => Ok(Self::Glass),
            "pbr" | "no-npr" => Ok(Self::Pbr),
            "unlit" => Ok(Self::Unlit),
            "ink" | "outline" => Ok(Self::Ink),
            other => anyhow::bail!("unknown preset {other:?} (gem, glass, pbr, unlit, ink)"),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gem => "gem",
            Self::Glass => "glass",
            Self::Pbr => "pbr",
            Self::Unlit => "unlit",
            Self::Ink => "ink",
        }
    }

    /// The edit bundle for this preset. `tint` is linear RGB 0..=1 where the
    /// preset has a color (gem sheen color, ink outline color).
    #[must_use]
    pub fn edits(self, tint: Option<[f64; 3]>) -> Vec<VmatEdit> {
        let int = |name: &str, value: i64| VmatEdit::Int {
            name: name.to_string(),
            value,
        };
        let float = |name: &str, value: f64| VmatEdit::Float {
            name: name.to_string(),
            value,
        };
        let vec3 = |name: &str, [r, g, b]: [f64; 3]| VmatEdit::Vector {
            name: name.to_string(),
            value: [r, g, b, 0.0],
        };
        match self {
            Self::Gem => {
                // Icy blue like Valve's snow dress unless the caller tints.
                let sheen = tint.unwrap_or([0.67, 0.76, 1.0]);
                vec![
                    int("F_SHEEN", 1),
                    int("g_bSheenMaskColorTint1", 0),
                    int("g_bSheenMaskVertexColorTint1", 1),
                    int("g_nSheenTextureColorTintMode1", 1),
                    float("g_fSheenVertexColorStrength1", 1.0),
                    vec3("g_vSheenColorTint1", sheen),
                    // Constant fallback for the unbound sheen-roughness
                    // sampler. Lower = tighter, more crystalline highlight
                    // (Valve ships ~0.5).
                    vec3("TextureSheenRoughness1", [0.25, 0.25, 0.25]),
                ]
            }
            Self::Glass => vec![
                int("F_GLASS", 1),
                vec3("TextureGlassMask1", [1.0, 1.0, 1.0]),
            ],
            Self::Pbr => vec![int("F_USE_NPR_LIGHTING", 0)],
            Self::Unlit => vec![int("F_UNLIT", 1)],
            Self::Ink => {
                let ink = tint.unwrap_or([1.0, 1.0, 1.0]);
                vec![
                    int("F_SOLID_COLOR_OUTLINE", 1),
                    int("F_OVERRIDE_NPR_OUTLINE", 1),
                    float("g_flOverrideNprOutlineThickness", 3.0),
                    vec3("g_vSolidOutlineTint", ink),
                ]
            }
        }
    }
}

/// Which materials a styling run targets.
#[derive(Debug, Clone)]
pub enum VmatTargets {
    /// Discover every `models/heroes*` material for a hero codename.
    Hero {
        codename: String,
        include_body: bool,
        include_weapons: bool,
    },
    /// Explicit `.vmat_c` entry paths.
    Entries(Vec<String>),
}

/// Summary returned by [`style_materials_to_addon`].
#[derive(Debug, Clone, Default)]
pub struct VmatStyleReport {
    pub materials_patched: usize,
    pub params_set: usize,
    pub params_inserted: usize,
    /// Materials skipped because their shader is not `pbr.vfx`.
    pub skipped_non_pbr: usize,
    pub skipped_unreadable: usize,
    /// `(entry, param)` pairs the byte-faithful patcher refused.
    pub failed_params: Vec<(String, String)>,
}

/// A discovered material and the bits of it relevant to styling decisions.
#[derive(Debug, Clone)]
pub struct VmatInfo {
    pub entry: String,
    pub shader: String,
    /// Nonzero `F_*` feature flags.
    pub flags: Vec<(String, i64)>,
    /// Bound texture samplers (`g_t*` name -> resource path).
    pub textures: Vec<(String, String)>,
}

fn discover_materials(vpks: &[valve_pak::VPK], targets: &VmatTargets) -> Vec<String> {
    match targets {
        VmatTargets::Entries(entries) => entries.clone(),
        VmatTargets::Hero {
            codename,
            include_body,
            include_weapons,
        } => {
            let mut out = std::collections::BTreeSet::new();
            for vpk in vpks {
                for path in vpk.file_paths() {
                    if path.ends_with(".vmat_c")
                        && path.starts_with("models/heroes")
                        && hero_path_match(path.as_str(), codename)
                    {
                        let weapon = is_weapon_path(path.as_str());
                        if (weapon && *include_weapons) || (!weapon && *include_body) {
                            out.insert(path.clone());
                        }
                    }
                }
            }
            out.into_iter().collect()
        }
    }
}

fn material_info(entry: &str, root: &Value) -> VmatInfo {
    let shader = root
        .get("m_shaderName")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();
    let mut flags = Vec::new();
    if let Some(Value::Array(params)) = root.get("m_intParams") {
        for p in params {
            if let (Some(name), Some(value)) = (
                p.get("m_name").and_then(Value::as_str),
                p.get("m_nValue").and_then(Value::as_int),
            ) {
                if name.starts_with("F_") && value != 0 {
                    flags.push((name.to_string(), value));
                }
            }
        }
    }
    let mut textures = Vec::new();
    if let Some(Value::Array(params)) = root.get("m_textureParams") {
        for p in params {
            if let (Some(name), Some(path)) = (
                p.get("m_name").and_then(Value::as_str),
                p.get("m_pValue").and_then(Value::as_str),
            ) {
                textures.push((name.to_string(), path.to_string()));
            }
        }
    }
    VmatInfo {
        entry: entry.to_string(),
        shader,
        flags,
        textures,
    }
}

/// Lists the targeted materials with their shader, active feature flags, and
/// bound texture channels: the "what is this skin made of / what is unbound"
/// view used to plan a styling run.
///
/// # Errors
/// Fails if a VPK cannot be opened.
pub fn list_materials(
    vpk: impl AsRef<Path>,
    base: Option<&Path>,
    targets: &VmatTargets,
) -> Result<Vec<VmatInfo>> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let mut out = Vec::new();
    for entry in discover_materials(&vpks, targets) {
        let Some(bytes) = read_entry(&vpks, &entry) else {
            continue;
        };
        let Ok(root) = morphic::decode_kv3_resource(&bytes) else {
            continue;
        };
        out.push(material_info(&entry, &root));
    }
    Ok(out)
}

/// Applies `edits` to every targeted `pbr.vfx` material and packs the patched
/// materials into a standalone addon VPK at their original entry paths.
///
/// # Errors
/// Fails if VPKs cannot be opened, no material accepted an edit, or the
/// output cannot be written.
pub fn style_materials_to_addon(
    vpk: impl AsRef<Path>,
    base: Option<&Path>,
    targets: &VmatTargets,
    edits: &[VmatEdit],
    out: impl AsRef<Path>,
) -> Result<VmatStyleReport> {
    anyhow::ensure!(!edits.is_empty(), "no edits given");
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let mut report = VmatStyleReport::default();
    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();

    for entry in discover_materials(&vpks, targets) {
        let Some(bytes) = read_entry(&vpks, &entry) else {
            report.skipped_unreadable += 1;
            continue;
        };
        let Ok(root) = morphic::decode_kv3_resource(&bytes) else {
            report.skipped_unreadable += 1;
            continue;
        };
        if root.get("m_shaderName").and_then(Value::as_str) != Some("pbr.vfx") {
            report.skipped_non_pbr += 1;
            continue;
        }
        let (patched, stats) =
            patch_vmat_params(&bytes, edits).with_context(|| format!("patching {entry}"))?;
        for name in stats.failed {
            report.failed_params.push((entry.clone(), name));
        }
        if stats.set + stats.inserted == 0 {
            continue;
        }
        report.params_set += stats.set;
        report.params_inserted += stats.inserted;
        report.materials_patched += 1;
        packed.push((entry, patched));
    }

    anyhow::ensure!(
        !packed.is_empty(),
        "no materials accepted the edits (targets matched {} unreadable, {} non-pbr)",
        report.skipped_unreadable,
        report.skipped_non_pbr
    );
    let files: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(e, b)| (e.as_str(), b.as_slice()))
        .collect();
    crate::pack(&files, out.as_ref())?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../morphic/fixtures/material/vindicta_headv2.vmat_c"
        ))
        .expect("committed vmat fixture")
    }

    fn int_param(root: &Value, name: &str) -> Option<i64> {
        let i = param_index(root, "m_intParams", name)?;
        root.get("m_intParams")?
            .as_array()?
            .get(i)?
            .get("m_nValue")?
            .as_int()
    }

    #[test]
    fn set_existing_and_insert_missing_params() {
        let bytes = fixture();
        let before = morphic::decode_kv3_resource(&bytes).unwrap();
        assert_eq!(
            int_param(&before, "F_USE_NPR_LIGHTING"),
            Some(1),
            "fixture should be an NPR hero material"
        );
        assert_eq!(int_param(&before, "F_SHEEN"), None);

        let edits = [
            VmatEdit::Int {
                name: "F_USE_NPR_LIGHTING".into(),
                value: 0,
            },
            VmatEdit::Int {
                name: "F_SHEEN".into(),
                value: 1,
            },
            VmatEdit::Float {
                name: "g_fSheenVertexColorStrength1".into(),
                value: 1.0,
            },
            VmatEdit::Vector {
                name: "g_vSheenColorTint1".into(),
                value: [0.5, 0.25, 1.0, 0.0],
            },
        ];
        let (patched, stats) = patch_vmat_params(&bytes, &edits).unwrap();
        assert_eq!(stats.set, 1, "{:?}", stats.failed);
        assert_eq!(stats.inserted, 3, "{:?}", stats.failed);
        assert!(stats.failed.is_empty(), "{:?}", stats.failed);

        let after = morphic::decode_kv3_resource(&patched).unwrap();
        assert_eq!(int_param(&after, "F_USE_NPR_LIGHTING"), Some(0));
        assert_eq!(int_param(&after, "F_SHEEN"), Some(1));
        let i = param_index(&after, "m_vectorParams", "g_vSheenColorTint1").unwrap();
        let v = after
            .get("m_vectorParams")
            .and_then(Value::as_array)
            .and_then(|a| a.get(i))
            .and_then(|p| p.get("m_value"))
            .and_then(Value::as_array)
            .unwrap();
        let comps: Vec<f64> = v.iter().filter_map(Value::as_f64).collect();
        assert_eq!(comps, vec![0.5, 0.25, 1.0, 0.0]);

        // Untouched tables survive byte-faithfully through a re-decode.
        assert_eq!(
            before.get("m_textureParams"),
            after.get("m_textureParams"),
            "texture params must be untouched"
        );
    }

    #[test]
    fn preset_names_round_trip() {
        for name in ["gem", "glass", "pbr", "unlit", "ink"] {
            let p = VmatPreset::from_name(name).unwrap();
            assert_eq!(p.as_str(), name);
            assert!(!p.edits(None).is_empty());
        }
        assert!(VmatPreset::from_name("nope").is_err());
    }
}
