//! Five quick, funny animation effects via the NM pose codec, each applied to a
//! set of clips and packed into one addon. All are pure compiled-clip edits (no
//! Blender, no SDK): decode -> transform -> re-encode/splice (or, for slow-mo, a
//! single duration patch). Only `tpose` needs the hero's `.vnmskel_c` (for the
//! bind/rest rotations); the rest need just the clips.
//!
//!   reverse  - play every frame backward (a run clip becomes a moonwalk)
//!   wobble   - add a slow sine sway to every animated bone (drunk/rubbery)
//!   amplify  - scale each bone's rotation arc about frame 0 (giant wind-up)
//!   tpose    - pin every bone to its bind/rest pose (rigid T/A-pose glide)
//!   slowmo   - stretch m_flDuration so the clip plays slower
//!
//! Usage:
//!   cargo run --release -p vpkmerge-core --example anim_fx -- \
//!       <pak01_dir.vpk> <effect> <out_dir> <skel_entry|-> <clip_entry...>

use std::f32::consts::TAU;

use anyhow::{Context, Result};
use morphic::kv3::{Seg, Value};
use morphic::model::{decode_nm_clip, encode_compressed_pose, NmClip, Quat};

const WOBBLE_DEG: f32 = 9.0;
const WOBBLE_CYCLES: f32 = 1.5;
const AMPLIFY: f32 = 2.0;
const SLOWMO: f32 = 3.0;

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

/// Rotation of `rad` about the bone's local Z (used for the wobble sway).
fn local_z(rad: f32) -> Quat {
    let h = rad * 0.5;
    Quat {
        x: 0.0,
        y: 0.0,
        z: h.sin(),
        w: h.cos(),
    }
}

/// Scale a rotation's angle by `f` (short way, axis preserved).
fn scale_angle(q: Quat, f: f32) -> Quat {
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
        };
    }
    let half = q.w.clamp(-1.0, 1.0).acos() * f;
    let k = half.sin() / s;
    Quat {
        x: q.x * k,
        y: q.y * k,
        z: q.z * k,
        w: half.cos(),
    }
}

/// Re-encode an edited clip and splice it back into `orig` (equal-length).
fn splice(orig: &[u8], clip: &NmClip, edited: &NmClip) -> Result<Vec<u8>> {
    let (blob, offsets) = encode_compressed_pose(edited);
    anyhow::ensure!(
        blob.len() == clip.compressed_pose_data.len() && offsets == clip.compressed_pose_offsets,
        "re-encoded stream changed shape"
    );
    morphic::patch_kv3_resource_blob(orig, &clip.compressed_pose_data, &blob)
        .context("splice edited stream")
}

fn reverse_clip(orig: &[u8]) -> Result<Vec<u8>> {
    let clip = decode_nm_clip(orig)?;
    let mut e = clip.clone();
    for t in &mut e.tracks {
        if let Some(v) = t.rotations.as_mut() {
            v.reverse();
        }
        if let Some(v) = t.translations.as_mut() {
            v.reverse();
        }
        if let Some(v) = t.scales.as_mut() {
            v.reverse();
        }
    }
    splice(orig, &clip, &e)
}

fn wobble_clip(orig: &[u8]) -> Result<Vec<u8>> {
    let clip = decode_nm_clip(orig)?;
    let frames = clip.frame_count as usize;
    let mut e = clip.clone();
    for (ti, t) in e.tracks.iter_mut().enumerate() {
        if let Some(rots) = t.rotations.as_mut() {
            #[allow(clippy::cast_precision_loss)]
            let phase = ti as f32 * 0.5;
            for (f, q) in rots.iter_mut().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let u = if frames > 1 {
                    f as f32 / (frames - 1) as f32
                } else {
                    0.0
                };
                let deg = WOBBLE_DEG * (WOBBLE_CYCLES * TAU * u + phase).sin();
                *q = normalize(qmul(*q, local_z(deg.to_radians())));
            }
        }
    }
    splice(orig, &clip, &e)
}

fn amplify_clip(orig: &[u8]) -> Result<Vec<u8>> {
    let clip = decode_nm_clip(orig)?;
    let mut e = clip.clone();
    for t in &mut e.tracks {
        if let Some(rots) = t.rotations.as_mut() {
            if rots.is_empty() {
                continue;
            }
            let reference = rots[0];
            for q in rots.iter_mut() {
                let delta = qmul(
                    Quat {
                        x: -reference.x,
                        y: -reference.y,
                        z: -reference.z,
                        w: reference.w,
                    },
                    *q,
                );
                *q = normalize(qmul(reference, scale_angle(delta, AMPLIFY)));
            }
        }
    }
    splice(orig, &clip, &e)
}

fn tpose_clip(orig: &[u8], bind: &[Quat]) -> Result<Vec<u8>> {
    let clip = decode_nm_clip(orig)?;
    let mut e = clip.clone();
    for (i, t) in e.tracks.iter_mut().enumerate() {
        let Some(&rest) = bind.get(i) else { continue };
        if let Some(rots) = t.rotations.as_mut() {
            for q in rots.iter_mut() {
                *q = rest;
            }
        }
    }
    let mut out = splice(orig, &clip, &e)?;
    // Pin the static rotation constants to bind too (the non-animated bones).
    let mut edits: Vec<(Vec<Seg>, f64)> = Vec::new();
    for (i, t) in clip.tracks.iter().enumerate() {
        if t.rotations.is_some() {
            continue;
        }
        let Some(&rest) = bind.get(i) else { continue };
        for (c, v) in [rest.x, rest.y, rest.z, rest.w].into_iter().enumerate() {
            edits.push((
                vec![
                    Seg::Key("m_trackCompressionSettings".into()),
                    Seg::Index(i),
                    Seg::Key("m_constantRotation".into()),
                    Seg::Index(c),
                ],
                f64::from(v),
            ));
        }
    }
    if !edits.is_empty() {
        out = morphic::patch_kv3_resource_doubles(&out, &edits)
            .or_else(|_| {
                let f: Vec<_> = edits.iter().map(|(p, v)| (p.clone(), *v as f32)).collect();
                morphic::patch_kv3_resource_floats(&out, &f)
            })
            .context("pin static constants to bind")?;
    }
    Ok(out)
}

fn slowmo_clip(orig: &[u8]) -> Result<Vec<u8>> {
    let clip = decode_nm_clip(orig)?;
    let new = f64::from(clip.duration) * f64::from(SLOWMO);
    let path = vec![Seg::Key("m_flDuration".into())];
    morphic::patch_kv3_resource_doubles(orig, &[(path.clone(), new)])
        .or_else(|_| morphic::patch_kv3_resource_floats(orig, &[(path, new as f32)]))
        .context("patch m_flDuration")
}

/// Read each bone's bind (rest) local rotation from a `.vnmskel_c`'s
/// `m_parentSpaceReferencePose` (per bone: `[px,py,pz, scale, qx,qy,qz,qw]`).
fn read_bind_rotations(skel_bytes: &[u8]) -> Result<Vec<Quat>> {
    let root = morphic::decode_kv3_resource(skel_bytes)?;
    let arr = root
        .get("m_parentSpaceReferencePose")
        .and_then(Value::as_array)
        .context("vnmskel missing m_parentSpaceReferencePose")?;
    let f = |v: &Value| v.as_f64().unwrap_or(0.0) as f32;
    // Either an array of 8-float arrays, or one flat array of N*8 floats.
    let nested = arr.first().map(Value::as_array).unwrap_or(None).is_some();
    let mut out = Vec::new();
    if nested {
        for e in arr {
            if let Some(a) = e.as_array() {
                if a.len() >= 8 {
                    out.push(Quat {
                        x: f(&a[4]),
                        y: f(&a[5]),
                        z: f(&a[6]),
                        w: f(&a[7]),
                    });
                }
            }
        }
    } else {
        for chunk in arr.chunks_exact(8) {
            out.push(Quat {
                x: f(&chunk[4]),
                y: f(&chunk[5]),
                z: f(&chunk[6]),
                w: f(&chunk[7]),
            });
        }
    }
    Ok(out)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: pak01_dir.vpk")?;
    let effect = args.next().context("missing arg: effect")?;
    let out_dir = args.next().context("missing arg: out_dir")?;
    let skel_arg = args.next().context("missing arg: skel entry or -")?;
    let entries: Vec<String> = args.collect();
    anyhow::ensure!(!entries.is_empty(), "no clip entries given");

    let bind = if effect == "tpose" {
        let skel = vpkmerge_core::read_vpk_entry(&pak, &skel_arg)?;
        Some(read_bind_rotations(&skel)?)
    } else {
        None
    };

    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in &entries {
        let Ok(bytes) = vpkmerge_core::read_vpk_entry(&pak, entry) else {
            println!("  skip (not found): {entry}");
            continue;
        };
        let out = match effect.as_str() {
            "reverse" => reverse_clip(&bytes),
            "wobble" => wobble_clip(&bytes),
            "amplify" => amplify_clip(&bytes),
            "tpose" => tpose_clip(&bytes, bind.as_deref().unwrap()),
            "slowmo" => slowmo_clip(&bytes),
            other => anyhow::bail!("unknown effect {other}"),
        };
        match out {
            Ok(b) => {
                println!("  {effect}: {entry}");
                packed.push((entry.clone(), b));
            }
            Err(e) => println!("  FAIL {entry}: {e}"),
        }
    }
    anyhow::ensure!(!packed.is_empty(), "nothing edited");

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    let out_vpk = format!("{out_dir}/{effect}_dir.vpk");
    vpkmerge_core::pack(&refs, &out_vpk)?;
    println!("packed {} clips -> {out_vpk}", packed.len());
    Ok(())
}
