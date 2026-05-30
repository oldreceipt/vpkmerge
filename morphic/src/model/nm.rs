//! Decode the newer Source 2 "NM" (motion-matching) animation resources: a
//! `.vnmskel_c` skeleton and a *static* single-frame `.vnmclip_c` pose.
//!
//! WIP Deadlock heroes (Apollo `fencer`, Billy `punkgoat`, Celeste `unicorn`,
//! Mina `vampirebat`, Paige `bookworm`, Rem `familiar`) ship their menu/idle
//! pose this way: loose files under `models/heroes_wip/<h>/clips/*.vnmclip_c`
//! plus one `<h>.vnmskel_c`, referenced through an animation graph. They embed
//! no `ANIM`/`AGRP`, so [`super::animation::decode_all`] finds nothing and the
//! hero would only ever bind-pose. The card-pose clips (`ui_hero_select`,
//! `<h>_hero_pose`, ...) are all **single-frame and fully static**: every
//! per-bone track stores a constant transform and `m_compressedPoseData` is
//! empty, so the pose reconstructs from `m_trackCompressionSettings` alone, with
//! no quantized-stream decode and no graph traversal.
//!
//! Only static tracks are decoded. A non-static track (none observed in any
//! menu/idle clip) is reported as `None` for that bone, which the baker treats
//! as "keep bind" rather than rendering a wrong transform; animated NM clips
//! would need the `m_compressedPoseData` dequantizer, which is out of scope.
//! Recon + the full layout writeup: `docs/handoff-nm-loose-clip-pose.md`.

// KV3 widens f32 transform components to f64 and stores frame counts as wider
// integers; narrowing them back is exact for real model data (the same contract
// `skeleton.rs` relies on).
#![allow(clippy::cast_possible_truncation)]

use std::collections::HashMap;

use crate::error::DecodeError;
use crate::kv3::Value;

use super::math::{Quat, Vec3};
use super::pose::LocalPose;
use super::Model;

/// A decoded NM skeleton. Only the bone *names*, in track order (the clip's
/// track `i` is bone `i`), are needed to map an NM pose onto a model skeleton by
/// name; parent indices and the reference pose are not required for posing.
#[derive(Debug, Clone)]
pub struct NmSkeleton {
    pub bone_names: Vec<String>,
}

/// A decoded static NM clip: one constant parent-space transform per skeleton
/// bone (in bone order), or `None` for a non-static (unsupported) track. Also
/// carries the `.vnmskel` resource path the clip references, so the caller can
/// resolve the matching skeleton.
#[derive(Debug, Clone)]
pub struct NmPose {
    /// The `m_skeleton` reference (uncompiled, e.g. `models/.../h.vnmskel`).
    pub skeleton_ref: String,
    /// `m_nNumFrames` (1 for every menu/idle pose seen).
    pub frame_count: u32,
    /// Per-bone constant local transform; `None` where the track is non-static.
    pub bones: Vec<Option<LocalPose>>,
}

impl NmPose {
    /// Number of bones that decoded to a constant transform.
    #[must_use]
    pub fn static_bone_count(&self) -> usize {
        self.bones.iter().filter(|b| b.is_some()).count()
    }
}

/// Decodes a `.vnmskel_c` into its ordered bone names.
pub fn decode_nm_skeleton(bytes: &[u8]) -> Result<NmSkeleton, DecodeError> {
    nm_skeleton_from_value(&crate::decode_kv3_resource(bytes)?)
}

/// Decodes a static `.vnmclip_c` into one constant local transform per bone.
pub fn decode_nm_pose(bytes: &[u8]) -> Result<NmPose, DecodeError> {
    nm_pose_from_value(&crate::decode_kv3_resource(bytes)?)
}

/// [`decode_nm_skeleton`] on an already-decoded `DATA` KV3 tree (split out so the
/// KV3 interpretation is unit-testable without a resource fixture).
fn nm_skeleton_from_value(data: &Value) -> Result<NmSkeleton, DecodeError> {
    let ids = data
        .get("m_boneIDs")
        .and_then(Value::as_array)
        .ok_or(DecodeError::Model("vnmskel missing m_boneIDs"))?;
    let bone_names = ids
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or(DecodeError::Model("vnmskel bone id not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NmSkeleton { bone_names })
}

/// [`decode_nm_pose`] on an already-decoded `DATA` KV3 tree.
fn nm_pose_from_value(data: &Value) -> Result<NmPose, DecodeError> {
    let skeleton_ref = data
        .get("m_skeleton")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let frame_count = data
        .get("m_nNumFrames")
        .and_then(|v| {
            v.as_uint()
                .or_else(|| v.as_int().and_then(|n| u64::try_from(n).ok()))
        })
        .unwrap_or(1) as u32;

    let tracks = data
        .get("m_trackCompressionSettings")
        .and_then(Value::as_array)
        .ok_or(DecodeError::Model(
            "vnmclip missing m_trackCompressionSettings",
        ))?;

    let mut bones = Vec::with_capacity(tracks.len());
    for t in tracks {
        bones.push(decode_track(t));
    }
    Ok(NmPose {
        skeleton_ref,
        frame_count,
        bones,
    })
}

/// One track -> a constant local pose, or `None` if any channel is non-static
/// (its value lives in the compressed stream we do not decode). For a static
/// channel the constant is: rotation = `m_constantRotation`; translation =
/// `(m_translationRange{X,Y,Z}).m_flRangeStart`; scale = `m_scaleRange.m_flRangeStart`.
fn decode_track(t: &Value) -> Option<LocalPose> {
    let is_static = |k: &str| t.get(k).and_then(Value::as_bool).unwrap_or(false);
    if !(is_static("m_bIsRotationStatic")
        && is_static("m_bIsTranslationStatic")
        && is_static("m_bIsScaleStatic"))
    {
        return None;
    }
    let rotation = read_quat(t.get("m_constantRotation")?)?;
    Some(LocalPose {
        translation: Vec3 {
            x: range_start(t, "m_translationRangeX")?,
            y: range_start(t, "m_translationRangeY")?,
            z: range_start(t, "m_translationRangeZ")?,
        },
        rotation,
        scale: range_start(t, "m_scaleRange")?,
    })
}

/// `m_flRangeStart` of a track's quantization-range object (the constant value
/// when that channel is static).
fn range_start(track: &Value, key: &str) -> Option<f32> {
    track
        .get(key)
        .and_then(|o| o.get("m_flRangeStart"))
        .and_then(Value::as_f64)
        .map(|d| d as f32)
}

fn read_quat(v: &Value) -> Option<Quat> {
    let a = v.as_array()?;
    if a.len() < 4 {
        return None;
    }
    Some(Quat {
        x: a[0].as_f64()? as f32,
        y: a[1].as_f64()? as f32,
        z: a[2].as_f64()? as f32,
        w: a[3].as_f64()? as f32,
    })
}

/// Bakes an NM static pose onto `model`'s mesh: zips `skeleton`'s bone names with
/// `pose`'s per-bone transforms (track `i` is bone `i`), maps them onto the model
/// skeleton by name, and folds the skinning into the vertices, returning a static
/// posed [`Model`] (no skeleton/skin/clips), exactly like [`super::bake_pose`].
///
/// The NM skeleton's bones are a by-name subset of the model's mesh skeleton
/// (the model's extra cloth/twist/helper bones are not driven by the clip and
/// keep their bind pose). Errors if the track and bone-name counts disagree.
pub fn bake_nm_pose(
    model: &Model,
    skeleton: &NmSkeleton,
    pose: &NmPose,
) -> Result<Model, DecodeError> {
    if pose.bones.len() != skeleton.bone_names.len() {
        return Err(DecodeError::Model(
            "vnmclip track count != vnmskel bone count",
        ));
    }
    let mut by_name: HashMap<String, LocalPose> = HashMap::with_capacity(pose.bones.len());
    for (name, bp) in skeleton.bone_names.iter().zip(pose.bones.iter()) {
        if let Some(lp) = bp {
            by_name.insert(name.clone(), *lp);
        }
    }
    Ok(super::pose::bake_pose_named(model, &by_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(pairs: Vec<(&str, Value)>) -> Value {
        Value::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }
    fn range(start: f64) -> Value {
        obj(vec![
            ("m_flRangeStart", Value::Double(start)),
            ("m_flRangeLength", Value::Double(0.1)),
        ])
    }
    /// A fully-static track: translation/scale from range starts, constant quat.
    fn static_track(tx: f64, ty: f64, tz: f64, s: f64, q: [f64; 4]) -> Value {
        obj(vec![
            ("m_translationRangeX", range(tx)),
            ("m_translationRangeY", range(ty)),
            ("m_translationRangeZ", range(tz)),
            ("m_scaleRange", range(s)),
            (
                "m_constantRotation",
                Value::Array(q.iter().map(|c| Value::Double(*c)).collect()),
            ),
            ("m_bIsRotationStatic", Value::Bool(true)),
            ("m_bIsTranslationStatic", Value::Bool(true)),
            ("m_bIsScaleStatic", Value::Bool(true)),
        ])
    }
    /// A track with an animated rotation: its constant is in the (here absent)
    /// compressed stream, so it must decode to `None`.
    fn animated_track() -> Value {
        let mut t = static_track(0.0, 0.0, 0.0, 1.0, [0.0, 0.0, 0.0, 1.0]);
        if let Value::Object(o) = &mut t {
            for (k, v) in o.iter_mut() {
                if k == "m_bIsRotationStatic" {
                    *v = Value::Bool(false);
                }
            }
        }
        t
    }

    #[test]
    fn skeleton_reads_bone_ids_in_order() {
        let data = obj(vec![(
            "m_boneIDs",
            Value::Array(vec![
                Value::String("root".into()),
                Value::String("spine".into()),
            ]),
        )]);
        let skel = nm_skeleton_from_value(&data).expect("skeleton");
        assert_eq!(skel.bone_names, vec!["root", "spine"]);
    }

    #[test]
    fn pose_decodes_static_track_and_skips_animated() {
        let data = obj(vec![
            ("m_skeleton", Value::String("models/h/h.vnmskel".into())),
            ("m_nNumFrames", Value::Int(1)),
            (
                "m_trackCompressionSettings",
                Value::Array(vec![
                    static_track(5.0, -2.0, 77.0, 1.0, [0.5, 0.5, 0.5, 0.5]),
                    animated_track(),
                ]),
            ),
        ]);
        let pose = nm_pose_from_value(&data).expect("pose");
        assert_eq!(pose.skeleton_ref, "models/h/h.vnmskel");
        assert_eq!(pose.bones.len(), 2);
        assert_eq!(pose.static_bone_count(), 1, "animated track -> None");

        let lp = pose.bones[0].expect("static bone decoded");
        // translation = the range starts; scale = scale range start; rot = constant.
        assert!((lp.translation.x - 5.0).abs() < 1e-5);
        assert!((lp.translation.y + 2.0).abs() < 1e-5);
        assert!((lp.translation.z - 77.0).abs() < 1e-5);
        assert!((lp.scale - 1.0).abs() < 1e-5);
        assert!((lp.rotation.w - 0.5).abs() < 1e-5);
        assert!(pose.bones[1].is_none(), "animated rotation -> None");
    }

    #[test]
    fn pose_requires_track_settings() {
        let data = obj(vec![("m_nNumFrames", Value::Int(1))]);
        assert!(nm_pose_from_value(&data).is_err());
    }
}
