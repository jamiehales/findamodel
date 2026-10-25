//! Plate footprint hulls: convex, concave and convex-sans-raft outlines projected onto the
//! X-Z plane (bird's eye view with Y up). Each hull is returned as a JSON array of
//! `[x, z]` pairs, in counter-clockwise order.
//!
//! The concave hull follows the JTS/NetTopologySuite "ConcaveHullByLengthRatio" algorithm:
//! a Delaunay triangulation is eroded from the border, longest boundary edge first, until
//! every remaining boundary edge is shorter than the length threshold.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use rustc_hash::FxHashSet;
use spade::{DelaunayTriangulation, Point2, Triangulation};
use tracing::{debug, info, warn};

use super::LoadedGeometry;

const LOG_TARGET: &str = "findamodel.Hull";

pub const DEFAULT_MAX_HULL_VERTICES: usize = 20;
/// Bump whenever hull output changes; see `scan_config::compute`.
pub const CURRENT_HULL_GENERATION_VERSION: i32 = 5;
pub const DEFAULT_RAFT_HEIGHT_MM: f32 = 2.0;

const MAX_CONCAVE_HULL_INPUT_POINTS: usize = 30_000;
const MIN_CONCAVE_RATIO: f64 = 0.08;
const MAX_CONCAVE_RATIO: f64 = 0.95;
const DEDUPE_EPSILON: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn dist(self, o: Point) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hulls {
    pub convex: Option<String>,
    pub concave: Option<String>,
    pub convex_sans_raft: Option<String>,
}

/// Calculates convex, concave and convex-sans-raft hulls. The sans-raft hull excludes
/// vertices below `raft_height_mm` before projection, removing raft/brim geometry.
pub fn calculate_hulls(geometry: &LoadedGeometry, max_hull_vertices: usize, raft_height_mm: f32) -> Hulls {
    let all_points =
        dedupe(geometry.triangles.iter().flat_map(|t| [t.v0, t.v1, t.v2]).map(|v| Point::new(v.x as f64, v.z as f64)));

    if all_points.len() < 3 {
        warn!(target: LOG_TARGET, "Geometry has fewer than 3 unique X-Z points, skipping hull calculation");
        return Hulls::default();
    }

    let convex = enclosing_convex_hull(&all_points, max_hull_vertices);
    let concave = concave_hull(&all_points, max_hull_vertices);

    let sans_raft_points = dedupe(
        geometry
            .triangles
            .iter()
            .flat_map(|t| [t.v0, t.v1, t.v2])
            .filter(|v| v.y >= raft_height_mm)
            .map(|v| Point::new(v.x as f64, v.z as f64)),
    );
    let convex_sans_raft =
        if sans_raft_points.len() >= 3 { enclosing_convex_hull(&sans_raft_points, max_hull_vertices) } else { None };

    info!(
        target: LOG_TARGET,
        "Hull calculation complete: {} convex vertices, {} concave vertices, {} sans-raft vertices (max {max_hull_vertices})",
        convex.as_ref().map_or(0, Vec::len),
        concave.as_ref().map_or(0, Vec::len),
        convex_sans_raft.as_ref().map_or(0, Vec::len),
    );

    Hulls {
        convex: convex.map(|c| to_json(&c)),
        concave: concave.map(|c| to_json(&c)),
        convex_sans_raft: convex_sans_raft.map(|c| to_json(&c)),
    }
}

pub fn to_json(points: &[Point]) -> String {
    let mut out = String::with_capacity(points.len() * 24 + 2);
    out.push('[');
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("[{},{}]", p.x, p.y));
    }
    out.push(']');
    out
}

/// Removes points within `DEDUPE_EPSILON` of an earlier point (grid-hashed), preserving
/// first-seen order.
fn dedupe(points: impl Iterator<Item = Point>) -> Vec<Point> {
    let mut seen: FxHashSet<(i64, i64)> = FxHashSet::default();
    let mut out = Vec::new();
    for p in points {
        if !p.x.is_finite() || !p.y.is_finite() {
            continue;
        }
        let key = ((p.x / DEDUPE_EPSILON).round() as i64, (p.y / DEDUPE_EPSILON).round() as i64);
        if seen.insert(key) {
            out.push(p);
        }
    }
    out
}

// ── Convex hull ──────────────────────────────────────────────────────────────

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Andrew's monotone chain; returns a counter-clockwise ring without the closing point.
pub fn convex_hull(points: &[Point]) -> Vec<Point> {
    let mut pts: Vec<Point> = points.to_vec();
    pts.sort_by(|a, b| {
        a.x.partial_cmp(&b.x).unwrap_or(Ordering::Equal).then(a.y.partial_cmp(&b.y).unwrap_or(Ordering::Equal))
    });
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }

    let mut lower: Vec<Point> = Vec::new();
    for p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0.0 {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<Point> = Vec::new();
    for p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0.0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Convex hull reduced to at most `max_vertices` vertices by building the smallest
/// circumscribing polygon from rotated support lines, so the result still encloses every
/// input point.
fn enclosing_convex_hull(points: &[Point], max_vertices: usize) -> Option<Vec<Point>> {
    if points.len() < 3 {
        return None;
    }

    let base = normalize_ring(&convex_hull(points));
    if base.len() < 3 {
        return None;
    }
    if base.len() <= max_vertices {
        return Some(base);
    }

    const PHASE_SAMPLES: usize = 16;
    const EPSILON: f64 = 1e-5;
    let mut best: Option<Vec<Point>> = None;
    let mut best_area = f64::INFINITY;

    for s in 0..PHASE_SAMPLES {
        let phase = (std::f64::consts::PI * 2.0 * s as f64) / (PHASE_SAMPLES * max_vertices) as f64;
        let Some(candidate) = build_support_polygon(&base, max_vertices, phase, EPSILON) else {
            continue;
        };
        if candidate.len() < 3 || !contains_all_points(&candidate, points, EPSILON * 4.0) {
            continue;
        }
        let area = signed_area(&candidate).abs();
        if area < best_area {
            best_area = area;
            best = Some(candidate);
        }
    }

    best.or_else(|| Some(bounding_rectangle(points)))
}

fn build_support_polygon(hull: &[Point], max_vertices: usize, phase: f64, epsilon: f64) -> Option<Vec<Point>> {
    let normals: Vec<(f64, f64, f64)> = (0..max_vertices)
        .map(|i| {
            let theta = phase + std::f64::consts::PI * 2.0 * i as f64 / max_vertices as f64;
            let (nx, ny) = (theta.cos(), theta.sin());
            let h = hull.iter().map(|p| nx * p.x + ny * p.y).fold(f64::NEG_INFINITY, f64::max);
            (nx, ny, h + epsilon)
        })
        .collect();

    let mut vertices = Vec::with_capacity(max_vertices);
    for i in 0..max_vertices {
        let a = normals[i];
        let b = normals[(i + 1) % max_vertices];
        let det = a.0 * b.1 - a.1 * b.0;
        if det.abs() < 1e-10 {
            return None;
        }
        let x = (a.2 * b.1 - b.2 * a.1) / det;
        let y = (a.0 * b.2 - b.0 * a.2) / det;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        vertices.push(Point::new(x, y));
    }

    Some(normalize_ring(&vertices))
}

fn bounding_rectangle(points: &[Point]) -> Vec<Point> {
    let min_x = points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let min_y = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let max_y = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    vec![Point::new(min_x, min_y), Point::new(max_x, min_y), Point::new(max_x, max_y), Point::new(min_x, max_y)]
}

fn contains_all_points(polygon: &[Point], points: &[Point], epsilon: f64) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let (ex, ey) = (b.x - a.x, b.y - a.y);
        for p in points {
            // CCW polygon: a point on the right side of an edge is outside.
            if ex * (p.y - a.y) - ey * (p.x - a.x) < -epsilon {
                return false;
            }
        }
    }
    true
}

/// Drops a closing duplicate and consecutive duplicates, and orients the ring CCW.
fn normalize_ring(ring: &[Point]) -> Vec<Point> {
    if ring.is_empty() {
        return Vec::new();
    }
    let mut end = ring.len();
    if ring.len() > 1 && ring[0] == ring[ring.len() - 1] {
        end -= 1;
    }
    let mut normalized: Vec<Point> = Vec::with_capacity(end);
    for p in &ring[..end] {
        if normalized.last() != Some(p) {
            normalized.push(*p);
        }
    }
    if normalized.len() >= 3 && signed_area(&normalized) < 0.0 {
        normalized.reverse();
    }
    normalized
}

pub fn signed_area(polygon: &[Point]) -> f64 {
    if polygon.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        area += a.x * b.y - b.x * a.y;
    }
    area / 2.0
}

// ── Concave hull ─────────────────────────────────────────────────────────────

fn concave_hull(points: &[Point], max_vertices: usize) -> Option<Vec<Point>> {
    if points.len() < 3 {
        return None;
    }

    let reduced = downsample_for_concave_hull(points, MAX_CONCAVE_HULL_INPUT_POINTS, DEDUPE_EPSILON);
    let convex_ring = normalize_ring(&convex_hull(points));
    let input = dedupe(reduced.iter().copied().chain(convex_ring.iter().copied()));
    let convex_area = signed_area(&convex_ring).abs().max(1e-9);

    let initial_ratio = estimate_initial_concave_ratio(reduced.len(), max_vertices);
    let mut candidate_ratios = vec![
        initial_ratio,
        (initial_ratio + 0.08).clamp(MIN_CONCAVE_RATIO, MAX_CONCAVE_RATIO),
        (initial_ratio + 0.18).clamp(MIN_CONCAVE_RATIO, MAX_CONCAVE_RATIO),
        (initial_ratio + 0.35).clamp(MIN_CONCAVE_RATIO, MAX_CONCAVE_RATIO),
        MAX_CONCAVE_RATIO,
    ];
    candidate_ratios.dedup();

    let Some(triangulation) = HullTriangulation::new(&input) else {
        debug!(target: LOG_TARGET, "Concave hull triangulation failed (degenerate input)");
        return None;
    };

    let mut best: Option<Vec<Point>> = None;
    let mut best_area_ratio = f64::NEG_INFINITY;
    let mut tried: Vec<f64> = Vec::new();

    for ratio in candidate_ratios {
        if tried.contains(&ratio) {
            continue;
        }
        tried.push(ratio);

        let Some(mut ring) = triangulation.concave_hull_by_length_ratio(ratio) else {
            debug!(target: LOG_TARGET, "Concave hull attempt failed at ratio {ratio}");
            continue;
        };
        ring = normalize_ring(&ring);
        if ring.len() < 3 {
            continue;
        }
        if ring.len() > max_vertices {
            ring = simplify_ring_to_vertex_budget(&ring, max_vertices);
        }
        if ring.len() < 3 || !is_valid_ring(&ring) {
            continue;
        }

        let area_ratio = signed_area(&ring).abs() / convex_area;
        // Reject unstable shapes that collapse too far inside the outer envelope.
        if area_ratio < 0.60 {
            continue;
        }
        if area_ratio > best_area_ratio {
            best_area_ratio = area_ratio;
            best = Some(ring);
        }
    }

    best
}

fn estimate_initial_concave_ratio(point_count: usize, max_vertices: usize) -> f64 {
    if point_count == 0 || max_vertices == 0 {
        return 0.22;
    }
    let density = (max_vertices as f64 / point_count as f64).clamp(0.0, 1.0);
    (0.16 + 0.55 * density).clamp(MIN_CONCAVE_RATIO, 0.45)
}

fn downsample_for_concave_hull(points: &[Point], max_points: usize, epsilon: f64) -> Vec<Point> {
    if points.len() <= max_points {
        return points.to_vec();
    }
    let min_x = points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let min_y = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let max_y = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);

    let width = (max_x - min_x).max(epsilon);
    let height = (max_y - min_y).max(epsilon);
    let area = (width * height).max(epsilon);
    let cell_size = (area / max_points as f64).sqrt().max(epsilon);

    let mut seen: FxHashSet<(i64, i64)> = FxHashSet::default();
    let mut sampled = Vec::new();
    for p in points {
        let key = (((p.x - min_x) / cell_size).floor() as i64, ((p.y - min_y) / cell_size).floor() as i64);
        if seen.insert(key) {
            sampled.push(*p);
        }
    }
    sampled
}

/// A Delaunay triangulation with explicit adjacency, eroded per ratio.
struct HullTriangulation {
    points: Vec<Point>,
    /// Vertex indices of each triangle in CCW order.
    tris: Vec<[usize; 3]>,
    /// `adjacent[t][i]` is the triangle across edge `i` (from vertex i to vertex i+1).
    adjacent: Vec<[Option<usize>; 3]>,
}

struct QueueEntry {
    size: f64,
    tri: usize,
}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size && self.tri == other.tri
    }
}
impl Eq for QueueEntry {}
impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Largest boundary edge first; ties broken deterministically by index.
        self.size.partial_cmp(&other.size).unwrap_or(Ordering::Equal).then(other.tri.cmp(&self.tri))
    }
}

impl HullTriangulation {
    fn new(input: &[Point]) -> Option<Self> {
        let mut dt: DelaunayTriangulation<Point2<f64>> = DelaunayTriangulation::new();
        for p in input {
            dt.insert(Point2::new(p.x, p.y)).ok()?;
        }
        if dt.num_inner_faces() == 0 {
            return None;
        }

        let points: Vec<Point> = dt.vertices().map(|v| Point::new(v.position().x, v.position().y)).collect();
        let mut tris = Vec::with_capacity(dt.num_inner_faces());
        for face in dt.inner_faces() {
            let [a, b, c] = face.vertices().map(|v| v.fix().index());
            let mut tri = [a, b, c];
            if cross(points[a], points[b], points[c]) < 0.0 {
                tri.swap(1, 2);
            }
            tris.push(tri);
        }

        let mut edge_map: HashMap<(usize, usize), (usize, usize)> = HashMap::with_capacity(tris.len() * 3);
        let mut adjacent = vec![[None; 3]; tris.len()];
        for (t, tri) in tris.iter().enumerate() {
            for i in 0..3 {
                let (a, b) = (tri[i], tri[(i + 1) % 3]);
                if let Some(&(other, other_edge)) = edge_map.get(&(b, a)) {
                    adjacent[t][i] = Some(other);
                    adjacent[other][other_edge] = Some(t);
                } else {
                    edge_map.insert((a, b), (t, i));
                }
            }
        }

        Some(Self { points, tris, adjacent })
    }

    fn edge_length(&self, tri: usize, edge: usize) -> f64 {
        let t = self.tris[tri];
        self.points[t[edge]].dist(self.points[t[(edge + 1) % 3]])
    }

    fn concave_hull_by_length_ratio(&self, ratio: f64) -> Option<Vec<Point>> {
        let threshold = self.target_edge_length(ratio);
        let mut adjacent = self.adjacent.clone();
        let mut removed = vec![false; self.tris.len()];
        let mut queue = BinaryHeap::new();

        for t in 0..self.tris.len() {
            self.push_border(t, &adjacent, &mut queue);
        }

        while let Some(QueueEntry { size, tri }) = queue.pop() {
            if removed[tri] {
                continue;
            }
            // The entry may be stale if neighbours changed since it was queued.
            if num_adjacent(&adjacent[tri]) != 2 || (self.boundary_size(tri, &adjacent) - size).abs() > 0.0 {
                continue;
            }
            if size < threshold {
                break;
            }
            if self.is_connecting(tri, &adjacent) {
                continue;
            }

            let neighbours = adjacent[tri];
            for n in neighbours.into_iter().flatten() {
                for slot in adjacent[n].iter_mut() {
                    if *slot == Some(tri) {
                        *slot = None;
                    }
                }
            }
            adjacent[tri] = [None; 3];
            removed[tri] = true;

            for n in neighbours.into_iter().flatten() {
                self.push_border(n, &adjacent, &mut queue);
            }
        }

        self.trace_border(&adjacent, &removed)
    }

    fn target_edge_length(&self, ratio: f64) -> f64 {
        if ratio == 0.0 {
            return 0.0;
        }
        let mut max_len = -1.0f64;
        let mut min_len = -1.0f64;
        for t in 0..self.tris.len() {
            for e in 0..3 {
                let len = self.edge_length(t, e);
                max_len = max_len.max(len);
                if min_len < 0.0 || len < min_len {
                    min_len = len;
                }
            }
        }
        if ratio == 1.0 {
            return 2.0 * max_len;
        }
        ratio * (max_len - min_len) + min_len
    }

    fn boundary_size(&self, tri: usize, adjacent: &[[Option<usize>; 3]]) -> f64 {
        (0..3).filter(|e| adjacent[tri][*e].is_none()).map(|e| self.edge_length(tri, e)).fold(0.0, f64::max)
    }

    fn push_border(&self, tri: usize, adjacent: &[[Option<usize>; 3]], queue: &mut BinaryHeap<QueueEntry>) {
        if num_adjacent(&adjacent[tri]) != 2 {
            return;
        }
        queue.push(QueueEntry { size: self.boundary_size(tri, adjacent), tri });
    }

    /// A border triangle is "connecting" when the vertex opposite its boundary edge is
    /// itself on the boundary; removing it would split the hull into two polygons.
    fn is_connecting(&self, tri: usize, adjacent: &[[Option<usize>; 3]]) -> bool {
        let adj = adjacent[tri];
        let vertex_index = if adj[0].is_some() && adj[1].is_some() {
            1
        } else if adj[1].is_some() && adj[2].is_some() {
            2
        } else if adj[2].is_some() && adj[0].is_some() {
            0
        } else {
            return true;
        };
        !self.is_interior_vertex(tri, vertex_index, adjacent)
    }

    /// Walks the triangle fan around a vertex; it is interior if the fan closes.
    fn is_interior_vertex(&self, start: usize, start_index: usize, adjacent: &[[Option<usize>; 3]]) -> bool {
        let vertex = self.tris[start][start_index];
        let mut curr = start;
        let mut curr_index = start_index;
        for _ in 0..self.tris.len() + 1 {
            let Some(adj) = adjacent[curr][curr_index] else {
                return false;
            };
            // Edge `curr_index` starts at the vertex; in the neighbour the shared edge ends
            // at it, so the next edge around the vertex starts at the vertex's index there.
            let Some(next_index) = self.tris[adj].iter().position(|v| *v == vertex) else {
                return false;
            };
            curr = adj;
            curr_index = next_index;
            if curr == start {
                return true;
            }
        }
        false
    }

    fn trace_border(&self, adjacent: &[[Option<usize>; 3]], removed: &[bool]) -> Option<Vec<Point>> {
        let mut next: HashMap<usize, usize> = HashMap::new();
        for t in 0..self.tris.len() {
            if removed[t] {
                continue;
            }
            for (e, neighbour) in adjacent[t].iter().enumerate() {
                if neighbour.is_none() {
                    next.insert(self.tris[t][e], self.tris[t][(e + 1) % 3]);
                }
            }
        }
        let &start = next.keys().min()?;
        let mut ring = vec![self.points[start]];
        let mut current = next[&start];
        while current != start {
            ring.push(self.points[current]);
            if ring.len() > next.len() + 1 {
                return None;
            }
            current = *next.get(&current)?;
        }
        (ring.len() >= 3).then_some(ring)
    }
}

fn num_adjacent(adj: &[Option<usize>; 3]) -> usize {
    adj.iter().filter(|a| a.is_some()).count()
}

// ── Ring simplification and validation ───────────────────────────────────────

/// Binary-searches the smallest Douglas-Peucker tolerance whose topology-preserving
/// simplification fits within `max_vertices`.
fn simplify_ring_to_vertex_budget(ring: &[Point], max_vertices: usize) -> Vec<Point> {
    if ring.len() <= max_vertices || max_vertices < 3 {
        return ring.to_vec();
    }
    let min_x = ring.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let max_x = ring.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let min_y = ring.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let max_y = ring.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    let diagonal = ((max_x - min_x).powi(2) + (max_y - min_y).powi(2)).sqrt();
    if diagonal <= 0.0 {
        return ring.to_vec();
    }

    let mut best = ring.to_vec();
    let (mut low, mut high) = (0.0, diagonal);
    for _ in 0..18 {
        let mid = (low + high) / 2.0;
        let candidate = normalize_ring(&simplify_ring(ring, mid));
        if candidate.len() < 3 || candidate.len() > max_vertices {
            low = mid;
        } else if is_valid_ring(&candidate) {
            best = candidate;
            high = mid;
        } else {
            // Over-simplified into a self-intersection: retry with a finer tolerance.
            high = mid;
        }
    }
    best
}

/// Douglas-Peucker simplification of a closed ring that refuses any shortcut segment
/// which would cross the rest of the ring, keeping the result simple.
pub fn simplify_ring(ring: &[Point], tolerance: f64) -> Vec<Point> {
    let n = ring.len();
    if n < 4 {
        return ring.to_vec();
    }
    // Split the ring at its first vertex and the vertex farthest from it, so both halves
    // are open polylines with fixed endpoints.
    let far = (1..n)
        .max_by(|a, b| ring[0].dist(ring[*a]).partial_cmp(&ring[0].dist(ring[*b])).unwrap_or(Ordering::Equal))
        .unwrap_or(n / 2);

    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    let closed: Vec<Point> = ring.iter().copied().chain(std::iter::once(ring[0])).collect();
    let mut stack = vec![(0usize, far), (far, n)];

    while let Some((start, end)) = stack.pop() {
        if end <= start + 1 {
            continue;
        }
        let (a, b) = (closed[start], closed[end]);
        let mut max_dist = -1.0;
        let mut max_index = start + 1;
        for (i, p) in closed.iter().enumerate().take(end).skip(start + 1) {
            let d = segment_distance(*p, a, b);
            if d > max_dist {
                max_dist = d;
                max_index = i;
            }
        }

        let crosses = segment_crosses_ring(&closed, start, end);
        if max_dist <= tolerance && !crosses {
            continue;
        }
        keep[max_index % n] = true;
        stack.push((start, max_index));
        stack.push((max_index, end));
    }

    // A ring needs at least three vertices: if both halves collapsed onto the anchor
    // segment, keep the vertex farthest from it.
    if keep.iter().filter(|k| **k).count() < 3 {
        let (a, b) = (ring[0], ring[far]);
        if let Some(extra) = (1..n).filter(|i| *i != far).max_by(|x, y| {
            segment_distance(ring[*x], a, b).partial_cmp(&segment_distance(ring[*y], a, b)).unwrap_or(Ordering::Equal)
        }) {
            keep[extra] = true;
        }
    }

    let simplified: Vec<Point> = (0..n).filter(|i| keep[*i]).map(|i| ring[i]).collect();
    if simplified.len() < 3 { ring.to_vec() } else { simplified }
}

fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len_sq).clamp(0.0, 1.0);
    p.dist(Point::new(a.x + t * dx, a.y + t * dy))
}

/// True when the shortcut from `closed[start]` to `closed[end]` properly intersects any
/// ring edge outside the shortcut's own section.
fn segment_crosses_ring(closed: &[Point], start: usize, end: usize) -> bool {
    let (a, b) = (closed[start], closed[end]);
    for i in 0..closed.len() - 1 {
        if i >= start && i < end {
            continue;
        }
        if segments_properly_intersect(a, b, closed[i], closed[i + 1]) {
            return true;
        }
    }
    false
}

fn segments_properly_intersect(p1: Point, p2: Point, q1: Point, q2: Point) -> bool {
    if p1 == q1 || p1 == q2 || p2 == q1 || p2 == q2 {
        return false;
    }
    let d1 = cross(q1, q2, p1);
    let d2 = cross(q1, q2, p2);
    let d3 = cross(p1, p2, q1);
    let d4 = cross(p1, p2, q2);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    let on_segment = |p: Point, a: Point, b: Point| {
        p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
    };
    (d1 == 0.0 && on_segment(p1, q1, q2))
        || (d2 == 0.0 && on_segment(p2, q1, q2))
        || (d3 == 0.0 && on_segment(q1, p1, p2))
        || (d4 == 0.0 && on_segment(q2, p1, p2))
}

/// A ring is valid when it has positive area and no two non-adjacent edges touch.
pub fn is_valid_ring(ring: &[Point]) -> bool {
    let n = ring.len();
    if n < 3 || signed_area(ring).abs() <= 0.0 {
        return false;
    }
    for i in 0..n {
        let (a1, a2) = (ring[i], ring[(i + 1) % n]);
        if a1 == a2 {
            return false;
        }
        for j in i + 1..n {
            if j == i || (j + 1) % n == i || (i + 1) % n == j {
                continue;
            }
            let (b1, b2) = (ring[j], ring[(j + 1) % n]);
            if segments_intersect_inclusive(a1, a2, b1, b2) {
                return false;
            }
        }
    }
    true
}

fn segments_intersect_inclusive(p1: Point, p2: Point, q1: Point, q2: Point) -> bool {
    let d1 = cross(q1, q2, p1);
    let d2 = cross(q1, q2, p2);
    let d3 = cross(p1, p2, q1);
    let d4 = cross(p1, p2, q2);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    let on_segment = |p: Point, a: Point, b: Point| {
        p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
    };
    (d1 == 0.0 && on_segment(p1, q1, q2))
        || (d2 == 0.0 && on_segment(p2, q1, q2))
        || (d3 == 0.0 && on_segment(q1, p1, p2))
        || (d4 == 0.0 && on_segment(q2, p1, p2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Triangle, Vec3};

    fn parse(json: &str) -> Vec<Point> {
        let raw: Vec<[f64; 2]> = serde_json::from_str(json).expect("hull json");
        raw.into_iter().map(|[x, y]| Point::new(x, y)).collect()
    }

    /// Flat "L" shape made of unit squares on the X-Z plane, raised by `height`.
    fn l_shape(height: f32) -> LoadedGeometry {
        let mut triangles = Vec::new();
        let cells: Vec<(i32, i32)> =
            (0..20).flat_map(|x| (0..20).map(move |z| (x, z))).filter(|(x, z)| *x < 5 || *z < 5).collect();
        for (x, z) in cells {
            let (x, z) = (x as f32, z as f32);
            let v = |dx: f32, dz: f32, y: f32| Vec3::new(x + dx, y, z + dz);
            for y in [0.0, height] {
                triangles.push(Triangle::new(v(0.0, 0.0, y), v(1.0, 0.0, y), v(1.0, 1.0, y), Vec3::UP));
                triangles.push(Triangle::new(v(0.0, 0.0, y), v(1.0, 1.0, y), v(0.0, 1.0, y), Vec3::UP));
            }
        }
        LoadedGeometry {
            triangles,
            sphere_centre: Vec3::new(0.0, height / 2.0, 0.0),
            sphere_radius: 20.0,
            dimension_x_mm: 20.0,
            dimension_y_mm: height,
            dimension_z_mm: 20.0,
        }
    }

    #[test]
    fn convex_hull_of_square_points_is_ccw_square() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.5, 0.5),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
        ];
        let hull = convex_hull(&points);
        assert_eq!(hull.len(), 4);
        assert!(signed_area(&hull) > 0.0);
    }

    #[test]
    fn hulls_are_within_vertex_budget_and_enclose_geometry() {
        let geometry = l_shape(5.0);
        let hulls = calculate_hulls(&geometry, DEFAULT_MAX_HULL_VERTICES, DEFAULT_RAFT_HEIGHT_MM);

        let convex = parse(hulls.convex.as_deref().expect("convex"));
        assert!(convex.len() >= 3 && convex.len() <= DEFAULT_MAX_HULL_VERTICES);
        let points: Vec<Point> = geometry
            .triangles
            .iter()
            .flat_map(|t| [t.v0, t.v1, t.v2])
            .map(|v| Point::new(v.x as f64, v.z as f64))
            .collect();
        assert!(contains_all_points(&convex, &points, 1e-3));

        let concave = parse(hulls.concave.as_deref().expect("concave"));
        assert!(concave.len() >= 3 && concave.len() <= DEFAULT_MAX_HULL_VERTICES);
        assert!(is_valid_ring(&concave));
        // The L-shape's concave hull must be noticeably smaller than its convex hull.
        assert!(signed_area(&concave).abs() < signed_area(&convex).abs() * 0.95);
    }

    #[test]
    fn sans_raft_hull_excludes_low_vertices() {
        let mut geometry = l_shape(5.0);
        // A wide raft below the cutoff should not influence the sans-raft hull.
        geometry.triangles.push(Triangle::new(
            Vec3::new(-50.0, 0.0, -50.0),
            Vec3::new(50.0, 0.0, -50.0),
            Vec3::new(0.0, 0.5, 50.0),
            Vec3::UP,
        ));
        let hulls = calculate_hulls(&geometry, DEFAULT_MAX_HULL_VERTICES, DEFAULT_RAFT_HEIGHT_MM);
        let convex = parse(hulls.convex.as_deref().unwrap());
        let sans_raft = parse(hulls.convex_sans_raft.as_deref().unwrap());
        assert!(signed_area(&sans_raft).abs() < signed_area(&convex).abs());
        assert!(sans_raft.iter().all(|p| p.x > -1.0 && p.y > -1.0));
    }

    #[test]
    fn fewer_than_three_points_produces_no_hulls() {
        let geometry = LoadedGeometry {
            triangles: vec![Triangle::new(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 2.0, 0.0),
                Vec3::UP,
            )],
            sphere_centre: Vec3::default(),
            sphere_radius: 1.0,
            dimension_x_mm: 0.0,
            dimension_y_mm: 2.0,
            dimension_z_mm: 0.0,
        };
        assert_eq!(calculate_hulls(&geometry, 20, 2.0), Hulls::default());
    }

    #[test]
    fn simplify_ring_never_produces_self_intersection() {
        // A comb-shaped ring with deep narrow notches.
        let mut ring = vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0), Point::new(20.0, 10.0)];
        for i in (0..10).rev() {
            let x = i as f64 * 2.0;
            ring.push(Point::new(x + 1.5, 10.0));
            ring.push(Point::new(x + 1.5, 1.0));
            ring.push(Point::new(x + 0.5, 1.0));
            ring.push(Point::new(x + 0.5, 10.0));
        }
        ring.push(Point::new(0.0, 10.0));
        let ring = normalize_ring(&ring);
        assert!(is_valid_ring(&ring));
        for tolerance in [0.1, 0.5, 1.0, 3.0, 8.0] {
            let simplified = normalize_ring(&simplify_ring(&ring, tolerance));
            assert!(is_valid_ring(&simplified), "invalid ring at tolerance {tolerance}");
        }
    }

    #[test]
    fn concave_hull_of_separated_clusters_fits_vertex_budget() {
        let mut points = Vec::new();
        for i in 0..10 {
            let x0 = (i * 2) as f64;
            points.extend([
                Point::new(-x0, 0.0),
                Point::new(-x0 - 2.0, 0.0),
                Point::new(-x0 - 2.0, 10.0),
                Point::new(-x0, 10.0),
            ]);
        }
        points.extend([Point::new(-30.0, 0.0), Point::new(-31.0, 0.0), Point::new(-31.0, 1.0), Point::new(-30.0, 1.0)]);
        let ring = concave_hull(&dedupe(points.into_iter()), DEFAULT_MAX_HULL_VERTICES).expect("concave hull");
        assert!(ring.len() <= DEFAULT_MAX_HULL_VERTICES, "{} vertices", ring.len());
        assert!(is_valid_ring(&ring));
    }

    #[test]
    fn simplify_ring_keeps_a_triangle_at_large_tolerance() {
        let ring: Vec<Point> = (0..12)
            .map(|i| {
                let a = i as f64 / 12.0 * std::f64::consts::TAU;
                Point::new(a.cos() * 10.0, a.sin() * 10.0)
            })
            .collect();
        let simplified = simplify_ring(&ring, 1000.0);
        assert_eq!(simplified.len(), 3);
        assert!(is_valid_ring(&normalize_ring(&simplified)));
    }

    #[test]
    fn json_output_uses_invariant_number_formatting() {
        assert_eq!(to_json(&[Point::new(1.5, -2.0), Point::new(0.25, 3.0)]), "[[1.5,-2],[0.25,3]]");
    }
}
