//! Exaggerate Yamato's power-slash with the pose codec: decode her
//! `ability_powerslash_charge` + `_cast` clips, amplify every animated bone's
//! rotation *relative to the clip's first frame* by a factor (so the windup and
//! swing arc grow while frame 0 stays put), re-encode each pose stream, splice it
//! back byte-faithfully, and pack both at their own paths so casting power slash
//! in-game plays the bigger motion. Both clips are non-additive, so the absolute
//! rotation edit shows directly. Also writes an animated GLB of the cast.
//!
//! Inspect:  cargo run --release -p vpkmerge-core --example yamato_powerslash_amp -- <pak01_dir.vpk>
//! Build:    cargo run --release -p vpkmerge-core --example yamato_powerslash_amp -- <pak01_dir.vpk> <out_dir>

use anyhow::{Context, Result};
use morphic::model::{
    decode, decode_nm_clip, decode_nm_skeleton, nm_clip_to_clip, to_glb, NmClip, Quat,
};

const MESH: &str = "models/heroes_staging/yamato_v2/yamato.vmdl_c";
const SKEL: &str = "models/heroes_wip/yamato/yamato.vnmskel_c";
const CHARGE: &str = "models/heroes_wip/yamato/clips/ability_powerslash_charge.vnmclip_c";
const CAST: &str = "models/heroes_wip/yamato/clips/ability_powerslash_cast.vnmclip_c";

/// How much to scale each bone's rotation arc away from frame 0. 1.0 = vanilla.
const AMPLIFY: f32 = 1.6;

fn qmul(a: Quat, b: Quat) -> Quat {
    Quat {
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
    }
}

fn conj(q: Quat) -> Quat {
    Quat {
        x: -q.x,
        y: -q.y,
        z: -q.z,
        w: q.w,
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

/// Scale a rotation's angle by `f` (axis preserved), taking the short way so the
/// scaling is about the small deviation, not its 360-complement.
fn scale_angle(q: Quat, f: f32) -> Quat {
    // q and -q are the same rotation; pick w >= 0 so the angle is in [0, pi].
    let q = if q.w < 0.0 {
        Quat {
            x: -q.x,
            y: -q.y,
            z: -q.z,
            w: -q.w,
        }
    } else {
        q
    };
    let s = (1.0 - q.w * q.w).max(0.0).sqrt();
    if s < 1e-6 {
        return Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        }; // ~no rotation
    }
    let angle = 2.0 * q.w.clamp(-1.0, 1.0).acos();
    let half = angle * f * 0.5;
    let k = half.sin() / s;
    Quat {
        x: q.x * k,
        y: q.y * k,
        z: q.z * k,
        w: half.cos(),
    }
}

/// Amplify every animated rotation track of `clip` about its frame-0 value,
/// returning a re-encoded, byte-faithfully spliced resource. Errors if the
/// re-encoded stream changes length (it never should: same channels).
fn amplify_clip(bytes: &[u8], factor: f32) -> Result<(NmClip, Vec<u8>)> {
    let clip = decode_nm_clip(bytes)?;
    let mut edited = clip.clone();
    for track in &mut edited.tracks {
        if let Some(rots) = track.rotations.as_mut() {
            if rots.is_empty() {
                continue;
            }
            let reference = rots[0];
            for q in rots.iter_mut() {
                let delta = qmul(conj(reference), *q); // motion since frame 0
                *q = normalize(qmul(reference, scale_angle(delta, factor)));
            }
        }
    }
    let (new_blob, new_offsets) = morphic::model::encode_compressed_pose(&edited);
    anyhow::ensure!(
        new_blob.len() == clip.compressed_pose_data.len()
            && new_offsets == clip.compressed_pose_offsets,
        "re-encoded stream changed shape; cannot splice in place"
    );
    let patched = morphic::patch_kv3_resource_blob(bytes, &clip.compressed_pose_data, &new_blob)
        .context("splice amplified pose stream")?;
    Ok((edited, patched))
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next();

    if out_dir.is_none() {
        for entry in [CHARGE, CAST] {
            let clip = decode_nm_clip(&vpkmerge_core::read_vpk_entry(&pak, entry)?)?;
            let anim_rot = clip.tracks.iter().filter(|t| t.rotations.is_some()).count();
            println!(
                "{entry}\n  {} frames, {:.3}s, additive={}, {anim_rot} animated rotation tracks",
                clip.frame_count, clip.duration, clip.additive
            );
        }
        println!("\n(inspect mode; pass an out-dir to build the addon)");
        return Ok(());
    }
    let out_dir = out_dir.unwrap();
    println!("amplifying power-slash rotation arcs x{AMPLIFY}");

    let charge_bytes = vpkmerge_core::read_vpk_entry(&pak, CHARGE)?;
    let cast_bytes = vpkmerge_core::read_vpk_entry(&pak, CAST)?;
    let (_charge_clip, charge_patched) = amplify_clip(&charge_bytes, AMPLIFY)?;
    let (cast_clip, cast_patched) = amplify_clip(&cast_bytes, AMPLIFY)?;
    println!("  charge + cast re-encoded and spliced (byte-faithful)");

    // Animated GLB of the amplified cast to eyeball.
    let skel = decode_nm_skeleton(&vpkmerge_core::read_vpk_entry(&pak, SKEL)?)?;
    let mut preview = decode(&vpkmerge_core::read_vpk_entry(&pak, MESH)?)?;
    preview.animations = vec![nm_clip_to_clip(
        &cast_clip,
        &skel,
        &preview.skeleton,
        "powerslash_cast",
    )];
    let glb = to_glb(&preview)?;
    let glb_path = format!("{out_dir}/yamato_powerslash_cast_amp.glb");
    std::fs::write(&glb_path, &glb)?;
    println!(
        "wrote {glb_path} ({} bytes) - play it in a glTF viewer",
        glb.len()
    );

    let out_vpk = format!("{out_dir}/yamato_powerslash_amp_dir.vpk");
    vpkmerge_core::pack(
        &[
            (CHARGE, charge_patched.as_slice()),
            (CAST, cast_patched.as_slice()),
        ],
        &out_vpk,
    )?;
    println!("\npacked amplified power-slash -> {out_vpk}");
    println!(
        "install: copy to game/citadel/addons/ as a free pakNN_dir.vpk; cast power slash in-game"
    );
    Ok(())
}
