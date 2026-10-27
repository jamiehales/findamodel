//! Separates a pre-supported mesh into model body and support components.
//!
//! Triangles are grouped into connected components (union-find on shared quantised
//! edges), so supports that are separate shells or touch the body only at break-away tips
//! form components of their own. Each component is then labelled from its height,
//! triangle count and volume (Y is up and the base sits at Y=0):
//!
//! 1. A closed component with negative volume is wound inside out: the inner shell of a
//!    hollowed model, or when smaller than [`INNER_SHELL_MAX_VOLUME_MM3`] a void inside the
//!    model. A component with next to no surface ([`SLIVER_MAX_AREA_MM2`]) is a sliver of
//!    degenerate triangles. Voids and slivers are scraps of the model: model geometry, but
//!    not counted as parts in the steps below.
//! 2. A component that stays below the raft height is raft.
//! 3. The seed is the largest component above the raft. Components with at least
//!    1/[`MODEL_SEED_DIVISOR`] of its triangles are model parts.
//! 4. Smaller components with at least [`MODEL_MIN_TRIANGLES`] triangles and at least
//!    1/[`MODEL_DENSITY_DIVISOR`] of the seed's triangles per mm² of surface are sculpted
//!    model details (kitbashed parts, buttons, pouches). Support primitives keep a fixed
//!    segment count whatever their size, so they stay far sparser than the sculpt. Dense
//!    joint balls and ball-and-strut shells are excluded by their shape: generated
//!    primitives repeat the same triangle around every ring (at least
//!    [`PRIMITIVE_MIN_REPEATED_FRACTION`] of them are repeats), while sculpted triangles
//!    are nearly all distinct.
//! 5. Smaller components reaching within [`TOP_MODEL_BAND_MM`] of the model's top are
//!    model details; the rest are supports.
//! 6. A support component wholly above the raft that touches no other support part must hold
//!    the model up on its own. One that is sculpted (next to no repeated triangles) and rests
//!    on an upward-facing model surface without meeting a downward-facing one at its top is a
//!    piece of the sculpt lying apart from the model (a pebble on the ground): a loose piece,
//!    kept with the model like a scrap.
//!
//! The split is confident only when support components average under a third of the
//! triangles of model components and add up to at least [`MIN_SUPPORT_TRIANGLES`]. Support
//! posts and tips are far lower-poly than sculpted parts, so this keeps multi-part models
//! without supports from being split. The approach follows DragonFruit's model/support
//! classifier.
//!
//! A model without supports made of separate low-poly parts (hard-surface kits: panels,
//! pipes, rivets) can still have parts labelled as supports. Supports are thin rods, cones
//! and balls, so when thicker closed parts make up more than [`MAX_THICK_SUPPORT_SHARE`] of
//! the support surface above the raft, nothing is classified. Thin parts that are not
//! supports are caught by [`Classification::holds_supports`]: the supports rebuilt from them
//! account for little of their surface, or do not hold the model up (pegs and spikes stick
//! out of side faces, pins sit inside the model). The rebuild is the costly part of
//! detection, so it is skipped for supports that are clearly generated: many components made
//! almost entirely of repeated triangles.
//!
//! The constants' notes on styles and data read as in [`super::support_layout`]; here the guard
//! sets (108 real models, 219 Engin3d models without supports) weigh most.

use rustc_hash::FxHashMap;

use super::support_layout::closest_on_triangle;
use super::{MIN_FACE_TILT, Triangle, Vec3, support_layout, to_z_up};

/// Increment when the classification changes so stored detections are recomputed.
pub const CURRENT_SUPPORT_DETECTION_VERSION: i32 = 5;

/// Components with at least 1/N of the seed's triangles are model parts. Styles: all, and the
/// unsupported group (multi-part kits). Sweep: fragile: 6 or 11 each change one corpus model's
/// split (Mezgike's ec811c80 recall at 0.3 mm down 0.06 at 6, Adament Arsenal's 64795f66 up 0.04 at
/// 11) and which Engin3d fakes are classified (none kept).
const MODEL_SEED_DIVISOR: u32 = 8;

/// Floor for the model-part triangle count, so a low-poly seed does not promote supports. Styles:
/// the unsupported group. Sweep: fragile: 150 keeps two Engin3d fakes (3a94eb64, b2747b25); 267
/// only leaves fakes unclassified.
const MODEL_MIN_TRIANGLES: u32 = 200;

/// Components with at least 1/N of the seed's surface triangle density are model details. Styles:
/// all, and the unsupported group. Sweep: fragile: 3 changes Mezgike's ec811c80 split (recall at
/// 0.3 mm down 0.04, precision 0.05) and 3 or 5.33 leave a rejected model unclassified.
const MODEL_DENSITY_DIVISOR: f64 = 4.0;

/// Components with at least this fraction of repeated triangles are generated primitives (joint
/// balls and ball-and-strut shells, which are dense like a sculpt). Styles: all. Sweep: no effect
/// on the round trip; 0.67 changes which fakes are classified (none kept).
const PRIMITIVE_MIN_REPEATED_FRACTION: f64 = 0.5;

/// Fewest congruent copies for a triangle to count as repeated. Styles: all. Sweep: no effect on
/// the round trip; 5 changes which fakes are classified (none kept).
const REPEATED_MIN_COPIES: u32 = 4;

/// Edge lengths within this many mm are treated as equal. A numeric tolerance.
const EDGE_GRID_SIZE_MM: f64 = 0.0001;

/// Small components reaching this close to the model's top are model details. Styles: all. Sweep:
/// no effect on the round trip; 25% either way changes which fakes are classified (none kept).
const TOP_MODEL_BAND_MM: f32 = 1.0;

/// Support components must average this many times fewer triangles than model components.
/// Confidence threshold: no case in the corpus or the fakes comes near it; it keeps multi-part
/// models without supports from being split.
const SUPPORT_DENSITY_RATIO: u64 = 3;

/// Fewest support triangles for a confident split. Confidence threshold: no case in the corpus or
/// the fakes comes near it.
const MIN_SUPPORT_TRIANGLES: u64 = 2_000;

/// Closed components with a signed volume below this (mm³) are wound inside out. A flat shell's
/// volume can come out a hair either side of zero. A numeric tolerance.
const INSIDE_OUT_MAX_VOLUME_MM3: f64 = -1e-6;

/// Inside-out components with a signed volume below this (mm³) are inner shells; the rest are voids
/// (scraps left inside the model by hollowing or repair tools). Styles: hollowed models
/// (hollow-struts). Safety limit: taking every inside-out shell for an inner shell changes nothing
/// in the sets (16 corpus models have voids), but would count a repaired mesh's many small voids as
/// model parts, shrinking the average model part the confidence check weighs supports against.
const INNER_SHELL_MAX_VOLUME_MM3: f64 = -0.01;

/// Safety limit: components with less surface than this (mm²) are slivers of degenerate triangles.
const SLIVER_MAX_AREA_MM2: f64 = 0.01;

/// Closed support components thicker than this (mean thickness 2V/A: a rod's radius, a slab's
/// thickness) are thicker than a support's shafts and joints. Styles: all, and the unsupported
/// group (hard-surface kits). Sweep: fragile: 0.75 leaves real models unclassified (fa77c7c4 and
/// 03ceba5d stop exporting supports); 1.33 only changes which fakes are classified.
const MAX_SUPPORT_THICKNESS_MM: f64 = 1.0;

/// Supports have at most this share of their surface above the raft in thick components.
/// Pre-supported models have next to none; on hard-surface kits without supports, the parts classed
/// as supports are often mostly thick. Styles: the unsupported group. Sweep: no effect on real
/// models; 25% either way changes which fakes are classified (none kept).
const MAX_THICK_SUPPORT_SHARE: f64 = 0.25;

/// Support parts with at least this many components, at least this fraction of whose triangles are
/// repeated (see [`REPEATED_MIN_COPIES`]), are generated supports. Most pre-supported models
/// qualify; hard-surface kits with that many parts have at most 81% of their triangles repeated.
/// Styles: all. Sweep: no effect (each).
const GENERATED_MIN_COMPONENTS: u64 = 100;
/// See [`GENERATED_MIN_COMPONENTS`].
const GENERATED_MIN_REPEATED_FRACTION: f64 = 0.9;

/// Support components whose boxes are this far apart or more touch no other support part. Styles:
/// hollow-struts (a pebble on Death x Tiles' 0e4bfe5b). Sweep: no effect: the margins of the
/// loose-piece rule are wide (each of its three values).
const LOOSE_MARGIN_MM: f32 = 0.05;

/// Lone support components with less than this fraction of repeated triangles are sculpted. Even a
/// distorted support primitive keeps a third or more (tip cones of 28 triangles at 43%), while
/// pieces of a sculpt have none. See [`LOOSE_MARGIN_MM`].
const LOOSE_MAX_REPEATED_FRACTION: f64 = 0.1;

/// How far from a lone support component's top and bottom the model surface it meets is looked for.
/// See [`LOOSE_MARGIN_MM`].
const LOOSE_CONTACT_MM: f32 = 1.0;

/// Vertex quantisation grid size in mm. Vertices within this distance are treated as coincident,
/// which handles floating-point rounding at shared edges. A numeric tolerance.
const VERTEX_GRID_SIZE: f32 = 0.01;

type QuantizedVertex = (i32, i32, i32);

#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    /// Per-triangle support flag.
    pub is_support: Vec<bool>,
    /// The split looks like a model plus support scaffolding rather than a multi-part model.
    pub confident: bool,
    /// The support parts are clearly generated supports (see [`GENERATED_MIN_COMPONENTS`]).
    pub generated: bool,
}

impl Classification {
    /// Whether the support parts are supports: generated ones are, and others must rebuild
    /// into supports ([`rebuilds_as_supports`]).
    pub fn holds_supports(&self, triangles: &[Triangle], raft_height_mm: f32) -> bool {
        self.generated || rebuilds_as_supports(triangles, &self.is_support, raft_height_mm)
    }

    /// The support flags to split with, given the model's supported metadata: never when
    /// it is unsupported, whenever supports were found when it is supported, and only for a
    /// confident split when unknown.
    pub fn mask_for(self, supported: Option<bool>) -> Option<Vec<bool>> {
        match supported {
            Some(false) => None,
            Some(true) => Some(self.is_support),
            None => self.confident.then_some(self.is_support),
        }
    }
}

/// Partitions the triangles into model body and support geometry, following
/// [`Classification::mask_for`]. `supports` is `None` when nothing is split off.
pub fn separate(
    triangles: &[Triangle],
    raft_height_mm: f32,
    supported: Option<bool>,
) -> (Vec<Triangle>, Option<Vec<Triangle>>) {
    match classify(triangles, raft_height_mm).and_then(|c| c.mask_for(supported)) {
        Some(mask) => {
            let (model, supports) = split(triangles, &mask);
            (model, Some(supports))
        }
        None => (triangles.to_vec(), None),
    }
}

/// Splits triangles by a per-triangle support flag into (model, supports).
pub fn split(triangles: &[Triangle], is_support: &[bool]) -> (Vec<Triangle>, Vec<Triangle>) {
    let support_count = is_support.iter().filter(|s| **s).count();
    let mut model = Vec::with_capacity(triangles.len() - support_count);
    let mut supports = Vec::with_capacity(support_count);
    for (tri, support) in triangles.iter().zip(is_support) {
        if *support {
            supports.push(*tri);
        } else {
            model.push(*tri);
        }
    }
    (model, supports)
}

#[derive(Debug, Clone, Copy)]
struct ComponentStats {
    triangles: u32,
    top: f32,
    volume: f64,
    area: f64,
    closed: bool,
    /// Bounding box corners.
    lo: Vec3,
    hi: Vec3,
    /// The highest and lowest vertices.
    peak: Vec3,
    bottom: Vec3,
}

impl ComponentStats {
    fn new(closed: bool) -> Self {
        let (low, high) = (Vec3::new(f32::MAX, f32::MAX, f32::MAX), Vec3::new(f32::MIN, f32::MIN, f32::MIN));
        Self {
            triangles: 0,
            top: f32::NEG_INFINITY,
            volume: 0.0,
            area: 0.0,
            closed,
            lo: low,
            hi: high,
            peak: high,
            bottom: low,
        }
    }

    fn add(&mut self, tri: &Triangle) {
        self.triangles += 1;
        self.volume += signed_volume(tri);
        self.area += f64::from((tri.v1 - tri.v0).cross(tri.v2 - tri.v0).length()) / 2.0;
        for v in [tri.v0, tri.v1, tri.v2] {
            self.lo = Vec3::new(self.lo.x.min(v.x), self.lo.y.min(v.y), self.lo.z.min(v.z));
            self.hi = Vec3::new(self.hi.x.max(v.x), self.hi.y.max(v.y), self.hi.z.max(v.z));
            if v.y > self.peak.y {
                self.peak = v;
            }
            if v.y < self.bottom.y {
                self.bottom = v;
            }
        }
        self.top = self.hi.y;
    }

    /// Triangles per mm² of surface.
    fn density(&self) -> f64 {
        if self.area > 0.0 { f64::from(self.triangles) / self.area } else { 0.0 }
    }

    /// Mean thickness in mm (2V/A); only meaningful for a closed shell.
    fn thickness(&self) -> f64 {
        if self.area > 0.0 { 2.0 * self.volume.abs() / self.area } else { 0.0 }
    }
}

/// Labels each triangle as model or support. `None` when the mesh has no component that
/// classifies as support (or none that classifies as model), or when the support components
/// are too thick to be supports (see [`MAX_THICK_SUPPORT_SHARE`]).
pub fn classify(triangles: &[Triangle], raft_height_mm: f32) -> Option<Classification> {
    let Components { component_of, closed } = components(triangles);
    if closed.len() < 2 {
        return None;
    }

    let mut stats: Vec<ComponentStats> = closed.iter().map(|&closed| ComponentStats::new(closed)).collect();
    for (tri, &c) in triangles.iter().zip(&component_of) {
        stats[c as usize].add(tri);
    }

    // Volume only means something for a closed shell.
    let inside_out = |s: &ComponentStats| s.closed && s.volume < INSIDE_OUT_MAX_VOLUME_MM3;
    let is_inner_shell = |s: &ComponentStats| inside_out(s) && s.volume < INNER_SHELL_MAX_VOLUME_MM3;
    let scrap: Vec<bool> =
        stats.iter().map(|s| s.area < SLIVER_MAX_AREA_MM2 || (inside_out(s) && !is_inner_shell(s))).collect();
    let parts = || stats.iter().enumerate().filter(|(c, _)| !scrap[*c]);

    let (_, seed) = parts().filter(|(_, s)| s.top > raft_height_mm).max_by_key(|(_, s)| s.triangles)?;
    let model_min_triangles = (seed.triangles / MODEL_SEED_DIVISOR).max(MODEL_MIN_TRIANGLES);
    let model_min_density = seed.density() / MODEL_DENSITY_DIVISOR;
    let model_top =
        parts().filter(|(_, s)| s.triangles >= model_min_triangles).map(|(_, s)| s.top).fold(seed.top, f32::max);

    let dense_detail: Vec<bool> = stats
        .iter()
        .zip(&scrap)
        .map(|(s, &scrap)| {
            !scrap
                && !is_inner_shell(s)
                && s.top > raft_height_mm
                && (MODEL_MIN_TRIANGLES..model_min_triangles).contains(&s.triangles)
                && s.density() >= model_min_density
        })
        .collect();
    let primitive = primitives(triangles, &component_of, &dense_detail);

    let mut component_is_support: Vec<bool> = stats
        .iter()
        .enumerate()
        .map(|(c, s)| {
            if scrap[c] || is_inner_shell(s) {
                false
            } else if s.top <= raft_height_mm {
                true
            } else if s.triangles >= model_min_triangles || (dense_detail[c] && !primitive[c]) {
                false
            } else {
                s.top < model_top - TOP_MODEL_BAND_MM
            }
        })
        .collect();
    let loose = loose_pieces(triangles, &component_of, &stats, &component_is_support, raft_height_mm);
    for (support, &loose) in component_is_support.iter_mut().zip(&loose) {
        *support &= !loose;
    }

    let (mut model_components, mut model_triangles, mut support_components, mut support_triangles) =
        (0u64, 0u64, 0u64, 0u64);
    for (c, s) in parts().filter(|(c, _)| !loose[*c]) {
        if component_is_support[c] {
            support_components += 1;
            support_triangles += u64::from(s.triangles);
        } else {
            model_components += 1;
            model_triangles += u64::from(s.triangles);
        }
    }
    if support_components == 0 || model_components == 0 {
        return None;
    }

    let (mut above_raft, mut thick) = (0.0, 0.0);
    for (s, &support) in stats.iter().zip(&component_is_support) {
        if support && s.top > raft_height_mm {
            above_raft += s.area;
            if s.closed && s.thickness() > MAX_SUPPORT_THICKNESS_MM {
                thick += s.area;
            }
        }
    }
    if thick > above_raft * MAX_THICK_SUPPORT_SHARE {
        return None;
    }

    let generated = support_components >= GENERATED_MIN_COMPONENTS && {
        let fractions = repeated_fractions(triangles, &component_of, &component_is_support);
        let repeated: f64 = stats
            .iter()
            .zip(&fractions)
            .zip(&component_is_support)
            .filter(|(_, support)| **support)
            .map(|((s, fraction), _)| fraction * f64::from(s.triangles))
            .sum();
        repeated >= GENERATED_MIN_REPEATED_FRACTION * support_triangles as f64
    };

    // Average support component < average model component / ratio, cross-multiplied.
    let low_poly_supports =
        support_triangles * model_components * SUPPORT_DENSITY_RATIO < model_triangles * support_components;
    let confident = low_poly_supports && support_triangles >= MIN_SUPPORT_TRIANGLES;

    let is_support = component_of.iter().map(|c| component_is_support[*c as usize]).collect();
    Some(Classification { is_support, confident, generated })
}

/// Whether the geometry confidently has supports.
pub fn detect(triangles: &[Triangle], raft_height_mm: f32) -> bool {
    classify(triangles, raft_height_mm).is_some_and(|c| c.confident)
}

/// Whether the triangles flagged in `is_support` rebuild into supports that account for
/// them ([`support_layout::SupportLayout::accounts_for`]). Thin separate parts of a model
/// without supports (rivets, spikes, straps) can classify as supports, but rebuild into next
/// to nothing, or into supports that do not hold the model up.
pub fn rebuilds_as_supports(triangles: &[Triangle], is_support: &[bool], raft_height_mm: f32) -> bool {
    let (model, supports) = split(triangles, is_support);
    let supports = to_z_up(&supports);
    support_layout::reconstruct(&to_z_up(&model), &supports).accounts_for(&supports, raft_height_mm)
}

/// Flags the support components that are loose pieces of the sculpt (see step 6 of the module
/// docs): wholly above the raft, touching no other support part, sculpted (see
/// [`LOOSE_MAX_REPEATED_FRACTION`]), with an upward-facing model surface nearest their
/// lowest point and no downward-facing one nearest their highest (within [`LOOSE_CONTACT_MM`]).
fn loose_pieces(
    triangles: &[Triangle],
    component_of: &[u32],
    stats: &[ComponentStats],
    is_support: &[bool],
    raft_height_mm: f32,
) -> Vec<bool> {
    let mut loose = vec![false; stats.len()];
    let alone = standalone(stats, is_support, raft_height_mm);
    if !alone.contains(&true) {
        return loose;
    }
    let fractions = repeated_fractions(triangles, component_of, &alone);
    let candidates: Vec<usize> =
        (0..stats.len()).filter(|&c| alone[c] && fractions[c] < LOOSE_MAX_REPEATED_FRACTION).collect();
    if candidates.is_empty() {
        return loose;
    }
    let ends: Vec<Vec3> = candidates.iter().flat_map(|&c| [stats[c].peak, stats[c].bottom]).collect();
    let faces = nearest_faces(triangles, |i| !is_support[component_of[i] as usize], &ends);
    for (k, &c) in candidates.iter().enumerate() {
        let holds = faces[2 * k].is_some_and(|n| n.y < -MIN_FACE_TILT);
        let rests = faces[2 * k + 1].is_some_and(|n| n.y > MIN_FACE_TILT);
        loose[c] = rests && !holds;
    }
    loose
}

/// Flags the support components wholly above the raft whose boxes come within
/// [`LOOSE_MARGIN_MM`] of no other support component's (sweep and prune along X).
fn standalone(stats: &[ComponentStats], is_support: &[bool], raft_height_mm: f32) -> Vec<bool> {
    let mut order: Vec<usize> = (0..stats.len()).filter(|&c| is_support[c]).collect();
    order.sort_unstable_by(|&a, &b| stats[a].lo.x.total_cmp(&stats[b].lo.x));
    let mut touched = vec![false; stats.len()];
    let mut active: Vec<usize> = Vec::new();
    for &c in &order {
        let s = &stats[c];
        active.retain(|&a| stats[a].hi.x + LOOSE_MARGIN_MM >= s.lo.x);
        for &a in &active {
            let t = &stats[a];
            if t.lo.y <= s.hi.y + LOOSE_MARGIN_MM
                && s.lo.y <= t.hi.y + LOOSE_MARGIN_MM
                && t.lo.z <= s.hi.z + LOOSE_MARGIN_MM
                && s.lo.z <= t.hi.z + LOOSE_MARGIN_MM
            {
                touched[a] = true;
                touched[c] = true;
            }
        }
        active.push(c);
    }
    (0..stats.len()).map(|c| is_support[c] && !touched[c] && stats[c].bottom.y > raft_height_mm).collect()
}

/// For each point, the outward normal of the nearest triangle within [`LOOSE_CONTACT_MM`]
/// among those `include` accepts. Each triangle is checked against the points in the grid
/// cells within its reach, or against every point when there are fewer points than cells.
fn nearest_faces(triangles: &[Triangle], include: impl Fn(usize) -> bool, points: &[Vec3]) -> Vec<Option<Vec3>> {
    let cell = |p: Vec3| {
        let k = |v: f32| (v / LOOSE_CONTACT_MM).floor() as i32;
        (k(p.x), k(p.y), k(p.z))
    };
    let corner =
        |pick: fn(f32, f32) -> f32, a: Vec3, b: Vec3| Vec3::new(pick(a.x, b.x), pick(a.y, b.y), pick(a.z, b.z));
    let reach = Vec3::new(LOOSE_CONTACT_MM, LOOSE_CONTACT_MM, LOOSE_CONTACT_MM);
    let mut grid: FxHashMap<(i32, i32, i32), Vec<u32>> = FxHashMap::default();
    let (mut lo, mut hi) = (points[0], points[0]);
    for (k, &p) in points.iter().enumerate() {
        grid.entry(cell(p)).or_default().push(k as u32);
        (lo, hi) = (corner(f32::min, lo, p), corner(f32::max, hi, p));
    }
    let (lo, hi) = (lo - reach, hi + reach);

    let mut best: Vec<Option<(f32, u32)>> = vec![None; points.len()];
    for (i, t) in triangles.iter().enumerate() {
        let t_lo = corner(f32::min, corner(f32::min, t.v0, t.v1), t.v2);
        let t_hi = corner(f32::max, corner(f32::max, t.v0, t.v1), t.v2);
        let apart = t_hi.x < lo.x || t_hi.y < lo.y || t_hi.z < lo.z || t_lo.x > hi.x || t_lo.y > hi.y || t_lo.z > hi.z;
        if apart || !include(i) {
            continue;
        }
        let mut check = |k: u32| {
            let p = points[k as usize];
            let d = (closest_on_triangle(p, t.v0, t.v1, t.v2) - p).length();
            if d <= LOOSE_CONTACT_MM && best[k as usize].is_none_or(|(b, _)| d < b) {
                best[k as usize] = Some((d, i as u32));
            }
        };
        let (a, b) = (cell(t_lo - reach), cell(t_hi + reach));
        let span = (b.0 - a.0 + 1) as usize * (b.1 - a.1 + 1) as usize * (b.2 - a.2 + 1) as usize;
        if span < points.len() {
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    for z in a.2..=b.2 {
                        grid.get(&(x, y, z)).into_iter().flatten().for_each(|&k| check(k));
                    }
                }
            }
        } else {
            (0..points.len() as u32).for_each(check);
        }
    }
    best.into_iter()
        .map(|hit| {
            hit.map(|(_, i)| {
                let t = &triangles[i as usize];
                (t.v1 - t.v0).cross(t.v2 - t.v0).normalized()
            })
        })
        .collect()
}

/// Flags the `candidate` components that are generated primitives (see
/// [`PRIMITIVE_MIN_REPEATED_FRACTION`]).
fn primitives(triangles: &[Triangle], component_of: &[u32], candidate: &[bool]) -> Vec<bool> {
    repeated_fractions(triangles, component_of, candidate)
        .into_iter()
        .map(|f| f >= PRIMITIVE_MIN_REPEATED_FRACTION)
        .collect()
}

/// Per candidate component, the fraction of its triangles congruent to at least
/// [`REPEATED_MIN_COPIES`] - 1 others (same edge lengths within [`EDGE_GRID_SIZE_MM`]).
fn repeated_fractions(triangles: &[Triangle], component_of: &[u32], candidate: &[bool]) -> Vec<f64> {
    let edge = |a: Vec3, b: Vec3| (f64::from((a - b).length()) / EDGE_GRID_SIZE_MM).round() as i64;
    let mut shapes: FxHashMap<(u32, [i64; 3]), u32> = FxHashMap::default();
    let mut totals = vec![0u32; candidate.len()];
    for (tri, &c) in triangles.iter().zip(component_of) {
        if candidate[c as usize] {
            let mut edges = [edge(tri.v0, tri.v1), edge(tri.v1, tri.v2), edge(tri.v2, tri.v0)];
            edges.sort_unstable();
            *shapes.entry((c, edges)).or_default() += 1;
            totals[c as usize] += 1;
        }
    }
    let mut repeated = vec![0u32; candidate.len()];
    for ((c, _), copies) in shapes {
        if copies >= REPEATED_MIN_COPIES {
            repeated[c as usize] += copies;
        }
    }
    repeated.iter().zip(&totals).map(|(&r, &t)| if t > 0 { f64::from(r) / f64::from(t) } else { 0.0 }).collect()
}

struct Components {
    /// Dense component id per triangle.
    component_of: Vec<u32>,
    /// Per component: every edge is shared by two or more triangles.
    closed: Vec<bool>,
}

/// Groups triangles into components. Edge-based connectivity keeps point-contact support
/// tips detached from the body.
fn components(triangles: &[Triangle]) -> Components {
    let mut parent: Vec<u32> = (0..triangles.len() as u32).collect();
    // Edge -> (first triangle using it, number of triangles using it).
    let mut edges: FxHashMap<(QuantizedVertex, QuantizedVertex), (u32, u32)> = FxHashMap::default();
    edges.reserve(triangles.len() * 3 / 2);

    for (i, tri) in triangles.iter().enumerate() {
        let i = i as u32;
        let q0 = quantize(tri.v0);
        let q1 = quantize(tri.v1);
        let q2 = quantize(tri.v2);
        for edge in [ordered(q0, q1), ordered(q1, q2), ordered(q2, q0)] {
            let entry = edges.entry(edge).or_insert((i, 0));
            entry.1 += 1;
            let owner = entry.0;
            union(&mut parent, i, owner);
        }
    }

    let mut dense = vec![u32::MAX; triangles.len()];
    let mut count = 0u32;
    let mut component_of = Vec::with_capacity(triangles.len());
    for i in 0..triangles.len() as u32 {
        let root = find(&mut parent, i) as usize;
        if dense[root] == u32::MAX {
            dense[root] = count;
            count += 1;
        }
        component_of.push(dense[root]);
    }

    let mut closed = vec![true; count as usize];
    for (owner, uses) in edges.into_values() {
        if uses < 2 {
            closed[component_of[owner as usize] as usize] = false;
        }
    }
    Components { component_of, closed }
}

/// Signed volume of the tetrahedron from the origin to the triangle; sums to the enclosed
/// volume of a closed shell, positive for outward winding.
fn signed_volume(tri: &Triangle) -> f64 {
    let [a, b, c] = [tri.v0, tri.v1, tri.v2].map(|v| [f64::from(v.x), f64::from(v.y), f64::from(v.z)]);
    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
}

fn quantize(v: Vec3) -> QuantizedVertex {
    (
        (v.x / VERTEX_GRID_SIZE).round_ties_even() as i32,
        (v.y / VERTEX_GRID_SIZE).round_ties_even() as i32,
        (v.z / VERTEX_GRID_SIZE).round_ties_even() as i32,
    )
}

fn ordered(a: QuantizedVertex, b: QuantizedVertex) -> (QuantizedVertex, QuantizedVertex) {
    if a <= b { (a, b) } else { (b, a) }
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize]; // path halving
        x = parent[x as usize];
    }
    x
}

fn union(parent: &mut [u32], a: u32, b: u32) {
    let a = find(parent, a);
    let b = find(parent, b);
    if a != b {
        parent[b as usize] = a;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const RAFT: f32 = 2.0;

    /// Closed axis-aligned box with outward winding, each face split into an n x n grid
    /// (12 n² triangles).
    pub(crate) fn cuboid(min: Vec3, max: Vec3, n: usize) -> Vec<Triangle> {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        // (fixed axis, value on that axis, u axis, v axis) with u x v pointing outward.
        let faces = [
            (1, min.y, 0, 2),
            (1, max.y, 2, 0),
            (0, min.x, 2, 1),
            (0, max.x, 1, 2),
            (2, min.z, 1, 0),
            (2, max.z, 0, 1),
        ];
        let (lo, hi) = ([min.x, min.y, min.z], [max.x, max.y, max.z]);
        let mut out = Vec::with_capacity(12 * n * n);
        for (axis, value, u, v) in faces {
            let point = |i: usize, j: usize| {
                let mut p = [0.0; 3];
                p[axis] = value;
                p[u] = lerp(lo[u], hi[u], i as f32 / n as f32);
                p[v] = lerp(lo[v], hi[v], j as f32 / n as f32);
                Vec3::new(p[0], p[1], p[2])
            };
            for i in 0..n {
                for j in 0..n {
                    out.push(Triangle::new(point(i, j), point(i + 1, j), point(i + 1, j + 1), Vec3::UP));
                    out.push(Triangle::new(point(i, j), point(i + 1, j + 1), point(i, j + 1), Vec3::UP));
                }
            }
        }
        out
    }

    fn inverted(triangles: Vec<Triangle>) -> Vec<Triangle> {
        triangles.into_iter().map(|t| Triangle::new(t.v0, t.v2, t.v1, t.normal)).collect()
    }

    /// 10 mm body (10,800 triangles) raised to 5-15 mm.
    pub(crate) fn body() -> Vec<Triangle> {
        cuboid(Vec3::new(0.0, 5.0, 0.0), Vec3::new(10.0, 15.0, 10.0), 30)
    }

    /// `count` thin posts (48 triangles each) from the bed to just under the body.
    pub(crate) fn posts(count: usize) -> Vec<Triangle> {
        (0..count)
            .flat_map(|k| {
                let (x, z) = (0.37 + (k % 8) as f32 * 1.21, 0.43 + (k / 8) as f32 * 1.17);
                cuboid(Vec3::new(x, 0.0, z), Vec3::new(x + 0.3, 4.9, z + 0.3), 2)
            })
            .collect()
    }

    /// Three thin straps (48 triangles each) along the body's side, like the panels of a
    /// hard-surface kit: they classify as supports but are model parts.
    pub(crate) fn straps() -> Vec<Triangle> {
        [6.0, 9.0, 11.5]
            .into_iter()
            .flat_map(|y| cuboid(Vec3::new(10.0, y, 1.0), Vec3::new(11.0, y + 2.0, 9.0), 2))
            .collect()
    }

    fn support_count(c: &Classification) -> usize {
        c.is_support.iter().filter(|s| **s).count()
    }

    #[test]
    fn empty_and_single_component_meshes_are_not_classified() {
        assert!(classify(&[], RAFT).is_none());
        assert!(classify(&body(), RAFT).is_none());
        let (model, supports) = separate(&body(), RAFT, Some(true));
        assert_eq!(model.len(), 10_800);
        assert!(supports.is_none());
    }

    #[test]
    fn low_poly_posts_under_the_body_are_confident_supports() {
        let mut mesh = body();
        mesh.extend(posts(50));
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.confident);
        assert_eq!(support_count(&c), 50 * 48);
        assert!(c.is_support[10_800..].iter().all(|s| *s));
        assert!(detect(&mesh, RAFT));
    }

    #[test]
    fn raft_below_the_cutoff_is_support_even_when_largest() {
        let mut mesh = body();
        mesh.extend(posts(50));
        mesh.extend(cuboid(Vec3::new(-5.0, 0.0, -5.0), Vec3::new(15.0, 1.0, 15.0), 40));
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.confident);
        assert_eq!(support_count(&c), 50 * 48 + 12 * 40 * 40);
    }

    #[test]
    fn raft_height_decides_whether_a_low_dense_piece_is_raft() {
        let mut mesh = body();
        mesh.extend(posts(50));
        mesh.extend(cuboid(Vec3::new(-5.0, 0.0, -5.0), Vec3::new(15.0, 3.0, 15.0), 30));
        let raft_triangles = 12 * 30 * 30;

        let low_cutoff = classify(&mesh, RAFT).expect("classification");
        assert!(low_cutoff.is_support[mesh.len() - raft_triangles..].iter().all(|s| !*s));

        let high_cutoff = classify(&mesh, 3.5).expect("classification");
        assert!(high_cutoff.is_support[mesh.len() - raft_triangles..].iter().all(|s| *s));
    }

    #[test]
    fn multi_part_model_without_supports_is_not_split() {
        let mut mesh = body();
        for k in 0..4 {
            let x = 12.0 + k as f32 * 4.0;
            mesh.extend(cuboid(Vec3::new(x, 3.0, 0.0), Vec3::new(x + 3.0, 8.0, 3.0), 12));
        }
        assert!(classify(&mesh, RAFT).is_none());
        assert!(!detect(&mesh, RAFT));
    }

    #[test]
    fn few_small_parts_are_found_but_not_confident() {
        let mut mesh = body();
        mesh.extend(posts(10));
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(!c.confident, "480 support triangles is below the minimum");
        assert!(!detect(&mesh, RAFT));

        let (_, unknown) = separate(&mesh, RAFT, None);
        assert!(unknown.is_none());
        let (model, supported) = separate(&mesh, RAFT, Some(true));
        assert_eq!(supported.expect("supports").len(), 480);
        assert_eq!(model.len(), 10_800);
    }

    #[test]
    fn supported_false_never_splits() {
        let mut mesh = body();
        mesh.extend(posts(50));
        let (model, supports) = separate(&mesh, RAFT, Some(false));
        assert_eq!(model.len(), mesh.len());
        assert!(supports.is_none());
    }

    #[test]
    fn similar_density_parts_are_not_confident() {
        // Small parts below the model-part threshold but nearly as dense as the body
        // look like a kit, not supports, even with enough of them to pass the minimum.
        let mut mesh = cuboid(Vec3::new(0.0, 5.0, 0.0), Vec3::new(10.0, 15.0, 10.0), 6);
        for k in 0..12 {
            let x = 12.0 + k as f32 * 2.0;
            mesh.extend(cuboid(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 1.0, 3.0, 1.0), 4));
        }
        let c = classify(&mesh, RAFT).expect("classification");
        assert_eq!(support_count(&c), 12 * 192);
        assert!(!c.confident);
    }

    #[test]
    fn small_detail_near_the_top_is_model() {
        let mut mesh = body();
        mesh.extend(posts(50));
        mesh.extend(cuboid(Vec3::new(20.0, 14.5, 0.0), Vec3::new(21.0, 14.8, 1.0), 1));
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.is_support[mesh.len() - 12..].iter().all(|s| !*s));
        assert_eq!(support_count(&c), 50 * 48);
    }

    /// Moves each vertex by a position-derived offset, so shared vertices stay shared but
    /// no two triangles are congruent, like a sculpt.
    fn sculpted(triangles: Vec<Triangle>, amount: f32) -> Vec<Triangle> {
        let jitter = |v: Vec3| {
            let h = |k: f32| (v.x * 12.9898 + v.y * 78.233 + v.z * 37.719 + k).sin() * amount;
            Vec3::new(v.x + h(0.0), v.y + h(1.0), v.z + h(2.0))
        };
        triangles.into_iter().map(|t| Triangle::new(jitter(t.v0), jitter(t.v1), jitter(t.v2), t.normal)).collect()
    }

    /// UV sphere with `segments` around and `rings` from pole to pole, like a support joint.
    fn sphere(center: Vec3, radius: f32, segments: usize, rings: usize) -> Vec<Triangle> {
        let point = |ring: usize, segment: usize| {
            let (theta, phi) = (
                std::f32::consts::PI * ring as f32 / rings as f32,
                std::f32::consts::TAU * (segment % segments) as f32 / segments as f32,
            );
            Vec3::new(
                center.x + radius * theta.sin() * phi.cos(),
                center.y + radius * theta.cos(),
                center.z + radius * theta.sin() * phi.sin(),
            )
        };
        let mut out = Vec::new();
        for ring in 0..rings {
            for segment in 0..segments {
                let (a, b, c, d) = (
                    point(ring, segment),
                    point(ring, segment + 1),
                    point(ring + 1, segment),
                    point(ring + 1, segment + 1),
                );
                if ring > 0 {
                    out.push(Triangle::new(a, b, c, Vec3::UP));
                }
                if ring + 1 < rings {
                    out.push(Triangle::new(b, d, c, Vec3::UP));
                }
            }
        }
        out
    }

    #[test]
    fn dense_kitbashed_parts_are_model_and_sparse_ones_are_support() {
        let mut mesh = body();
        mesh.extend(posts(50));
        // 1 mm button, 300 triangles over 6 mm² (the body has 18 per mm²).
        let dense = sculpted(cuboid(Vec3::new(20.0, 8.0, 0.0), Vec3::new(21.0, 9.0, 1.0), 5), 0.01);
        // 12 mm bar, 300 triangles over 77 mm²: a merged support structure.
        let sparse = cuboid(Vec3::new(30.0, 0.0, 0.0), Vec3::new(31.5, 12.0, 1.5), 5);
        mesh.extend(&dense);
        mesh.extend(&sparse);
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.confident);
        let sparse_start = mesh.len() - sparse.len();
        assert!(c.is_support[sparse_start - dense.len()..sparse_start].iter().all(|s| !*s));
        assert!(c.is_support[sparse_start..].iter().all(|s| *s));
        assert_eq!(support_count(&c), 50 * 48 + sparse.len());
    }

    #[test]
    fn mirror_symmetric_sculpted_detail_is_model() {
        // A dense 1 mm button sculpted symmetrically: each of its triangles has a mirror image,
        // but a pair is not how a generated primitive repeats its triangles around every ring.
        let mut mesh = body();
        mesh.extend(posts(50));
        // One half sculpted (its vertices on the mirror plane only along it), then mirrored.
        let mirror = 20.5;
        let jitter = |v: Vec3| {
            let h = |k: f32| (v.x * 12.9898 + v.y * 78.233 + v.z * 37.719 + k * 1.618).sin() * 0.03;
            Vec3::new(if v.x == mirror { v.x } else { v.x + h(0.0) }, v.y + h(1.0), v.z + h(2.0))
        };
        let half: Vec<Triangle> = cuboid(Vec3::new(20.0, 8.0, 0.0), Vec3::new(21.0, 9.0, 1.0), 6)
            .into_iter()
            .filter(|t| [t.v0, t.v1, t.v2].iter().all(|v| v.x <= mirror))
            .map(|t| Triangle::new(jitter(t.v0), jitter(t.v1), jitter(t.v2), t.normal))
            .collect();
        let flip = |v: Vec3| Vec3::new(2.0 * mirror - v.x, v.y, v.z);
        let button: Vec<Triangle> =
            half.iter().flat_map(|t| [*t, Triangle::new(flip(t.v0), flip(t.v2), flip(t.v1), t.normal)]).collect();
        mesh.extend(&button);
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.is_support[mesh.len() - button.len()..].iter().all(|s| !*s));
    }

    #[test]
    fn parts_with_an_eighth_of_the_seeds_triangles_are_model_parts() {
        // The body has 10,800 triangles, so a part with 1,452 (over an eighth) is a model part
        // whatever its density, and a 12 mm bar with 768 (under it) is support geometry.
        let mut mesh = body();
        mesh.extend(posts(50));
        let part = cuboid(Vec3::new(20.0, 0.0, 0.0), Vec3::new(30.0, 8.0, 10.0), 11);
        mesh.extend(&part);
        let bar = cuboid(Vec3::new(35.0, 0.0, 0.0), Vec3::new(36.5, 12.0, 1.5), 8);
        mesh.extend(&bar);
        let c = classify(&mesh, RAFT).expect("classification");
        let bar_start = mesh.len() - bar.len();
        assert!(c.is_support[bar_start - part.len()..bar_start].iter().all(|s| !*s));
        assert!(c.is_support[bar_start..].iter().all(|s| *s));
    }

    #[test]
    fn sculpted_parts_are_model_details_by_their_triangle_density() {
        // The body has 18 triangles per mm². A sculpted 2.9 mm block with 6 per mm² (over a
        // quarter of that) is a model detail; a sculpted plate with 3 per mm² is support
        // geometry.
        let mut mesh = body();
        mesh.extend(posts(50));
        let block = sculpted(cuboid(Vec3::new(20.0, 0.0, 0.0), Vec3::new(22.9, 2.9, 2.9), 5), 0.01);
        mesh.extend(&block);
        let plate = sculpted(cuboid(Vec3::new(30.0, 0.0, 0.0), Vec3::new(36.0, 6.3, 0.8), 5), 0.01);
        mesh.extend(&plate);
        let c = classify(&mesh, RAFT).expect("classification");
        let plate_start = mesh.len() - plate.len();
        assert!(c.is_support[plate_start - block.len()..plate_start].iter().all(|s| !*s));
        assert!(c.is_support[plate_start..].iter().all(|s| *s));
    }

    #[test]
    fn thick_parts_classed_as_supports_are_not_supports() {
        // 8 mm blocks (2V/A = 2.7 mm) beside the posts: hull pieces of a kit, not supports.
        let mut mesh = body();
        mesh.extend(posts(50));
        for x in [20.0, 30.0] {
            mesh.extend(cuboid(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 8.0, 8.0, 8.0), 5));
        }
        assert!(classify(&mesh, RAFT).is_none());

        // One 3.5 mm block (2V/A = 1.2 mm) among the posts is tolerated.
        let mut mesh = body();
        mesh.extend(posts(50));
        mesh.extend(cuboid(Vec3::new(20.0, 1.0, 0.0), Vec3::new(23.5, 4.5, 3.5), 2));
        let c = classify(&mesh, RAFT).expect("classification");
        assert_eq!(support_count(&c), 50 * 48 + 48);

        // A thick raft is not counted against the supports.
        let mut mesh = body();
        mesh.extend(posts(50));
        mesh.extend(cuboid(Vec3::new(-5.0, 0.0, -5.0), Vec3::new(15.0, 1.5, 15.0), 10));
        assert!(classify(&mesh, RAFT).is_some());
    }

    #[test]
    fn many_repeated_support_parts_are_generated_supports() {
        let mut mesh = body();
        mesh.extend(posts(150));
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.generated);
        assert!(c.holds_supports(&mesh, RAFT));

        let mut few = body();
        few.extend(posts(50));
        assert!(!classify(&few, RAFT).expect("classification").generated);

        // As many parts, but no two triangles alike.
        let mut irregular = body();
        irregular.extend(sculpted(posts(150), 0.01));
        assert!(!classify(&irregular, RAFT).expect("classification").generated);
    }

    #[test]
    fn posts_rebuild_as_supports_and_straps_do_not() {
        let mut supported = body();
        supported.extend(posts(50));
        let c = classify(&supported, RAFT).expect("classification");
        assert!(rebuilds_as_supports(&supported, &c.is_support, RAFT));

        let mut strapped = body();
        strapped.extend(straps());
        let c = classify(&strapped, RAFT).expect("thin straps classify as supports");
        assert_eq!(support_count(&c), straps().len());
        assert!(!rebuilds_as_supports(&strapped, &c.is_support, RAFT));
        assert!(!c.holds_supports(&strapped, RAFT));
    }

    #[test]
    fn a_part_resting_against_a_side_face_under_another_part_is_not_a_support() {
        // A box standing on the plate with a bullet beside it, flush with its top, and the
        // bullet's casing under it, its foot against the box's side: the casing rebuilds as a
        // strut, but its lower end meets the side face, so it holds nothing up.
        let mut mesh = cuboid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 15.0, 10.0), 30);
        mesh.extend(cuboid(Vec3::new(10.3, 12.0, 4.3), Vec3::new(11.7, 15.0, 5.7), 12));
        mesh.extend(cuboid(Vec3::new(10.2, 3.0, 4.2), Vec3::new(11.8, 12.0, 5.8), 2));
        let c = classify(&mesh, RAFT).expect("classification");
        assert_eq!(support_count(&c), 48);
        let (model, supports) = split(&mesh, &c.is_support);
        let supports = to_z_up(&supports);
        let layout = support_layout::reconstruct(&to_z_up(&model), &supports);
        assert_eq!(layout.supports.len(), 1);
        assert!(matches!(layout.supports[0].base, support_layout::SupportBase::Model(_)));
        assert!(layout.coverage(&supports, RAFT).is_some_and(|c| c > 0.9));
        assert_eq!(layout.holding_share(), Some(0.0));
        assert!(!rebuilds_as_supports(&mesh, &c.is_support, RAFT));
    }

    #[test]
    fn dense_support_joint_balls_are_support() {
        let mut mesh = body();
        mesh.extend(posts(50));
        // 1 mm balls, 448 triangles each: as dense as a sculpted detail but all repeats.
        let balls: Vec<Triangle> =
            (0..5).flat_map(|k| sphere(Vec3::new(20.0 + k as f32 * 2.0, 8.0, 0.0), 0.5, 16, 15)).collect();
        assert_eq!(balls.len(), 5 * 448);
        mesh.extend(&balls);
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.is_support[mesh.len() - balls.len()..].iter().all(|s| *s));
        assert_eq!(support_count(&c), 50 * 48 + balls.len());
    }

    #[test]
    fn inner_shell_of_a_hollowed_model_is_model() {
        let mut mesh = body();
        mesh.extend(posts(50));
        mesh.extend(inverted(cuboid(Vec3::new(2.0, 7.0, 2.0), Vec3::new(8.0, 13.0, 8.0), 4)));
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.is_support[mesh.len() - 192..].iter().all(|s| !*s));
        assert_eq!(support_count(&c), 50 * 48);
    }

    /// Two triangles on the same corners facing opposite ways: a closed sliver with no area.
    fn sliver(at: Vec3) -> Vec<Triangle> {
        let (a, b, c) = (at, at + Vec3::new(0.05, 0.0, 0.0), at + Vec3::new(0.0, 0.0, 0.05));
        vec![Triangle::new(a, b, c, Vec3::UP), Triangle::new(a, c, b, Vec3::UP)]
    }

    #[test]
    fn slivers_and_voids_are_model_scraps() {
        let mut mesh = body();
        mesh.extend(posts(50));
        let scraps = mesh.len();
        // A 0.2 mm void inside the body (0.008 mm³, wound inside out) and slivers beside it.
        mesh.extend(inverted(cuboid(Vec3::new(4.0, 9.0, 4.0), Vec3::new(4.2, 9.2, 4.2), 1)));
        for k in 0..200 {
            mesh.extend(sliver(Vec3::new(20.0 + (k % 20) as f32 * 0.1, 6.0 + (k / 20) as f32 * 0.1, 0.0)));
        }
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.is_support[scraps..].iter().all(|s| !*s));
        assert_eq!(support_count(&c), 50 * 48);
        // Counted as model parts, the 201 scraps would bring the average part down to near a
        // post's size.
        assert!(c.confident);
    }

    #[test]
    fn loose_pieces_lying_on_the_model_are_model() {
        let mut mesh = body();
        mesh.extend(posts(50));
        // A slab beside the body and a ledge over part of it, both model parts.
        mesh.extend(cuboid(Vec3::new(12.0, 5.0, 0.0), Vec3::new(22.0, 8.0, 10.0), 30));
        mesh.extend(cuboid(Vec3::new(14.0, 10.0, 3.0), Vec3::new(20.0, 12.0, 7.0), 30));
        let model = mesh.len();
        // A sculpted pebble lying on the slab, clear of the ledge.
        let pebble = sculpted(sphere(Vec3::new(16.5, 8.25, 1.5), 0.25, 8, 6), 0.01);
        mesh.extend(&pebble);
        let pebble = model..mesh.len();
        // A sculpted strut on its own from the slab up into the ledge's underside, and a
        // sculpted twig hanging from the body's underside: they hold the model up.
        mesh.extend(sculpted(cuboid(Vec3::new(19.0, 8.0, 6.0), Vec3::new(19.3, 10.2, 6.3), 2), 0.01));
        mesh.extend(sculpted(cuboid(Vec3::new(9.6, 3.0, 9.5), Vec3::new(9.9, 5.2, 9.8), 2), 0.01));
        let twigs = pebble.end..mesh.len();
        // The same pebble as a generated primitive is left as support.
        mesh.extend(sphere(Vec3::new(16.5, 8.25, 0.5), 0.25, 8, 6));
        let primitive = twigs.end..mesh.len();

        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.is_support[pebble].iter().all(|s| !*s));
        assert!(c.is_support[twigs].iter().all(|s| *s));
        assert!(c.is_support[primitive].iter().all(|s| *s));
        assert!(c.confident);
    }

    #[test]
    fn point_contact_support_tip_is_its_own_component() {
        let mut mesh = body();
        mesh.extend(posts(50));
        // Tip touching a body corner at a single vertex.
        mesh.push(Triangle::new(
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::new(-0.2, 4.0, 0.0),
            Vec3::new(0.0, 4.0, -0.2),
            Vec3::UP,
        ));
        let c = classify(&mesh, RAFT).expect("classification");
        assert!(c.is_support[mesh.len() - 1]);
    }

    #[test]
    fn shared_edge_within_quantisation_tolerance_is_connected() {
        let triangles = vec![
            Triangle::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), Vec3::UP),
            Triangle::new(Vec3::new(0.0, 0.0, 1.001), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0), Vec3::UP),
        ];
        assert_eq!(components(&triangles).closed.len(), 1);
    }

    #[test]
    fn closed_shells_and_open_fragments_are_told_apart() {
        let mut mesh = cuboid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0), 2);
        mesh.push(Triangle::new(
            Vec3::new(5.0, 0.0, 0.0),
            Vec3::new(6.0, 0.0, 0.0),
            Vec3::new(5.0, 1.0, 0.0),
            Vec3::UP,
        ));
        assert_eq!(components(&mesh).closed, vec![true, false]);
    }

    #[test]
    fn outward_cuboid_has_positive_volume() {
        let volume: f64 = cuboid(Vec3::new(1.0, 2.0, 3.0), Vec3::new(3.0, 5.0, 7.0), 2).iter().map(signed_volume).sum();
        assert!((volume - 24.0).abs() < 1e-3, "{volume}");
    }
}
