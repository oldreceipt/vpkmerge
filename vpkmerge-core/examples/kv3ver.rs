// Tally the KV3 version of each color-bearing .vpcf_c under a hero's particle
// prefixes in a VPK. Diagnoses why the in-place scalar patcher (v5-only) skips
// some particles. usage: kv3ver <vpk> <prefix>...
use std::collections::BTreeMap;

/// First KV3 block version in `bytes`: the magic is [ver, 0x33,0x56,0x4B] ("3VK"
/// little-endian of 0x4B563300|ver). Find that signature, read the version byte.
fn kv3_version(bytes: &[u8]) -> Option<u8> {
    bytes
        .windows(3)
        .position(|w| w == [0x33, 0x56, 0x4B])
        .and_then(|i| {
            if i == 0 {
                None
            } else {
                let v = bytes[i - 1];
                (1..=5).contains(&v).then_some(v)
            }
        })
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = valve_pak::open(a.next().expect("vpk"))?;
    let prefixes: Vec<String> = a.collect();
    let mut by_version: BTreeMap<u8, u32> = BTreeMap::new();
    let mut unknown = 0u32;
    let paths: Vec<String> = vpk.file_paths().cloned().collect();
    for p in &paths {
        if !p.ends_with(".vpcf_c") || !prefixes.iter().any(|pre| p.starts_with(pre)) {
            continue;
        }
        let Ok(mut f) = vpk.get_file(p) else { continue };
        let Ok(bytes) = f.read_all() else { continue };
        match kv3_version(&bytes) {
            Some(v) => *by_version.entry(v).or_default() += 1,
            None => unknown += 1,
        }
    }
    for (v, n) in &by_version {
        println!("KV3 v{v}: {n}");
    }
    if unknown > 0 {
        println!("no-kv3-magic: {unknown}");
    }
    Ok(())
}
