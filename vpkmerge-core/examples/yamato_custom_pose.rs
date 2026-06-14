//! Author the first custom Deadlock animation: edit Yamato's static
//! `ui_hero_select` pose (raise an arm, tilt the head, lean the torso),
//! byte-faithfully patch the `m_constantRotation` quaternions in place, and pack
//! the result at her `reload_idle` + `reload_idle_quick` paths (the proven
//! press-R taunt slots). Offline verification: bakes the edited pose onto Yamato's
//! mesh and reports vertex displacement vs. the unedited pose, and writes a GLB to
//! eyeball.
//!
//! Run with no out-dir to INSPECT (dump bone names + candidate target bones):
//!   cargo run --release -p vpkmerge-core --example yamato_custom_pose -- <pak01_dir.vpk>
//! Run with an out-dir to BUILD the addon + a before/after GLB:
//!   cargo run --release -p vpkmerge-core --example yamato_custom_pose -- <pak01_dir.vpk> <out_dir>

use anyhow::{Context, Result};
use morphic::kv3::Seg;
use morphic::model::{bake_nm_pose, decode, decode_nm_pose, decode_nm_skeleton, to_glb, Quat};

const CLIP: &str = "models/heroes_wip/yamato/clips/ui_hero_select.vnmclip_c";
const SKEL: &str = "models/heroes_wip/yamato/yamato.vnmskel_c";
const MESH: &str = "models/heroes_staging/yamato_v2/yamato.vmdl_c";
const RELOAD_IDLE: &str = "models/heroes_wip/yamato/clips/reload_idle.vnmclip_c";
const RELOAD_IDLE_QUICK: &str = "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c";

/// Hamilton product (q then r, i.e. r applied after q in the same frame).
fn qmul(a: Quat, b: Quat) -> Quat {
    Quat {
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
    }
}

fn axis_angle(axis: [f32; 3], deg: f32) -> Quat {
    let r = deg.to_radians() * 0.5;
    let s = r.sin();
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    Quat {
        x: axis[0] / n * s,
        y: axis[1] / n * s,
        z: axis[2] / n * s,
        w: r.cos(),
    }
}

fn find(names: &[String], wants: &[&str]) -> Option<usize> {
    wants.iter().find_map(|w| {
        names
            .iter()
            .position(|n| n.to_ascii_lowercase().contains(w))
    })
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next();

    let clip_bytes = vpkmerge_core::read_vpk_entry(&pak, CLIP)?;
    let skel_bytes = vpkmerge_core::read_vpk_entry(&pak, SKEL)?;
    let skel = decode_nm_skeleton(&skel_bytes)?;
    let pose = decode_nm_pose(&clip_bytes)?;
    println!(
        "clip {CLIP}\n  skeleton_ref {} | {} bones | {} static",
        pose.skeleton_ref,
        skel.bone_names.len(),
        pose.static_bone_count()
    );

    // Pick target bones by name (Deadlock rigs vary; try a few aliases).
    let head = find(&skel.bone_names, &["head"]);
    let arm = find(
        &skel.bone_names,
        &[
            "arm_upper_l",
            "l_upperarm",
            "upperarm_l",
            "clavicle_l",
            "shoulder_l",
        ],
    );
    let spine = find(
        &skel.bone_names,
        &["spine_2", "spine2", "spine_1", "spine_0", "spine"],
    );
    println!(
        "  targets: head={:?} arm={:?} spine={:?}",
        head.map(|i| &skel.bone_names[i]),
        arm.map(|i| &skel.bone_names[i]),
        spine.map(|i| &skel.bone_names[i]),
    );

    if out_dir.is_none() {
        println!("\nall bones:");
        for (i, n) in skel.bone_names.iter().enumerate() {
            println!("  {i:>3} {n}");
        }
        println!("\n(inspect mode; pass an out-dir to build the addon)");
        return Ok(());
    }
    let out_dir = out_dir.unwrap();

    // The edit: a dramatic, unmistakable pose. Each delta is applied in the bone's
    // LOCAL space (post-multiply), so it composes onto the authored bind rotation.
    let edits: Vec<(usize, Quat)> = [
        // raise the left arm out and up
        arm.map(|i| (i, axis_angle([0.0, 0.0, 1.0], 75.0))),
        // tilt the head
        head.map(|i| (i, axis_angle([1.0, 0.0, 0.0], 25.0))),
        // lean the torso back
        spine.map(|i| (i, axis_angle([1.0, 0.0, 0.0], 20.0))),
    ]
    .into_iter()
    .flatten()
    .collect();
    println!("\napplying {} bone edits", edits.len());

    // Build the edited pose (for offline bake verification) and the float-patch
    // edit list (for the addon).
    let mut edited_pose = pose.clone();
    let mut patch_d: Vec<(Vec<Seg>, f64)> = Vec::new();
    for &(bone, delta) in &edits {
        let Some(lp) = edited_pose.bones.get_mut(bone).and_then(Option::as_mut) else {
            println!("  bone {bone} is not static; skipping");
            continue;
        };
        let nq = qmul(lp.rotation, delta);
        let n = (nq.x * nq.x + nq.y * nq.y + nq.z * nq.z + nq.w * nq.w).sqrt();
        let nq = Quat {
            x: nq.x / n,
            y: nq.y / n,
            z: nq.z / n,
            w: nq.w / n,
        };
        println!(
            "  {:>3} {}: ({:.3},{:.3},{:.3},{:.3}) -> ({:.3},{:.3},{:.3},{:.3})",
            bone,
            skel.bone_names[bone],
            lp.rotation.x,
            lp.rotation.y,
            lp.rotation.z,
            lp.rotation.w,
            nq.x,
            nq.y,
            nq.z,
            nq.w
        );
        lp.rotation = nq;
        for (c, v) in [nq.x, nq.y, nq.z, nq.w].into_iter().enumerate() {
            patch_d.push((
                vec![
                    Seg::Key("m_trackCompressionSettings".into()),
                    Seg::Index(bone),
                    Seg::Key("m_constantRotation".into()),
                    Seg::Index(c),
                ],
                f64::from(v),
            ));
        }
    }

    // Offline verification: bake unedited + edited onto Yamato's mesh, measure
    // how much the pose moved, and write a GLB to eyeball.
    let model = decode(&vpkmerge_core::read_vpk_entry(&pak, MESH)?)?;
    let baked_before = bake_nm_pose(&model, &skel, &pose)?;
    let baked_after = bake_nm_pose(&model, &skel, &edited_pose)?;
    report_displacement(&baked_before, &baked_after);
    let glb = to_glb(&baked_after)?;
    let glb_path = format!("{out_dir}/yamato_custom_pose.glb");
    std::fs::write(&glb_path, &glb)?;
    println!("wrote {glb_path} ({} bytes) for visual check", glb.len());

    // Apply the byte-faithful float patches to the clip. Try doubles then floats
    // per component (KV3 stores these either way).
    let mut patched = clip_bytes.clone();
    let mut applied = 0usize;
    for (path, v) in &patch_d {
        if let Ok(p) = morphic::patch_kv3_resource_doubles(&patched, &[(path.clone(), *v)]) {
            patched = p;
            applied += 1;
        } else if let Ok(p) =
            morphic::patch_kv3_resource_floats(&patched, &[(path.clone(), *v as f32)])
        {
            patched = p;
            applied += 1;
        } else {
            println!("  WARN: could not patch {path:?}");
        }
    }
    println!("patched {applied}/{} rotation components", patch_d.len());

    // Sanity: re-decode the patched clip and confirm the targeted rotations match.
    let redec = decode_nm_pose(&patched)?;
    for &(bone, _) in &edits {
        if let (Some(a), Some(b)) = (
            redec.bones.get(bone).and_then(|o| o.as_ref()),
            edited_pose.bones.get(bone).and_then(|o| o.as_ref()),
        ) {
            let d = (a.rotation.x - b.rotation.x).abs()
                + (a.rotation.y - b.rotation.y).abs()
                + (a.rotation.z - b.rotation.z).abs()
                + (a.rotation.w - b.rotation.w).abs();
            assert!(d < 1e-3, "bone {bone} re-decode mismatch {d}");
        }
    }
    println!("re-decode confirms patched rotations");

    // Pack the edited clip at both reload slots -> one addon VPK.
    let out_vpk = format!("{out_dir}/yamato_reload_taunt_dir.vpk");
    vpkmerge_core::pack(
        &[
            (RELOAD_IDLE, patched.as_slice()),
            (RELOAD_IDLE_QUICK, patched.as_slice()),
        ],
        &out_vpk,
    )?;
    println!("\npacked custom pose at reload_idle + reload_idle_quick -> {out_vpk}");
    println!(
        "install: copy to Deadlock game/citadel/addons/ as a free pakNN_dir.vpk; press R in-game"
    );
    Ok(())
}

fn report_displacement(before: &morphic::model::Model, after: &morphic::model::Model) {
    let (mut moved, mut total, mut maxd) = (0usize, 0usize, 0f32);
    for (pb, bb) in after.meshes.iter().zip(before.meshes.iter()) {
        for (pvb, bvb) in pb.vertex_buffers.iter().zip(bb.vertex_buffers.iter()) {
            for (pp, bp) in pvb.positions.iter().zip(bvb.positions.iter()) {
                let d =
                    ((pp[0] - bp[0]).powi(2) + (pp[1] - bp[1]).powi(2) + (pp[2] - bp[2]).powi(2))
                        .sqrt();
                total += 1;
                maxd = maxd.max(d);
                if d > 0.5 {
                    moved += 1;
                }
            }
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let pct = if total > 0 {
        moved as f64 / total as f64 * 100.0
    } else {
        0.0
    };
    println!(
        "pose displacement vs unedited: {moved}/{total} verts moved >0.5u ({pct:.1}%), max {maxd:.1}u"
    );
}
