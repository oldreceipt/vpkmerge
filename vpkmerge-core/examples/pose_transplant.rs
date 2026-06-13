//! Viscous/Kelvin hero-select pose transplant: the zero-codec animation
//! experiment from docs/customization-frontier.md. Kelvin and Viscous are the
//! only two heroes sharing a skeleton, so Kelvin's `ui_hero_select` clip can
//! be packed at Viscous's entry path as a plain VPK override. If the engine
//! honors it, Viscous strikes Kelvin's pose on the hero-select screen and the
//! animation frontier is open.
//!
//! Usage: cargo run --release -p vpkmerge-core --example pose_transplant -- \
//!     <pak01_dir.vpk> <out_dir.vpk>

use anyhow::{bail, Context, Result};

const DONOR: &str = "models/heroes_staging/kelvin_v2/clip/ui_hero_select.vnmclip_c";
const RECIPIENT: &str = "models/heroes_staging/viscous/clips/ui_hero_select.vnmclip_c";

fn clip_summary(label: &str, bytes: &[u8]) -> Result<String> {
    let root = morphic::decode_kv3_resource(bytes)
        .with_context(|| format!("decoding {label} clip KV3"))?;
    let skeleton = root
        .get("m_skeleton")
        .and_then(|v| match v {
            morphic::kv3::Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "<missing>".into());
    let duration = root.get("m_flDuration").map_or(f64::NAN, |v| match v {
        morphic::kv3::Value::Double(d) => *d,
        morphic::kv3::Value::Int(i) => {
            // KV3 stores whole-number floats as ints; widen for display only.
            #[allow(clippy::cast_precision_loss)]
            {
                *i as f64
            }
        }
        _ => f64::NAN,
    });
    let additive = root
        .get("m_bIsAdditive")
        .map_or(false, |v| matches!(v, morphic::kv3::Value::Bool(true)));
    println!("{label}: {} bytes", bytes.len());
    println!("  skeleton: {skeleton}");
    println!("  duration: {duration:.3}s  additive: {additive}");
    Ok(skeleton)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out = args
        .next()
        .context("missing arg: output addon _dir.vpk path")?;

    let donor = vpkmerge_core::read_vpk_entry(&pak, DONOR)?;
    let recipient = vpkmerge_core::read_vpk_entry(&pak, RECIPIENT)?;

    let donor_skel = clip_summary("donor (kelvin)", &donor)?;
    let recipient_skel = clip_summary("recipient (viscous)", &recipient)?;

    if donor_skel != recipient_skel {
        bail!(
            "skeleton mismatch: donor targets {donor_skel}, recipient targets \
             {recipient_skel}; transplant would be rejected"
        );
    }
    println!("skeletons match; proceeding with transplant");

    vpkmerge_core::pack(&[(RECIPIENT, donor.as_slice())], &out)?;
    println!("packed donor clip at recipient path -> {out}");
    Ok(())
}
