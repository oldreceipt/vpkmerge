//! Turn Yamato's power slash into a full rest-pose (T/A-pose) spin, for fun and as
//! a stress test of the pose codec's two edit paths at once. For her
//! `ability_powerslash_charge` + `_cast` clips it:
//!   - forces every bone to its skeleton **bind (rest) pose** -- animated rotation
//!     tracks are rewritten to the bind local rotation (re-encoded into the pose
//!     stream), and *static* rotation tracks have their `m_constantRotation`
//!     patched to the bind rotation;
//!   - adds a progressive **world-up yaw** on the body root across the frames so
//!     the whole rigid figure spins like a top.
//! Both clips are non-additive, so the absolute pose shows directly. Packs both at
//! their own paths and writes an animated GLB of the cast.
//!
//! Inspect:  cargo run --release -p vpkmerge-core --example yamato_tpose_spin -- <pak01_dir.vpk>
//! Build:    cargo run --release -p vpkmerge-core --example yamato_tpose_spin -- <pak01_dir.vpk> <out_dir>

use std::collections::HashMap;
use std::f32::consts::TAU;

use anyhow::{Context, Result};
use morphic::kv3::Seg;
use morphic::model::{
    decode, decode_nm_clip, decode_nm_skeleton, nm_clip_to_clip, to_glb, Model, NmClip, NmSkeleton,
    Quat, Vec3,
};

const MESH: &str = "models/heroes_staging/yamato_v2/yamato.vmdl_c";
const SKEL: &str = "models/heroes_wip/yamato/yamato.vnmskel_c";
const CHARGE: &str = "models/heroes_wip/yamato/clips/ability_powerslash_charge.vnmclip_c";
const CAST: &str = "models/heroes_wip/yamato/clips/ability_powerslash_cast.vnmclip_c";

/// Number of full turns over each clip.
const SPINS: f32 = 2.0;
/// Body-root candidates, nearest the root first; the spin lands on the first one
/// whose rotation is animated (so it can live in the pose stream).
const ROOT_CANDIDATES: &[&str] = &["pelvis", "root_motion", "spine_0"];

fn qmul(a: Quat, b: Quat) -> Quat {
    Quat {
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
    }
}

fn normalize(q: Quat) -> Quat {
    let n = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
    Quat {
        x: q.x / n,
        y: q.y / n,
        z: q.z / n,
        w: q.w / n,
    }
}

fn axis_angle(axis: Vec3, rad: f32) -> Quat {
    let h = rad * 0.5;
    let s = h.sin();
    Quat {
        x: axis.x * s,
        y: axis.y * s,
        z: axis.z * s,
        w: h.cos(),
    }
}

fn model_bone(model: &Model, name: &str) -> Option<usize> {
    model.skeleton.bones.iter().position(|b| b.name == name)
}

/// Rewrite `bytes` into a rest-pose spin and splice it back. Returns the decoded
/// edited clip (for preview) and the patched resource bytes.
fn tpose_spin(bytes: &[u8], model: &Model, skel: &NmSkeleton) -> Result<(NmClip, Vec<u8>)> {
    let clip = decode_nm_clip(bytes)?;
    let frames = clip.frame_count as usize;

    // NM track index -> model bone (by name), with its bind local rotation.
    let bind_rot = |i: usize| -> Option<Quat> {
        let name = skel.bone_names.get(i)?;
        let mi = model_bone(model, name)?;
        Some(model.skeleton.bones[mi].rotation)
    };
    let name_to_track: HashMap<&str, usize> = skel
        .bone_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();

    // Pick the spin bone: the first root candidate with an animated rotation track.
    let spin_track = ROOT_CANDIDATES.iter().find_map(|n| {
        let &t = name_to_track.get(n)?;
        clip.tracks
            .get(t)
            .filter(|tr| tr.rotations.is_some())
            .map(|_| (t, *n))
    });
    let (spin_idx, spin_name) = spin_track.context("no body-root bone has an animated rotation")?;
    let spin_axis_local = {
        // World up (Z) expressed in the spin bone's local frame.
        let mi = model_bone(model, spin_name).unwrap();
        model.skeleton.bones[mi]
            .inverse_bind
            .transform_vector(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            })
            .normalized()
    };

    // --- edit the animated tracks (pose stream) ---
    let mut edited = clip.clone();
    for (i, track) in edited.tracks.iter_mut().enumerate() {
        let Some(rest) = bind_rot(i) else { continue };
        if let Some(rots) = track.rotations.as_mut() {
            for (f, q) in rots.iter_mut().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let t = if frames > 1 {
                    f as f32 / (frames - 1) as f32
                } else {
                    0.0
                };
                *q = if i == spin_idx {
                    normalize(qmul(rest, axis_angle(spin_axis_local, SPINS * TAU * t)))
                } else {
                    rest
                };
            }
        }
        // Pin animated translations so the body does not drift while spinning.
        if let Some(trans) = track.translations.as_mut() {
            if let Some(mi) = skel.bone_names.get(i).and_then(|n| model_bone(model, n)) {
                let p = model.skeleton.bones[mi].position;
                for v in trans.iter_mut() {
                    *v = p;
                }
            }
        }
    }
    let (new_blob, new_offsets) = morphic::model::encode_compressed_pose(&edited);
    anyhow::ensure!(
        new_blob.len() == clip.compressed_pose_data.len()
            && new_offsets == clip.compressed_pose_offsets,
        "re-encoded stream changed shape"
    );
    let mut out = morphic::patch_kv3_resource_blob(bytes, &clip.compressed_pose_data, &new_blob)
        .context("splice rest-pose stream")?;

    // --- patch the static rotation constants to bind (the non-animated bones) ---
    let mut edits: Vec<(Vec<Seg>, f64)> = Vec::new();
    for (i, track) in clip.tracks.iter().enumerate() {
        if track.rotations.is_some() {
            continue; // animated: already handled in the stream
        }
        let Some(rest) = bind_rot(i) else { continue };
        for (c, val) in [rest.x, rest.y, rest.z, rest.w].into_iter().enumerate() {
            edits.push((
                vec![
                    Seg::Key("m_trackCompressionSettings".into()),
                    Seg::Index(i),
                    Seg::Key("m_constantRotation".into()),
                    Seg::Index(c),
                ],
                f64::from(val),
            ));
        }
    }
    out = patch_constants(&out, &edits)?;
    println!(
        "  spin on '{spin_name}', {} static rotation constants pinned to bind",
        edits.len() / 4
    );
    Ok((edited, out))
}

/// Patch a batch of constant doubles, trying the whole batch as f64 then as f32
/// (KV3 stores `m_constantRotation` either way; a clip uses one consistently).
fn patch_constants(bytes: &[u8], edits: &[(Vec<Seg>, f64)]) -> Result<Vec<u8>> {
    if edits.is_empty() {
        return Ok(bytes.to_vec());
    }
    if let Ok(p) = morphic::patch_kv3_resource_doubles(bytes, edits) {
        return Ok(p);
    }
    let as_f32: Vec<(Vec<Seg>, f32)> = edits.iter().map(|(p, v)| (p.clone(), *v as f32)).collect();
    morphic::patch_kv3_resource_floats(bytes, &as_f32).context("patch constant rotations")
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next();

    let skel = decode_nm_skeleton(&vpkmerge_core::read_vpk_entry(&pak, SKEL)?)?;
    let model = decode(&vpkmerge_core::read_vpk_entry(&pak, MESH)?)?;

    if out_dir.is_none() {
        for entry in [CHARGE, CAST] {
            let clip = decode_nm_clip(&vpkmerge_core::read_vpk_entry(&pak, entry)?)?;
            println!(
                "{entry}: {} frames, {:.3}s",
                clip.frame_count, clip.duration
            );
        }
        println!("\n(inspect mode; pass an out-dir to build the addon)");
        return Ok(());
    }
    let out_dir = out_dir.unwrap();
    println!("building T-pose spin ({SPINS} turns/clip)");

    println!("charge:");
    let (_charge_clip, charge_patched) =
        tpose_spin(&vpkmerge_core::read_vpk_entry(&pak, CHARGE)?, &model, &skel)?;
    println!("cast:");
    let (_cast_clip, cast_patched) =
        tpose_spin(&vpkmerge_core::read_vpk_entry(&pak, CAST)?, &model, &skel)?;

    // Re-decode the fully-patched bytes so the preview reflects BOTH edits (the
    // animated stream and the pinned static constants), and validate the result.
    let final_clip = decode_nm_clip(&cast_patched).context("re-decode patched cast")?;
    let mut preview = model.clone();
    preview.animations = vec![nm_clip_to_clip(
        &final_clip,
        &skel,
        &model.skeleton,
        "tpose_spin",
    )];
    let glb = to_glb(&preview)?;
    let glb_path = format!("{out_dir}/yamato_tpose_spin_cast.glb");
    std::fs::write(&glb_path, &glb)?;
    println!(
        "wrote {glb_path} ({} bytes) - play it in a glTF viewer",
        glb.len()
    );

    let out_vpk = format!("{out_dir}/yamato_tpose_spin_dir.vpk");
    vpkmerge_core::pack(
        &[
            (CHARGE, charge_patched.as_slice()),
            (CAST, cast_patched.as_slice()),
        ],
        &out_vpk,
    )?;
    println!("\npacked T-pose spin -> {out_vpk}");
    println!("install: copy to game/citadel/addons/ as a free pakNN_dir.vpk; cast power slash");
    Ok(())
}
