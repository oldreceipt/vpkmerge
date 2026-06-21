//! Local `FeModel` decode check against a real cloth hero. Gated on
//! `MORPHIC_MODEL_VPK` pointing at a Deadlock pak that ships
//! `models/heroes_wip/necro/necro.vmdl_c` (override the entry with
//! `MORPHIC_MODEL_ENTRY`); skipped when unset so CI stays green.
//!
//! ```text
//! MORPHIC_MODEL_VPK=/path/to/pak50_dir.vpk \
//!   cargo test -p morphic --test femodel_local -- --nocapture
//! ```

use morphic::model::decode_fe_model;

#[test]
fn necro_fe_model_decodes_expected_shape() {
    let Ok(vpk_path) = std::env::var("MORPHIC_MODEL_VPK") else {
        eprintln!("MORPHIC_MODEL_VPK not set; skipping local FeModel check");
        return;
    };
    let entry = std::env::var("MORPHIC_MODEL_ENTRY")
        .unwrap_or_else(|_| "models/heroes_wip/necro/necro.vmdl_c".to_string());

    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let bytes = vpk
        .get_file(&entry)
        .and_then(|mut f| f.read_all())
        .unwrap_or_else(|e| panic!("read {entry}: {e:?}"));

    let fe = decode_fe_model(&bytes).expect("necro carries an FeModel");
    eprintln!(
        "{entry}: {} nodes, {} rods, {} capsules, {} pinned, iters {}/{}",
        fe.nodes.len(),
        fe.rods.len(),
        fe.capsules.len(),
        fe.pinned_count(),
        fe.extra_iterations,
        fe.extra_goal_iterations,
    );

    // Necro's authored data (verified against the committed sim.json dump).
    assert_eq!(fe.nodes.len(), 539, "node count");
    assert_eq!(fe.rods.len(), 2390, "rod count");
    assert_eq!(fe.capsules.len(), 19, "capsule count");
    assert_eq!(fe.pinned_count(), 86, "pinned (inv_mass 0) count");

    // Per-node collision masks fold from the BVH leaves (necro ships layers 7/11/15
    // on hair/backpack/accessory nodes); confirm the fold actually fired.
    assert!(
        fe.nodes.iter().any(|n| n.collision_mask != 0xFFFF),
        "per-node collision masks folded from the collision-tree leaves"
    );
}
