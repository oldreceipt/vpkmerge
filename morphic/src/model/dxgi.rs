//! The subset of Microsoft `DXGI_FORMAT` values Source 2 uses for vertex
//! attributes, plus the (element-size, element-count) lookup VRF's
//! `VBIB.GetFormatInfo` exposes. Values match `ValveResourceFormat`'s
//! `DXGI_FORMAT` enum (which mirrors the Windows header); only the formats that
//! appear as `m_inputLayoutFields[].m_Format` in compiled models are named.

/// A vertex-attribute storage format, identified by its DXGI numeric id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxgiFormat {
    R32G32B32A32Float = 2,
    R32G32B32A32Sint = 4,
    R32G32B32Float = 6,
    R16G16B16A16Float = 10,
    R16G16B16A16Unorm = 11,
    R16G16B16A16Uint = 12,
    R16G16B16A16Sint = 14,
    R32G32Float = 16,
    R8G8B8A8Unorm = 28,
    R8G8B8A8Uint = 30,
    R16G16Float = 34,
    R16G16Unorm = 35,
    R16G16Snorm = 37,
    R16G16Sint = 38,
    R32Float = 41,
    R32Uint = 42,
}

impl DxgiFormat {
    /// Maps a raw `m_Format` int to a known vertex format, or `None` if it is a
    /// format Source 2 never uses for the attributes we decode.
    pub fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            2 => Self::R32G32B32A32Float,
            4 => Self::R32G32B32A32Sint,
            6 => Self::R32G32B32Float,
            10 => Self::R16G16B16A16Float,
            11 => Self::R16G16B16A16Unorm,
            12 => Self::R16G16B16A16Uint,
            14 => Self::R16G16B16A16Sint,
            16 => Self::R32G32Float,
            28 => Self::R8G8B8A8Unorm,
            30 => Self::R8G8B8A8Uint,
            34 => Self::R16G16Float,
            35 => Self::R16G16Unorm,
            37 => Self::R16G16Snorm,
            38 => Self::R16G16Sint,
            41 => Self::R32Float,
            42 => Self::R32Uint,
            _ => return None,
        })
    }

    /// `(component byte size, component count)`, mirroring `VBIB.GetFormatInfo`.
    /// The product is the attribute's packed byte width inside the vertex.
    pub fn format_info(self) -> (usize, usize) {
        match self {
            Self::R8G8B8A8Uint | Self::R8G8B8A8Unorm => (1, 4),
            Self::R16G16Float | Self::R16G16Sint | Self::R16G16Snorm | Self::R16G16Unorm => (2, 2),
            Self::R16G16B16A16Float
            | Self::R16G16B16A16Sint
            | Self::R16G16B16A16Uint
            | Self::R16G16B16A16Unorm => (2, 4),
            Self::R32Float | Self::R32Uint => (4, 1),
            Self::R32G32Float => (4, 2),
            Self::R32G32B32Float => (4, 3),
            Self::R32G32B32A32Float | Self::R32G32B32A32Sint => (4, 4),
        }
    }
}
