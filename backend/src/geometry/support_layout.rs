//! Rebuilds the layout of a pre-supported model's supports (contact tips, shaft paths,
//! branch points and roots) from their geometry, so a slicer can regenerate them.
//!
//! The support triangles are sliced into thin horizontal layers. Each closed outline in a
//! layer is a blob, so separate shells stay apart even where they overlap (crowded or crossing
//! shafts), while an outline lying inside another (a shaft inside its joint ball) joins it.
//! Blobs in neighbouring layers that overlap are linked. That graph follows every shaft
//! whether the supports are separate primitives or one unioned mesh. A blob with nothing above it that touches the model is a contact tip; the contact
//! itself is where the shaft leaves the model, since tips are buried a little way into it.
//! Walking down from the tip, at each fork taking the most vertical way (a brace leaves its
//! post at an angle) and then the one that best continues the shaft, traces the shaft to the
//! plate (a trunk) or into a shaft an earlier walk claimed (a branch). Tips are walked from
//! the highest down, so the tallest support of a tree is its trunk. Blobs much wider than
//! the shaft, or off its line (raft, braces, crossing shafts), are passed through without
//! bending the shaft's centre line or ending the walk.
//!
//! Coordinates are Z-up millimetres with the plate at Z = 0.

use rayon::prelude::*;
use rustc_hash::FxHashMap;

use super::{Triangle, Vec3};

/// Slice spacing; coarser when the supports are too tall for [`MAX_LAYERS`].
const LAYER_MM: f32 = 0.1;
const MAX_LAYERS: usize = 4_000;
/// Layers sliced together on one thread.
const LAYERS_PER_BAND: usize = 32;
/// Cut points closer than this are one point: neighbouring triangles cut a shared edge at the
/// same place, which is what joins their segments into one outline.
const OUTLINE_SNAP_MM: f32 = 0.0005;
/// Grid cell for blob containment and layer-to-layer overlap lookups.
const BLOB_CELL_MM: f32 = 1.0;
/// Blobs in neighbouring layers whose boxes come this close are linked.
const LINK_MARGIN_MM: f32 = 0.05;
/// Grid cell for nearest-surface lookups on the model.
const SURFACE_CELL_MM: f32 = 1.0;
/// A blob top within this distance of the model surface is a contact tip.
const CONTACT_MAX_MM: f32 = 1.0;
/// How far from the shaft the model surface is searched for where the tip enters it.
const ENTRY_SEARCH_MM: f32 = 1.5;
/// A blob wider than `width * WIDEN_RATIO + WIDEN_SLACK_MM` of the shaft being walked is a
/// raft, brace or crossing: walked through, but neither a shaft point nor a branch point.
const WIDEN_RATIO: f32 = 1.5;
const WIDEN_SLACK_MM: f32 = 0.25;
/// How quickly the walked shaft width follows the blobs it passes.
const WIDTH_SMOOTHING: f32 = 0.25;
/// The walked shaft width grows by at most this much per mm walked: enough to follow a
/// widening tip, too little to follow a brace peeling off the shaft (which widens the blob
/// a little each layer).
const MAX_WIDTH_GROWTH: f32 = 0.15;
/// A blob whose centre is further than `DRIFT_MM + width * DRIFT_RATIO` from where the shaft
/// is heading is a brace merging into the shaft, so it is not a shaft point either.
const DRIFT_MM: f32 = 0.1;
const DRIFT_RATIO: f32 = 0.3;
/// Drift is measured from the shaft as it was this far back, so a blob creeping sideways a
/// little each layer cannot drag the measure along with it.
const DRIFT_LAG_MM: f32 = 0.5;
/// After this much height of skipped blobs narrower than [`MAX_RESET_WIDTH_MM`], the shaft
/// really is thicker than its tip or really does bend, so the walk takes the blob as is.
const SKIP_RESET_MM: f32 = 1.5;
const MAX_RESET_WIDTH_MM: f32 = 2.0;
/// Height over which the shaft direction is measured.
const DIRECTION_SPAN_MM: f32 = 1.0;
/// Where the structure forks going down, each way is followed this far and the most
/// vertical one taken: a brace leaves the post it hangs from at an angle, and a bent shaft
/// can point straight at it.
const FORK_LOOKAHEAD_MM: f32 = 2.0;
/// Ways whose slopes differ by less than this are told apart by the shaft's direction.
const FORK_SLOPE_TOLERANCE: f32 = 0.15;
/// How far down the shaft the model surface is looked for, from the buried tip top.
const ENTRY_WINDOW_MM: f32 = 5.0;
/// The tip's socket is looked for within this distance of the contact.
const TIP_WINDOW_MM: f32 = 4.0;
const MIN_TIP_LENGTH_MM: f32 = 0.6;
/// The socket is the first point reaching this fraction of the tip window's widest blob (a
/// joint ball), or where the tip stops widening: by less than `PLATEAU_GROWTH_MM` over the
/// next `PLATEAU_SPAN_MM`.
const TIP_BODY_FRACTION: f32 = 0.85;
const PLATEAU_GROWTH_MM: f32 = 0.03;
const PLATEAU_SPAN_MM: f32 = 0.3;
/// A tip may take up this share of its support's length; a strut's two tips share one shaft.
const TIP_SHARE: f32 = 0.6;
const STRUT_TIP_SHARE: f32 = 0.4;
/// The contact diameter is the tip's narrow end, which is usually buried a little way in the
/// model: the top slice that is at least this fraction of the slice below it (narrower top
/// slices cut through an end cap).
const CAP_WIDTH_FRACTION: f32 = 0.8;
const MIN_CONTACT_DIAMETER_MM: f32 = 0.2;
const MAX_CONTACT_DIAMETER_MM: f32 = 1.5;
const MAX_BODY_DIAMETER_MM: f32 = 3.0;
const MIN_SHAFT_DIAMETER_MM: f32 = 0.3;
/// The shaft diameter is this quantile (1/N) of its cross-sections: a shaft keeps one width,
/// and joint balls, root flares and merged braces only ever widen a section.
const SHAFT_WIDTH_QUANTILE: usize = 4;
const MAX_SHAFT_DIAMETER_MM: f32 = 5.0;
/// Bends smaller than this are dropped from the shaft path.
const SIMPLIFY_MM: f32 = 0.3;
/// A contact this close to an earlier support's contact is a second top of that tip.
const DUPLICATE_CONTACT_MM: f32 = 0.5;
/// A walk ending within this height of the lowest support layer stands on the plate, when that
/// layer is itself this close to the plate (Z = 0).
const PLATE_REACH_MM: f32 = 0.5;
pub const ROOT_DISK_HEIGHT_MM: f32 = 0.5;
pub const ROOT_CONE_HEIGHT_MM: f32 = 1.5;
/// Where a plate support's shaft leaves its root.
pub const ROOT_TOP_MM: f32 = ROOT_DISK_HEIGHT_MM + ROOT_CONE_HEIGHT_MM;
/// Root diameter as a multiple of the shaft diameter, within the min/max.
const ROOT_DIAMETER_RATIO: f32 = 3.0;
const MIN_ROOT_DIAMETER_MM: f32 = 2.0;
const MAX_ROOT_DIAMETER_MM: f32 = 6.0;
/// The socket must sit at least this far above the root top for a shaft to fit.
const MIN_SHAFT_LENGTH_MM: f32 = 0.2;

const NONE: u32 = u32::MAX;

/// The tip where a support touches the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactTip {
    /// Point on the model surface.
    pub contact: Vec3,
    /// Model surface normal at the contact, facing the support.
    pub surface_normal: Vec3,
    /// Where the tip meets the shaft.
    pub socket: Vec3,
    pub contact_diameter_mm: f32,
    pub body_diameter_mm: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SupportBase {
    /// A root on the plate; the shaft leaves the root's top ([`ROOT_TOP_MM`]) at `(x, y)`.
    Plate { x: f32, y: f32, diameter_mm: f32 },
    /// Hangs from `point` on the shaft of an earlier support.
    Shaft { parent: usize, point: Vec3 },
    /// A strut between two parts of the model: the shaft starts at a second, lower contact.
    Model(ContactTip),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SupportPath {
    pub tip: ContactTip,
    pub base: SupportBase,
    /// Bends between the base and the socket, bottom to top.
    pub joints: Vec<Vec3>,
    pub shaft_diameter_mm: f32,
}

impl SupportPath {
    /// Where the shaft starts: the root's top or the knot on the parent shaft.
    pub fn shaft_start(&self) -> Vec3 {
        match &self.base {
            SupportBase::Plate { x, y, .. } => Vec3::new(*x, *y, ROOT_TOP_MM),
            SupportBase::Shaft { point, .. } => *point,
            SupportBase::Model(lower) => lower.socket,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SupportLayout {
    /// A support's parent always comes before it.
    pub supports: Vec<SupportPath>,
}

/// Increment when the reconstruction changes, so anything derived from a layout is rebuilt.
pub const CURRENT_SUPPORT_LAYOUT_VERSION: i32 = 1;

/// Rebuilds the support layout from Z-up support geometry and the model it holds up.
pub fn reconstruct(model: &[Triangle], supports: &[Triangle]) -> SupportLayout {
    let Some(graph) = Graph::slice(supports) else { return SupportLayout::default() };
    let surface = SurfaceIndex::new(model);

    let mut tops: Vec<(u32, SurfacePoint)> = (0..graph.blobs.len() as u32)
        .filter(|&i| graph.up[i as usize].is_empty())
        .filter_map(|i| surface.nearest(graph.centre(i), CONTACT_MAX_MM).map(|hit| (i, hit)))
        .collect();
    tops.sort_by(|a, b| graph.blobs[b.0 as usize].z.total_cmp(&graph.blobs[a.0 as usize].z));

    let mut claimed = vec![NONE; graph.blobs.len()];
    let mut layout = SupportLayout::default();
    let (mut stranded, mut duplicate, mut too_low) = (0usize, 0usize, 0usize);
    for (top, top_contact) in tops {
        let walk = graph.walk(top, &claimed);
        let points: Vec<Vec3> = walk.nodes.iter().map(|&i| graph.centre(i)).collect();
        let (contact, entry) = surface.entry(&points).unwrap_or((top_contact, 0));
        log::trace!(
            "Tip candidate top {} contact {} ({} shaft blobs, ends {})",
            graph.centre(top),
            contact.point,
            walk.nodes.len(),
            match walk.end {
                WalkEnd::Plate => "on the plate".to_string(),
                WalkEnd::Joined(parent, _) => format!("on support {parent}"),
                WalkEnd::Stranded => "stranded".to_string(),
            }
        );
        let lower = match walk.end {
            WalkEnd::Stranded => lower_end(&graph, &surface, &walk),
            _ => None,
        };
        let support = if matches!(walk.end, WalkEnd::Stranded) && lower.is_none() {
            stranded += 1;
            None
        } else if layout.supports.iter().any(|s| (s.tip.contact - contact.point).length() < DUPLICATE_CONTACT_MM) {
            duplicate += 1;
            None
        } else {
            let support = build_support(&graph, &walk, &contact, entry, lower);
            too_low += usize::from(support.is_none());
            support
        };
        if let Some(support) = support {
            let id = layout.supports.len() as u32;
            for &node in &walk.nodes {
                claimed[node as usize] = id;
            }
            layout.supports.push(support);
        }
    }
    log::debug!(
        "Support layout: {} supports from {} support triangles ({} stranded, {} duplicate tips, {} too low)",
        layout.supports.len(),
        supports.len(),
        stranded,
        duplicate,
        too_low,
    );
    layout
}

/// Builds a support from its walk. `entry` is the first walked blob outside the model; the
/// blobs above it are the part of the tip buried in the model.
/// The far end of a walk that stopped short of the plate, when it stops in the model.
struct LowerEnd {
    contact: SurfacePoint,
    /// The walk's blobs from the bottom up.
    nodes: Vec<u32>,
    /// The first of `nodes` outside the model.
    entry: usize,
}

/// Where a walk that stopped short of the plate goes into the model, for a strut between two
/// parts of it. None when it stopped in mid-air.
fn lower_end(graph: &Graph, surface: &SurfaceIndex, walk: &Walk) -> Option<LowerEnd> {
    let nodes: Vec<u32> = walk.visited.iter().rev().copied().collect();
    surface.nearest(graph.centre(nodes[0]), CONTACT_MAX_MM)?;
    let points: Vec<Vec3> = nodes.iter().map(|&i| graph.centre(i)).collect();
    let (contact, entry) = surface.entry(&points)?;
    Some(LowerEnd { contact, nodes, entry })
}

/// A tip cone fitted to blobs running away from its contact.
#[derive(Clone, Copy)]
struct TipFit {
    /// The socket, counted from the first blob outside the model.
    socket_index: usize,
    socket: Vec3,
    socket_width: f32,
    contact_diameter: f32,
}

/// Fits a tip to `nodes`, which run from the tip's buried end outward; `entry` is the first
/// one outside the model. The tip may take up `share` of the length from the contact on.
fn fit_tip(graph: &Graph, nodes: &[u32], entry: usize, contact: Vec3, share: f32) -> TipFit {
    let points: Vec<Vec3> = nodes[entry..].iter().map(|&i| graph.centre(i)).collect();
    let widths: Vec<f32> = nodes[entry..].iter().map(|&i| graph.blobs[i as usize].half_min * 2.0).collect();
    let total = (points[0] - contact).length() + points.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f32>();
    // The tip is a straight cone, so it is measured by straight distance from the contact:
    // the blob centres of a steeply inclined tip jump about from slice to slice.
    let reach: Vec<f32> = points.iter().map(|p| (*p - contact).length()).collect();

    let window = TIP_WINDOW_MM.min(total * share);
    let widest = (0..points.len()).take_while(|&i| reach[i] <= window).map(|i| widths[i]).fold(0.0, f32::max);
    let plateau = |i: usize| {
        (i..points.len())
            .find(|&j| reach[j] >= reach[i] + PLATEAU_SPAN_MM)
            .is_some_and(|j| widths[j] - widths[i] < PLATEAU_GROWTH_MM)
    };
    let socket_index = (0..points.len())
        .take_while(|&i| reach[i] <= window)
        .find(|&i| reach[i] >= MIN_TIP_LENGTH_MM && (widths[i] >= widest * TIP_BODY_FRACTION || plateau(i)))
        .or_else(|| (0..points.len()).rev().find(|&i| reach[i] <= window))
        .unwrap_or(0);

    let tip_widths: Vec<f32> =
        nodes[..=entry + socket_index].iter().map(|&i| graph.blobs[i as usize].half_min * 2.0).collect();
    let contact_diameter = tip_widths
        .windows(2)
        .find(|w| w[0] >= w[1] * CAP_WIDTH_FRACTION)
        .map_or(tip_widths[0], |w| w[0])
        .clamp(MIN_CONTACT_DIAMETER_MM, MAX_CONTACT_DIAMETER_MM);
    TipFit { socket_index, socket: points[socket_index], socket_width: widths[socket_index], contact_diameter }
}

/// The contact tip for a fit, with the surface normal turned to face the support.
fn contact_tip(contact: &SurfacePoint, fit: &TipFit, shaft_diameter: f32) -> ContactTip {
    let toward_socket = fit.socket - contact.point;
    let surface_normal = if contact.normal.dot(toward_socket) < 0.0 { -contact.normal } else { contact.normal };
    // DragonFruit never makes a tip wider than its shaft.
    let contact_diameter = fit.contact_diameter.min(shaft_diameter);
    ContactTip {
        contact: contact.point,
        surface_normal,
        socket: fit.socket,
        contact_diameter_mm: contact_diameter,
        body_diameter_mm: fit.socket_width.min(shaft_diameter).clamp(contact_diameter, MAX_BODY_DIAMETER_MM),
    }
}

/// The shaft diameter from its cross-sections: a shaft keeps one width, and joint balls, root
/// flares and merged braces only ever widen a section.
fn shaft_diameter(mut widths: Vec<f32>, fallback: f32) -> f32 {
    widths.sort_by(f32::total_cmp);
    widths
        .get(widths.len() / SHAFT_WIDTH_QUANTILE)
        .copied()
        .unwrap_or(fallback)
        .clamp(MIN_SHAFT_DIAMETER_MM, MAX_SHAFT_DIAMETER_MM)
}

fn build_support(
    graph: &Graph,
    walk: &Walk,
    contact: &SurfacePoint,
    entry: usize,
    lower: Option<LowerEnd>,
) -> Option<SupportPath> {
    if let Some(lower) = lower {
        return Some(build_strut(graph, walk, contact, entry, &lower));
    }
    let fit = fit_tip(graph, &walk.nodes, entry, contact.point, TIP_SHARE);
    let points: Vec<Vec3> = walk.nodes[entry..].iter().map(|&i| graph.centre(i)).collect();
    let widths: Vec<f32> =
        walk.nodes[entry + fit.socket_index + 1..].iter().map(|&i| graph.blobs[i as usize].half_min * 2.0).collect();
    let shaft_diameter = shaft_diameter(widths, fit.socket_width);
    let tip = contact_tip(contact, &fit, shaft_diameter);
    let (socket_index, socket) = (fit.socket_index, fit.socket);

    // Shaft centre line from the socket down, ending at the base.
    let mut line: Vec<Vec3> = points[socket_index..].to_vec();
    let base = match walk.end {
        WalkEnd::Joined(parent, node) => {
            let point = graph.centre(node);
            line.push(point);
            SupportBase::Shaft { parent, point }
        }
        WalkEnd::Plate => {
            if socket.z < ROOT_TOP_MM + MIN_SHAFT_LENGTH_MM {
                return None;
            }
            // Cut the line where it passes the root top, so the shaft leaves the root there.
            let above = line.iter().take_while(|p| p.z > ROOT_TOP_MM).count();
            let foot = match line.get(above) {
                Some(&below) if above > 0 => {
                    let upper = line[above - 1];
                    let t = (upper.z - ROOT_TOP_MM) / (upper.z - below.z);
                    upper + (below - upper) * t
                }
                _ => line[above.saturating_sub(1)],
            };
            line.truncate(above.max(1));
            let foot = Vec3::new(foot.x, foot.y, ROOT_TOP_MM);
            line.push(foot);
            let diameter_mm = (shaft_diameter * ROOT_DIAMETER_RATIO).clamp(MIN_ROOT_DIAMETER_MM, MAX_ROOT_DIAMETER_MM);
            SupportBase::Plate { x: foot.x, y: foot.y, diameter_mm }
        }
        WalkEnd::Stranded => return None,
    };

    Some(SupportPath { tip, base, joints: bends(&line), shaft_diameter_mm: shaft_diameter })
}

/// A strut between two parts of the model, from its upper contact down to its lower one.
fn build_strut(graph: &Graph, walk: &Walk, contact: &SurfacePoint, entry: usize, lower: &LowerEnd) -> SupportPath {
    let mut upper_fit = fit_tip(graph, &walk.nodes, entry, contact.point, STRUT_TIP_SHARE);
    let mut lower_fit = fit_tip(graph, &lower.nodes, lower.entry, lower.contact.point, STRUT_TIP_SHARE);
    if upper_fit.socket.z <= lower_fit.socket.z {
        // The two tips overlap on a short strut: put each socket a third of the way along.
        let span = lower.contact.point - contact.point;
        upper_fit.socket = contact.point + span * (1.0 / 3.0);
        lower_fit.socket = contact.point + span * (2.0 / 3.0);
    }
    let (upper_socket, lower_socket) = (upper_fit.socket, lower_fit.socket);
    let between: Vec<f32> = walk
        .visited
        .iter()
        .map(|&i| &graph.blobs[i as usize])
        .filter(|b| b.z < upper_socket.z && b.z > lower_socket.z)
        .map(|b| b.half_min * 2.0)
        .collect();
    let shaft_diameter = shaft_diameter(between, upper_fit.socket_width.min(lower_fit.socket_width));

    // Shaft centre line from the upper socket down to the lower one.
    let mut line = vec![upper_socket];
    line.extend(walk.nodes.iter().map(|&i| graph.centre(i)).filter(|p| p.z < upper_socket.z && p.z > lower_socket.z));
    line.push(lower_socket);
    SupportPath {
        tip: contact_tip(contact, &upper_fit, shaft_diameter),
        base: SupportBase::Model(contact_tip(&lower.contact, &lower_fit, shaft_diameter)),
        joints: bends(&line),
        shaft_diameter_mm: shaft_diameter,
    }
}

/// The bends of a shaft line running from the top down, bottom to top.
fn bends(line: &[Vec3]) -> Vec<Vec3> {
    let kept = simplify(line, SIMPLIFY_MM);
    kept[1..kept.len() - 1].iter().rev().map(|&i| line[i]).collect()
}

/// Douglas-Peucker: indices of the points kept, always including both ends.
fn simplify(points: &[Vec3], tolerance: f32) -> Vec<usize> {
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (mut worst, mut worst_distance) = (0, tolerance);
        for i in a + 1..b {
            let distance = distance_to_segment(points[i], points[a], points[b]);
            if distance > worst_distance {
                (worst, worst_distance) = (i, distance);
            }
        }
        if worst != 0 {
            keep[worst] = true;
            stack.push((a, worst));
            stack.push((worst, b));
        }
    }
    (0..points.len()).filter(|&i| keep[i]).collect()
}

fn distance_to_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
}

/// An axis-aligned box on a slice plane.
#[derive(Debug, Clone, Copy)]
struct Rect {
    min: [f32; 2],
    max: [f32; 2],
}

impl Rect {
    fn point(p: [f32; 2]) -> Self {
        Self { min: p, max: p }
    }

    fn grow(&mut self, p: [f32; 2]) {
        self.min = [self.min[0].min(p[0]), self.min[1].min(p[1])];
        self.max = [self.max[0].max(p[0]), self.max[1].max(p[1])];
    }

    fn union(&mut self, other: &Rect) {
        self.grow(other.min);
        self.grow(other.max);
    }

    fn centre(&self) -> [f32; 2] {
        [(self.min[0] + self.max[0]) / 2.0, (self.min[1] + self.max[1]) / 2.0]
    }

    fn area(&self) -> f32 {
        (self.max[0] - self.min[0]) * (self.max[1] - self.min[1])
    }

    fn encloses(&self, other: &Rect) -> bool {
        self.min[0] <= other.min[0]
            && self.min[1] <= other.min[1]
            && self.max[0] >= other.max[0]
            && self.max[1] >= other.max[1]
    }

    fn overlaps(&self, other: &Rect, margin: f32) -> bool {
        self.min[0] <= other.max[0] + margin
            && other.min[0] <= self.max[0] + margin
            && self.min[1] <= other.max[1] + margin
            && other.min[1] <= self.max[1] + margin
    }

    /// Grid cells (of `size`) the box covers.
    fn cells(&self, size: f32) -> impl Iterator<Item = (i32, i32)> {
        let (x0, y0) = cell2(self.min, size);
        let (x1, y1) = cell2(self.max, size);
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }
}

fn cell2(p: [f32; 2], size: f32) -> (i32, i32) {
    ((p[0] / size).floor() as i32, (p[1] / size).floor() as i32)
}

#[derive(Debug, Clone, Copy)]
struct Blob {
    x: f32,
    y: f32,
    z: f32,
    half_min: f32,
    half_max: f32,
}

struct Graph {
    blobs: Vec<Blob>,
    up: Vec<Vec<u32>>,
    down: Vec<Vec<u32>>,
    /// Height of the lowest slice.
    floor: f32,
}

enum WalkEnd {
    /// Reached the plate.
    Plate,
    /// Reached a blob claimed by `supports[.0]`, at blob `.1`.
    Joined(usize, u32),
    /// Ran out of blobs above the plate: a strut ending on the model, or a dead end.
    Stranded,
}

struct Walk {
    /// Shaft blobs from the top down, leaving out widened ones.
    nodes: Vec<u32>,
    /// Every blob passed, from the top down.
    visited: Vec<u32>,
    end: WalkEnd,
}

impl Graph {
    fn slice(triangles: &[Triangle]) -> Option<Graph> {
        let (lo, hi) = triangles
            .iter()
            .flat_map(|t| [t.v0.z, t.v1.z, t.v2.z])
            .fold((f32::MAX, f32::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
        if hi - lo <= LAYER_MM || !(hi - lo).is_finite() {
            return None;
        }
        let step = LAYER_MM.max((hi - lo) / MAX_LAYERS as f32);
        let count = ((hi - lo) / step).floor() as usize;
        let plane = |k: usize| lo + (k as f32 + 0.5) * step;

        let layers: Vec<Vec<Rect>> = (0..count)
            .step_by(LAYERS_PER_BAND)
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|start| {
                let end = (start + LAYERS_PER_BAND).min(count);
                let (band_lo, band_hi) = (plane(start), plane(end - 1));
                let band: Vec<&Triangle> = triangles
                    .iter()
                    .filter(|t| t.v0.z.min(t.v1.z).min(t.v2.z) <= band_hi && t.v0.z.max(t.v1.z).max(t.v2.z) >= band_lo)
                    .collect();
                (start..end).map(|k| layer_blobs(&band, plane(k))).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect();

        let mut offsets = Vec::with_capacity(layers.len() + 1);
        let mut blobs = Vec::new();
        for (k, rects) in layers.iter().enumerate() {
            offsets.push(blobs.len() as u32);
            blobs.extend(rects.iter().map(|r| {
                let [x, y] = r.centre();
                let (w, h) = (r.max[0] - r.min[0], r.max[1] - r.min[1]);
                Blob { x, y, z: plane(k), half_min: w.min(h) / 2.0, half_max: w.max(h) / 2.0 }
            }));
        }
        offsets.push(blobs.len() as u32);

        let links: Vec<(u32, u32)> = (0..layers.len().saturating_sub(1))
            .into_par_iter()
            .flat_map_iter(|k| {
                let (lower, upper) = (&layers[k], &layers[k + 1]);
                let mut grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
                for (j, r) in upper.iter().enumerate() {
                    for c in r.cells(BLOB_CELL_MM) {
                        grid.entry(c).or_default().push(j as u32);
                    }
                }
                let mut pairs = Vec::new();
                let mut candidates = Vec::new();
                for (i, r) in lower.iter().enumerate() {
                    candidates.clear();
                    let reach = Rect {
                        min: [r.min[0] - LINK_MARGIN_MM, r.min[1] - LINK_MARGIN_MM],
                        max: [r.max[0] + LINK_MARGIN_MM, r.max[1] + LINK_MARGIN_MM],
                    };
                    for c in reach.cells(BLOB_CELL_MM) {
                        candidates.extend(grid.get(&c).into_iter().flatten().copied());
                    }
                    candidates.sort_unstable();
                    candidates.dedup();
                    for &j in &candidates {
                        if r.overlaps(&upper[j as usize], LINK_MARGIN_MM) {
                            pairs.push((offsets[k] + i as u32, offsets[k + 1] + j));
                        }
                    }
                }
                pairs
            })
            .collect();

        let mut up = vec![Vec::new(); blobs.len()];
        let mut down = vec![Vec::new(); blobs.len()];
        for (below, above) in links {
            up[below as usize].push(above);
            down[above as usize].push(below);
        }
        Some(Graph { blobs, up, down, floor: plane(0) })
    }

    fn centre(&self, i: u32) -> Vec3 {
        let b = &self.blobs[i as usize];
        Vec3::new(b.x, b.y, b.z)
    }

    /// Follows the shaft down from `start` until the plate, a claimed blob or a dead end.
    fn walk(&self, start: u32, claimed: &[u32]) -> Walk {
        let mut nodes = vec![start];
        let mut visited = vec![start];
        let mut width = self.blobs[start as usize].half_max;
        // Top of the current run of skipped blobs.
        let mut skipped_from: Option<f32> = None;
        let mut current = start;
        loop {
            let below = &self.down[current as usize];
            let next = match below.as_slice() {
                [] => break,
                [only] => *only,
                many => {
                    let z = self.blobs[many[0] as usize].z;
                    let (px, py) = self.predict(&nodes, z);
                    let miss = |i: u32| {
                        let b = &self.blobs[i as usize];
                        (b.x - px).powi(2) + (b.y - py).powi(2)
                    };
                    let slopes: Vec<f32> = many.iter().map(|&i| self.descent_slope(i)).collect();
                    let steepest = slopes.iter().copied().fold(f32::MAX, f32::min);
                    many.iter()
                        .zip(&slopes)
                        .filter(|(_, slope)| **slope <= steepest + FORK_SLOPE_TOLERANCE)
                        .map(|(&i, _)| i)
                        .min_by(|&a, &b| miss(a).total_cmp(&miss(b)))
                        .expect("non-empty")
                }
            };
            visited.push(next);
            let blob = &self.blobs[next as usize];
            let last_z = self.blobs[*nodes.last().expect("walk has a start") as usize].z;
            let settled = nodes.iter().rposition(|&i| self.blobs[i as usize].z >= last_z + DRIFT_LAG_MM);
            let (px, py) = self.predict(&nodes[..=settled.unwrap_or(nodes.len() - 1)], blob.z);
            let drift = ((blob.x - px).powi(2) + (blob.y - py).powi(2)).sqrt();
            let narrow = blob.half_max <= width * WIDEN_RATIO + WIDEN_SLACK_MM;
            let aligned = drift <= DRIFT_MM + width * DRIFT_RATIO;
            let reset =
                blob.half_max <= MAX_RESET_WIDTH_MM && skipped_from.is_some_and(|top| top - blob.z >= SKIP_RESET_MM);
            if reset {
                width = width.max(blob.half_max);
            }
            if (narrow && aligned) || reset {
                let owner = claimed[next as usize];
                if owner != NONE {
                    return Walk { nodes, visited, end: WalkEnd::Joined(owner as usize, next) };
                }
                let step = (self.centre(next) - self.centre(*nodes.last().expect("walk has a start"))).length();
                width += ((blob.half_max - width) * WIDTH_SMOOTHING).min(MAX_WIDTH_GROWTH * step);
                skipped_from = None;
                nodes.push(next);
            } else {
                skipped_from.get_or_insert(blob.z);
            }
            current = next;
        }
        let end = if self.floor <= PLATE_REACH_MM && self.blobs[current as usize].z <= self.floor + PLATE_REACH_MM {
            WalkEnd::Plate
        } else {
            WalkEnd::Stranded
        };
        Walk { nodes, visited, end }
    }

    /// Horizontal run per unit of height going down from `start` for [`FORK_LOOKAHEAD_MM`],
    /// staying on the blob nearest above at each fork.
    fn descent_slope(&self, start: u32) -> f32 {
        let top = self.blobs[start as usize];
        let mut current = start;
        loop {
            let here = self.blobs[current as usize];
            let next = self.down[current as usize].iter().copied().min_by(|&a, &b| {
                let d = |i: u32| {
                    let b = &self.blobs[i as usize];
                    (b.x - here.x).powi(2) + (b.y - here.y).powi(2)
                };
                d(a).total_cmp(&d(b))
            });
            match next {
                Some(next) if top.z - here.z < FORK_LOOKAHEAD_MM => current = next,
                _ => break,
            }
        }
        let end = self.blobs[current as usize];
        let drop = top.z - end.z;
        if drop <= 0.0 {
            return f32::MAX;
        }
        ((end.x - top.x).powi(2) + (end.y - top.y).powi(2)).sqrt() / drop
    }

    /// Where the shaft through `nodes` (top down) continues at height `z`.
    fn predict(&self, nodes: &[u32], z: f32) -> (f32, f32) {
        let last = self.blobs[*nodes.last().expect("walk has a start") as usize];
        let anchor = nodes.iter().rev().map(|&i| self.blobs[i as usize]).find(|b| b.z - last.z >= DIRECTION_SPAN_MM);
        match anchor {
            Some(a) => {
                let t = (z - last.z) / (last.z - a.z);
                (last.x + (last.x - a.x) * t, last.y + (last.y - a.y) * t)
            }
            None => (last.x, last.y),
        }
    }
}

/// One slice's cross-section as blobs. Each closed outline is its own blob, so separate
/// shells stay apart even where they overlap, and an outline lying wholly inside another (a
/// shaft inside its joint ball, or inside the raft) joins the one around it.
fn layer_blobs(triangles: &[&Triangle], z: f32) -> Vec<Rect> {
    let segments: Vec<[[f32; 2]; 2]> = triangles.iter().filter_map(|t| cross_section(t, z)).collect();
    if segments.is_empty() {
        return Vec::new();
    }

    // Segments sharing an end point belong to one outline.
    let mut parent: Vec<u32> = (0..segments.len() as u32).collect();
    let mut ends: FxHashMap<(i32, i32), u32> = FxHashMap::default();
    for (i, segment) in segments.iter().enumerate() {
        for &p in segment {
            let (cx, cy) = cell2(p, OUTLINE_SNAP_MM);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(&j) = ends.get(&(cx + dx, cy + dy)) {
                        union(&mut parent, i as u32, j);
                    }
                }
            }
            ends.entry((cx, cy)).or_insert(i as u32);
        }
    }

    let mut outlines: Vec<Outline> = Vec::new();
    let mut slot: FxHashMap<u32, usize> = FxHashMap::default();
    for (i, segment) in segments.iter().enumerate() {
        let root = find(&mut parent, i as u32);
        let k = *slot.entry(root).or_insert_with(|| {
            outlines.push(Outline { rect: Rect::point(segment[0]), segments: Vec::new() });
            outlines.len() - 1
        });
        outlines[k].rect.grow(segment[0]);
        outlines[k].rect.grow(segment[1]);
        outlines[k].segments.push(i as u32);
    }
    merge_enclosed(&segments, outlines)
}

/// One closed cross-section outline: its box and the indices of its segments.
struct Outline {
    rect: Rect,
    segments: Vec<u32>,
}

/// Merges each outline lying wholly inside a larger one into it.
fn merge_enclosed(segments: &[[[f32; 2]; 2]], outlines: Vec<Outline>) -> Vec<Rect> {
    if outlines.len() < 2 {
        return outlines.into_iter().map(|o| o.rect).collect();
    }
    let mut grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
    for (i, o) in outlines.iter().enumerate() {
        for c in o.rect.cells(BLOB_CELL_MM) {
            grid.entry(c).or_default().push(i as u32);
        }
    }
    let mut parent: Vec<u32> = (0..outlines.len() as u32).collect();
    for (i, inner) in outlines.iter().enumerate() {
        let centre = inner.rect.centre();
        for &j in grid.get(&cell2(centre, BLOB_CELL_MM)).into_iter().flatten() {
            let outer = &outlines[j as usize];
            if j as usize != i
                && outer.rect.area() > inner.rect.area()
                && outer.rect.encloses(&inner.rect)
                && encircles(segments, &outer.segments, centre)
            {
                union(&mut parent, i as u32, j);
            }
        }
    }
    let mut merged: Vec<Rect> = Vec::new();
    let mut slot: FxHashMap<u32, usize> = FxHashMap::default();
    for (i, o) in outlines.iter().enumerate() {
        let root = find(&mut parent, i as u32);
        match slot.get(&root) {
            Some(&s) => merged[s].union(&o.rect),
            None => {
                slot.insert(root, merged.len());
                merged.push(o.rect);
            }
        }
    }
    merged
}

/// Whether `p` lies inside the outline made of `outline`'s segments (even-odd rule).
fn encircles(segments: &[[[f32; 2]; 2]], outline: &[u32], p: [f32; 2]) -> bool {
    let mut inside = false;
    for &i in outline {
        let [a, b] = segments[i as usize];
        if (a[1] > p[1]) != (b[1] > p[1]) && a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) > p[0] {
            inside = !inside;
        }
    }
    inside
}

/// The segment where the plane at height `z` cuts the triangle.
fn cross_section(t: &Triangle, z: f32) -> Option<[[f32; 2]; 2]> {
    let mut out = [[0.0; 2]; 2];
    let mut n = 0;
    for (a, b) in [(t.v0, t.v1), (t.v1, t.v2), (t.v2, t.v0)] {
        if (a.z < z) != (b.z < z) {
            // Cut each edge from the same end, so the triangles sharing it get the same point.
            let (a, b) = if (a.z, a.x, a.y) <= (b.z, b.x, b.y) { (a, b) } else { (b, a) };
            let s = (z - a.z) / (b.z - a.z);
            if n < 2 {
                out[n] = [a.x + (b.x - a.x) * s, a.y + (b.y - a.y) * s];
            }
            n += 1;
        }
    }
    (n == 2).then_some(out)
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

fn union(parent: &mut [u32], a: u32, b: u32) {
    let (a, b) = (find(parent, a), find(parent, b));
    if a != b {
        parent[b as usize] = a;
    }
}

struct SurfacePoint {
    point: Vec3,
    normal: Vec3,
}

/// Nearest-point lookups on the model surface.
struct SurfaceIndex<'a> {
    triangles: &'a [Triangle],
    cells: FxHashMap<(i32, i32, i32), Vec<u32>>,
}

fn cell3(p: Vec3) -> (i32, i32, i32) {
    let c = |v: f32| (v / SURFACE_CELL_MM).floor() as i32;
    (c(p.x), c(p.y), c(p.z))
}

impl<'a> SurfaceIndex<'a> {
    fn new(triangles: &'a [Triangle]) -> Self {
        let mut cells: FxHashMap<(i32, i32, i32), Vec<u32>> = FxHashMap::default();
        for (i, t) in triangles.iter().enumerate() {
            let lo = cell3(Vec3::new(
                t.v0.x.min(t.v1.x).min(t.v2.x),
                t.v0.y.min(t.v1.y).min(t.v2.y),
                t.v0.z.min(t.v1.z).min(t.v2.z),
            ));
            let hi = cell3(Vec3::new(
                t.v0.x.max(t.v1.x).max(t.v2.x),
                t.v0.y.max(t.v1.y).max(t.v2.y),
                t.v0.z.max(t.v1.z).max(t.v2.z),
            ));
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    for z in lo.2..=hi.2 {
                        cells.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        Self { triangles, cells }
    }

    /// Where the shaft through `points` (top down) leaves the model, and the index of the first
    /// point outside it. A tip is usually buried a little way into the model, so its top is
    /// not where it touches. None when no point within the tip window is outside (or the
    /// model's winding is inverted).
    fn entry(&self, points: &[Vec3]) -> Option<(SurfacePoint, usize)> {
        let mut previous: Option<(Vec3, f32)> = None;
        let mut walked = 0.0;
        for (i, &p) in points.iter().enumerate() {
            if i > 0 {
                walked += (p - points[i - 1]).length();
                if walked > ENTRY_WINDOW_MM {
                    return None;
                }
            }
            let side = self.nearest(p, ENTRY_SEARCH_MM).map(|hit| (p - hit.point).dot(hit.normal));
            match (side, previous) {
                (Some(side), _) if side < 0.0 => previous = Some((p, side)),
                // Outside: the crossing lies between this point and the last inside one.
                (side, Some((inside, inside_side))) => {
                    let side = side.unwrap_or(ENTRY_SEARCH_MM);
                    let crossing = inside + (p - inside) * (inside_side / (inside_side - side));
                    return self.nearest(crossing, ENTRY_SEARCH_MM).map(|hit| (hit, i));
                }
                (Some(_), None) => return self.nearest(p, ENTRY_SEARCH_MM).map(|hit| (hit, i)),
                (None, None) => return None,
            }
        }
        None
    }

    /// The closest surface point within `max` of `p`, with its triangle's face normal.
    fn nearest(&self, p: Vec3, max: f32) -> Option<SurfacePoint> {
        let reach = (max / SURFACE_CELL_MM).ceil() as i32;
        let (cx, cy, cz) = cell3(p);
        let mut best: Option<(f32, Vec3, u32)> = None;
        for x in cx - reach..=cx + reach {
            for y in cy - reach..=cy + reach {
                for z in cz - reach..=cz + reach {
                    for &i in self.cells.get(&(x, y, z)).into_iter().flatten() {
                        let t = &self.triangles[i as usize];
                        let q = closest_on_triangle(p, t.v0, t.v1, t.v2);
                        let d = (q - p).length_sq();
                        if d <= max * max && best.is_none_or(|(b, _, _)| d < b) {
                            best = Some((d, q, i));
                        }
                    }
                }
            }
        }
        best.map(|(_, point, i)| {
            let t = &self.triangles[i as usize];
            SurfacePoint { point, normal: (t.v1 - t.v0).cross(t.v2 - t.v0).normalized() }
        })
    }
}

/// Closest point to `p` on triangle `abc` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = va + vb + vc;
    if denom.abs() < f32::EPSILON {
        return a;
    }
    a + ab * (vb / denom) + ac * (vc / denom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::support_separation::tests::cuboid;

    /// Closed frustum from `a` (radius `ra`) to `b` (radius `rb`) with 16 sides.
    fn frustum(a: Vec3, b: Vec3, ra: f32, rb: f32) -> Vec<Triangle> {
        let axis = (b - a).normalized();
        let helper = if axis.z.abs() < 0.9 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::new(1.0, 0.0, 0.0) };
        let u = axis.cross(helper).normalized();
        let v = axis.cross(u);
        let ring = |c: Vec3, r: f32, k: usize| {
            let t = std::f32::consts::TAU * k as f32 / 16.0;
            c + u * (r * t.cos()) + v * (r * t.sin())
        };
        let mut out = Vec::new();
        for k in 0..16 {
            let (p0, p1, q0, q1) = (ring(a, ra, k), ring(a, ra, k + 1), ring(b, rb, k), ring(b, rb, k + 1));
            out.push(Triangle::new(p0, p1, q1, u));
            out.push(Triangle::new(p0, q1, q0, u));
            out.push(Triangle::new(a, p1, p0, -axis));
            out.push(Triangle::new(b, q0, q1, axis));
        }
        out
    }

    /// A support in DragonFruit's anatomy: a tip cone buried 0.2 mm into the model at
    /// `contact`, a joint ball at the socket, then a shaft through `path` to the plate.
    fn support(contact: Vec3, path: &[Vec3]) -> Vec<Triangle> {
        let mut out = frustum(contact + Vec3::new(0.0, 0.0, 0.2), path[0], 0.12, 0.4);
        for w in path.windows(2) {
            out.extend(frustum(w[0], w[1], 0.4, 0.4));
            out.extend(frustum(w[0] + Vec3::new(0.0, 0.0, 0.5), w[0] - Vec3::new(0.0, 0.0, 0.5), 0.5, 0.5));
        }
        out
    }

    fn box_model(min: Vec3, max: Vec3) -> Vec<Triangle> {
        cuboid(min, max, 4)
    }

    fn close(a: Vec3, b: Vec3, within: f32) -> bool {
        (a - b).length() <= within
    }

    #[test]
    fn vertical_post_becomes_a_trunk_touching_the_model_underside() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let supports = support(Vec3::new(1.0, 2.0, 10.0), &[Vec3::new(1.0, 2.0, 8.0), Vec3::new(1.0, 2.0, 0.0)]);
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        assert!(close(s.tip.contact, Vec3::new(1.0, 2.0, 10.0), 0.1), "{}", s.tip.contact);
        assert!(close(s.tip.surface_normal, Vec3::new(0.0, 0.0, -1.0), 0.01), "{}", s.tip.surface_normal);
        assert!((7.4..=9.0).contains(&s.tip.socket.z), "{}", s.tip.socket);
        assert!((0.2..=0.4).contains(&s.tip.contact_diameter_mm), "{}", s.tip.contact_diameter_mm);
        assert!((0.7..=0.9).contains(&s.shaft_diameter_mm), "{}", s.shaft_diameter_mm);
        assert!(s.joints.is_empty(), "{:?}", s.joints);
        let SupportBase::Plate { x, y, .. } = s.base else { panic!("{:?}", s.base) };
        assert!((x - 1.0).abs() < 0.1 && (y - 2.0).abs() < 0.1, "{x} {y}");
    }

    #[test]
    fn bent_shaft_keeps_a_joint_at_the_bend() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let path = [Vec3::new(0.0, 0.0, 8.0), Vec3::new(3.0, 0.0, 5.0), Vec3::new(3.0, 0.0, 0.0)];
        let layout = reconstruct(&model, &support(Vec3::new(0.0, 0.0, 10.0), &path));

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        let on_path = |j: Vec3| path.windows(2).any(|w| distance_to_segment(j, w[0], w[1]) <= 0.4);
        assert!(s.joints.iter().all(|j| on_path(*j)), "{:?}", s.joints);
        assert!(s.joints.iter().any(|j| close(*j, path[1], 0.7)), "{:?}", s.joints);
        assert!(s.joints.windows(2).all(|w| w[0].z < w[1].z), "{:?}", s.joints);
        let SupportBase::Plate { x, .. } = s.base else { panic!("{:?}", s.base) };
        assert!((x - 3.0).abs() < 0.2, "{x}");
    }

    #[test]
    fn lower_tip_joining_another_shaft_is_a_branch_of_it() {
        let mut model = box_model(Vec3::new(-2.0, -2.0, 10.0), Vec3::new(2.0, 2.0, 15.0));
        model.extend(box_model(Vec3::new(2.0, -2.0, 9.0), Vec3::new(6.0, 2.0, 15.0)));
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(4.0, 0.0, 9.0), &[Vec3::new(4.0, 0.0, 7.0), Vec3::new(0.3, 0.0, 4.0)]));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2);
        assert!(matches!(layout.supports[0].base, SupportBase::Plate { .. }));
        assert!(close(layout.supports[1].tip.contact, Vec3::new(4.0, 0.0, 9.0), 0.1));
        let SupportBase::Shaft { parent, point } = layout.supports[1].base else {
            panic!("{:?}", layout.supports[1].base)
        };
        assert_eq!(parent, 0);
        assert!(point.x.abs() < 0.2 && (3.0..=5.0).contains(&point.z), "{point}");
    }

    #[test]
    fn raft_under_the_supports_does_not_end_the_trunk() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(1.0, 1.0, 10.0), &[Vec3::new(1.0, 1.0, 8.0), Vec3::new(1.0, 1.0, 0.5)]);
        supports.extend(cuboid(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 1.0), 4));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let SupportBase::Plate { x, y, .. } = layout.supports[0].base else { panic!() };
        assert!((x - 1.0).abs() < 0.1 && (y - 1.0).abs() < 0.1, "{x} {y}");
    }

    #[test]
    fn diagonal_brace_does_not_pull_the_trunk_off_its_post() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(3.0, 0.0, 10.0), &[Vec3::new(3.0, 0.0, 8.0), Vec3::new(3.0, 0.0, 0.0)]));
        // Brace from high on the first post down to the second.
        supports.extend(frustum(Vec3::new(0.0, 0.0, 6.0), Vec3::new(3.0, 0.0, 3.0), 0.3, 0.3));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2);
        for s in &layout.supports {
            let SupportBase::Plate { x, .. } = s.base else { panic!("{:?}", s.base) };
            assert!((x - s.tip.contact.x).abs() < 0.2, "{x} under {}", s.tip.contact);
            assert!(s.joints.is_empty(), "{:?}", s.joints);
        }
    }

    #[test]
    fn overlapping_separate_posts_stay_separate_at_their_own_width() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        // Two 0.8 mm posts whose shells overlap by 0.1 mm all the way down.
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(0.7, 0.0, 10.0), &[Vec3::new(0.7, 0.0, 8.0), Vec3::new(0.7, 0.0, 0.0)]));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2);
        for s in &layout.supports {
            let SupportBase::Plate { x, .. } = s.base else { panic!("{:?}", s.base) };
            assert!((x - s.tip.contact.x).abs() < 0.1, "{x} under {}", s.tip.contact);
            assert!((0.7..=0.9).contains(&s.shaft_diameter_mm), "{}", s.shaft_diameter_mm);
        }
    }

    #[test]
    fn strut_between_two_model_parts_keeps_both_contacts() {
        let mut model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        model.extend(box_model(Vec3::new(1.0, -3.0, 0.0), Vec3::new(6.0, 3.0, 6.0)));
        // A 0.5 mm rod from the upper part's underside down to the lower part's top, buried
        // a little way into each.
        let strut = frustum(Vec3::new(-0.05, 0.0, 10.2), Vec3::new(2.05, 0.0, 5.8), 0.25, 0.25);
        let layout = reconstruct(&model, &strut);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        let SupportBase::Model(lower) = &s.base else { panic!("{:?}", s.base) };
        assert!(close(s.tip.contact, Vec3::new(0.0, 0.0, 10.0), 0.3), "{}", s.tip.contact);
        assert!(close(lower.contact, Vec3::new(1.95, 0.0, 6.0), 0.3), "{}", lower.contact);
        assert!(close(lower.surface_normal, Vec3::new(0.0, 0.0, 1.0), 0.01), "{}", lower.surface_normal);
        assert!(s.tip.socket.z > lower.socket.z);
        assert!((0.4..=0.65).contains(&s.shaft_diameter_mm), "{}", s.shaft_diameter_mm);
    }

    #[test]
    fn supports_away_from_the_model_and_tips_too_low_for_a_root_are_left_out() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let far = support(Vec3::new(20.0, 0.0, 10.0), &[Vec3::new(20.0, 0.0, 8.0), Vec3::new(20.0, 0.0, 0.0)]);
        assert!(reconstruct(&model, &far).supports.is_empty());

        let low_model = box_model(Vec3::new(-5.0, -5.0, 1.5), Vec3::new(5.0, 5.0, 4.0));
        let low = support(Vec3::new(0.0, 0.0, 1.5), &[Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 0.0)]);
        assert!(reconstruct(&low_model, &low).supports.is_empty());
        assert!(reconstruct(&model, &[]).supports.is_empty());
    }

    #[test]
    fn simplify_keeps_the_ends_and_real_bends_only() {
        let line = [
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(0.05, 0.0, 9.0),
            Vec3::new(0.0, 0.0, 8.0),
            Vec3::new(2.0, 0.0, 6.0),
            Vec3::new(2.0, 0.0, 0.0),
        ];
        assert_eq!(simplify(&line, SIMPLIFY_MM), vec![0, 2, 3, 4]);
        assert_eq!(simplify(&line[..2], SIMPLIFY_MM), vec![0, 1]);
    }
}
