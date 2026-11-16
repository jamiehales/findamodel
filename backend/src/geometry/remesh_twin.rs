//! Remeshed twin: the twin lookup's fallback ([`crate::geometry::twin`]) for a supported file
//! whose twin (the same sculpt without supports) was remeshed or decimated, so its triangles match
//! none of the supported file's and the exact keys find nothing.
//!
//! 1. **Descriptor.** Every model stores a thick-point D2 shape distribution ([`Descriptor`]): area
//!    weighted surface samples whose inward ray meets no surface within [`THICK_MM`] (supports are
//!    thin, so on a supported file this keeps mostly the model), and the distances between pairs
//!    of them in [`D2_BINS`] log bins, as raw pair areas (mm^4) rather than shares. A twin's thick
//!    pairs are then about a subset of the supported file's: a candidate must be contained,
//!    `sum(min(S, U)) / sum(U)`, with checks on thick area and spread, and candidates rank by
//!    `sum(min(S, U)) / sum(max(S, U))`, which also wants the twin about as large ([`rank`]). A
//!    supported file's descriptor leaves out its raft ([`RAFT_CUT_MM`]).
//! 2. **Global alignment.** Point pair feature voting (Drost et al. 2010): the twin's oriented
//!    samples are hashed by the features of every pair of them, and pairs in the supported file
//!    vote for a twin point and a rotation about its normal, so each vote is a pose. Supports
//!    only add background votes ([`ppf_poses`]).
//! 3. **Refinement.** Point-to-plane ICP of the twin onto the supported file's surface, pairs with
//!    disagreeing normals left out and far pairs weighed down (Welsch), the scale held at 1
//!    ([`icp`]).
//! 4. **Labelling and guards.** The twin must lie on the supported file closely
//!    ([`MAX_P90_DEVIATION_MM`]); then each supported triangle within [`TOLERANCE_MM`] of the twin
//!    with an agreeing normal is model, and the twin split's guards run on it ([`twin::combine`]:
//!    coverage, largest gap, inner walls, seams). On top of those: no large connected patch of the
//!    twin off the supported file ([`MAX_PATCH_MM2`], another weapon or flag). A large thick area
//!    left as support is flagged ([`THICK_SUPPORT_FLAG_SHARE`]).
//! 5. **Choice.** Up to [`MAX_CANDIDATES`] candidates are verified in rank order; of those passing,
//!    the one explaining the most of the supported file's area is kept ([`lookup`]).

use crate::geometry::Triangle;
use crate::geometry::support_reader::math::{
    BoxIndex, P, add, closest_on_tri, cross, dist, dot, eig3, len, mul, points_of, sub, unit,
};
use crate::geometry::twin::{self, Labels, Mesh, Registration, Similarity, TwinSplit};
use rayon::prelude::*;
use rustc_hash::FxHashMap;

/// Increment when the descriptor, the alignment, the labels or the guards change, so stored
/// descriptors and the splits made from them are rebuilt.
pub const CURRENT_REMESH_TWIN_VERSION: i32 = 1;

/// D2 bins: log2 of the pair distance from [`D2_MIN_MM`] over [`D2_OCTAVES`] octaves.
pub const D2_BINS: usize = 32;
pub const D2_MIN_MM: f64 = 0.25;
pub const D2_OCTAVES: f64 = 10.0;
/// A surface point is thick when its inward ray meets no surface within this (mm). Support rods
/// and tips are thinner.
pub const THICK_MM: f64 = 2.0;
/// A supported file's descriptor and alignment leave out its points this close to its bottom
/// (mm, Y-up): the raft.
pub const RAFT_CUT_MM: f64 = 4.0;
/// Surface samples of a descriptor, and D2 pairs drawn from its thick ones.
pub const DESCRIPTOR_SAMPLES: usize = 8192;
pub const D2_PAIRS: usize = 100_000;

/// A candidate must have this share of its D2 pair area inside the supported file's, and be this
/// alike ([`similarity`]; a hollowed supported file and its solid twin are 0.53).
pub const MIN_CONTAINMENT: f64 = 0.6;
pub const MIN_SIMILARITY: f64 = 0.4;
/// Once a candidate passes, later ones are verified only when this close in similarity (a near
/// variant, which may explain more of the supported file).
pub const NEAR_VARIANT_MARGIN: f64 = 0.1;
/// The candidate's thick area over the supported file's: the twin's thick parts are about all
/// of the supported file's (its supports are thin), never much more.
pub const MIN_THICK_AREA_RATIO: f64 = 0.4;
pub const MAX_THICK_AREA_RATIO: f64 = 1.3;
/// The candidate's largest thick spread (PCA) over the supported file's.
pub const MIN_SPREAD_RATIO: f64 = 0.5;
pub const MAX_SPREAD_RATIO: f64 = 1.3;
/// Candidates verified per supported file, in rank order.
pub const MAX_CANDIDATES: usize = 3;

/// Twin points the pair feature model is sampled to (about): its sampling step follows.
pub const PPF_MODEL_POINTS: usize = 1000;
/// Pair feature angle bins (over 0 to pi) and rotation bins (over a turn).
const PPF_ANGLE_BINS: usize = 15;
const PPF_ROTATION_BINS: usize = 30;
/// Every this many scene points is a reference point.
const PPF_REF_STEP: usize = 4;
/// Poses kept after clustering, each refined and scored.
pub const PPF_POSES: usize = 12;

/// Samples of the twin the ICP moves (of which every pose's coarse refinement moves a few), and
/// of the supported file's surface it pairs them with; poses refined finely.
const ICP_POINTS: usize = 3000;
const COARSE_POINTS: usize = 750;
const FINE_POSES: usize = 2;
const CLOUD_SAMPLES: usize = 200_000;
/// Pairs whose normals disagree more than this (dot) are left out.
pub const ICP_MIN_NORMAL_DOT: f64 = 0.5;

/// Twin samples measured for the deviation (distance onto the supported file's surface).
const DEVIATION_SAMPLES: usize = 20_000;
/// Deviation measures stop at this (mm): a point further has no partner.
const DEVIATION_CAP_MM: f64 = 1.0;
/// The twin's 90th percentile distance onto the supported file must be at most this (mm):
/// remeshing moves the surface by microns, another sculpt or pose by tenths.
pub const MAX_P90_DEVIATION_MM: f64 = 0.01;
/// A supported triangle within this (mm) of the twin, with a normal agreeing to
/// [`LABEL_MIN_NORMAL_DOT`], is model. Fixed, so a poor fit cannot widen it.
pub const TOLERANCE_MM: f64 = 0.1;
pub const LABEL_MIN_NORMAL_DOT: f64 = 0.7;
/// A support group (edge-connected) wholly within this (mm) of the twin with agreeing normals is
/// model the remeshing moved further than [`TOLERANCE_MM`] (a support has rods off the twin).
pub const NOISE_ISLAND_MM: f64 = 0.3;
/// A twin triangle whose centre has no surface of the supported file (model or support) within
/// this (mm) is off it. Under a support's contact the twin is inside the support's tip, a few
/// tenths from its surface; a part the supported file lacks is off it by its own thickness.
pub const PATCH_DEVIATION_MM: f64 = 1.0;
/// Largest connected area of the twin off the supported file (mm^2): another weapon or flag.
/// The limit grows with the twin ([`MAX_PATCH_SHARE`]): a hollowed file's drain holes are such
/// patches, 47 mm^2 on a large Loot model of 34,000 mm^2.
pub const MAX_PATCH_MM2: f64 = 10.0;
pub const MAX_PATCH_SHARE: f64 = 0.0025;
/// Thick area left as support (above the raft), as a share of the twin's area, above which a split
/// is flagged: supports are thin and a part the twin lacks is thick. Only flagged, not refused:
/// heavy trunk supports and tall rafts are thick too (the Loot answer keys leave 0.3% to 45% of
/// the twin's area thick on their support side), and a part the twin lacks already fails the seam
/// guard ([`twin::support_like`]).
pub const THICK_SUPPORT_FLAG_SHARE: f64 = 0.05;

type M3 = [[f64; 3]; 3];

const NONE: u32 = u32::MAX;

/// Deterministic xorshift.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn tri_area(t: &[P; 3]) -> f64 {
    0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0])))
}

fn tri_normal(t: &[P; 3]) -> P {
    unit(cross(sub(t[1], t[0]), sub(t[2], t[0])))
}

fn centroid(t: &[P; 3]) -> P {
    mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0)
}

fn mat_vec(r: &M3, v: P) -> P {
    [dot(r[0], v), dot(r[1], v), dot(r[2], v)]
}

fn mat_mul(a: &M3, b: &M3) -> M3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, x) in row.iter_mut().enumerate() {
            *x = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

fn transpose(a: &M3) -> M3 {
    [0, 1, 2].map(|i| [a[0][i], a[1][i], a[2][i]])
}

/// The rotation by `angle` about unit `axis` (Rodrigues).
fn axis_angle(axis: P, angle: f64) -> M3 {
    let (s, c) = angle.sin_cos();
    let [x, y, z] = axis;
    let t = 1.0 - c;
    [
        [c + x * x * t, x * y * t - z * s, x * z * t + y * s],
        [y * x * t + z * s, c + y * y * t, y * z * t - x * s],
        [z * x * t - y * s, z * y * t + x * s, c + z * z * t],
    ]
}

/// The rotation of a rotation vector (axis times angle).
fn rotation_of(w: P) -> M3 {
    let a = len(w);
    if a < 1e-15 { [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] } else { axis_angle(mul(w, 1.0 / a), a) }
}

/// The angle (radians) of the rotation taking `a` to `b`.
fn rotation_angle(a: &M3, b: &M3) -> f64 {
    let m = mat_mul(b, &transpose(a));
    ((m[0][0] + m[1][1] + m[2][2] - 1.0) * 0.5).clamp(-1.0, 1.0).acos()
}

/// Area-weighted surface samples: point, unit normal and triangle.
pub fn sample(tris: &[[P; 3]], n: usize, seed: u64) -> Vec<(P, P, u32)> {
    let mut cum = Vec::with_capacity(tris.len());
    let mut acc = 0.0;
    for t in tris {
        acc += tri_area(t);
        cum.push(acc);
    }
    if tris.is_empty() || acc <= 0.0 {
        return Vec::new();
    }
    let mut r = Rng::new(seed);
    (0..n)
        .filter_map(|_| {
            let x = r.f() * acc;
            let i = cum.partition_point(|&c| c < x).min(tris.len() - 1);
            let t = &tris[i];
            let nn = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            let (a, b) = (r.f().sqrt(), r.f());
            if len(nn) < 1e-20 {
                return None;
            }
            let p = add(add(mul(t[0], 1.0 - a), mul(t[1], a * (1.0 - b))), mul(t[2], a * b));
            Some((p, unit(nn), i as u32))
        })
        .collect()
}

/// Mean edge length of a sample of the triangles.
fn mean_edge(tris: &[[P; 3]]) -> f64 {
    if tris.is_empty() {
        return 1.0;
    }
    let step = (tris.len() / 20000).max(1);
    let (sum, n) = tris.iter().step_by(step).fold((0.0, 0usize), |(s, n), t| (s + dist(t[1], t[0]), n + 1));
    sum / n as f64
}

/// A grid cell for indexing `tris` for queries of about `query` mm.
fn cell_for(tris: &[[P; 3]], query: f64) -> f64 {
    (mean_edge(tris) * 2.0).max(query).clamp(0.05, 4.0)
}

/// Whether the ray from `o` along unit `d` crosses triangle `t` within `reach` (Moller-Trumbore).
fn ray_meets(o: P, d: P, t: &[P; 3], reach: f64) -> bool {
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-14 {
        return false;
    }
    let f = 1.0 / det;
    let sv = sub(o, t[0]);
    let u = f * dot(sv, h);
    let q = cross(sv, e1);
    let v = f * dot(d, q);
    let x = f * dot(e2, q);
    (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && x > 1e-6 && x <= reach
}

/// Cell (mm) of a [`RayIndex`].
const RAY_CELL_MM: f64 = 0.5;
/// A triangle whose box spans more cells than this is checked by every ray instead.
const RAY_MAX_CELLS: i64 = 4096;

/// The triangles along a known set of short rays, indexed only in the cells those rays pass
/// through: a mesh of millions of triangles is bucketed in one pass without a sort, for the few
/// thousand rays of the thickness test.
struct RayIndex {
    cells: FxHashMap<[i32; 3], Vec<u32>>,
    big: Vec<u32>,
}

/// Cells to index, as a bitmap over their box (a hash set when the box is huge): most of a
/// mesh's triangles are tested against it, so the test must be cheap.
struct CellSet {
    lo: [i32; 3],
    dims: [i64; 3],
    bits: Vec<u64>,
    set: rustc_hash::FxHashSet<[i32; 3]>,
}

impl CellSet {
    const MAX_BITS: i64 = 1 << 28;

    fn new(set: rustc_hash::FxHashSet<[i32; 3]>) -> Self {
        let lo = [0, 1, 2].map(|k| set.iter().map(|c| c[k]).min().unwrap_or(0));
        let hi = [0, 1, 2].map(|k| set.iter().map(|c| c[k]).max().unwrap_or(-1));
        let dims = [0, 1, 2].map(|k| (i64::from(hi[k]) - i64::from(lo[k]) + 1).max(0));
        let n = dims[0] * dims[1] * dims[2];
        if n > Self::MAX_BITS {
            return CellSet { lo, dims, bits: Vec::new(), set };
        }
        let mut bits = vec![0u64; (n as usize).div_ceil(64)];
        for c in &set {
            let i = Self::at(lo, dims, *c).unwrap();
            bits[i / 64] |= 1 << (i % 64);
        }
        CellSet { lo, dims, bits, set: Default::default() }
    }

    fn at(lo: [i32; 3], dims: [i64; 3], c: [i32; 3]) -> Option<usize> {
        let d = [0, 1, 2].map(|k| i64::from(c[k]) - i64::from(lo[k]));
        (0..3).all(|k| d[k] >= 0 && d[k] < dims[k]).then(|| ((d[0] * dims[1] + d[1]) * dims[2] + d[2]) as usize)
    }

    fn contains(&self, c: &[i32; 3]) -> bool {
        if self.bits.is_empty() {
            return self.set.contains(c);
        }
        Self::at(self.lo, self.dims, *c).is_some_and(|i| self.bits[i / 64] >> (i % 64) & 1 == 1)
    }
}

fn ray_cell(p: P) -> [i32; 3] {
    p.map(|x| (x / RAY_CELL_MM).floor().clamp(-1e9, 1e9) as i32)
}

/// Points along a ray every quarter cell, so it misses no cell but by a corner.
fn ray_points(o: P, d: P, reach: f64) -> impl Iterator<Item = P> {
    let n = (reach / (RAY_CELL_MM * 0.25)).ceil().max(1.0) as usize;
    (0..=n).map(move |k| add(o, mul(d, reach * k as f64 / n as f64)))
}

impl RayIndex {
    /// Indexes `tris` for rays (origin, unit direction) of length `reach`.
    fn new(tris: &[[P; 3]], rays: &[(P, P)], reach: f64) -> Self {
        let mut need: rustc_hash::FxHashSet<[i32; 3]> = rustc_hash::FxHashSet::default();
        for &(o, d) in rays {
            need.extend(ray_points(o, d, reach).map(ray_cell));
        }
        let need = CellSet::new(need);
        // Per-thread lists (no allocation per triangle); the order within a cell does not matter.
        type Found = (Vec<([i32; 3], u32)>, Vec<u32>);
        let parts: Vec<Found> = tris
            .par_iter()
            .enumerate()
            .fold(
                || (Vec::new(), Vec::new()),
                |(mut found, mut big), (i, t)| {
                    let lo = ray_cell([0, 1, 2].map(|k| t[0][k].min(t[1][k]).min(t[2][k])));
                    let hi = ray_cell([0, 1, 2].map(|k| t[0][k].max(t[1][k]).max(t[2][k])));
                    let span: i64 = (0..3).map(|k| i64::from(hi[k]) - i64::from(lo[k]) + 1).product();
                    if span > RAY_MAX_CELLS {
                        big.push(i as u32);
                    } else {
                        for x in lo[0]..=hi[0] {
                            for y in lo[1]..=hi[1] {
                                for z in lo[2]..=hi[2] {
                                    if need.contains(&[x, y, z]) {
                                        found.push(([x, y, z], i as u32));
                                    }
                                }
                            }
                        }
                    }
                    (found, big)
                },
            )
            .collect();
        let mut cells: FxHashMap<[i32; 3], Vec<u32>> = FxHashMap::default();
        let mut big = Vec::new();
        for (found, b) in parts {
            for (c, i) in found {
                cells.entry(c).or_default().push(i);
            }
            big.extend(b);
        }
        RayIndex { cells, big }
    }

    /// Whether the ray from `o` along unit `d` meets a triangle other than `skip` within `reach`.
    fn hits(&self, tris: &[[P; 3]], o: P, d: P, reach: f64, skip: u32, buf: &mut Vec<u32>) -> bool {
        buf.clear();
        let mut last = None;
        for c in ray_points(o, d, reach).map(ray_cell) {
            if last == Some(c) {
                continue;
            }
            last = Some(c);
            if let Some(v) = self.cells.get(&c) {
                buf.extend_from_slice(v);
            }
        }
        buf.extend_from_slice(&self.big);
        buf.sort_unstable();
        buf.dedup();
        buf.iter().any(|&i| i != skip && ray_meets(o, d, &tris[i as usize], reach))
    }
}

/// For each sample, whether it is thick: its inward ray meets no surface within [`THICK_MM`].
fn thick_of(tris: &[[P; 3]], samples: &[(P, P, u32)]) -> Vec<bool> {
    let rays: Vec<(P, P)> = samples.iter().map(|&(p, n, _)| (p, mul(n, -1.0))).collect();
    let idx = RayIndex::new(tris, &rays, THICK_MM);
    samples
        .par_iter()
        .zip(&rays)
        .map_init(Vec::new, |buf, (&(_, _, i), &(o, d))| !idx.hits(tris, o, d, THICK_MM, i, buf))
        .collect()
}

/// PCA spreads (standard deviations, mm) of points, largest first.
fn spreads(pts: &[P]) -> [f64; 3] {
    if pts.is_empty() {
        return [0.0; 3];
    }
    let n = pts.len() as f64;
    let c = mul(pts.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / n);
    let mut m = [[0.0; 3]; 3];
    for p in pts {
        let d = sub(*p, c);
        for (i, row) in m.iter_mut().enumerate() {
            for (j, x) in row.iter_mut().enumerate() {
                *x += d[i] * d[j] / n;
            }
        }
    }
    let (e, _) = eig3(m);
    [e[2], e[1], e[0]].map(|x| x.max(0.0).sqrt())
}

/// The D2 bin of a distance (None outside the bins).
fn d2_bin(d: f64) -> Option<usize> {
    let x = ((d.max(1e-9) / D2_MIN_MM).log2() / D2_OCTAVES * D2_BINS as f64).floor();
    (x >= 0.0 && x < D2_BINS as f64).then_some(x as usize)
}

/// A model's stored shape descriptor for the remeshed twin lookup (see the module notes).
#[derive(Debug, Clone, PartialEq)]
pub struct Descriptor {
    /// Pair area per distance bin (mm^4): the thick area squared, spread over the bins as its
    /// pairs' distances fall.
    pub d2: [f32; D2_BINS],
    /// Thick surface area (mm^2) and total surface area.
    pub thick_area: f32,
    pub area: f32,
    /// PCA spreads of the thick points (mm), largest first.
    pub spreads: [f32; 3],
}

impl Descriptor {
    /// The descriptor of loaded (Y-up) triangles; `raft_cut` leaves out the bottom
    /// [`RAFT_CUT_MM`] (a supported file's raft).
    pub fn of_triangles(triangles: &[Triangle], raft_cut: bool) -> Self {
        Self::of(&points_of(triangles), raft_cut)
    }

    /// The descriptor of triangles' corners.
    pub fn of(tris: &[[P; 3]], raft_cut: bool) -> Self {
        let area: f64 = tris.iter().map(tri_area).sum();
        let ymin = tris.iter().flatten().map(|p| p[1]).fold(f64::MAX, f64::min);
        let samples = sample(tris, DESCRIPTOR_SAMPLES, 7);
        let kept: Vec<(P, P, u32)> =
            samples.iter().filter(|s| !raft_cut || s.0[1] >= ymin + RAFT_CUT_MM).copied().collect();
        let thick = thick_of(tris, &kept);
        let pts: Vec<P> = kept.iter().zip(&thick).filter(|x| *x.1).map(|x| x.0.0).collect();
        let thick_area = area * pts.len() as f64 / samples.len().max(1) as f64;
        let mut d2 = [0f32; D2_BINS];
        if pts.len() >= 2 {
            let mut r = Rng::new(3);
            let w = thick_area * thick_area / D2_PAIRS as f64;
            for _ in 0..D2_PAIRS {
                let (a, b) = (pts[r.below(pts.len())], pts[r.below(pts.len())]);
                if let Some(k) = d2_bin(dist(a, b)) {
                    d2[k] += w as f32;
                }
            }
        }
        Descriptor { d2, thick_area: thick_area as f32, area: area as f32, spreads: spreads(&pts).map(|x| x as f32) }
    }

    /// Stored form: version (u8), the bins, thick area, area and spreads (f32, little endian).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(1 + (D2_BINS + 5) * 4);
        b.push(CURRENT_REMESH_TWIN_VERSION as u8);
        for x in self.d2.iter().chain([&self.thick_area, &self.area]).chain(&self.spreads) {
            b.extend(x.to_le_bytes());
        }
        b
    }

    /// Reads a stored descriptor; None when it is malformed or from another version.
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() != 1 + (D2_BINS + 5) * 4 || b[0] != CURRENT_REMESH_TWIN_VERSION as u8 {
            return None;
        }
        let f: Vec<f32> = b[1..].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
        let mut d2 = [0f32; D2_BINS];
        d2.copy_from_slice(&f[..D2_BINS]);
        Some(Descriptor {
            d2,
            thick_area: f[D2_BINS],
            area: f[D2_BINS + 1],
            spreads: [f[D2_BINS + 2], f[D2_BINS + 3], f[D2_BINS + 4]],
        })
    }
}

/// Share of twin `u`'s pair area inside supported `s`'s: `sum(min(S, U)) / sum(U)`.
pub fn containment(s: &Descriptor, u: &Descriptor) -> f64 {
    let total: f64 = u.d2.iter().map(|&x| f64::from(x)).sum();
    if total <= 0.0 {
        return 0.0;
    }
    s.d2.iter().zip(&u.d2).map(|(&a, &b)| f64::from(a.min(b))).sum::<f64>() / total
}

/// How alike the pair areas are, `sum(min(S, U)) / sum(max(S, U))` (weighted Jaccard): high only
/// when the twin is contained and about as large. Containment alone favours any small part (on
/// the Iron Sultinate pool 2 of 20 twins rank first by it, and 20 of 20 by this).
pub fn similarity(s: &Descriptor, u: &Descriptor) -> f64 {
    let (mut lo, mut hi) = (0.0, 0.0);
    for (&a, &b) in s.d2.iter().zip(&u.d2) {
        lo += f64::from(a.min(b));
        hi += f64::from(a.max(b));
    }
    if hi > 0.0 { lo / hi } else { 0.0 }
}

/// A candidate's descriptor scores against the supported file's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ranked {
    /// Index in the candidates given.
    pub index: usize,
    /// The rank's measure ([`similarity`]) and the twin's share inside ([`containment`]).
    pub similarity: f64,
    pub containment: f64,
    /// Thick area and largest spread, candidate over supported file.
    pub area_ratio: f64,
    pub spread_ratio: f64,
}

impl Ranked {
    /// Whether the candidate is worth verifying.
    pub fn plausible(&self) -> bool {
        self.similarity >= MIN_SIMILARITY
            && self.containment >= MIN_CONTAINMENT
            && (MIN_THICK_AREA_RATIO..=MAX_THICK_AREA_RATIO).contains(&self.area_ratio)
            && (MIN_SPREAD_RATIO..=MAX_SPREAD_RATIO).contains(&self.spread_ratio)
    }
}

/// The candidates scored against supported `s`, plausible ones first, each group by similarity.
pub fn rank(s: &Descriptor, candidates: &[Descriptor]) -> Vec<Ranked> {
    let mut out: Vec<Ranked> = candidates
        .iter()
        .enumerate()
        .map(|(index, u)| Ranked {
            index,
            similarity: similarity(s, u),
            containment: containment(s, u),
            area_ratio: f64::from(u.thick_area) / f64::from(s.thick_area).max(1e-9),
            spread_ratio: f64::from(u.spreads[0]) / f64::from(s.spreads[0]).max(1e-9),
        })
        .collect();
    out.sort_by(|a, b| {
        b.plausible().cmp(&a.plausible()).then(b.similarity.total_cmp(&a.similarity)).then(a.index.cmp(&b.index))
    });
    out
}

/// An oriented surface point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oriented {
    pub p: P,
    pub n: P,
}

/// One sample per `cell` mm voxel (the first, in sample order).
pub fn voxel_downsample(samples: &[(P, P, u32)], cell: f64) -> Vec<Oriented> {
    let mut seen: FxHashMap<[i32; 3], ()> = FxHashMap::default();
    samples
        .iter()
        .filter(|s| seen.insert(s.0.map(|x| (x / cell).floor() as i32), ()).is_none())
        .map(|s| Oriented { p: s.0, n: s.1 })
        .collect()
}

/// The frame taking `p` to the origin and unit `n` onto +x: (rotation, translation).
fn frame_of(p: P, n: P) -> (M3, P) {
    let x = [1.0, 0.0, 0.0];
    let axis = cross(n, x);
    let s = len(axis);
    let c = dot(n, x);
    let r = if s < 1e-12 {
        if c > 0.0 { axis_angle([0.0, 0.0, 1.0], 0.0) } else { axis_angle([0.0, 0.0, 1.0], std::f64::consts::PI) }
    } else {
        axis_angle(mul(axis, 1.0 / s), s.atan2(c))
    };
    (r, mul(mat_vec(&r, p), -1.0))
}

/// The pair feature key of oriented points `a` and `b` at distance step `step`, None beyond
/// `max_d` or when they coincide.
fn ppf_key(a: &Oriented, b: &Oriented, step: f64, max_d: f64) -> Option<u32> {
    let d = sub(b.p, a.p);
    let l = len(d);
    if l < 1e-9 || l > max_d {
        return None;
    }
    let du = mul(d, 1.0 / l);
    let ang = |x: f64| {
        let q = (x.clamp(-1.0, 1.0).acos() / std::f64::consts::PI * PPF_ANGLE_BINS as f64) as u32;
        q.min(PPF_ANGLE_BINS as u32 - 1)
    };
    let nb = PPF_ANGLE_BINS as u32;
    let k = ((((l / step) as u32 * nb + ang(dot(a.n, du))) * nb + ang(dot(b.n, du))) * nb) + ang(dot(a.n, b.n));
    Some(k)
}

/// The angle of a point about +x in a frame ([`frame_of`]).
fn alpha_of(frame: &(M3, P), p: P) -> f64 {
    let q = add(mat_vec(&frame.0, p), frame.1);
    q[2].atan2(q[1])
}

/// A twin's pair feature model ([`ppf_poses`]).
pub struct PpfModel {
    pub step: f64,
    pub diameter: f64,
    pub points: Vec<Oriented>,
    frames: Vec<(M3, P)>,
    /// (key, reference point, angle of the other point about the reference's normal), sorted.
    table: Vec<(u32, u32, f32)>,
}

impl PpfModel {
    /// The model of the twin's triangles, sampled to about [`PPF_MODEL_POINTS`].
    pub fn new(tris: &[[P; 3]]) -> Self {
        let area: f64 = tris.iter().map(tri_area).sum();
        let dense = sample(tris, (PPF_MODEL_POINTS * 40).max(4000), 17);
        let mut step = (area / PPF_MODEL_POINTS as f64).sqrt().max(0.05);
        let mut points = voxel_downsample(&dense, step);
        while points.len() > PPF_MODEL_POINTS * 3 / 2 {
            step *= 1.15;
            points = voxel_downsample(&dense, step);
        }
        let lo = [0, 1, 2].map(|k| points.iter().map(|o| o.p[k]).fold(f64::MAX, f64::min));
        let hi = [0, 1, 2].map(|k| points.iter().map(|o| o.p[k]).fold(f64::MIN, f64::max));
        let diameter = dist(lo, hi).max(step);
        let frames: Vec<(M3, P)> = points.iter().map(|o| frame_of(o.p, o.n)).collect();
        let mut table: Vec<(u32, u32, f32)> = (0..points.len())
            .into_par_iter()
            .flat_map_iter(|i| {
                let (a, f) = (&points[i], &frames[i]);
                points.iter().enumerate().filter(move |(j, _)| *j != i).filter_map(move |(_, b)| {
                    ppf_key(a, b, step, diameter).map(|k| (k, i as u32, alpha_of(f, b.p) as f32))
                })
            })
            .collect();
        table.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)));
        PpfModel { step, diameter, points, frames, table }
    }

    fn list(&self, key: u32) -> &[(u32, u32, f32)] {
        let lo = self.table.partition_point(|e| e.0 < key);
        let hi = lo + self.table[lo..].partition_point(|e| e.0 == key);
        &self.table[lo..hi]
    }
}

/// A pose hypothesis: the transform taking the twin onto the supported file, and its votes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub transform: Similarity,
    pub votes: u32,
}

/// Pose hypotheses of `model` in `scene` (the supported file's oriented samples at the model's
/// step), by pair feature voting, clustered, most votes first (at most [`PPF_POSES`]).
pub fn ppf_poses(model: &PpfModel, scene: &[Oriented]) -> Vec<Pose> {
    let nm = model.points.len();
    if nm < 3 || scene.len() < 3 {
        return Vec::new();
    }
    let refs: Vec<usize> = (0..scene.len()).step_by(PPF_REF_STEP).collect();
    let bins = PPF_ROTATION_BINS as f64;
    let tau = std::f64::consts::TAU;
    let raw: Vec<Pose> = refs
        .par_iter()
        .map_init(
            || vec![0u16; nm * PPF_ROTATION_BINS],
            |acc, &r| {
                acc.iter_mut().for_each(|x| *x = 0);
                let sr = &scene[r];
                let fs = frame_of(sr.p, sr.n);
                for (i, si) in scene.iter().enumerate() {
                    if i == r {
                        continue;
                    }
                    let Some(k) = ppf_key(sr, si, model.step, model.diameter) else { continue };
                    let list = model.list(k);
                    if list.is_empty() {
                        continue;
                    }
                    let a_s = alpha_of(&fs, si.p);
                    for &(_, mr, a_m) in list {
                        let th = (a_s - f64::from(a_m)).rem_euclid(tau);
                        let b = ((th / tau * bins) as usize).min(PPF_ROTATION_BINS - 1);
                        let c = &mut acc[mr as usize * PPF_ROTATION_BINS + b];
                        *c = c.saturating_add(1);
                    }
                }
                let (best, at) =
                    acc.iter().enumerate().fold((0u16, 0usize), |b, (i, &v)| if v > b.0 { (v, i) } else { b });
                let (mr, b) = (at / PPF_ROTATION_BINS, at % PPF_ROTATION_BINS);
                let th = (b as f64 + 0.5) / bins * tau;
                // T = Fs^-1 * Rx(th) * Fm.
                let rx = axis_angle([1.0, 0.0, 0.0], th);
                let (rm, tm) = &model.frames[mr];
                let rst = transpose(&fs.0);
                let r_all = mat_mul(&rst, &mat_mul(&rx, rm));
                let t = mat_vec(&rst, sub(mat_vec(&rx, *tm), fs.1));
                Pose { transform: Similarity { r: r_all, scale: 1.0, t }, votes: u32::from(best) }
            },
        )
        .filter(|p| p.votes > 0)
        .collect();
    cluster(raw, model)
}

/// Greedy pose clustering: poses within 15 degrees and two steps (at the model's centre) of a
/// cluster's first pose join it; clusters by their summed votes.
fn cluster(mut poses: Vec<Pose>, model: &PpfModel) -> Vec<Pose> {
    poses.sort_by_key(|p| std::cmp::Reverse(p.votes));
    let n = model.points.len() as f64;
    let c = mul(model.points.iter().fold([0.0; 3], |a, o| add(a, o.p)), 1.0 / n);
    let mut clusters: Vec<Pose> = Vec::new();
    for p in poses {
        let pc = p.transform.apply(c);
        let near = clusters.iter_mut().take(256).find(|q| {
            dist(q.transform.apply(c), pc) < 2.0 * model.step
                && rotation_angle(&q.transform.r, &p.transform.r) < 15f64.to_radians()
        });
        match near {
            Some(q) => q.votes += p.votes,
            None => clusters.push(p),
        }
    }
    clusters.sort_by_key(|p| std::cmp::Reverse(p.votes));
    clusters.truncate(PPF_POSES);
    clusters
}

/// Oriented points in cubic cells, for nearest partners with agreeing normals.
pub struct Cloud {
    cell: f64,
    points: Vec<Oriented>,
    grid: FxHashMap<[i32; 3], Vec<u32>>,
}

impl Cloud {
    pub fn new(points: Vec<Oriented>, cell: f64) -> Self {
        let mut grid: FxHashMap<[i32; 3], Vec<u32>> = FxHashMap::default();
        for (i, o) in points.iter().enumerate() {
            grid.entry(o.p.map(|x| (x / cell).floor() as i32)).or_default().push(i as u32);
        }
        Cloud { cell, points, grid }
    }

    /// The nearest point within `maxr` whose normal agrees with `n` to `min_dot`.
    fn nearest(&self, p: P, n: P, maxr: f64, min_dot: f64) -> Option<Oriented> {
        let c = p.map(|x| (x / self.cell).floor() as i32);
        let rings = (maxr / self.cell).ceil() as i32;
        let mut best: Option<(f64, u32)> = None;
        for ring in 0..=rings {
            if best.is_some_and(|b| b.0 < (f64::from(ring) - 1.0) * self.cell) {
                break;
            }
            for dx in -ring..=ring {
                for dy in -ring..=ring {
                    for dz in -ring..=ring {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != ring {
                            continue;
                        }
                        let Some(v) = self.grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) else { continue };
                        for &i in v {
                            let o = &self.points[i as usize];
                            let d = dist(p, o.p);
                            if d <= maxr && dot(n, o.n) >= min_dot && best.is_none_or(|b| d < b.0) {
                                best = Some((d, i));
                            }
                        }
                    }
                }
            }
        }
        best.map(|b| self.points[b.1 as usize])
    }
}

/// A surface's triangles indexed for closest points, with their normals. Triangles much larger
/// than the cell (a raft's, a flat base's) go in a coarser index of their own: in the fine one
/// they would be checked by every query.
pub struct Surface {
    fine: BoxIndex,
    coarse: BoxIndex,
    coarse_ids: Vec<u32>,
    normals: Vec<P>,
}

thread_local! {
    static COARSE_HITS: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Surface {
    pub fn new(tris: &[[P; 3]], cell: f64) -> Self {
        let limit = 3.0 * cell;
        let extent = |t: &[P; 3]| {
            (0..3).map(|k| t[0][k].max(t[1][k]).max(t[2][k]) - t[0][k].min(t[1][k]).min(t[2][k])).fold(0.0, f64::max)
        };
        let big: Vec<bool> = tris.par_iter().map(|t| extent(t) > limit).collect();
        // The large ones stand far away in the fine index, which keeps only boxes once built.
        let far = [1e12; 3];
        let fine_tris: Vec<[P; 3]> = tris.par_iter().zip(&big).map(|(t, &b)| if b { [far; 3] } else { *t }).collect();
        let fine = BoxIndex::new(&fine_tris, cell);
        drop(fine_tris);
        let coarse_ids: Vec<u32> = big.iter().enumerate().filter(|x| *x.1).map(|x| x.0 as u32).collect();
        let coarse_tris: Vec<[P; 3]> = coarse_ids.iter().map(|&i| tris[i as usize]).collect();
        let coarse_cell = coarse_tris.iter().map(extent).sum::<f64>() / coarse_tris.len().max(1) as f64;
        let coarse = BoxIndex::new(&coarse_tris, coarse_cell.max(limit));
        Surface { fine, coarse, coarse_ids, normals: tris.par_iter().map(tri_normal).collect() }
    }

    /// The triangles whose box meets `[lo, hi]`, into `buf` (cleared).
    fn query(&self, lo: P, hi: P, buf: &mut Vec<u32>) {
        self.fine.query_into(lo, hi, buf);
        if !self.coarse_ids.is_empty() {
            COARSE_HITS.with(|c| {
                let mut c = c.borrow_mut();
                self.coarse.query_into(lo, hi, &mut c);
                buf.extend(c.iter().map(|&j| self.coarse_ids[j as usize]));
            });
        }
    }

    /// The closest point of `tris` (as indexed) to `p` within `maxr` on a triangle whose normal
    /// agrees with `n` to `min_dot` (any triangle when `n` is None): distance, point, normal.
    fn nearest(
        &self,
        tris: &[[P; 3]],
        p: P,
        n: Option<P>,
        maxr: f64,
        min_dot: f64,
        buf: &mut Vec<u32>,
    ) -> Option<(f64, P, P)> {
        let mut r = (maxr / 8.0).max(0.02).min(maxr);
        loop {
            self.query([p[0] - r, p[1] - r, p[2] - r], [p[0] + r, p[1] + r, p[2] + r], buf);
            let best = buf
                .iter()
                .filter(|&&i| n.is_none_or(|n| dot(n, self.normals[i as usize]) >= min_dot))
                .map(|&i| {
                    let q = closest_on_tri(p, &tris[i as usize]);
                    (dist(p, q), q, self.normals[i as usize])
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some(b) = best.filter(|b| b.0 <= r) {
                return Some(b);
            }
            if r >= maxr {
                return None;
            }
            r = (r * 3.0).min(maxr);
        }
    }

    /// Whether `p` lies within `tol` of a triangle (normal agreeing with `n` when given).
    fn within(&self, tris: &[[P; 3]], p: P, n: Option<P>, tol: f64, min_dot: f64, buf: &mut Vec<u32>) -> bool {
        self.query([p[0] - tol, p[1] - tol, p[2] - tol], [p[0] + tol, p[1] + tol, p[2] + tol], buf);
        buf.iter().any(|&i| {
            n.is_none_or(|n| dot(n, self.normals[i as usize]) >= min_dot)
                && dist(p, closest_on_tri(p, &tris[i as usize])) <= tol
        })
    }

    /// [`Surface::within`] with the normal as an axis (either way round): a boolean union leaves
    /// some faces of the model wound the other way.
    fn within_axis(&self, tris: &[[P; 3]], p: P, n: Option<P>, tol: f64, min_dot: f64, buf: &mut Vec<u32>) -> bool {
        self.query([p[0] - tol, p[1] - tol, p[2] - tol], [p[0] + tol, p[1] + tol, p[2] + tol], buf);
        buf.iter().any(|&i| {
            n.is_none_or(|n| dot(n, self.normals[i as usize]).abs() >= min_dot)
                && dist(p, closest_on_tri(p, &tris[i as usize])) <= tol
        })
    }
}

/// Triangles thinner than this (twice their area over their longest edge squared) have no
/// reliable normal: slivers a boolean union leaves along seams. They are labelled by distance.
pub const SLIVER_ROUNDNESS: f64 = 0.05;

/// A triangle's normal, None for a sliver ([`SLIVER_ROUNDNESS`]).
fn reliable_normal(t: &[P; 3], n: P) -> Option<P> {
    let longest = (0..3).map(|k| dist(t[(k + 1) % 3], t[k])).fold(0.0, f64::max);
    (2.0 * tri_area(t) >= SLIVER_ROUNDNESS * longest * longest).then_some(n)
}

/// What ICP pairs a moved point with: a partner point and its normal.
pub trait Target: Sync {
    fn partner(&self, p: P, n: P, maxr: f64, buf: &mut Vec<u32>) -> Option<(P, P)>;
}

impl Target for Cloud {
    fn partner(&self, p: P, n: P, maxr: f64, _: &mut Vec<u32>) -> Option<(P, P)> {
        self.nearest(p, n, maxr, ICP_MIN_NORMAL_DOT).map(|o| (o.p, o.n))
    }
}

/// A surface with its triangles, as an ICP target.
pub struct SurfaceTarget<'a> {
    pub tris: &'a [[P; 3]],
    pub surface: &'a Surface,
}

impl Target for SurfaceTarget<'_> {
    fn partner(&self, p: P, n: P, maxr: f64, buf: &mut Vec<u32>) -> Option<(P, P)> {
        self.surface.nearest(self.tris, p, Some(n), maxr, ICP_MIN_NORMAL_DOT, buf).map(|(_, q, m)| (q, m))
    }
}

/// Solves the symmetric 6x6 system `a x = b` (Gaussian elimination with pivoting).
#[allow(clippy::needless_range_loop)]
fn solve6(mut a: [[f64; 6]; 6], mut b: [f64; 6]) -> Option<[f64; 6]> {
    for c in 0..6 {
        let p = (c..6).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..6 {
            let f = a[r][c] / a[c][c];
            for k in c..6 {
                a[r][k] -= f * a[c][k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = [0.0; 6];
    for c in (0..6).rev() {
        let s: f64 = (c + 1..6).map(|k| a[c][k] * x[k]).sum();
        x[c] = (b[c] - s) / a[c][c];
    }
    Some(x)
}

/// One stage of ICP: search radius (mm), Welsch scale (mm) and iterations.
#[derive(Debug, Clone, Copy)]
pub struct Stage {
    pub radius: f64,
    pub sigma: f64,
    pub iters: usize,
}

/// Point-to-plane ICP of `src` (twin points with normals) onto `target` from `x`, the scale held:
/// each iteration pairs every moved point with its target partner within the stage's radius
/// (normals agreeing), weighs the pair by Welsch on its plane distance, and solves the
/// linearised rigid motion.
pub fn icp(src: &[Oriented], target: &dyn Target, mut x: Similarity, stages: &[Stage]) -> Similarity {
    for st in stages {
        for _ in 0..st.iters {
            let pairs: Vec<(P, P, P)> = src
                .par_iter()
                .map_init(Vec::new, |buf, o| {
                    let p = x.apply(o.p);
                    let n = mat_vec(&x.r, o.n);
                    target.partner(p, n, st.radius, buf).map(|(q, m)| (p, q, m))
                })
                .flatten()
                .collect();
            if pairs.len() < 12 {
                return x;
            }
            let mut a = [[0.0; 6]; 6];
            let mut b = [0.0; 6];
            let s2 = 2.0 * st.sigma * st.sigma;
            for (p, q, n) in &pairs {
                let res = dot(sub(*q, *p), *n);
                let w = (-res * res / s2).exp();
                let c = cross(*p, *n);
                let row = [c[0], c[1], c[2], n[0], n[1], n[2]];
                for i in 0..6 {
                    for j in 0..6 {
                        a[i][j] += w * row[i] * row[j];
                    }
                    b[i] += w * row[i] * res;
                }
            }
            let Some(xi) = solve6(a, b) else { return x };
            let dr = rotation_of([xi[0], xi[1], xi[2]]);
            x = Similarity { r: mat_mul(&dr, &x.r), scale: x.scale, t: add(mat_vec(&dr, x.t), [xi[3], xi[4], xi[5]]) };
            if len([xi[0], xi[1], xi[2]]) < 1e-7 && len([xi[3], xi[4], xi[5]]) < 1e-6 {
                break;
            }
        }
    }
    x
}

/// Share of `src` moved by `x` that lands within `tol` of `target` (plane distance, partner
/// within twice that), normals agreeing.
pub fn inlier_share(src: &[Oriented], target: &dyn Target, x: &Similarity, tol: f64) -> f64 {
    let n = src
        .par_iter()
        .map_init(Vec::new, |buf, o| {
            let p = x.apply(o.p);
            let n = mat_vec(&x.r, o.n);
            target.partner(p, n, 2.0 * tol, buf).is_some_and(|(q, m)| dot(sub(p, q), m).abs() <= tol)
        })
        .filter(|&b| b)
        .count();
    n as f64 / src.len().max(1) as f64
}

/// Coarse ICP stages (onto the supported file's dense samples) after voting, and fine ones (onto
/// its triangles).
pub const COARSE: [Stage; 3] = [
    Stage { radius: 4.0, sigma: 1.0, iters: 8 },
    Stage { radius: 1.5, sigma: 0.4, iters: 8 },
    Stage { radius: 0.6, sigma: 0.15, iters: 6 },
];
pub const FINE: [Stage; 2] =
    [Stage { radius: 0.4, sigma: 0.08, iters: 8 }, Stage { radius: 0.15, sigma: 0.02, iters: 8 }];

/// A supported file prepared for verifying candidates: its mesh, surface index, dense samples
/// above the raft, and descriptor.
pub struct Supported {
    pub mesh: Mesh,
    pub surface: Surface,
    /// Area-weighted samples of the whole surface, with their triangles: those above the raft
    /// are the alignment's scene and cloud; all count area for the thick support guard.
    samples: Vec<(P, P, u32)>,
    raft_y: f64,
    pub cloud: Cloud,
    pub area: f64,
    pub descriptor: Descriptor,
}

impl Supported {
    /// Prepares loaded (Y-up) triangles.
    pub fn new(triangles: &[Triangle]) -> Self {
        Self::from_mesh(Mesh::new(triangles))
    }

    /// Prepares a supported file's mesh (as [`Mesh::new`] makes it of its loaded triangles).
    pub fn from_mesh(mesh: Mesh) -> Self {
        let descriptor = Descriptor::of(&mesh.tris, true);
        Self::with_descriptor(mesh, descriptor)
    }

    /// [`Supported::from_mesh`] with its descriptor made already.
    pub fn with_descriptor(mesh: Mesh, descriptor: Descriptor) -> Self {
        let surface = Surface::new(&mesh.tris, cell_for(&mesh.tris, 0.25));
        let area: f64 = mesh.tris.iter().map(tri_area).sum();
        let ymin = mesh.tris.iter().flatten().map(|p| p[1]).fold(f64::MAX, f64::min);
        let raft_y = ymin + RAFT_CUT_MM;
        let samples = sample(&mesh.tris, CLOUD_SAMPLES, 23);
        let above: Vec<Oriented> =
            samples.iter().filter(|s| s.0[1] >= raft_y).map(|s| Oriented { p: s.0, n: s.1 }).collect();
        let cloud = Cloud::new(above, 0.5);
        Supported { mesh, surface, samples, raft_y, cloud, area, descriptor }
    }

    pub fn target(&self) -> SurfaceTarget<'_> {
        SurfaceTarget { tris: &self.mesh.tris, surface: &self.surface }
    }

    /// The scene for pair feature voting: samples above the raft, one per `step` voxel.
    pub fn scene(&self, step: f64) -> Vec<Oriented> {
        let above: Vec<(P, P, u32)> = self.samples.iter().filter(|s| s.0[1] >= self.raft_y).copied().collect();
        voxel_downsample(&above, step)
    }
}

/// Distance quantiles (mm): 50th, 90th and 99th percentile.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Deviation {
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
}

/// Why a candidate was refused, or that it passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No pose put most of it on the supported file.
    Unaligned,
    /// Aligned, but too far off the supported file ([`MAX_P90_DEVIATION_MM`]).
    Deviates,
    /// The supported file covers too little of it, or leaves a large part uncovered.
    Uncovered,
    /// A large connected patch of it lies off the supported file ([`MAX_PATCH_MM2`]).
    Patch,
    /// What is left does not look like supports ([`twin::support_like`]).
    NotSupports,
    Twin,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Unaligned => "unaligned",
            Verdict::Deviates => "deviates",
            Verdict::Uncovered => "uncovered",
            Verdict::Patch => "patch",
            Verdict::NotSupports => "not_supports",
            Verdict::Twin => "twin",
        }
    }
}

/// What verifying one candidate measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub verdict: Verdict,
    /// Takes the twin onto the supported file.
    pub transform: Option<Similarity>,
    /// Pose hypotheses voted, and the best one's inlier share after refinement.
    pub poses: usize,
    pub inliers: f64,
    pub deviation: Deviation,
    pub coverage: f64,
    pub largest_gap: f64,
    /// Largest connected patch of the twin off the supported file (mm^2).
    pub patch_mm2: f64,
    /// Thick area of the supported file left as support, above the raft (mm^2), and as a share of
    /// the twin's area.
    pub thick_support_mm2: f64,
    pub thick_support_share: f64,
    /// The thick area left as support is large ([`THICK_SUPPORT_FLAG_SHARE`]): recorded, not refused.
    pub thick_flagged: bool,
    /// Area of the supported file labelled model (mm^2).
    pub explained_mm2: f64,
    pub twin_area: f64,
    pub shape: twin::SupportShape,
    pub align_ms: f64,
    pub label_ms: f64,
}

impl Check {
    fn new(verdict: Verdict) -> Self {
        Check {
            verdict,
            transform: None,
            poses: 0,
            inliers: 0.0,
            deviation: Deviation::default(),
            coverage: 0.0,
            largest_gap: 0.0,
            patch_mm2: 0.0,
            thick_support_mm2: 0.0,
            thick_support_share: 0.0,
            thick_flagged: false,
            explained_mm2: 0.0,
            twin_area: 0.0,
            shape: twin::SupportShape::default(),
            align_ms: 0.0,
            label_ms: 0.0,
        }
    }
}

fn ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Share of the twin within 0.05 mm of the supported file's surface after refinement below which
/// the pose is not the twin's (the twin lies on the supported file).
pub const MIN_POSE_INLIERS: f64 = 0.5;

/// Aligns twin `u` onto supported `s`: pair feature poses, each refined coarsely and scored, the
/// best refined finely. Returns the transform, poses voted and its inlier share.
pub fn align(s: &Supported, u: &Mesh) -> Option<(Similarity, usize, f64)> {
    let model = PpfModel::new(&u.tris);
    let scene = s.scene(model.step);
    let poses = ppf_poses(&model, &scene);
    align_from(s, u, &poses.iter().map(|p| p.transform).collect::<Vec<_>>())
}

/// [`align`] from given pose hypotheses.
pub fn align_from(s: &Supported, u: &Mesh, poses: &[Similarity]) -> Option<(Similarity, usize, f64)> {
    let src: Vec<Oriented> = sample(&u.tris, ICP_POINTS, 29).into_iter().map(|x| Oriented { p: x.0, n: x.1 }).collect();
    let few: Vec<Oriented> = src.iter().step_by(ICP_POINTS / COARSE_POINTS).copied().collect();
    // Every pose refined coarsely on a few points and ranked on the dense samples (loosely: their
    // spacing on a large model is most of a millimetre); the best few refined on the surface.
    let mut coarse: Vec<(f64, Similarity)> = poses
        .par_iter()
        .map(|&x0| {
            let x = icp(&few, &s.cloud, x0, &COARSE);
            (inlier_share(&few, &s.cloud, &x, 0.5), x)
        })
        .collect();
    coarse.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (x, share) = coarse
        .iter()
        .take(FINE_POSES)
        .map(|&(_, x0)| {
            let x = icp(&src, &s.target(), icp(&few, &s.cloud, x0, &COARSE[1..]), &FINE);
            (x, inlier_share(&src, &s.target(), &x, 0.05))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    Some((x, poses.len(), share))
}

/// Edge-connected groups of triangles with the same flag (by `nb`): each triangle's group, and the
/// groups.
fn groups(nb: &[[u32; 3]], flag: &[bool]) -> Vec<Vec<u32>> {
    let mut of = vec![NONE; nb.len()];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for s in 0..nb.len() {
        if of[s] != NONE {
            continue;
        }
        let g = out.len() as u32;
        of[s] = g;
        let mut members = vec![s as u32];
        stack.push(s as u32);
        while let Some(i) = stack.pop() {
            for &j in &nb[i as usize] {
                if j != NONE && of[j as usize] == NONE && flag[j as usize] == flag[s] {
                    of[j as usize] = g;
                    members.push(j);
                    stack.push(j);
                }
            }
        }
        out.push(members);
    }
    out
}

/// The labels of `s` by twin `u` moved by `x` ([`TOLERANCE_MM`], normals agreeing), support groups
/// wholly near the twin turned to model ([`NOISE_ISLAND_MM`]), and the twin's coverage; with the
/// largest connected patch of the twin off `s` (mm^2).
fn label(s: &Supported, u: &Mesh, x: &Similarity) -> (Labels, f64) {
    let moved: Vec<[P; 3]> = u.tris.par_iter().map(|t| t.map(|p| x.apply(p))).collect();
    let twin_surface = Surface::new(&moved, cell_for(&moved, TOLERANCE_MM));
    let st = &s.mesh.tris;
    let tol = TOLERANCE_MM;
    // Corners are shared: each is tested once.
    let nv = s.mesh.corners.iter().flatten().max().map_or(0, |&x| x as usize + 1);
    let mut at = vec![[0.0; 3]; nv];
    for (t, c) in st.iter().zip(&s.mesh.corners) {
        for k in 0..3 {
            at[c[k] as usize] = t[k];
        }
    }
    let corner_on: Vec<bool> =
        at.par_iter().map_init(Vec::new, |buf, &p| twin_surface.within(&moved, p, None, tol, 0.0, buf)).collect();
    let mut on_twin: Vec<bool> = st
        .par_iter()
        .zip(&s.surface.normals)
        .zip(&s.mesh.corners)
        .map_init(Vec::new, |buf, ((t, &n), c)| {
            c.iter().all(|&v| corner_on[v as usize])
                && twin_surface.within_axis(&moved, centroid(t), reliable_normal(t, n), tol, LABEL_MIN_NORMAL_DOT, buf)
        })
        .collect();
    // Support groups wholly near the twin: model the remeshing moved past the tolerance.
    let near: Vec<bool> = st
        .par_iter()
        .zip(&s.surface.normals)
        .zip(&on_twin)
        .map_init(Vec::new, |buf, ((t, &n), &on)| {
            on || twin_surface.within_axis(
                &moved,
                centroid(t),
                reliable_normal(t, n),
                NOISE_ISLAND_MM,
                LABEL_MIN_NORMAL_DOT,
                buf,
            )
        })
        .collect();
    for g in groups(&s.mesh.nb, &on_twin) {
        if !on_twin[g[0] as usize] && g.iter().all(|&i| near[i as usize]) {
            for &i in &g {
                on_twin[i as usize] = true;
            }
        }
    }
    // The twin's coverage by the model side.
    let model: Vec<[P; 3]> = st.iter().zip(&on_twin).filter(|x| *x.1).map(|x| *x.0).collect();
    let model_surface = Surface::new(&model, cell_for(&model, tol));
    let covered: Vec<f64> = moved
        .par_iter()
        .map_init(Vec::new, |buf, t| {
            if model_surface.within(&model, centroid(t), None, tol, 0.0, buf) {
                return 1.0;
            }
            let at = |w: [f64; 3]| add(add(mul(t[0], w[0]), mul(t[1], w[1])), mul(t[2], w[2]));
            let (a, b) = (2.0 / 3.0, 1.0 / 6.0);
            let points = [[a, b, b], [b, a, b], [b, b, a], [0.5, 0.5, 0.0], [0.0, 0.5, 0.5], [0.5, 0.0, 0.5]];
            points.iter().filter(|&&w| model_surface.within(&model, at(w), None, tol, 0.0, buf)).count() as f64 / 7.0
        })
        .collect();
    let areas: Vec<f64> = moved.iter().map(tri_area).collect();
    let total: f64 = areas.iter().sum::<f64>().max(1e-30);
    let uncovered: Vec<bool> = covered.iter().map(|&c| c < 0.5).collect();
    let gap: f64 = areas.iter().zip(&covered).map(|(a, c)| a * (1.0 - c)).sum();
    let largest = groups(&u.nb, &uncovered)
        .iter()
        .filter(|g| uncovered[g[0] as usize])
        .map(|g| g.iter().map(|&i| areas[i as usize]).sum::<f64>())
        .fold(0.0, f64::max);
    // Patches of the twin off the supported file's whole surface (model or support).
    let off: Vec<bool> = moved
        .par_iter()
        .map_init(Vec::new, |buf, t| {
            // Growing reach: a query box of a millimetre on a dense mesh holds thousands of faces.
            let c = centroid(t);
            ![TOLERANCE_MM, 0.3, PATCH_DEVIATION_MM].iter().any(|&r| s.surface.within(st, c, None, r, 0.0, buf))
        })
        .collect();
    let patch = groups(&u.nb, &off)
        .iter()
        .filter(|g| off[g[0] as usize])
        .map(|g| g.iter().map(|&i| areas[i as usize]).sum::<f64>())
        .fold(0.0, f64::max);
    let labels = Labels {
        on_twin,
        twin_coverage: 1.0 - gap / total,
        largest_gap: largest / total,
        twin_uncovered: uncovered,
        twin: moved,
    };
    (labels, patch)
}

/// The twin's distance quantiles onto the supported file (normals agreeing), capped at
/// [`DEVIATION_CAP_MM`].
fn deviation(s: &Supported, u: &Mesh, x: &Similarity) -> Deviation {
    let mut d: Vec<f64> = sample(&u.tris, DEVIATION_SAMPLES, 31)
        .par_iter()
        .map_init(Vec::new, |buf, &(p, n, _)| {
            let (p, n) = (x.apply(p), mat_vec(&x.r, n));
            s.surface
                .nearest(&s.mesh.tris, p, Some(n), DEVIATION_CAP_MM, ICP_MIN_NORMAL_DOT, buf)
                .map_or(DEVIATION_CAP_MM, |r| r.0)
        })
        .collect();
    if d.is_empty() {
        return Deviation { p50: DEVIATION_CAP_MM, p90: DEVIATION_CAP_MM, p99: DEVIATION_CAP_MM };
    }
    d.sort_by(f64::total_cmp);
    let q = |f: f64| d[((d.len() as f64 * f) as usize).min(d.len() - 1)];
    Deviation { p50: q(0.5), p90: q(0.9), p99: q(0.99) }
}

/// Thick area (mm^2) of the supported file on support above the raft, by its samples.
pub fn thick_support_area(s: &Supported, is_support: &[bool]) -> f64 {
    let on: Vec<(P, P, u32)> =
        s.samples.iter().filter(|x| is_support[x.2 as usize] && x.0[1] >= s.raft_y).copied().collect();
    let thick = thick_of(&s.mesh.tris, &on).iter().filter(|&&b| b).count();
    s.area * thick as f64 / s.samples.len().max(1) as f64
}

/// Verifies twin `u` against supported `s`: aligns, measures the deviation, labels and guards.
/// The split is given for a candidate that passed the deviation guard.
pub fn verify(s: &Supported, u: &Mesh) -> (Check, Option<TwinSplit>) {
    let t = std::time::Instant::now();
    let aligned = align(s, u);
    verify_aligned(s, u, aligned, ms(t))
}

/// [`verify`] after alignment (`aligned` as [`align`] gives it, taking `align_ms`).
pub fn verify_aligned(
    s: &Supported,
    u: &Mesh,
    aligned: Option<(Similarity, usize, f64)>,
    align_ms: f64,
) -> (Check, Option<TwinSplit>) {
    let mut check = Check::new(Verdict::Unaligned);
    check.align_ms = align_ms;
    let Some((x, poses, inliers)) = aligned else { return (check, None) };
    check.transform = Some(x);
    check.poses = poses;
    check.inliers = inliers;
    check.twin_area = u.tris.iter().map(tri_area).sum();
    if inliers < MIN_POSE_INLIERS {
        return (check, None);
    }
    let t = std::time::Instant::now();
    check.deviation = deviation(s, u, &x);
    if check.deviation.p90 > MAX_P90_DEVIATION_MM {
        check.verdict = Verdict::Deviates;
        check.label_ms = ms(t);
        return (check, None);
    }
    let (labels, patch) = label(s, u, &x);
    check.coverage = labels.twin_coverage;
    check.largest_gap = labels.largest_gap;
    check.patch_mm2 = patch;
    let reg = Registration { transform: x, matches: 0, inliers: 0 };
    let split = twin::combine(&s.mesh, &[(0, reg, labels.twin_coverage, labels)]);
    check.label_ms = ms(t);
    let Some(split) = split else { return (check, None) };
    check.shape = split.shape;
    check.explained_mm2 =
        s.mesh.tris.iter().zip(&split.is_support).filter(|x| !*x.1).map(|x| tri_area(x.0)).sum::<f64>();
    check.thick_support_mm2 = thick_support_area(s, &split.is_support);
    check.thick_support_share = check.thick_support_mm2 / check.twin_area.max(1e-9);
    check.thick_flagged = check.thick_support_share > THICK_SUPPORT_FLAG_SHARE;
    check.label_ms = ms(t);
    check.verdict = if check.coverage < twin::MIN_TWIN_COVERAGE || check.largest_gap > twin::MAX_TWIN_GAP {
        Verdict::Uncovered
    } else if check.patch_mm2 > MAX_PATCH_MM2.max(MAX_PATCH_SHARE * check.twin_area) {
        Verdict::Patch
    } else if !(split.supports && split.is_support.contains(&true)) {
        Verdict::NotSupports
    } else {
        Verdict::Twin
    };
    (check, Some(split))
}

/// One candidate verified.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub ranked: Ranked,
    pub check: Check,
}

/// A remeshed twin lookup of one supported file.
#[derive(Debug, Clone)]
pub struct Lookup {
    /// Candidates scored, best first (plausible ones first).
    pub ranked: Vec<Ranked>,
    pub checked: Vec<Checked>,
    /// The candidate chosen (its index in `checked`), and its split.
    pub chosen: Option<usize>,
    pub split: Option<TwinSplit>,
}

impl Lookup {
    /// The chosen candidate's check.
    pub fn chosen_check(&self) -> Option<&Checked> {
        self.chosen.map(|i| &self.checked[i])
    }
}

/// Looks up supported `s` among candidate descriptors: verifies up to [`MAX_CANDIDATES`]
/// plausible ones in rank order, each loaded by `load` (index in `candidates`; None when it
/// cannot be), and keeps the passing one that explains the most of the supported file's area
/// (then the lowest 90th percentile deviation).
pub fn lookup(
    s: &Supported,
    candidates: &[Descriptor],
    mut load: impl FnMut(usize) -> Option<Vec<Triangle>>,
) -> Lookup {
    let ranked = rank(&s.descriptor, candidates);
    let mut checked: Vec<Checked> = Vec::new();
    let mut best: Option<(usize, TwinSplit)> = None;
    for r in ranked.iter().filter(|r| r.plausible()).take(MAX_CANDIDATES) {
        if best.as_ref().is_some_and(|(i, _)| r.similarity < checked[*i].ranked.similarity - NEAR_VARIANT_MARGIN) {
            break;
        }
        let Some(t) = load(r.index) else { continue };
        let u = Mesh::new(&t);
        let (check, split) = verify(s, &u);
        if check.verdict == Verdict::Twin
            && let Some(split) = split
        {
            let better = best.as_ref().is_none_or(|(i, _)| {
                let b: &Check = &checked[*i].check;
                check.explained_mm2 > b.explained_mm2 * 1.001
                    || (check.explained_mm2 >= b.explained_mm2 * 0.999 && check.deviation.p90 < b.deviation.p90)
            });
            if better {
                best = Some((checked.len(), split));
            }
        }
        checked.push(Checked { ranked: *r, check });
    }
    let (chosen, split) = match best {
        Some((i, s)) => (Some(i), Some(s)),
        None => (None, None),
    };
    Lookup { ranked, checked, chosen, split }
}

/// [`lookup`] of a supported file's mesh, prepared only when a candidate is plausible: most
/// supported files have none, and their lookup costs a descriptor.
pub fn lookup_mesh(mesh: Mesh, candidates: &[Descriptor], load: impl FnMut(usize) -> Option<Vec<Triangle>>) -> Lookup {
    let descriptor = Descriptor::of(&mesh.tris, true);
    let ranked = rank(&descriptor, candidates);
    if !ranked.iter().any(Ranked::plausible) {
        return Lookup { ranked, checked: Vec::new(), chosen: None, split: None };
    }
    lookup(&Supported::with_descriptor(mesh, descriptor), candidates, load)
}

/// The split of loaded `supported` triangles by remeshed twin `twin`, made again (a lookup's
/// split when its export entry is missing). None when the twin no longer passes.
pub fn split(supported: &[Triangle], twin: &[Triangle]) -> Option<TwinSplit> {
    let s = Supported::new(supported);
    let (check, split) = verify(&s, &Mesh::new(twin));
    split.filter(|_| check.verdict == Verdict::Twin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Vec3;

    /// A smooth closed blob (UV sphere with a smooth irregular radius) at `rings` resolution: the
    /// same surface at any resolution, so a coarser one is its remeshed twin.
    fn blob(rings: usize, bump: f64) -> Vec<[P; 3]> {
        let segs = 2 * rings;
        // Constant at the poles, so they close.
        let radius = |th: f64, ph: f64| {
            10.0 + th.sin()
                * (1.2 * (3.0 * th).sin() * (2.0 * ph).cos()
                    + 0.8 * (5.0 * ph + 1.0).sin()
                    + 0.5 * (4.0 * th + 2.0 * ph).cos())
                + bump * (-((th - 1.2).powi(2) + (ph - 0.5).powi(2)) / 0.08).exp()
        };
        let pt = |th: f64, ph: f64| {
            let r = radius(th, ph);
            [r * th.sin() * ph.cos(), r * th.cos(), r * th.sin() * ph.sin()]
        };
        let pi = std::f64::consts::PI;
        let v = |i: usize, j: usize| pt(pi * i as f64 / rings as f64, 2.0 * pi * (j % segs) as f64 / segs as f64);
        let mut tris = Vec::new();
        for i in 0..rings {
            for j in 0..segs {
                let (a, b, c, d) = (v(i, j), v(i, j + 1), v(i + 1, j), v(i + 1, j + 1));
                if i > 0 {
                    tris.push([a, b, c]);
                }
                if i + 1 < rings {
                    tris.push([b, d, c]);
                }
            }
        }
        tris
    }

    fn to_tris(t: &[[P; 3]]) -> Vec<Triangle> {
        let v = |p: P| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
        t.iter().map(|t| Triangle::new(v(t[0]), v(t[1]), v(t[2]), Vec3::UP)).collect()
    }

    fn moved(t: &[[P; 3]], x: &Similarity) -> Vec<[P; 3]> {
        t.iter().map(|t| t.map(|p| x.apply(p))).collect()
    }

    /// `body` with supports welded in as a boolean union leaves them: some of its lowest
    /// triangles replaced by thin downward spikes (Y-up), and a raft plate under them. Returns the
    /// triangles and how many are body.
    fn supported(body: &[[P; 3]]) -> (Vec<[P; 3]>, usize) {
        let mut order: Vec<usize> = (0..body.len()).collect();
        order.sort_by(|&a, &b| centroid(&body[a])[1].total_cmp(&centroid(&body[b])[1]));
        let picked: Vec<usize> = order.iter().take(order.len() / 8).copied().step_by(order.len() / 80).collect();
        let mut s: Vec<[P; 3]> =
            body.iter().enumerate().filter(|(i, _)| !picked.contains(i)).map(|(_, t)| *t).collect();
        let n = s.len();
        let ymin = body.iter().flatten().map(|p| p[1]).fold(f64::MAX, f64::min);
        for &i in &picked {
            let t = body[i];
            let c = centroid(&t);
            let apex = [c[0], ymin - 8.0, c[2]];
            s.extend([[t[0], t[1], apex], [t[1], t[2], apex], [t[2], t[0], apex]]);
        }
        (s, n)
    }

    fn rot(a: f64, b: f64) -> M3 {
        mat_mul(&axis_angle([0.0, 1.0, 0.0], a), &axis_angle([1.0, 0.0, 0.0], b))
    }

    #[test]
    fn descriptor_round_trips_and_contains_the_twin() {
        let fine = blob(60, 0.0);
        let (s, _) = supported(&fine);
        let ds = Descriptor::of_triangles(&to_tris(&s), true);
        let du = Descriptor::of_triangles(&to_tris(&blob(36, 0.0)), false);
        assert_eq!(Descriptor::from_bytes(&ds.to_bytes()), Some(ds.clone()));
        let mut bad = ds.to_bytes();
        bad[0] = bad[0].wrapping_add(1);
        assert_eq!(Descriptor::from_bytes(&bad), None);
        assert!(du.thick_area > 1000.0, "{du:?}");
        // The twin is contained; a model twice the size and a small part are not plausible.
        let big: Vec<[P; 3]> = blob(36, 0.0).iter().map(|t| t.map(|p| mul(p, 2.0))).collect();
        let small: Vec<[P; 3]> = blob(36, 0.0).iter().map(|t| t.map(|p| mul(p, 0.4))).collect();
        let cands = [
            Descriptor::of_triangles(&to_tris(&big), false),
            du.clone(),
            Descriptor::of_triangles(&to_tris(&small), false),
        ];
        let r = rank(&ds, &cands);
        assert_eq!(r[0].index, 1, "{r:?}");
        assert!(r[0].plausible() && r[0].containment > 0.85, "{r:?}");
        assert!(!r[1].plausible() && !r[2].plausible(), "{r:?}");
    }

    #[test]
    fn splits_supports_by_a_remeshed_twin() {
        let fine = blob(72, 0.0);
        let x = Similarity { r: rot(0.8, 0.5), scale: 1.0, t: [3.0, 25.0, -2.0] };
        let (s, body_n) = supported(&moved(&fine, &x));
        let twin = blob(48, 0.0);
        let st = to_tris(&s);
        let prepared = Supported::new(&st);
        let (check, split) = verify(&prepared, &Mesh::new(&to_tris(&twin)));
        assert_eq!(check.verdict, Verdict::Twin, "{check:?}");
        let found = check.transform.unwrap();
        assert!(rotation_angle(&found.r, &x.r) < 0.2f64.to_radians(), "{found:?}");
        assert!(dist(found.t, x.t) < 0.05, "{found:?}");
        assert!(check.deviation.p90 < MAX_P90_DEVIATION_MM, "{check:?}");
        let split = split.unwrap();
        let wrong = split.is_support[..body_n].iter().filter(|s| **s).count();
        assert!(wrong * 1000 < body_n, "{wrong} body triangles cut");
        assert!(split.is_support[body_n..].iter().all(|s| *s));
        // Made again, the same split.
        assert_eq!(super::split(&st, &to_tris(&twin)).map(|x| x.is_support), Some(split.is_support));
    }

    #[test]
    fn refuses_a_twin_with_another_part() {
        // The twin has a bump the supported file lacks (another weapon): a patch off it.
        let (s, _) = supported(&blob(72, 0.0));
        let prepared = Supported::new(&to_tris(&s));
        let (check, _) = verify(&prepared, &Mesh::new(&to_tris(&blob(48, 3.0))));
        assert_ne!(check.verdict, Verdict::Twin, "{check:?}");
        // The supported file has the bump and the twin does not: refused too.
        let (s, _) = supported(&blob(72, 3.0));
        let prepared = Supported::new(&to_tris(&s));
        let (check, _) = verify(&prepared, &Mesh::new(&to_tris(&blob(48, 0.0))));
        assert_ne!(check.verdict, Verdict::Twin, "{check:?}");
    }

    #[test]
    fn lookup_picks_the_twin_among_candidates() {
        let (s, _) = supported(&blob(72, 0.0));
        let prepared = Supported::new(&to_tris(&s));
        let twin = to_tris(&blob(48, 0.0));
        let other = to_tris(&blob(48, 3.0));
        let cands = [Descriptor::of_triangles(&other, false), Descriptor::of_triangles(&twin, false)];
        let files = [other, twin];
        let mut loaded = Vec::new();
        let l = lookup(&prepared, &cands, |i| {
            loaded.push(i);
            Some(files[i].clone())
        });
        let chosen = l.chosen_check().expect("a twin");
        assert_eq!(chosen.ranked.index, 1, "{:?}", l.checked);
        assert!(loaded.len() <= MAX_CANDIDATES);
        assert!(l.split.is_some());
    }

    #[test]
    fn unrelated_model_is_refused() {
        let (s, _) = supported(&blob(72, 0.0));
        let prepared = Supported::new(&to_tris(&s));
        let other: Vec<[P; 3]> = blob(48, 0.0).iter().map(|t| t.map(|p| [p[0] * 1.3, p[1] * 0.8, p[2]])).collect();
        let (check, split) = verify(&prepared, &Mesh::new(&to_tris(&other)));
        assert_ne!(check.verdict, Verdict::Twin, "{check:?}");
        assert!(split.is_none() || check.verdict != Verdict::Twin);
    }

    #[test]
    fn solve6_inverts_a_system() {
        let mut a = [[0.0; 6]; 6];
        for (i, row) in a.iter_mut().enumerate() {
            row[i] = 2.0 + i as f64;
            row[(i + 1) % 6] = 0.5;
        }
        let x = [1.0, -2.0, 3.0, 0.5, 0.0, 4.0];
        let b: [f64; 6] = std::array::from_fn(|i| (0..6).map(|j| a[i][j] * x[j]).sum());
        let got = solve6(a, b).unwrap();
        assert!(got.iter().zip(&x).all(|(a, b)| (a - b).abs() < 1e-9), "{got:?}");
    }
}
