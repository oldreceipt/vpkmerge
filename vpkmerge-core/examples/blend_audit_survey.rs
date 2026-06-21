// BLEND-AUDIT-TEMP  --  throwaway read-only survey, safe to delete.
// Scans every hero .vmat_c in a pak and reports REAL examples per "improvement"
// category (self-illum localized/colored, additive glow, glass, translucency,
// advanced translucency, backfaces, NPR/cel). Reads only; writes nothing.
//
//   cargo run --release -p vpkmerge-core --example blend_audit_survey -- "<pak01_dir.vpk>" [pathfilter]
//
// default pathfilter = "heroes" (hero materials only). To remove: rm this file.

use morphic::material::Material;

struct Hit {
    path: String,
    detail: String,
    rank: i32, // higher = better demonstration, for sorting
}

fn push(v: &mut Vec<Hit>, path: &str, detail: String, rank: i32) {
    v.push(Hit {
        path: path.to_string(),
        detail,
        rank,
    });
}

fn print_cat(title: &str, total: usize, hits: &mut Vec<Hit>, show: usize) {
    hits.sort_by(|a, b| b.rank.cmp(&a.rank).then(a.path.cmp(&b.path)));
    println!("\n## {title}  ({total} materials)");
    for h in hits.iter().take(show) {
        // strip the long "materials/models/heroes/" prefix for readability
        let short = h.path.strip_prefix("materials/").unwrap_or(&h.path);
        println!("  {short}\n      {}", h.detail);
    }
    if total > show {
        println!("  ... +{} more", total - show);
    }
}

fn fget(m: &Material, names: &[&str]) -> Option<f32> {
    names.iter().find_map(|n| m.float_params.get(*n).copied())
}
fn iflag(m: &Material, n: &str) -> bool {
    m.int_params.get(n).copied().unwrap_or(0) != 0
}
fn find_key<'a, V>(
    map: &'a std::collections::BTreeMap<String, V>,
    needle: &str,
) -> Option<(&'a String, &'a V)> {
    map.iter()
        .find(|(k, _)| k.to_ascii_lowercase().contains(needle))
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a
        .next()
        .expect("usage: blend_audit_survey <pak> [pathfilter]");
    let filter = a.next().unwrap_or_else(|| "heroes".into());

    let info = vpkmerge_core::inspect(&pak)?;
    let vmats: Vec<&String> = info
        .file_paths
        .iter()
        .filter(|p| p.ends_with(".vmat_c") && p.to_ascii_lowercase().contains(&filter))
        .collect();
    eprintln!(
        "scanning {} .vmat_c entries matching '{filter}'",
        vmats.len()
    );

    let (mut decoded, mut failed) = (0usize, 0usize);
    let mut npr = 0usize;
    let (
        mut self_illum,
        mut additive,
        mut glass,
        mut translucent,
        mut advanced,
        mut backfaces,
        mut unlit,
        mut sheen,
        mut cloak,
    ) = (
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    for entry in &vmats {
        let Ok(bytes) = vpkmerge_core::read_vpk_entry(&pak, entry) else {
            failed += 1;
            continue;
        };
        let Ok(m) = morphic::material::parse(&bytes) else {
            failed += 1;
            continue;
        };
        decoded += 1;
        let p = entry.as_str();
        let shader = m.shader_name.as_str();

        if iflag(&m, "F_USE_NPR_LIGHTING") {
            npr += 1;
        }

        // --- self-illum: scale + dynamic expr + tint color + mask texture -----
        if iflag(&m, "F_SELF_ILLUM") {
            let scale = fget(&m, &["g_flSelfIllumScale1", "g_flSelfIllumScale"]).unwrap_or(0.0);
            let dyn_scale =
                find_key(&m.dynamic_params, "selfillumscale").map(|(_, e)| e.source.clone());
            let tint = find_key(&m.vector_params, "selfillum")
                .filter(|(k, _)| {
                    k.to_ascii_lowercase().contains("tint")
                        || k.to_ascii_lowercase().contains("color")
                })
                .map(|(_, v)| *v);
            let masked = find_key(&m.texture_params, "selfillum").is_some();
            let colored = tint
                .map(|t| {
                    let (r, g, b) = (t[0], t[1], t[2]);
                    (r - g).abs() > 0.04 || (g - b).abs() > 0.04 || (r - b).abs() > 0.04
                })
                .unwrap_or(false);
            let active = scale > 0.05 || dyn_scale.is_some();
            if active {
                let mut d = format!("scale={scale:.2}");
                if let Some(e) = &dyn_scale {
                    d += &format!(" dyn=\"{e}\"");
                }
                if let Some(t) = tint {
                    d += &format!(" tint=[{:.2},{:.2},{:.2}]", t[0], t[1], t[2]);
                }
                d += &format!(
                    " mask={} colored={} valid={}",
                    masked,
                    colored,
                    iflag(&m, "F_SELF_ILLUM")
                ); // valid flag lives in extras; flag here is the feature
                   // rank: colored+masked is the best "localized/colored" demo
                let rank = (colored as i32) * 100 + (masked as i32) * 50 + scale.min(20.0) as i32;
                push(&mut self_illum, p, d, rank);
            }
        }

        if iflag(&m, "F_ADDITIVE_BLEND") {
            let scale = fget(&m, &["g_flSelfIllumScale1", "g_flSelfIllumScale"]).unwrap_or(0.0);
            push(
                &mut additive,
                p,
                format!("shader={shader} siScale={scale:.2}"),
                scale.min(20.0) as i32,
            );
        }
        if iflag(&m, "F_GLASS") || shader.ends_with("_glass.vfx") {
            let ior = fget(&m, &["g_flIOR", "g_flRefractionIndex"]).unwrap_or(0.0);
            push(
                &mut glass,
                p,
                format!("shader={shader} IOR={ior:.2}"),
                (iflag(&m, "F_GLASS") as i32) * 10,
            );
        }
        if iflag(&m, "F_TRANSLUCENT") {
            push(&mut translucent, p, format!("shader={shader}"), 0);
        }
        if iflag(&m, "F_ADVANCED_TRANSLUCENCY") {
            push(&mut advanced, p, format!("shader={shader}"), 0);
        }
        if iflag(&m, "F_RENDER_BACKFACES") {
            push(&mut backfaces, p, format!("shader={shader}"), 0);
        }
        if iflag(&m, "F_UNLIT") {
            push(&mut unlit, p, format!("shader={shader}"), 0);
        }
        if iflag(&m, "F_SHEEN") {
            push(&mut sheen, p, format!("shader={shader}"), 0);
        }
        if iflag(&m, "F_CLOAK") {
            push(&mut cloak, p, format!("shader={shader}"), 0);
        }
    }

    println!(
        "\n# decoded {decoded} / {} ({failed} failed)  |  F_USE_NPR_LIGHTING=1: {npr}",
        vmats.len()
    );
    print_cat(
        "SELF-ILLUM (localized=mask, colored=non-gray tint)",
        self_illum.len(),
        &mut self_illum,
        18,
    );
    print_cat(
        "ADDITIVE GLOW OVERLAY (F_ADDITIVE_BLEND)",
        additive.len(),
        &mut additive,
        18,
    );
    print_cat("GLASS (F_GLASS / *_glass.vfx)", glass.len(), &mut glass, 14);
    print_cat(
        "TRANSLUCENT (F_TRANSLUCENT)",
        translucent.len(),
        &mut translucent,
        14,
    );
    print_cat(
        "ADVANCED TRANSLUCENCY (F_ADVANCED_TRANSLUCENCY)",
        advanced.len(),
        &mut advanced,
        14,
    );
    print_cat(
        "BACKFACES (F_RENDER_BACKFACES)",
        backfaces.len(),
        &mut backfaces,
        14,
    );
    print_cat("UNLIT (F_UNLIT)", unlit.len(), &mut unlit, 10);
    print_cat("SHEEN (F_SHEEN)", sheen.len(), &mut sheen, 10);
    print_cat("CLOAK (F_CLOAK)", cloak.len(), &mut cloak, 10);
    Ok(())
}
