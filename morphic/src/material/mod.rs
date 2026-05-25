//! Compiled material (`.vmat_c`) parsing, ported from VRF
//! `ResourceTypes/Material.cs` (`Material.Read`). A material's `DATA` block is a
//! KV3 tree of shader name + named parameter tables; the texture parameters name
//! the `.vtex` paths the model's textures live at.
//!
//! morphic stays pure (bytes in): this produces a [`Material`] and a best-effort
//! [`PbrSlots`] name mapping. Resolving those texture paths across VPKs,
//! decoding them (via [`crate::decode`]), and packing them into glTF PBR images
//! happens in the orchestration + GLB-writer layers (M5/M6), which is where
//! VRF's shader-aware channel splitting / ORM packing also belongs. The v1 goal
//! is base color + normal, with the rest exposed "as available".

// KV3 stores material float/vector params as f64-widened f32; narrowing back is
// exact for real material data.
#![allow(clippy::cast_possible_truncation)]

use std::collections::BTreeMap;

use crate::error::DecodeError;
use crate::kv3::{self, Value};
use crate::resource::Resource;

/// A decoded compiled material.
#[derive(Debug, Clone, Default)]
pub struct Material {
    /// `m_materialName` (e.g. `models/.../vindicta_headv2.vmat`).
    pub name: String,
    /// `m_shaderName` (e.g. `pbr.vfx`).
    pub shader_name: String,
    /// Texture slot -> `.vtex` path (`m_textureParams`).
    pub texture_params: BTreeMap<String, String>,
    /// `m_intParams` (shader feature flags like `F_TRANSLUCENT`, plus others).
    pub int_params: BTreeMap<String, i64>,
    /// `m_floatParams`.
    pub float_params: BTreeMap<String, f32>,
    /// `m_vectorParams` (each a 4-component vector).
    pub vector_params: BTreeMap<String, [f32; 4]>,
}

/// glTF alpha handling derived from the material's feature flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}

/// Best-effort PBR texture slots, resolved from the material's texture
/// parameter names. Source paths only (no decode). The mapping is name-based
/// (not shader-aware like VRF), which covers the common Deadlock hero shaders;
/// `normal` may be a combined normal+roughness texture (`g_tNormalRoughness`),
/// which the GLB writer splits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PbrSlots<'a> {
    pub base_color: Option<&'a str>,
    pub normal: Option<&'a str>,
    pub occlusion: Option<&'a str>,
    pub emissive: Option<&'a str>,
    pub roughness: Option<&'a str>,
    pub metalness: Option<&'a str>,
}

/// Parses a compiled `.vmat_c` resource into a [`Material`].
pub fn parse(bytes: &[u8]) -> Result<Material, DecodeError> {
    let resource = Resource::parse(bytes)?;
    let data = kv3::parse(resource.data_block()?)?;
    Material::from_data(&data)
}

impl Material {
    /// Builds a [`Material`] from a parsed `.vmat_c` `DATA` KV3 tree.
    pub fn from_data(data: &Value) -> Result<Material, DecodeError> {
        let mut mat = Material {
            name: string_field(data, "m_materialName"),
            shader_name: string_field(data, "m_shaderName"),
            ..Material::default()
        };

        for kv in array(data, "m_textureParams") {
            if let (Some(name), Some(value)) = (name_of(kv), str_of(kv, "m_pValue")) {
                mat.texture_params.insert(name, value.to_owned());
            }
        }
        for kv in array(data, "m_intParams") {
            if let (Some(name), Some(v)) = (name_of(kv), kv.get("m_nValue").and_then(Value::as_int))
            {
                mat.int_params.insert(name, v);
            }
        }
        for kv in array(data, "m_floatParams") {
            if let (Some(name), Some(v)) =
                (name_of(kv), kv.get("m_flValue").and_then(Value::as_f64))
            {
                mat.float_params.insert(name, v as f32);
            }
        }
        for kv in array(data, "m_vectorParams") {
            if let (Some(name), Some(v)) = (name_of(kv), kv.get("m_value").and_then(vec4)) {
                mat.vector_params.insert(name, v);
            }
        }

        Ok(mat)
    }

    /// The `.vtex` path bound to a texture slot, if present.
    #[must_use]
    pub fn texture(&self, slot: &str) -> Option<&str> {
        self.texture_params.get(slot).map(String::as_str)
    }

    /// Best-effort PBR slot mapping by texture-parameter name.
    #[must_use]
    pub fn pbr(&self) -> PbrSlots<'_> {
        PbrSlots {
            base_color: self.first_texture(&["g_tColor", "g_tColorA", "g_tBaseColor", "g_tAlbedo"]),
            normal: self.first_texture(&["g_tNormal", "g_tNormalRoughness"]),
            occlusion: self.first_texture(&["g_tAmbientOcclusion", "g_tOcclusion"]),
            emissive: self.first_texture(&["g_tSelfIllumMask", "g_tEmissive", "g_tSelfIllum"]),
            roughness: self.first_texture(&["g_tRoughness"]),
            metalness: self.first_texture(&["g_tMetalness", "g_tMetalnessMask"]),
        }
    }

    /// glTF alpha mode from `F_TRANSLUCENT` / `F_ALPHA_TEST` (and `*_glass.vfx`),
    /// matching VRF's `GenerateGLTFMaterialFromRenderMaterial`.
    #[must_use]
    pub fn alpha_mode(&self) -> AlphaMode {
        let translucent = self.int_params.get("F_TRANSLUCENT").copied().unwrap_or(0) > 0
            || self.shader_name.ends_with("_glass.vfx");
        if translucent {
            AlphaMode::Blend
        } else if self.int_params.get("F_ALPHA_TEST").copied().unwrap_or(0) > 0 {
            AlphaMode::Mask
        } else {
            AlphaMode::Opaque
        }
    }

    /// Alpha cutoff for `AlphaMode::Mask` materials (`g_flAlphaTestReference`).
    #[must_use]
    pub fn alpha_cutoff(&self) -> Option<f32> {
        self.float_params.get("g_flAlphaTestReference").copied()
    }

    fn first_texture(&self, slots: &[&str]) -> Option<&str> {
        slots.iter().find_map(|s| self.texture(s))
    }
}

// --- KV3 field helpers ---

fn string_field(data: &Value, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn array<'a>(data: &'a Value, key: &str) -> &'a [Value] {
    data.get(key).and_then(Value::as_array).unwrap_or(&[])
}

fn name_of(kv: &Value) -> Option<String> {
    kv.get("m_name").and_then(Value::as_str).map(str::to_owned)
}

fn str_of<'a>(kv: &'a Value, key: &str) -> Option<&'a str> {
    kv.get(key).and_then(Value::as_str)
}

fn vec4(v: &Value) -> Option<[f32; 4]> {
    let a = v.as_array()?;
    if a.len() < 4 {
        return None;
    }
    Some([
        a[0].as_f64()? as f32,
        a[1].as_f64()? as f32,
        a[2].as_f64()? as f32,
        a[3].as_f64()? as f32,
    ])
}

#[cfg(test)]
mod tests;
