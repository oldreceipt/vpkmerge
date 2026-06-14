//! glTF animation importer: read a `.glb`'s animation back into per-bone-name
//! TRS keyframe tracks and map them onto an existing NM clip.
//!
//! This is the engine-side of the Blender authoring loop (see
//! `docs/anim-authoring-pipeline.md`). An artist exports the slot's clip to a
//! `.glb` ([`super::nm_clip_to_clip`] + [`super::to_glb`]), keyframes the
//! armature in Blender, and exports it back. This module reads that animation and
//! produces an edited [`NmClip`] that [`super::reencode_nm_clip`] splices into the
//! compiled `.vnmclip_c` (v5 in-place, the engine-confirmed path).
//!
//! Coordinate space: the `.glb` writer keeps per-bone local transforms in raw
//! Source space (only the skeleton wrapper node carries the inches->meters /
//! Z-up->Y-up `TRANSFORMSOURCETOGLTF`), so the per-bone animation TRS values read
//! straight back with no inverse transform. Bones map **by node name** (the
//! writer names each joint node after its bone), exactly like the export side maps
//! NM track `i` to `skel.bone_names[i]`.
//!
//! Scope (matches what the engine accepts via [`super::reencode_nm_clip`]): the
//! target frame count is fixed to the slot's clip, the source animation is
//! time-stretched onto it, and a bone's translation/scale are **edited only where
//! the slot already animates them** (adding a translation/scale channel needs the
//! full v4 re-encode, which is engine-inert). Rotations may be edited or **added**
//! (a static bone becomes animated). Imported scale is uniform (glTF's `x`).

// glTF stores everything as f32; the resampler rounds source-clock times onto the
// target frame grid. These narrowings are exact for real clips.
#![allow(clippy::cast_precision_loss)]

use std::collections::HashMap;

use crate::error::DecodeError;

use super::math::{Quat, Vec3};
use super::nm::{decode_nm_clip, reencode_nm_clip, NmClip, NmSkeleton};

/// One bone's imported keyframes, in raw Source local space. Each present channel
/// is a list of `(time_seconds, value)` samples in ascending time order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GltfBoneTrack {
    pub translations: Option<Vec<(f32, Vec3)>>,
    pub rotations: Option<Vec<(f32, Quat)>>,
    /// Uniform scale (glTF stores a Vec3; the `x` component is taken, matching the
    /// uniform bone scale the NM codec stores).
    pub scales: Option<Vec<(f32, f32)>>,
}

/// A glTF animation read back from a `.glb`, keyed by bone (joint node) name.
#[derive(Debug, Clone, Default)]
pub struct GltfAnimation {
    pub name: Option<String>,
    pub bones: HashMap<String, GltfBoneTrack>,
}

impl GltfAnimation {
    /// The `[earliest, latest]` keyframe time across every channel, or `None` when
    /// the animation has no samples. Used to time-stretch onto a target frame grid.
    #[must_use]
    pub fn time_range(&self) -> Option<(f32, f32)> {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for track in self.bones.values() {
            for t in track
                .translations
                .iter()
                .flatten()
                .map(|(t, _)| *t)
                .chain(track.rotations.iter().flatten().map(|(t, _)| *t))
                .chain(track.scales.iter().flatten().map(|(t, _)| *t))
            {
                lo = lo.min(t);
                hi = hi.max(t);
            }
        }
        (lo <= hi).then_some((lo, hi))
    }
}

/// Reads one animation out of a `.glb`. With `name = None` the first animation is
/// used; otherwise the named one (error if absent). Channels are grouped by target
/// joint-node name into per-bone TRS tracks.
pub fn read_glb_animation(glb: &[u8], name: Option<&str>) -> Result<GltfAnimation, DecodeError> {
    let (doc, buffers, _images) =
        gltf::import_slice(glb).map_err(|_| DecodeError::Model("failed to parse glb"))?;

    let anim = match name {
        Some(want) => doc
            .animations()
            .find(|a| a.name() == Some(want))
            .ok_or(DecodeError::Model("glb has no animation with that name"))?,
        None => doc
            .animations()
            .next()
            .ok_or(DecodeError::Model("glb carries no animation"))?,
    };

    let node_name: Vec<Option<String>> = doc.nodes().map(|n| n.name().map(str::to_owned)).collect();

    let mut bones: HashMap<String, GltfBoneTrack> = HashMap::new();
    for channel in anim.channels() {
        let node = channel.target().node().index();
        let Some(Some(bone)) = node_name.get(node) else {
            continue; // an unnamed node can't map to a bone
        };
        let reader = channel.reader(|b| buffers.get(b.index()).map(|d| d.0.as_slice()));
        let Some(times) = reader.read_inputs() else {
            continue;
        };
        let times: Vec<f32> = times.collect();
        let Some(outputs) = reader.read_outputs() else {
            continue;
        };
        let track = bones.entry(bone.clone()).or_default();
        match outputs {
            gltf::animation::util::ReadOutputs::Translations(it) => {
                track.translations = Some(
                    times
                        .iter()
                        .copied()
                        .zip(it.map(|v| Vec3 {
                            x: v[0],
                            y: v[1],
                            z: v[2],
                        }))
                        .collect(),
                );
            }
            gltf::animation::util::ReadOutputs::Rotations(rots) => {
                track.rotations = Some(
                    times
                        .iter()
                        .copied()
                        .zip(rots.into_f32().map(|q| Quat {
                            x: q[0],
                            y: q[1],
                            z: q[2],
                            w: q[3],
                        }))
                        .collect(),
                );
            }
            gltf::animation::util::ReadOutputs::Scales(it) => {
                // Source bone scale is uniform; take x (the writer emits [s,s,s]).
                track.scales = Some(times.iter().copied().zip(it.map(|v| v[0])).collect());
            }
            gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => {}
        }
    }

    Ok(GltfAnimation {
        name: anim.name().map(str::to_owned),
        bones,
    })
}

/// Imports a `.glb` animation onto the slot's compiled clip and returns the edited
/// `.vnmclip_c` bytes (v5 in-place, via [`reencode_nm_clip`]).
///
/// The source animation is **time-stretched** onto the slot's frame count (the
/// engine plays a clip over its own duration, so the authored motion fills the
/// slot). Bones map by name through `skel`; a glb bone not in the skeleton is
/// ignored, and a skeleton bone the glb does not animate keeps the slot's original
/// channels. Per the in-place encoder's limits, translation/scale are edited only
/// where the slot already animates them (an attempt to add one is ignored, not an
/// error); rotations may be edited or added.
pub fn import_glb_onto_nm_clip(
    original: &[u8],
    skel: &NmSkeleton,
    glb: &[u8],
    name: Option<&str>,
) -> Result<Vec<u8>, DecodeError> {
    let clip = decode_nm_clip(original)?;
    let anim = read_glb_animation(glb, name)?;
    let edited = apply_animation(&clip, skel, &anim);
    reencode_nm_clip(original, &edited)
}

/// Produces an edited [`NmClip`] by sampling `anim` onto `clip`'s frame grid and
/// mapping bones by name through `skel`. Split out from [`import_glb_onto_nm_clip`]
/// so the resample/map logic is unit-testable without a compiled resource.
#[must_use]
pub fn apply_animation(clip: &NmClip, skel: &NmSkeleton, anim: &GltfAnimation) -> NmClip {
    let frames = clip.frame_count as usize;
    // Sample times on the source clock, time-stretched to cover the target frames.
    let sample_times: Vec<f32> = match anim.time_range() {
        Some((lo, hi)) if frames > 1 && hi > lo => (0..frames)
            .map(|f| lo + (f as f32 / (frames - 1) as f32) * (hi - lo))
            .collect(),
        _ => vec![0.0; frames],
    };

    let mut edited = clip.clone();
    for (i, track) in edited.tracks.iter_mut().enumerate() {
        let Some(bone_name) = skel.bone_names.get(i) else {
            continue;
        };
        let Some(src) = anim.bones.get(bone_name) else {
            continue; // bone not animated by the glb: keep the slot's channels
        };

        // Rotations: edit or add (a static bone may become animated).
        if let Some(keys) = &src.rotations {
            if !keys.is_empty() {
                track.rotations =
                    Some(sample_times.iter().map(|&t| sample_quat(keys, t)).collect());
            }
        }
        // Translation/scale: edit only where the slot already animates them.
        if track.translations.is_some() {
            if let Some(keys) = &src.translations {
                if !keys.is_empty() {
                    track.translations =
                        Some(sample_times.iter().map(|&t| sample_vec3(keys, t)).collect());
                }
            }
        }
        if track.scales.is_some() {
            if let Some(keys) = &src.scales {
                if !keys.is_empty() {
                    track.scales = Some(
                        sample_times
                            .iter()
                            .map(|&t| sample_scalar(keys, t))
                            .collect(),
                    );
                }
            }
        }
    }
    edited
}

/// Locates `t` in an ascending `(time, _)` keyframe list, returning either the
/// exact/clamped sample index (`Exact`) or the bracketing pair plus the
/// interpolation fraction (`Between`).
enum Bracket {
    Exact(usize),
    Between(usize, usize, f32),
}

fn bracket<T>(keys: &[(f32, T)], t: f32) -> Bracket {
    debug_assert!(!keys.is_empty());
    if t <= keys[0].0 {
        return Bracket::Exact(0);
    }
    if t >= keys[keys.len() - 1].0 {
        return Bracket::Exact(keys.len() - 1);
    }
    // First key whose time is >= t (keys are ascending).
    let hi = keys.partition_point(|(kt, _)| *kt < t);
    let lo = hi - 1;
    let span = keys[hi].0 - keys[lo].0;
    let frac = if span > 0.0 {
        (t - keys[lo].0) / span
    } else {
        0.0
    };
    Bracket::Between(lo, hi, frac)
}

fn sample_vec3(keys: &[(f32, Vec3)], t: f32) -> Vec3 {
    match bracket(keys, t) {
        Bracket::Exact(i) => keys[i].1,
        Bracket::Between(lo, hi, u) => {
            let a = keys[lo].1;
            let b = keys[hi].1;
            Vec3 {
                x: a.x + (b.x - a.x) * u,
                y: a.y + (b.y - a.y) * u,
                z: a.z + (b.z - a.z) * u,
            }
        }
    }
}

fn sample_scalar(keys: &[(f32, f32)], t: f32) -> f32 {
    match bracket(keys, t) {
        Bracket::Exact(i) => keys[i].1,
        Bracket::Between(lo, hi, u) => keys[lo].1 + (keys[hi].1 - keys[lo].1) * u,
    }
}

/// Normalized-lerp between bracketing rotation keys (sign-aligned so the short arc
/// is taken). nlerp, not slerp: cheap, and at the dense per-frame grid the angular
/// error is negligible, while it can never produce a non-unit result.
fn sample_quat(keys: &[(f32, Quat)], t: f32) -> Quat {
    match bracket(keys, t) {
        Bracket::Exact(i) => keys[i].1,
        Bracket::Between(lo, hi, frac) => {
            let from = keys[lo].1;
            let mut to = keys[hi].1;
            let dot = from.x * to.x + from.y * to.y + from.z * to.z + from.w * to.w;
            if dot < 0.0 {
                to = Quat {
                    x: -to.x,
                    y: -to.y,
                    z: -to.z,
                    w: -to.w,
                };
            }
            let lerped = Quat {
                x: from.x + (to.x - from.x) * frac,
                y: from.y + (to.y - from.y) * frac,
                z: from.z + (to.z - from.z) * frac,
                w: from.w + (to.w - from.w) * frac,
            };
            let norm = (lerped.x * lerped.x
                + lerped.y * lerped.y
                + lerped.z * lerped.z
                + lerped.w * lerped.w)
                .sqrt();
            if norm > 0.0 {
                Quat {
                    x: lerped.x / norm,
                    y: lerped.y / norm,
                    z: lerped.z / norm,
                    w: lerped.w / norm,
                }
            } else {
                from
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_clamps_and_interpolates() {
        let keys = [(0.0f32, 0.0f32), (1.0, 10.0), (2.0, 30.0)];
        let close = |got: f32, want: f32, msg: &str| assert!((got - want).abs() < 1e-6, "{msg}");
        close(sample_scalar(&keys, -1.0), 0.0, "clamp low");
        close(sample_scalar(&keys, 3.0), 30.0, "clamp high");
        close(sample_scalar(&keys, 0.5), 5.0, "mid first span");
        close(sample_scalar(&keys, 1.5), 20.0, "mid second span");
        close(sample_scalar(&keys, 1.0), 10.0, "exact key");
    }

    #[test]
    fn quat_sample_stays_unit_and_short_arc() {
        let a = Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
        // 90deg about Z stored as its negation, so the naive lerp would go the
        // long way; sign alignment must pick the short arc.
        let half = 45.0_f32.to_radians();
        let b = Quat {
            x: 0.0,
            y: 0.0,
            z: -half.sin(),
            w: -half.cos(),
        };
        let keys = [(0.0f32, a), (1.0, b)];
        let mid = sample_quat(&keys, 0.5);
        let n = (mid.x * mid.x + mid.y * mid.y + mid.z * mid.z + mid.w * mid.w).sqrt();
        assert!((n - 1.0).abs() < 1e-5, "result must be unit, got {n}");
        // Short arc midpoint of 0 and 90deg is +45deg about Z (positive z, positive w).
        assert!(mid.z > 0.0 && mid.w > 0.0, "short arc: {mid:?}");
    }

    #[test]
    fn time_range_spans_all_channels() {
        let mut bones = HashMap::new();
        bones.insert(
            "a".to_owned(),
            GltfBoneTrack {
                rotations: Some(vec![(0.5, Quat::default()), (1.5, Quat::default())]),
                ..Default::default()
            },
        );
        bones.insert(
            "b".to_owned(),
            GltfBoneTrack {
                translations: Some(vec![(0.1, Vec3::default()), (2.0, Vec3::default())]),
                ..Default::default()
            },
        );
        let anim = GltfAnimation { name: None, bones };
        let (lo, hi) = anim.time_range().expect("has samples");
        assert!((lo - 0.1).abs() < 1e-6 && (hi - 2.0).abs() < 1e-6);
    }
}
