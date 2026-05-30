// Diagnose .vpcf_c re-encode: dump the Source 2 resource block table for the
// base entry vs a decode->encode identity round-trip, to see what the encoder
// changes/drops.
// usage: cargo run --example resource_blocks -- <vpk> <entry>
fn rd_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn dump_blocks(label: &str, b: &[u8]) {
    println!("-- {label}: {} bytes --", b.len());
    if b.len() < 16 {
        println!("   too small");
        return;
    }
    let file_size = rd_u32(b, 0);
    let header_version = rd_u16(b, 4);
    let resource_version = rd_u16(b, 6);
    let block_offset = rd_u32(b, 8); // relative to offset 8
    let block_count = rd_u32(b, 12);
    println!(
        "   file_size={file_size} header_ver={header_version} res_ver={resource_version} block_count={block_count}"
    );
    let mut p = 8 + block_offset as usize;
    for i in 0..block_count {
        if p + 12 > b.len() {
            println!("   block {i}: TRUNCATED (offset {p})");
            break;
        }
        let ty = String::from_utf8_lossy(&b[p..p + 4]).to_string();
        let rel = rd_u32(b, p + 4);
        let size = rd_u32(b, p + 8);
        let abs = p + 4 + rel as usize;
        println!("   block {i}: {ty:?} offset={abs} size={size}");
        p += 12;
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let vpk_path = args.next().expect("vpk");
    let entry = args.next().expect("entry");
    let vpk = valve_pak::open(&vpk_path)?;
    let mut f = vpk.get_file(&entry).expect("entry");
    let orig = f.read_all()?;

    dump_blocks("ORIGINAL (base .vpcf_c)", &orig);

    // identity round-trip: decode the DATA KV3 and re-encode it unchanged.
    let value = morphic::decode_kv3_resource(&orig)?;
    let reencoded = morphic::encode_kv3_resource(&orig, &value)?;
    println!();
    dump_blocks("RE-ENCODED (identity, no edits)", &reencoded);

    // KV3 magic of the DATA block, base vs re-encoded (first 4 bytes of DATA).
    let data_off = |b: &[u8]| -> usize {
        let bo = rd_u32(b, 8) as usize + 8;
        // DATA is block index 2 here; walk to it
        let mut p = bo;
        for _ in 0..rd_u32(b, 12) {
            let ty = &b[p..p + 4];
            if ty == b"DATA" {
                return p + 4 + rd_u32(b, p + 4) as usize;
            }
            p += 12;
        }
        0
    };
    let od = data_off(&orig);
    let rd = data_off(&reencoded);
    println!(
        "\nDATA KV3 magic  base={:02x?}  reencoded={:02x?}",
        &orig[od..od + 4],
        &reencoded[rd..rd + 4]
    );

    // Stability: decode the re-encoded DATA and compare to the first decode.
    let redecoded = morphic::decode_kv3_resource(&reencoded)?;
    println!("decode(encode(decode)) == decode : {}", redecoded == value);

    Ok(())
}
