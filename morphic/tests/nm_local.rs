//! Local (non-CI) check for the NM loose-clip pose path: the static menu-pose
//! decode (`.vnmclip_c` + `.vnmskel_c`) and the by-name bake onto a WIP hero's
//! mesh skeleton. Gated on `MORPHIC_MODEL_VPK` pointing at a Deadlock
//! `pak01_dir.vpk` and skipped otherwise (the model + clip + skeleton are
//! multi-megabyte and not committed). Asserts the structural invariants the
//! recon established (see `docs/handoff-nm-loose-clip-pose.md`):
//!  - the menu clip is single-frame-equivalent and fully static,
//!  - the NM bones are a by-name subset of the model's mesh skeleton whose model
//!    parents are also NM bones (so FK through the model skeleton is consistent),
//!  - and the baked pose meaningfully displaces the mesh from bind.
//!
//! Defaults target Apollo (`fencer`); override with `MORPHIC_NM_*` to point at
//! another WIP hero.

use std::collections::HashSet;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[test]
fn nm_static_pose_bakes_a_real_pose() {
    let Ok(vpk_path) = std::env::var("MORPHIC_MODEL_VPK") else {
        eprintln!("MORPHIC_MODEL_VPK not set; skipping local NM pose check");
        return;
    };
    let model_e = env_or("MORPHIC_NM_MODEL", "models/heroes_wip/fencer/fencer.vmdl_c");
    let clip_e = env_or(
        "MORPHIC_NM_CLIP",
        "models/heroes_wip/fencer/clips/ui_hero_select.vnmclip_c",
    );
    let skel_e = env_or(
        "MORPHIC_NM_SKEL",
        "models/heroes_wip/fencer/fencer.vnmskel_c",
    );

    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let read = |e: &str| {
        vpk.get_file(e)
            .unwrap_or_else(|_| panic!("entry {e} not found"))
            .read_all()
            .expect("read entry")
    };

    let model = morphic::model::decode(&read(&model_e)).expect("decode model");
    let skel = morphic::model::decode_nm_skeleton(&read(&skel_e)).expect("decode vnmskel");
    let pose = morphic::model::decode_nm_pose(&read(&clip_e)).expect("decode vnmclip");

    // The menu pose is fully static: every track decoded to a constant.
    assert_eq!(
        pose.bones.len(),
        skel.bone_names.len(),
        "clip track count must equal skeleton bone count"
    );
    assert_eq!(
        pose.static_bone_count(),
        pose.bones.len(),
        "menu pose clip is expected to be fully static (no compressed stream)"
    );

    // NM bones are a by-name subset of the model skeleton, and each NM bone's
    // parent in the model skeleton is itself an NM bone (or a root): the
    // precondition for FK through the model skeleton to reproduce the pose.
    let nm: HashSet<&str> = skel.bone_names.iter().map(String::as_str).collect();
    for name in &skel.bone_names {
        let bone = model
            .skeleton
            .bones
            .iter()
            .find(|b| &b.name == name)
            .unwrap_or_else(|| panic!("nm bone {name} missing from model skeleton"));
        if let Some(p) = bone.parent {
            let parent = &model.skeleton.bones[p].name;
            assert!(
                nm.contains(parent.as_str()),
                "nm bone {name} has non-nm model parent {parent}"
            );
        }
    }

    // The baked pose must move a large fraction of the mesh away from bind.
    let bind = morphic::model::bake_pose(&model, &["__no_such_clip__"], 0);
    let posed = morphic::model::bake_nm_pose(&model, &skel, &pose).expect("bake nm pose");
    let (mut moved, mut total) = (0usize, 0usize);
    for (pb, bb) in posed.meshes.iter().zip(bind.meshes.iter()) {
        for (pvb, bvb) in pb.vertex_buffers.iter().zip(bb.vertex_buffers.iter()) {
            for (pp, bp) in pvb.positions.iter().zip(bvb.positions.iter()) {
                let d2 =
                    (pp[0] - bp[0]).powi(2) + (pp[1] - bp[1]).powi(2) + (pp[2] - bp[2]).powi(2);
                total += 1;
                if d2 > 0.0001 {
                    moved += 1;
                }
            }
        }
    }
    assert!(total > 0, "posed mesh has vertices");
    #[allow(clippy::cast_precision_loss)]
    let frac = moved as f64 / total as f64;
    assert!(
        frac > 0.5,
        "expected the menu pose to displace most vertices from bind, got {:.1}%",
        frac * 100.0
    );
    // Posed output is a static mesh: no skeleton, skin, or clips.
    assert!(posed.skeleton.bones.is_empty());
    assert!(posed.animations.is_empty());
}
