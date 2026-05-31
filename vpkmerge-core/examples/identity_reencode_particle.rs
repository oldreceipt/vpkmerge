// Pack a one-entry structural probe: decode a compiled particle's KV3 DATA,
// re-encode it with no semantic edits, and put that re-encoded particle back at
// the same VPK path.
//
// If this loads in game, we have evidence that at least some structural KV3
// particle edits may be viable. If it fails, true operator insertion needs a
// byte-faithful v5 structural editor rather than the existing v4 re-encoder.
//
// usage:
//   cargo run -p vpkmerge-core --example identity_reencode_particle -- \
//     <base_dir.vpk> <out_dir.vpk> <entry.vpcf_c>

fn rd_u32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *b.get(o)?,
        *b.get(o + 1)?,
        *b.get(o + 2)?,
        *b.get(o + 3)?,
    ]))
}

fn data_magic(bytes: &[u8]) -> Option<[u8; 4]> {
    let block_offset = rd_u32(bytes, 8)? as usize;
    let block_count = rd_u32(bytes, 12)? as usize;
    let mut p = 8 + block_offset;
    for _ in 0..block_count {
        let kind = bytes.get(p..p + 4)?;
        let rel = rd_u32(bytes, p + 4)? as usize;
        if kind == b"DATA" {
            let off = p + 4 + rel;
            return Some(bytes.get(off..off + 4)?.try_into().ok()?);
        }
        p += 12;
    }
    None
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().expect("base_dir.vpk");
    let out = args.next().expect("out_dir.vpk");
    let entry = args.next().expect("entry.vpcf_c");

    let vpk = valve_pak::open(&base)?;
    let original = vpk.get_file(&entry)?.read_all()?;
    let value = morphic::decode_kv3_resource(&original)?;
    let reencoded = morphic::encode_kv3_resource(&original, &value)?;

    vpkmerge_core::pack(&[(entry.as_str(), reencoded.as_slice())], &out)?;
    println!(
        "wrote {out}: {entry}; original={} bytes magic={:02x?}; reencoded={} bytes magic={:02x?}",
        original.len(),
        data_magic(&original),
        reencoded.len(),
        data_magic(&reencoded),
    );
    Ok(())
}
