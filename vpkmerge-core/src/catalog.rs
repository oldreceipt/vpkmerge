//! Build the Foundry asset catalog from a Deadlock install.
//!
//! The marquee piece is the **voice-line search index**: one searchable row per
//! VO sound event ([`VoiceLine`]), carrying the event name, hero, clip path(s), a
//! human-readable label, and the engine duration. A UI searches it (by the
//! descriptive label, e.g. "ally atlas killed in lane") and forges a swap for the
//! chosen event.
//!
//! Note on subtitles: the game also ships a compiled caption database
//! (`resource/localization/citadel_generated_vo/citadel_generated_vo_<lang>.dat`,
//! VCCD v2, keyed by `crc32(token)`). [`CaptionDb`] reads it, but empirically the
//! per-hero VO events all map to *empty* English captions (Deadlock does not
//! subtitle hero combat barks); the authored caption text lives under a separate
//! token namespace not tied to swappable sound events. So caption text is only a
//! best-effort enrichment on the index, almost always `None` for hero VO. The
//! searchable text is the descriptive event name itself. Findings pinned down
//! against the live pak; see `grimoire/docs/foundry-tab-design.md`.
//!
//! Pure Rust, no extra dependencies (the caption hash is the standard
//! CRC-32/ISO-HDLC, the same one `zlib`/`gzip` use).

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use morphic::kv3::Value;

use crate::soundevents::SoundEvents;

/// Entry path of the English compiled VO captions inside `citadel/pak01`. Other
/// languages live at the same relative path inside their `citadel_<lang>` pak.
pub const ENGLISH_CAPTIONS_ENTRY: &str =
    "resource/localization/citadel_generated_vo/citadel_generated_vo_english.dat";

/// The per-hero / announcer VO tree.
const VO_TREE_PREFIX: &str = "soundevents/vo/";
/// Per-hero VO files are named `generated_vo_hero_<code>.vsndevts_c`.
const HERO_VO_STEM: &str = "generated_vo_hero_";

/// One VO sound event, ready to search and swap.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceLine {
    /// Soundevent name, e.g. `bebop_self_ultimate_cast_01_hero_3d`. Usable
    /// verbatim as the swap target in the soundevents layer.
    pub event: String,
    /// Hero codename (`bebop`, `astro`, ...) from the event's `context_name`,
    /// falling back to the source filename. `None` for announcer / non-hero VO.
    pub hero: Option<String>,
    /// Human-readable label derived from the event name, e.g.
    /// `"ally atlas killed in lane"`. This is the searchable text.
    pub label: String,
    /// Clip path(s) the event plays (more than one == a randomizer pool).
    pub vsnd: Vec<String>,
    /// Engine playback duration in seconds, if the event records one.
    pub duration: Option<f64>,
    /// Authored English subtitle, if the caption database resolves this event.
    /// Almost always `None` for hero VO; see the module note.
    pub caption: Option<String>,
}

/// A decoded VCCD (Valve Compiled Caption Database).
///
/// Maps `crc32(token)` to caption text. For generated VO the token is the
/// soundevent name, so [`Self::for_event`] resolves a line's subtitle directly.
pub struct CaptionDb {
    by_hash: HashMap<u32, String>,
}

impl CaptionDb {
    /// Parse a `.dat` caption database (VCCD v2).
    ///
    /// Header (24 bytes, little-endian): `magic 'VCCD' | version | numBlocks |
    /// blockSize | dirEntries | dataOffset`, then `dirEntries` 12-byte directory
    /// records (`u32 hash | i32 blockNum | u16 offset | u16 length`), then
    /// UTF-16LE NUL-terminated strings packed into `blockSize` blocks beginning
    /// at `dataOffset`.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 24 || &bytes[0..4] != b"VCCD" {
            bail!("not a VCCD caption database (bad magic)");
        }
        let rd = |off: usize| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
        let version = rd(4);
        if version != 1 && version != 2 {
            bail!("unsupported VCCD version {version}");
        }
        let block_size = rd(12) as usize;
        let dir_entries = rd(16) as usize;
        let data_offset = rd(20) as usize;

        let dir_start = 24usize;
        let dir_bytes = dir_entries
            .checked_mul(12)
            .and_then(|n| n.checked_add(dir_start))
            .context("caption directory size overflow")?;
        if dir_bytes > bytes.len() {
            bail!("caption directory ({dir_entries} entries) runs past end of file");
        }

        let mut by_hash: HashMap<u32, String> = HashMap::with_capacity(dir_entries);
        for i in 0..dir_entries {
            let base = dir_start + i * 12;
            let hash = u32::from_le_bytes(bytes[base..base + 4].try_into().unwrap());
            let block = i32::from_le_bytes(bytes[base + 4..base + 8].try_into().unwrap());
            let offset =
                u16::from_le_bytes(bytes[base + 8..base + 10].try_into().unwrap()) as usize;
            let length = usize::from(u16::from_le_bytes(
                bytes[base + 10..base + 12].try_into().unwrap(),
            ));
            let Ok(block) = usize::try_from(block) else {
                continue;
            };
            if length == 0 {
                // Empty caption (common for routine combat callouts): skip it,
                // but never let a later empty dup clobber a real string.
                by_hash.entry(hash).or_default();
                continue;
            }
            let start = data_offset + block * block_size + offset;
            let Some(raw) = bytes.get(start..start + length) else {
                continue;
            };
            let text = decode_utf16le(raw);
            if text.is_empty() {
                by_hash.entry(hash).or_default();
            } else {
                // Prefer a non-empty string on a hash collision.
                by_hash.insert(hash, text);
            }
        }
        Ok(Self { by_hash })
    }

    /// Caption text for a raw token hash, if present and non-empty.
    #[must_use]
    pub fn by_hash(&self, hash: u32) -> Option<&str> {
        self.by_hash
            .get(&hash)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

    /// Caption text for a token string (hashed with [`caption_hash`]).
    #[must_use]
    pub fn get(&self, token: &str) -> Option<&str> {
        self.by_hash(caption_hash(token))
    }

    /// Caption text for a soundevent name (the generated-VO token is the event
    /// name verbatim).
    #[must_use]
    pub fn for_event(&self, event: &str) -> Option<&str> {
        self.get(event)
    }

    /// Total directory entries retained (including empty captions).
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    /// Number of distinct hashes carrying non-empty caption text.
    #[must_use]
    pub fn non_empty_count(&self) -> usize {
        self.by_hash.values().filter(|s| !s.is_empty()).count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }
}

/// Build the voice-line search index for one VPK whose caption database it also
/// carries (the base `citadel/pak01` for English). Convenience over
/// [`build_voiceline_index_with_captions`].
pub fn build_voiceline_index(vpk_path: impl AsRef<Path>) -> Result<Vec<VoiceLine>> {
    let vpk_path = vpk_path.as_ref();
    let caption_bytes = crate::read_vpk_entry(vpk_path, ENGLISH_CAPTIONS_ENTRY)
        .with_context(|| format!("reading {ENGLISH_CAPTIONS_ENTRY}"))?;
    let captions = CaptionDb::parse(&caption_bytes)?;
    build_voiceline_index_with_captions(vpk_path, &captions)
}

/// Build the voice-line index from the VO soundevents tree in `vpk_path`,
/// enriching each row with caption text from `captions` where it resolves. Use
/// this overload when the captions live in a different pak (a non-English
/// `citadel_<lang>` pak).
///
/// Every event in `soundevents/vo/*.vsndevts_c` becomes a [`VoiceLine`]; the
/// `base` inheritance template is not an event and is skipped. Lines are sorted
/// by hero then event for a stable index.
pub fn build_voiceline_index_with_captions(
    vpk_path: impl AsRef<Path>,
    captions: &CaptionDb,
) -> Result<Vec<VoiceLine>> {
    let vpk_path = vpk_path.as_ref();
    let vpk =
        valve_pak::open(vpk_path).with_context(|| format!("opening {}", vpk_path.display()))?;

    let mut vo_entries: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.starts_with(VO_TREE_PREFIX) && p.ends_with(".vsndevts_c"))
        .cloned()
        .collect();
    vo_entries.sort();

    let mut lines = Vec::new();
    for entry in &vo_entries {
        let file_hero = hero_from_vo_entry(entry);
        let mut file = vpk
            .get_file(entry)
            .with_context(|| format!("locating {entry}"))?;
        let bytes = file
            .read_all()
            .with_context(|| format!("reading {entry}"))?;
        // A VO file that fails to decode should not sink the whole index.
        let Ok(se) = SoundEvents::from_bytes(bytes) else {
            continue;
        };
        let Value::Object(pairs) = &se.root else {
            continue;
        };
        for (name, event) in pairs {
            // The `base` key is a shared template, not a playable event.
            if name == "base" {
                continue;
            }
            let hero = event
                .get("context_name")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| file_hero.clone());
            let vsnd = match event.get("vsnd_files") {
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
                Some(Value::String(s)) => vec![s.clone()],
                _ => Vec::new(),
            };
            lines.push(VoiceLine {
                label: pretty_label(name, hero.as_deref()),
                caption: captions.for_event(name).map(str::to_owned),
                duration: event.get("vsnd_duration").and_then(Value::as_f64),
                hero,
                vsnd,
                event: name.clone(),
            });
        }
    }

    lines.sort_by(|a, b| a.hero.cmp(&b.hero).then_with(|| a.event.cmp(&b.event)));
    Ok(lines)
}

/// Hero codename from a VO entry path, or `None` for announcer / non-hero VO.
/// `soundevents/vo/generated_vo_hero_bebop.vsndevts_c` -> `Some("bebop")`.
fn hero_from_vo_entry(entry: &str) -> Option<String> {
    let stem = entry.rsplit('/').next()?.strip_suffix(".vsndevts_c")?;
    stem.strip_prefix(HERO_VO_STEM).map(str::to_owned)
}

/// Turn a soundevent name into searchable prose: drop the speaker (hero) prefix,
/// the `_hero_3d` / `_2d` channel suffix and a trailing take number, then spell
/// the underscores as spaces. `bebop_ally_atlas_killed_in_lane_01_hero_3d` with
/// hero `bebop` -> `"ally atlas killed in lane"`.
fn pretty_label(event: &str, hero: Option<&str>) -> String {
    let mut s = event;
    if let Some(h) = hero {
        if let Some(rest) = s.strip_prefix(h) {
            s = rest.trim_start_matches('_');
        }
    }
    for suffix in ["_hero_3d", "_hero_2d", "_3d", "_2d"] {
        if let Some(rest) = s.strip_suffix(suffix) {
            s = rest;
            break;
        }
    }
    // Trailing take/variant segments (`_01`, `_alt`, and combinations like
    // `_01_alt_01`), peeled one segment at a time.
    while let Some((head, tail)) = s.rsplit_once('_') {
        if tail == "alt" || (!tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit())) {
            s = head;
        } else {
            break;
        }
    }
    s.replace('_', " ").trim().to_owned()
}

/// Decode a UTF-16LE byte slice, dropping the trailing NUL terminator(s).
fn decode_utf16le(raw: &[u8]) -> String {
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// CRC-32/ISO-HDLC (the `zlib`/`gzip` CRC): reflected poly `0xEDB88320`, init
/// and xorout `0xFFFFFFFF`. This is the hash VCCD uses to key caption tokens.
#[must_use]
pub fn caption_hash(token: &str) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in token.as_bytes() {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_canonical_check_value() {
        // The standard CRC-32 check vector.
        assert_eq!(caption_hash("123456789"), 0xCBF4_3926);
    }

    /// Build a minimal valid VCCD v2 blob with two tokens and parse it back.
    #[test]
    fn parses_and_resolves_tokens() {
        let block_size = 8192u32;
        let tok_a = "hero_ability_cast_01";
        let tok_b = "hero_self_death_01";
        let text_a = "Witness my power!";
        let text_b = ""; // empty caption, must be dropped

        let enc = |s: &str| -> Vec<u8> {
            let mut v: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
            v.extend_from_slice(&[0, 0]); // NUL terminator
            v
        };
        let data_a = enc(text_a);
        let data_b = enc(text_b);

        let header_len = 24u32;
        let dir_len = 2 * 12;
        let data_offset = header_len + dir_len;
        let len_a = u16::try_from(data_a.len()).unwrap();
        let len_b = u16::try_from(data_b.len()).unwrap();

        let mut buf = Vec::new();
        buf.extend_from_slice(b"VCCD");
        buf.extend_from_slice(&2u32.to_le_bytes()); // version
        buf.extend_from_slice(&1u32.to_le_bytes()); // numBlocks
        buf.extend_from_slice(&block_size.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes()); // dirEntries
        buf.extend_from_slice(&data_offset.to_le_bytes());

        // Directory: hash, blockNum, offset, length.
        buf.extend_from_slice(&caption_hash(tok_a).to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&len_a.to_le_bytes());

        buf.extend_from_slice(&caption_hash(tok_b).to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&len_a.to_le_bytes());
        buf.extend_from_slice(&len_b.to_le_bytes());

        // Data block.
        buf.extend_from_slice(&data_a);
        buf.extend_from_slice(&data_b);

        let db = CaptionDb::parse(&buf).expect("parse VCCD");
        assert_eq!(db.get(tok_a), Some(text_a));
        assert_eq!(db.for_event(tok_a), Some(text_a));
        // Empty caption resolves to None.
        assert_eq!(db.get(tok_b), None);
        // Unknown token.
        assert_eq!(db.get("no_such_event"), None);
        assert_eq!(db.non_empty_count(), 1);
    }

    #[test]
    fn label_is_searchable_prose() {
        assert_eq!(
            pretty_label("bebop_ally_atlas_killed_in_lane_01_hero_3d", Some("bebop")),
            "ally atlas killed in lane"
        );
        assert_eq!(
            pretty_label("bebop_self_ultimate_cast_01_hero_3d", Some("bebop")),
            "self ultimate cast"
        );
        assert_eq!(
            pretty_label(
                "bebop_ally_warden_killed_in_lane_01_alt_01_hero_3d",
                Some("bebop")
            ),
            "ally warden killed in lane"
        );
        // No hero prefix to strip (announcer-style).
        assert_eq!(pretty_label("round_start_01", None), "round start");
    }

    #[test]
    fn hero_parsing_from_vo_path() {
        assert_eq!(
            hero_from_vo_entry("soundevents/vo/generated_vo_hero_bebop.vsndevts_c").as_deref(),
            Some("bebop")
        );
        assert_eq!(
            hero_from_vo_entry("soundevents/vo/announcer.vsndevts_c"),
            None
        );
    }

    #[test]
    fn rejects_non_vccd() {
        assert!(CaptionDb::parse(b"not a caption db").is_err());
    }
}
