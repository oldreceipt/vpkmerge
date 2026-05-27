//! Model skeleton, ported from VRF `Skeleton.FromModelData` +
//! `Bone` (`ResourceTypes/ModelAnimation`). Built from the model `DATA` block's
//! `m_modelSkeleton`; this is the joint set the glTF skin uses, and its bone
//! *names* are the retarget key Grimoire matches the shared animation clips
//! against, so they must equal what VRF emits.
//!
//! Local bind pose = `fromQuat(rotation) * translate(position)` (scale is
//! deliberately ignored, matching VRF's `Bone`). Global bind pose chains up the
//! parent hierarchy; the inverse-bind matrix is its inverse.

// KV3 stores bone flags as wider integers and positions/rotations as f64-widened
// f32; narrowing them back is exact for real model data. Sign/range are checked
// before the index casts that need it.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use crate::error::DecodeError;
use crate::kv3::Value;

use super::math::{Mat4, Quat, Vec3};

/// One bone in the model skeleton.
#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    /// Index of the parent bone, or `None` for a root.
    pub parent: Option<usize>,
    /// `ModelSkeletonBoneFlags`, stored verbatim for later filtering.
    pub flags: u32,
    /// Parent-space (local) translation.
    pub position: Vec3,
    /// Parent-space (local) rotation.
    pub rotation: Quat,
    /// Local (parent-space) bind pose.
    pub local_bind: Mat4,
    /// Model-space bind pose (local chained through ancestors).
    pub global_bind: Mat4,
    /// Inverse of [`Bone::global_bind`]: the glTF inverse-bind matrix.
    pub inverse_bind: Mat4,
}

/// The model's bone hierarchy.
#[derive(Debug, Clone)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
}

impl Skeleton {
    /// Builds the skeleton from the model `DATA` KV3 tree (`m_modelSkeleton`).
    /// Returns an empty skeleton when the model carries no skeleton data.
    pub fn from_model_data(data: &Value) -> Result<Skeleton, DecodeError> {
        let Some(skel) = data.get("m_modelSkeleton") else {
            return Ok(Skeleton { bones: Vec::new() });
        };

        let names = skel
            .get("m_boneName")
            .and_then(Value::as_array)
            .ok_or(DecodeError::Model("skeleton missing m_boneName"))?;
        let parents = skel
            .get("m_nParent")
            .and_then(Value::as_array)
            .ok_or(DecodeError::Model("skeleton missing m_nParent"))?;
        let flags = skel.get("m_nFlag").and_then(Value::as_array);
        let positions = skel
            .get("m_bonePosParent")
            .and_then(Value::as_array)
            .ok_or(DecodeError::Model("skeleton missing m_bonePosParent"))?;
        let rotations = skel
            .get("m_boneRotParent")
            .and_then(Value::as_array)
            .ok_or(DecodeError::Model("skeleton missing m_boneRotParent"))?;

        let count = names.len();
        if parents.len() != count || positions.len() != count || rotations.len() != count {
            return Err(DecodeError::Model("skeleton array length mismatch"));
        }

        // First pass: names, parents, local transforms, local bind pose.
        let mut bones: Vec<Bone> = Vec::with_capacity(count);
        for i in 0..count {
            let name = names[i]
                .as_str()
                .ok_or(DecodeError::Model("bone name not a string"))?
                .to_owned();
            let parent_raw = parents[i]
                .as_int()
                .ok_or(DecodeError::Model("bone parent not an int"))?;
            let parent = if parent_raw < 0 {
                None
            } else {
                Some(parent_raw as usize)
            };
            let flag = flags
                .and_then(|f| f.get(i))
                .and_then(Value::as_uint)
                .unwrap_or(0) as u32;
            let position = read_vec3(&positions[i])?;
            let rotation = read_quat(&rotations[i])?;
            let local_bind = Mat4::from_quaternion(rotation).mul(&Mat4::from_translation(position));

            bones.push(Bone {
                name,
                parent,
                flags: flag,
                position,
                rotation,
                local_bind,
                global_bind: Mat4::IDENTITY,
                inverse_bind: Mat4::IDENTITY,
            });
        }

        // A parent must precede its child for a single forward pass to resolve
        // global poses; Source 2 skeletons are emitted parent-first, but guard
        // it rather than silently producing wrong matrices.
        for (i, bone) in bones.iter().enumerate() {
            if let Some(p) = bone.parent {
                if p >= i {
                    return Err(DecodeError::Model("bone parent not topologically ordered"));
                }
            }
        }

        // Second pass: global bind = local * parent_global, then invert.
        for i in 0..count {
            let global = match bones[i].parent {
                Some(p) => bones[i].local_bind.mul(&bones[p].global_bind),
                None => bones[i].local_bind,
            };
            let inverse = global
                .invert()
                .ok_or(DecodeError::Model("bind pose not invertible"))?;
            bones[i].global_bind = global;
            bones[i].inverse_bind = inverse;
        }

        Ok(Skeleton { bones })
    }

    /// Bone names sorted ascending: the stable set used to validate against the
    /// golden skin and to retarget clips by name.
    #[must_use]
    pub fn sorted_bone_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.bones.iter().map(|b| b.name.clone()).collect();
        names.sort();
        names
    }
}

/// Reads the per-mesh bone remap table from the model `DATA` block.
///
/// Mirrors VRF `Model.GetRemapTable`: slices `m_remappingTable` using
/// `m_remappingTableStarts[meshIndex] .. [meshIndex + 1]`. The result maps a
/// mesh-local `BLENDINDICES` value to a model skeleton bone index. Returns
/// `None` when the mesh has no remap entry (treated as identity by the caller).
pub fn remap_table(data: &Value, mesh_index: usize) -> Option<Vec<usize>> {
    let starts = data
        .get("m_remappingTableStarts")
        .and_then(Value::as_array)?;
    if mesh_index >= starts.len() {
        return None;
    }
    let table = data.get("m_remappingTable").and_then(Value::as_array)?;

    let start = usize::try_from(starts[mesh_index].as_int()?).ok()?;
    let end = if mesh_index + 1 < starts.len() {
        usize::try_from(starts[mesh_index + 1].as_int()?).ok()?
    } else {
        table.len()
    };
    if start > end || end > table.len() {
        return None;
    }

    let mut out = Vec::with_capacity(end - start);
    for entry in &table[start..end] {
        out.push(usize::try_from(entry.as_int()?).ok()?);
    }
    Some(out)
}

fn read_vec3(v: &Value) -> Result<Vec3, DecodeError> {
    let a = v
        .as_array()
        .ok_or(DecodeError::Model("vec3 not an array"))?;
    if a.len() < 3 {
        return Err(DecodeError::Model("vec3 too short"));
    }
    Ok(Vec3 {
        x: f64_to_f32(&a[0])?,
        y: f64_to_f32(&a[1])?,
        z: f64_to_f32(&a[2])?,
    })
}

fn read_quat(v: &Value) -> Result<Quat, DecodeError> {
    let a = v
        .as_array()
        .ok_or(DecodeError::Model("quat not an array"))?;
    if a.len() < 4 {
        return Err(DecodeError::Model("quat too short"));
    }
    Ok(Quat {
        x: f64_to_f32(&a[0])?,
        y: f64_to_f32(&a[1])?,
        z: f64_to_f32(&a[2])?,
        w: f64_to_f32(&a[3])?,
    })
}

fn f64_to_f32(v: &Value) -> Result<f32, DecodeError> {
    v.as_f64()
        .map(|d| d as f32)
        .ok_or(DecodeError::Model("expected numeric component"))
}
