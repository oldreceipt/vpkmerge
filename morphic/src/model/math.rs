//! Minimal `f32` linear algebra for skeleton bind poses, matching .NET's
//! `System.Numerics` row-vector convention (the convention VRF builds bones in)
//! so morphic's matrices line up with the golden GLB. Matrices are row-major:
//! index `row * 4 + col`, and a point transforms as `v' = v * M` (translation
//! lives in row 3). Pure-Rust; morphic keeps no external math dependency.

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;
    /// Component-wise sum. Used by the delta-compressed Vector3 decoder
    /// (`base + half-precision delta`), matching VRF's `Vector3 + Half3`.
    fn add(self, rhs: Vec3) -> Vec3 {
        Vec3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// Row-major 4x4 matrix. Layout and arithmetic mirror `Matrix4x4`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    pub m: [f32; 16],
}

impl Quat {
    /// `Quaternion.CreateFromYawPitchRoll`, formula-for-formula. Used to build
    /// the source-space -> glTF axis change.
    #[must_use]
    pub fn from_yaw_pitch_roll(yaw: f32, pitch: f32, roll: f32) -> Quat {
        let (sr, cr) = (roll * 0.5).sin_cos();
        let (sp, cp) = (pitch * 0.5).sin_cos();
        let (sy, cy) = (yaw * 0.5).sin_cos();
        Quat {
            x: cy * sp * cr + sy * cp * sr,
            y: sy * cp * cr - cy * sp * sr,
            z: cy * cp * sr - sy * sp * cr,
            w: cy * cp * cr + sy * sp * sr,
        }
    }
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    /// `Matrix4x4.CreateTranslation`: translation in row 3.
    #[must_use]
    pub fn from_translation(p: Vec3) -> Mat4 {
        let mut out = Mat4::IDENTITY;
        out.m[12] = p.x;
        out.m[13] = p.y;
        out.m[14] = p.z;
        out
    }

    /// `Matrix4x4.CreateScale(s)`: uniform scale.
    #[must_use]
    pub fn from_scale(s: f32) -> Mat4 {
        let mut out = Mat4::IDENTITY;
        out.m[0] = s;
        out.m[5] = s;
        out.m[10] = s;
        out
    }

    /// `Matrix4x4.CreateFromQuaternion`, formula-for-formula.
    #[must_use]
    pub fn from_quaternion(q: Quat) -> Mat4 {
        let xx = q.x * q.x;
        let yy = q.y * q.y;
        let zz = q.z * q.z;
        let xy = q.x * q.y;
        let wz = q.z * q.w;
        let xz = q.z * q.x;
        let wy = q.y * q.w;
        let yz = q.y * q.z;
        let wx = q.x * q.w;
        Mat4 {
            m: [
                1.0 - 2.0 * (yy + zz),
                2.0 * (xy + wz),
                2.0 * (xz - wy),
                0.0,
                2.0 * (xy - wz),
                1.0 - 2.0 * (zz + xx),
                2.0 * (yz + wx),
                0.0,
                2.0 * (xz + wy),
                2.0 * (yz - wx),
                1.0 - 2.0 * (yy + xx),
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
            ],
        }
    }

    /// `a * b`, standard row-by-column product (same as `Matrix4x4.operator*`).
    #[must_use]
    pub fn mul(&self, b: &Mat4) -> Mat4 {
        let a = &self.m;
        let b = &b.m;
        let mut out = [0.0f32; 16];
        for row in 0..4 {
            for col in 0..4 {
                out[row * 4 + col] = a[row * 4] * b[col]
                    + a[row * 4 + 1] * b[4 + col]
                    + a[row * 4 + 2] * b[8 + col]
                    + a[row * 4 + 3] * b[12 + col];
            }
        }
        Mat4 { m: out }
    }

    /// General 4x4 inverse via cofactor expansion. Returns `None` if singular.
    /// Used for inverse-bind matrices; tolerance-compared, not bit-compared.
    #[must_use]
    pub fn invert(&self) -> Option<Mat4> {
        let m = &self.m;
        let mut inv = [0.0f32; 16];

        inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
            + m[9] * m[7] * m[14]
            + m[13] * m[6] * m[11]
            - m[13] * m[7] * m[10];
        inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
            - m[8] * m[7] * m[14]
            - m[12] * m[6] * m[11]
            + m[12] * m[7] * m[10];
        inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
            + m[8] * m[7] * m[13]
            + m[12] * m[5] * m[11]
            - m[12] * m[7] * m[9];
        inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
            - m[8] * m[6] * m[13]
            - m[12] * m[5] * m[10]
            + m[12] * m[6] * m[9];
        inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
            - m[9] * m[3] * m[14]
            - m[13] * m[2] * m[11]
            + m[13] * m[3] * m[10];
        inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
            + m[8] * m[3] * m[14]
            + m[12] * m[2] * m[11]
            - m[12] * m[3] * m[10];
        inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
            - m[8] * m[3] * m[13]
            - m[12] * m[1] * m[11]
            + m[12] * m[3] * m[9];
        inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
            + m[8] * m[2] * m[13]
            + m[12] * m[1] * m[10]
            - m[12] * m[2] * m[9];
        inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
            + m[5] * m[3] * m[14]
            + m[13] * m[2] * m[7]
            - m[13] * m[3] * m[6];
        inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
            - m[4] * m[3] * m[14]
            - m[12] * m[2] * m[7]
            + m[12] * m[3] * m[6];
        inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
            + m[4] * m[3] * m[13]
            + m[12] * m[1] * m[7]
            - m[12] * m[3] * m[5];
        inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
            - m[4] * m[2] * m[13]
            - m[12] * m[1] * m[6]
            + m[12] * m[2] * m[5];
        inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
            - m[5] * m[3] * m[10]
            - m[9] * m[2] * m[7]
            + m[9] * m[3] * m[6];
        inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
            + m[4] * m[3] * m[10]
            + m[8] * m[2] * m[7]
            - m[8] * m[3] * m[6];
        inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
            - m[4] * m[3] * m[9]
            - m[8] * m[1] * m[7]
            + m[8] * m[3] * m[5];
        inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
            + m[4] * m[2] * m[9]
            + m[8] * m[1] * m[6]
            - m[8] * m[2] * m[5];

        let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
        if det == 0.0 {
            return None;
        }
        let inv_det = 1.0 / det;
        for v in &mut inv {
            *v *= inv_det;
        }
        Some(Mat4 { m: inv })
    }
}
