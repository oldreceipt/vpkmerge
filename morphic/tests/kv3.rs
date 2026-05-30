//! Binary KV3 decode + round-trip against a real Deadlock soundevents file.
//!
//! `gigawatt.vsndevts_c` is committed under `fixtures/kv3/`. It is Valve's
//! shipped v5/LZ4 file, so decoding it exercises the v5 two-buffer reader and
//! LZ4 path. Re-encoding (uncompressed v4) and decoding again must reproduce the
//! exact same tree, which exercises the v4 single-buffer reader and the writer.

use morphic::kv3::Value;

fn fixture() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/kv3/gigawatt.vsndevts_c"
    );
    std::fs::read(path).expect("read gigawatt fixture")
}

#[test]
fn decodes_gigawatt_v5_lz4() {
    let bytes = fixture();
    let root = morphic::decode_kv3_resource(&bytes).expect("decode");

    let Value::Object(events) = &root else {
        panic!("root is not an object");
    };
    // 44 hero soundevents (the survey counted 44 hero vsndevts_c entries; this
    // file holds gigawatt's events).
    assert_eq!(events.len(), 44, "unexpected top-level event count");

    // A known event with the documented shape: a vsnd_files array and a base.
    let fire = root.get("Seven.Wpn.Fire").expect("Seven.Wpn.Fire event");
    assert_eq!(
        fire.get("base").and_then(Value::as_str),
        Some("Base.Weapon.Pistol")
    );

    let vsnd = fire
        .get("vsnd_files")
        .and_then(Value::as_array)
        .expect("vsnd_files array");
    assert_eq!(vsnd.len(), 7);
    assert_eq!(
        vsnd[0].as_str(),
        Some("sounds/weapons/gigawatt/gigawatt_weapon_fire_01.vsnd")
    );

    // volume is present and numeric.
    assert!(fire.get("volume").and_then(Value::as_f64).is_some());
}

#[test]
fn round_trips_through_uncompressed_v4() {
    let bytes = fixture();
    let original = morphic::decode_kv3_resource(&bytes).expect("decode original");

    // Re-encode into a full resource (uncompressed v4 DATA + original RED2),
    // then decode again. The tree must be identical.
    let reencoded = morphic::encode_kv3_resource(&bytes, &original).expect("encode");
    let roundtripped = morphic::decode_kv3_resource(&reencoded).expect("decode reencoded");

    assert_eq!(original, roundtripped, "tree changed across encode/decode");

    // The re-emitted file is a valid container the resource parser accepts, and
    // (being uncompressed) is larger than Valve's LZ4 original.
    assert!(
        reencoded.len() > bytes.len(),
        "uncompressed re-encode should be larger than the LZ4 original"
    );
}

#[test]
fn modify_then_round_trip() {
    let bytes = fixture();
    let mut root = morphic::decode_kv3_resource(&bytes).expect("decode");

    // Phase 2: swap a vsnd path and change a volume.
    root.for_each_string_mut(&mut |s| {
        if s == "sounds/weapons/gigawatt/gigawatt_weapon_fire_01.vsnd" {
            *s = "sounds/custom/my_fire.vsnd".to_string();
        }
    });
    if let Some(vol) = root
        .get_mut("Seven.Wpn.Fire")
        .and_then(|e| e.get_mut("volume"))
    {
        *vol = Value::Double(0.25);
    }

    let reencoded = morphic::encode_kv3_resource(&bytes, &root).expect("encode");
    let back = morphic::decode_kv3_resource(&reencoded).expect("decode");

    let fire = back.get("Seven.Wpn.Fire").unwrap();
    assert_eq!(
        fire.get("vsnd_files").and_then(Value::as_array).unwrap()[0].as_str(),
        Some("sounds/custom/my_fire.vsnd")
    );
    assert_eq!(fire.get("volume").and_then(Value::as_f64), Some(0.25));
}

#[test]
fn set_scalars_patches_a_v4_block_in_place() {
    use morphic::kv3::Seg;

    // `encode_kv3_resource` emits an uncompressed **v4** DATA block, so this builds
    // a real v4 block to exercise the v4 single-buffer scalar patch (OBJECT member
    // counts from the b4 lane, INT64 from b8, inline strings/types) end to end. The
    // base-game particles that need this are also v4; this is the committed
    // regression for that path (their in-game recolor is the live proof).
    let template = fixture(); // reused only for a valid resource envelope / RED2
    let tree = Value::Object(vec![
        ("m_nFirst".to_string(), Value::Int(100)),
        (
            "m_sKeep".to_string(),
            Value::String("unchanged".to_string()),
        ),
        ("m_nSecond".to_string(), Value::Int(7)),
    ]);
    let v4 = morphic::encode_kv3_resource(&template, &tree).expect("encode v4 resource");

    // Patch one scalar in place; the block stays v4, everything else byte-faithful.
    let patched = morphic::patch_kv3_resource_scalars(
        &v4,
        &[(vec![Seg::Key("m_nSecond".to_string())], 4242)],
    )
    .expect("v4 scalar patch");

    let root = morphic::decode_kv3_resource(&patched).expect("decode patched v4");
    assert_eq!(root.get("m_nSecond").and_then(Value::as_int), Some(4242));
    // Siblings are untouched (the patch is surgical, not a re-encode).
    assert_eq!(root.get("m_nFirst").and_then(Value::as_int), Some(100));
    assert_eq!(
        root.get("m_sKeep").and_then(Value::as_str),
        Some("unchanged")
    );

    // A path that resolves to no integer scalar is an error, not a silent no-op.
    assert!(morphic::patch_kv3_resource_scalars(
        &v4,
        &[(vec![Seg::Key("m_sKeep".to_string())], 1)],
    )
    .is_err());
}

#[test]
fn set_doubles_patches_a_tint_vector_in_place() {
    use morphic::kv3::Seg;

    // The material g_vColorTint shape: an RGBA f64 vector. Patch two channels in
    // place (the double sibling of the scalar patch); the third is untouched and the
    // tagless 1.0 alpha is not patchable.
    let template = fixture();
    let tree = Value::Object(vec![(
        "m_value".to_string(),
        Value::Array(vec![
            Value::Double(0.9),
            Value::Double(0.5),
            Value::Double(0.2),
            Value::Double(1.0),
        ]),
    )]);
    let v = morphic::encode_kv3_resource(&template, &tree).expect("encode");

    let key = |i: usize| vec![Seg::Key("m_value".to_string()), Seg::Index(i)];
    let patched = morphic::patch_kv3_resource_doubles(&v, &[(key(0), 0.123), (key(2), 0.456)])
        .expect("patch");
    let root = morphic::decode_kv3_resource(&patched).expect("decode patched");
    let arr = root
        .get("m_value")
        .and_then(Value::as_array)
        .expect("m_value");
    assert!((arr[0].as_f64().unwrap() - 0.123).abs() < 1e-9);
    assert!(
        (arr[1].as_f64().unwrap() - 0.5).abs() < 1e-9,
        "untouched channel preserved"
    );
    assert!((arr[2].as_f64().unwrap() - 0.456).abs() < 1e-9);

    // The 1.0 alpha is a tagless DOUBLE_ONE (no stored bytes), so it is not a
    // patchable target.
    assert!(morphic::patch_kv3_resource_doubles(&v, &[(key(3), 0.5)]).is_err());
}
