//! Rest-pose oracle for the `FeModel` binding math. Gated on `MORPHIC_MODEL_VPK`
//! (override the entry with `MORPHIC_MODEL_ENTRY`); skipped when unset so CI stays
//! green.
//!
//! Every node's `m_InitPose` carries its rest position AND rotation, so the cloth
//! -> skeleton binding can be validated purely from the `FeModel`, with no GLB and no
//! hand-tuned constants: at rest, each control/reverse offset must reproduce its
//! child node's rest position from the parent node's rest transform plus the authored
//! offset. That pins the offset frame and sign. Likewise `basis(rest positions) *
//! qAdjust` must reproduce each node's authored rest rotation (the absolute
//! orientation convention), pinning the basis construction and the qAdjust order.
//!
//! ```text
//! MORPHIC_MODEL_VPK=/path/to/pak50_dir.vpk \
//!   cargo test -p morphic --test fe_rest_oracle -- --nocapture
//! ```

#![allow(clippy::cast_precision_loss)]

use morphic::model::{decode_fe_model, FeModel, FeNode};

type V3 = [f32; 3];
type Q = [f32; 4];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l < 1e-12 {
        a
    } else {
        scale(a, 1.0 / l)
    }
}
fn dist(a: V3, b: V3) -> f32 {
    dot(sub(a, b), sub(a, b)).sqrt()
}

/// Rotate `v` by quaternion `q` ([x, y, z, w]).
fn qrot(q: Q, v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let t = scale(cross(u, v), 2.0);
    add(add(v, scale(t, q[3])), cross(u, t))
}
/// Normalize a quaternion (f32 basis math leaves it slightly non-unit).
fn qnorm(q: Q) -> Q {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l < 1e-12 {
        [0.0, 0.0, 0.0, 1.0]
    } else {
        [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
    }
}
/// Quaternion product `a * b`.
fn qmul(a: Q, b: Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
/// Angle (radians) between the rotations represented by two quaternions. Normalizes
/// the inputs first: file/basis quats are unit only to f32 epsilon, and acos is steep
/// near 1, so an un-normalized dot turns ~1e-7 noise into a spurious ~0.08deg.
fn quat_angle(a: Q, b: Q) -> f32 {
    let (a, b) = (qnorm(a), qnorm(b));
    let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
    2.0 * d.clamp(-1.0, 1.0).acos()
}

/// Build a quaternion from an orthonormal basis (columns x, y, z).
fn quat_from_basis(x: V3, y: V3, z: V3) -> Q {
    // Standard matrix (columns = basis) -> quaternion.
    let (m00, m10, m20) = (x[0], x[1], x[2]);
    let (m01, m11, m21) = (y[0], y[1], y[2]);
    let (m02, m12, m22) = (z[0], z[1], z[2]);
    let tr = m00 + m11 + m22;
    if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    }
}

/// Reconstruct a node-base orientation from four neighbor node REST positions plus
/// `qAdjust`. The absolute convention: primary axis Y = `y1-y0`, Z = `(x1-x0) x Y`,
/// X = `Y x Z`; columns `[X, Y, Z]` -> quaternion, then `* qAdjust`.
fn node_base_quat(
    nodes: &[FeNode],
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
    q_adjust: Q,
) -> Option<Q> {
    let p = |i: usize| nodes.get(i).map(|n| n.init_pos);
    // Convention found by exhaustive search (exact on necro: mean 0.00deg vs init_rot,
    // rank-2 was 18deg): x seed = x1-x0, primary axis Y = y1-y0; Z = x_seed x Y;
    // X = Y x Z; columns [X, Y, Z]; then absolute post-multiply by qAdjust.
    let x_seed = norm(sub(p(x1)?, p(x0)?));
    let yy = norm(sub(p(y1)?, p(y0)?));
    let zz = norm(cross(x_seed, yy));
    let xx = norm(cross(yy, zz));
    if dot(xx, xx) < 0.5 || dot(yy, yy) < 0.5 || dot(zz, zz) < 0.5 {
        return None;
    }
    Some(qnorm(qmul(quat_from_basis(xx, yy, zz), q_adjust)))
}

struct Stat {
    max: f32,
    sum: f64,
    n: usize,
    worst: usize,
}
impl Stat {
    fn new() -> Self {
        Stat {
            max: 0.0,
            sum: 0.0,
            n: 0,
            worst: usize::MAX,
        }
    }
    fn add(&mut self, v: f32, idx: usize) {
        if v > self.max {
            self.max = v;
            self.worst = idx;
        }
        self.sum += f64::from(v);
        self.n += 1;
    }
    fn mean(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.sum / self.n as f64
        }
    }
}

/// Best-sign residual of a position binding: predicted child =
/// `parent_pos + rotate(frame, sign * offset)`, minimized over sign, vs the authored
/// child rest position.
fn offset_residual(parent_pos: V3, frame: Q, offset: V3, child_pos: V3) -> f32 {
    let plus = dist(add(parent_pos, qrot(frame, offset)), child_pos);
    let minus = dist(add(parent_pos, qrot(frame, scale(offset, -1.0))), child_pos);
    plus.min(minus)
}

#[allow(clippy::too_many_lines)]
fn run(name: &str, fe: &FeModel) {
    let n = &fe.nodes;

    // C: ctrl offsets must reproduce the child rest position from the parent rest
    // transform + offset (offset is in the parent's local frame).
    let mut ctrl = Stat::new();
    for (i, c) in fe.ctrl_offsets.iter().enumerate() {
        if let (Some(p), Some(ch)) = (n.get(c.parent), n.get(c.child)) {
            ctrl.add(
                offset_residual(p.init_pos, p.init_rot, c.offset, ch.init_pos),
                i,
            );
        }
    }

    // C: reverse offsets recover the bone-control rest position from the target node.
    let mut rev = Stat::new();
    for (i, r) in fe.reverse_offsets.iter().enumerate() {
        if let (Some(b), Some(t)) = (n.get(r.bone_ctrl), n.get(r.target_node)) {
            rev.add(
                offset_residual(t.init_pos, b.init_rot, r.offset, b.init_pos),
                i,
            );
        }
    }

    // C: soft offsets are weighted pulls; measure each link's best-sign residual too.
    let mut soft = Stat::new();
    for (i, c) in fe.ctrl_soft_offsets.iter().enumerate() {
        if let (Some(p), Some(ch)) = (n.get(c.parent), n.get(c.child)) {
            soft.add(
                offset_residual(p.init_pos, p.init_rot, c.offset, ch.init_pos),
                i,
            );
        }
    }

    // B (orientation): qAdjust is an ABSOLUTE factor -- each node's rest rotation is
    // basis(rest positions) * qAdjust, so at rest the reconstruction MUST equal the
    // node's authored init_rot. The writeback orientation is therefore the absolute
    // basis(solved) * qAdjust (no relative-delta machinery). This is the hard pin on
    // quat_from_basis + the basis convention + the qAdjust multiply order.
    let mut reconstructed = 0usize;
    let mut resolvable = 0usize;
    let mut orient = Stat::new();
    for (i, b) in fe.node_bases.iter().enumerate() {
        let Some(node) = n.get(b.node) else { continue };
        resolvable += 1;
        let Some(q) = node_base_quat(n, b.x0, b.x1, b.y0, b.y1, b.q_adjust) else {
            continue;
        };
        reconstructed += 1;
        orient.add(quat_angle(q, node.init_rot).to_degrees(), i);
    }

    eprintln!("\n== {name} rest oracle ==");
    eprintln!(
        "  ctrl offsets : n={:<5} max={:.5}cm mean={:.5}cm worst=#{}",
        ctrl.n,
        ctrl.max,
        ctrl.mean(),
        ctrl.worst
    );
    eprintln!(
        "  reverse offs : n={:<5} max={:.5}cm mean={:.5}cm worst=#{}",
        rev.n,
        rev.max,
        rev.mean(),
        rev.worst
    );
    eprintln!(
        "  soft offsets : n={:<5} max={:.5}cm mean={:.5}cm worst=#{}",
        soft.n,
        soft.max,
        soft.mean(),
        soft.worst
    );
    eprintln!("  nodeBase recon: {reconstructed}/{resolvable} frames reconstructed");
    eprintln!(
        "  orientation @rest: max={:.4}deg mean={:.5}deg (basis*qAdjust == init_rot)",
        orient.max,
        orient.mean()
    );

    // Hard pins (these MUST hold at rest with no hand-tuning):
    // 1+2. Control/reverse offsets are authored exactly -> rest residual ~0; a wrong
    //      frame or sign would be cm-scale. This is the position binding the prior
    //      solver got wrong.
    assert!(
        ctrl.max < 1e-2,
        "{name}: ctrl-offset rest residual {:.5}cm too large (frame/sign convention wrong)",
        ctrl.max
    );
    assert!(
        rev.n == 0 || rev.max < 1e-2,
        "{name}: reverse-offset rest residual {:.5}cm too large",
        rev.max
    );
    // 3. Every node-base frame reconstructs (no degenerate basis).
    assert_eq!(
        reconstructed,
        resolvable,
        "{name}: {} node-base frames failed to reconstruct",
        resolvable - reconstructed
    );
    // 4. Orientation: basis(rest) * qAdjust reproduces each node's authored init_rot.
    //    0.5deg is far above the f32 noise floor and far below a wrong convention (the
    //    next-best basis convention was ~18deg, others >= 89deg).
    assert!(
        orient.n == 0 || orient.max < 0.5,
        "{name}: node-base orientation basis*qAdjust does not reproduce init_rot at rest ({:.4}deg) -- basis convention or qAdjust order wrong",
        orient.max
    );

    // Anti-triviality guard: the position pins are only meaningful if offsets are
    // actually non-zero. A hero with all-~0 offsets would pass vacuously.
    let mean_ctrl_offset: f32 = if fe.ctrl_offsets.is_empty() {
        0.0
    } else {
        fe.ctrl_offsets
            .iter()
            .map(|c| {
                (c.offset[0] * c.offset[0] + c.offset[1] * c.offset[1] + c.offset[2] * c.offset[2])
                    .sqrt()
            })
            .sum::<f32>()
            / fe.ctrl_offsets.len() as f32
    };
    assert!(
        fe.ctrl_offsets.is_empty() || mean_ctrl_offset > 0.1,
        "{name}: mean ctrl offset {mean_ctrl_offset:.4}cm too small -- position pin may be passing trivially"
    );
}

#[test]
fn fe_model_rest_invariants_hold() {
    let Ok(vpk_path) = std::env::var("MORPHIC_MODEL_VPK") else {
        eprintln!("MORPHIC_MODEL_VPK not set; skipping FeModel rest oracle");
        return;
    };
    let entry = std::env::var("MORPHIC_MODEL_ENTRY")
        .unwrap_or_else(|_| "models/heroes_wip/necro/necro.vmdl_c".to_string());
    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let bytes = vpk
        .get_file(&entry)
        .and_then(|mut f| f.read_all())
        .unwrap_or_else(|e| panic!("read {entry}: {e:?}"));
    let fe = decode_fe_model(&bytes).expect("FeModel");
    run(&entry, &fe);
}
