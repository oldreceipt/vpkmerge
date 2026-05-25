//! Source 2 model (`.vmdl_c`) decoding.
//!
//! For now this exposes [`inspect`], a cheap structural read of a compiled
//! model: it parses the resource container and summarizes the block table so
//! callers (and `vpkmerge model inspect`) can see how heavy a model is and
//! whether it carries embedded geometry vs. only material overrides. Full
//! mesh decode (meshoptimizer `MVTX`/`MIDX` + KV3 `MDAT`) lands in a later
//! milestone; see `vpkmerge/docs/vmdl-glb-exporter.md`.

use crate::error::DecodeError;
use crate::resource::Resource;

/// One entry in a model's block table.
#[derive(Debug, Clone)]
pub struct BlockSummary {
    /// 4-char block type (e.g. `MVTX`, `MIDX`, `MDAT`, `DATA`).
    pub kind: String,
    /// Block size in bytes.
    pub size: u32,
}

/// Structural summary of a compiled model resource.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Every block in declaration order.
    pub blocks: Vec<BlockSummary>,
    /// Number of `MVTX` vertex buffers (one per renderable mesh part).
    pub mesh_parts: usize,
    /// Number of `MIDX` index buffers.
    pub index_buffers: usize,
    /// True if the model carries its own geometry (`MVTX` present) rather than
    /// only overriding materials and referencing a base-game mesh.
    pub has_embedded_geometry: bool,
    /// True if skeleton/animation blocks (`ANIM`/`ASEQ`/`AGRP`) are present.
    pub has_skeleton_anim: bool,
    /// True if a collision block (`PHYS`) is present.
    pub has_physics: bool,
    /// Sum of all `MVTX` block sizes, a rough geometry-weight signal.
    pub vertex_bytes: u64,
}

const MVTX: [u8; 4] = *b"MVTX";
const MIDX: [u8; 4] = *b"MIDX";
const PHYS: [u8; 4] = *b"PHYS";
const ANIM: [u8; 4] = *b"ANIM";
const ASEQ: [u8; 4] = *b"ASEQ";
const AGRP: [u8; 4] = *b"AGRP";

/// Parse a `.vmdl_c` resource and summarize its block table. Does not decode
/// geometry; this is the cheap structural read.
pub fn inspect(bytes: &[u8]) -> Result<ModelInfo, DecodeError> {
    let resource = Resource::parse(bytes)?;

    let mut blocks = Vec::new();
    let mut mesh_parts = 0usize;
    let mut index_buffers = 0usize;
    let mut has_skeleton_anim = false;
    let mut has_physics = false;
    let mut vertex_bytes = 0u64;

    for b in resource.blocks() {
        match b.kind {
            MVTX => {
                mesh_parts += 1;
                vertex_bytes += u64::from(b.size);
            }
            MIDX => index_buffers += 1,
            PHYS => has_physics = true,
            ANIM | ASEQ | AGRP => has_skeleton_anim = true,
            _ => {}
        }
        blocks.push(BlockSummary {
            kind: String::from_utf8_lossy(&b.kind).into_owned(),
            size: b.size,
        });
    }

    Ok(ModelInfo {
        has_embedded_geometry: mesh_parts > 0,
        blocks,
        mesh_parts,
        index_buffers,
        has_skeleton_anim,
        has_physics,
        vertex_bytes,
    })
}
