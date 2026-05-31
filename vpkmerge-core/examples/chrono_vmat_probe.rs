// Probe whether chrono_v2.vmat_c scroll/scale params are patchable IN PLACE
// (real stored doubles/floats) vs tagless zeros that need a re-encode. This
// decides how to set g_vAlbedoScrollSpeed1 without the full re-encode that broke
// the material in-game. usage: chrono_vmat_probe <pak01_dir.vpk>
use morphic::kv3::{Seg, Value};

const BODY_VMAT: &str = "models/heroes_staging/chrono/materials/chrono_v2.vmat_c";

fn vparam_index(v: &Value, name: &str) -> Option<usize> {
    v.get("m_vectorParams")?
        .as_array()?
        .iter()
        .position(|p| p.get("m_name").and_then(Value::as_str) == Some(name))
}
fn fparam_index(v: &Value, name: &str) -> Option<usize> {
    v.get("m_floatParams")?
        .as_array()?
        .iter()
        .position(|p| p.get("m_name").and_then(Value::as_str) == Some(name))
}

fn main() -> anyhow::Result<()> {
    let pak = std::env::args()
        .nth(1)
        .expect("usage: chrono_vmat_probe <pak01_dir.vpk>");
    let bytes = vpkmerge_core::read_vpk_entry(&pak, BODY_VMAT)?;
    let v = morphic::decode_kv3_resource(&bytes)?;

    // Try to set g_vAlbedoScrollSpeed1[0] in place via the double patcher.
    if let Some(i) = vparam_index(&v, "g_vAlbedoScrollSpeed1") {
        let path = vec![
            Seg::Key("m_vectorParams".into()),
            Seg::Index(i),
            Seg::Key("m_value".into()),
            Seg::Index(0),
        ];
        let r = morphic::patch_kv3_resource_doubles(&bytes, &[(path, 0.08)]);
        println!(
            "scroll[0] in-place double patch: {}",
            match &r {
                Ok(_) => "OK (real double, patchable!)".into(),
                Err(e) => format!("ERR ({e}) -> tagless"),
            }
        );
    } else {
        println!("g_vAlbedoScrollSpeed1 not found");
    }

    // Sanity: a param known to hold a real nonzero value (self-illum scale 3.649).
    if let Some(i) = fparam_index(&v, "g_flSelfIllumScale1") {
        let path = vec![
            Seg::Key("m_floatParams".into()),
            Seg::Index(i),
            Seg::Key("m_flValue".into()),
        ];
        let rd = morphic::patch_kv3_resource_doubles(&bytes, &[(path.clone(), 5.0)]);
        let rf = morphic::patch_kv3_resource_floats(&bytes, &[(path, 5.0f32)]);
        println!(
            "selfIllumScale double-patch: {}  float-patch: {}",
            if rd.is_ok() { "OK" } else { "ERR" },
            if rf.is_ok() { "OK" } else { "ERR" }
        );
    }

    // Does an in-place rewrap (no value change) even round-trip? (set_doubles with
    // a real hit rewraps uncompressed; if that alone loads in-game, scroll is viable.)
    println!("has_blobs={:?}", morphic::kv3_resource_has_blobs(&bytes));
    Ok(())
}
