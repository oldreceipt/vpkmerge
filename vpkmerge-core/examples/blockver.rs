// Parse a .vmdl_c container and report each MVTX/MIDX-ish block's meshopt
// codec version byte (0xaV vertex, 0xeV index) + size.
// usage: cargo run --example blockver -- <vpk> <entry>
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = valve_pak::open(&a.next().expect("vpk"))?;
    let b = vpk
        .get_file(&a.next().expect("entry"))
        .expect("e")
        .read_all()?;
    let table = 8 + u32le(&b, 8) as usize;
    let n = u32le(&b, 12) as usize;
    println!("file {} bytes, {} blocks, table@{}", b.len(), n, table);
    for i in 0..n {
        let e = table + i * 12;
        let kind = String::from_utf8_lossy(&b[e..e + 4]).to_string();
        let abs = (e + 4) + u32le(&b, e + 4) as usize;
        let size = u32le(&b, e + 8) as usize;
        let first = b.get(abs).copied().unwrap_or(0);
        let note = if first & 0xF0 == 0xa0 {
            format!("meshopt-VERTEX v{}", first & 0xF)
        } else if first & 0xF0 == 0xe0 {
            format!("meshopt-INDEX v{}", first & 0xF)
        } else {
            String::new()
        };
        println!("  [{i}] {kind} off={abs} size={size} first=0x{first:02x} {note}");
    }
    Ok(())
}
