//! Export an NM animation clip (`.vnmclip_c`) as a **playable animated GLB**: load
//! the hero mesh (skeleton + skinned geometry), decode the clip with the pose
//! codec, convert it to a glTF animation driving the mesh by bone name, and write
//! a `.glb` any viewer can scrub/play. Works on the original game clip or an
//! edited one (a loose file or a VPK entry), so you can preview a custom motion
//! before installing it in-game.
//!
//! Usage:
//!   cargo run --release -p vpkmerge-core --example nm_clip_preview_glb -- \
//!       <pak01_dir.vpk> <mesh_entry> <clip: vpk-entry|file.vnmclip_c> <out.glb> [vpk-for-clip]
//! Example (Yamato's reload, from the base pak):
//!   ... pak01_dir.vpk models/heroes_staging/yamato_v2/yamato.vmdl_c \
//!       models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c yamato_reload.glb
//! Example (an edited clip packed in an addon):
//!   ... pak01_dir.vpk models/heroes_staging/yamato_v2/yamato.vmdl_c \
//!       models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c bow.glb addon_dir.vpk

use anyhow::{Context, Result};
use morphic::model::{decode, decode_nm_clip, decode_nm_skeleton, nm_clip_to_clip, to_glb};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: pak01_dir.vpk")?;
    let mesh_entry = args.next().context("missing arg: mesh entry")?;
    let clip_ref = args.next().context("missing arg: clip entry or file")?;
    let out = args.next().context("missing arg: out.glb")?;
    let clip_vpk = args.next(); // optional: a different VPK to read the clip from

    // Clip bytes: a loose file if it exists on disk, else a VPK entry.
    let clip_bytes = if std::path::Path::new(&clip_ref).is_file() {
        std::fs::read(&clip_ref)?
    } else {
        vpkmerge_core::read_vpk_entry(clip_vpk.as_deref().unwrap_or(&pak), &clip_ref)?
    };

    let clip = decode_nm_clip(&clip_bytes).context("decode clip")?;
    let skel = decode_nm_skeleton(&vpkmerge_core::read_vpk_entry(
        &pak,
        &resolve_skel(&clip.skeleton_ref),
    )?)
    .context("decode vnmskel")?;
    let mut model =
        decode(&vpkmerge_core::read_vpk_entry(&pak, &mesh_entry)?).context("decode mesh")?;

    let name = std::path::Path::new(&clip_ref)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("nm_clip");
    let anim = nm_clip_to_clip(&clip, &skel, &model.skeleton, name);
    println!(
        "clip '{name}': {} frames, {:.3}s ({:.1} fps), {} animated bone tracks of {} model bones",
        clip.frame_count,
        clip.duration,
        clip.fps(),
        anim.tracks.len(),
        model.skeleton.bones.len()
    );
    model.animations = vec![anim];

    let glb = to_glb(&model).context("write glb")?;
    std::fs::write(&out, &glb)?;
    println!(
        "wrote {out} ({} bytes) - open in any glTF viewer and play the animation",
        glb.len()
    );
    Ok(())
}

/// The clip's `m_skeleton` is an uncompiled ref (`models/.../h.vnmskel`); the VPK
/// entry is the compiled `_c`. Add the suffix if it is missing.
fn resolve_skel(reference: &str) -> String {
    if reference.ends_with("_c") {
        reference.to_owned()
    } else {
        format!("{reference}_c")
    }
}
