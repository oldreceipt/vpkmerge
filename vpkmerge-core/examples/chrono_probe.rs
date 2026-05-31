// One-off: probe Paradox (chrono) skin assets for the trippy-scroll reskin.
// Prints texture format/dims/HDR-ness for the body+gun albedo, and KV3 blob
// status for the body+gun materials (decides whether the scroll-param re-encode
// is safe). usage: cargo run --release --example chrono_probe -- <pak01_dir.vpk>
use morphic::ImageData;

const BODY_COLOR: &str =
    "models/heroes_staging/chrono/materials/chrono_v2_color_png_d1d22ba7.vtex_c";
const GUN_COLOR: &str =
    "models/heroes_staging/chrono/chrono_gun/materials/chrono_gun_vmat_g_tcolor_7d4419c1.vtex_c";
const EMISSIVE: &str =
    "models/heroes_staging/chrono/materials/chrono_v2_emissive_png_718bd18c.vtex_c";
const BODY_VMAT: &str = "models/heroes_staging/chrono/materials/chrono_v2.vmat_c";
const GUN_VMAT: &str = "models/heroes_staging/chrono/chrono_gun/materials/chrono_gun.vmat_c";

fn main() -> anyhow::Result<()> {
    let pak = std::env::args()
        .nth(1)
        .expect("usage: chrono_probe <pak01_dir.vpk>");

    for e in [BODY_COLOR, GUN_COLOR, EMISSIVE] {
        let b = vpkmerge_core::read_vpk_entry(&pak, e)?;
        let info = morphic::inspect(&b)?;
        let hdr = match morphic::decode(&b).map(|i| i.data) {
            Ok(ImageData::Rgba8(_)) => "LDR8",
            Ok(ImageData::Rgba16F(_)) => "HDR16F",
            Err(err) => {
                println!(
                    "{e}\n  {:?} {}x{} mips={} (decode err: {err})",
                    info.format, info.width, info.height, info.mip_count
                );
                continue;
            }
        };
        println!(
            "{e}\n  {:?} {}x{} mips={} {hdr}",
            info.format, info.width, info.height, info.mip_count
        );
    }

    for v in [BODY_VMAT, GUN_VMAT] {
        let b = vpkmerge_core::read_vpk_entry(&pak, v)?;
        println!("{v}\n  has_blobs={:?}", morphic::kv3_resource_has_blobs(&b));
    }
    Ok(())
}
