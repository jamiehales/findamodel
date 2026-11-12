//! Method 2, the level-set remesh: a signed distance on a sparse grid band round the surface, its
//! sign from the generalised winding number (inside where it is over a half, so overlapping parts
//! merge, internal faces vanish and small holes close over), its size the distance to the nearest
//! face, contoured by marching tetrahedra (six per cube along the main diagonal), which always
//! gives a closed, manifold, consistently oriented surface that does not cross itself. The band
//! starts at the cells the faces pass through and grows wherever the sign changes across its edge
//! (the membrane spanning a hole lies away from any face).

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use super::mesh::Indexed;
use crate::geometry::support_reader::contacts::Wind;
use crate::geometry::support_reader::math::{BoxIndex, P, closest_on_tri, cross, dist, dot, len, sub};

const BITS: u32 = 21;
/// A surface point stays this share of an edge away from its ends: no sliver triangles round a
/// grid point on the surface, and the surface moves at most this share of a cell.
const EDGE_MARGIN: f64 = 0.02;
const MASK: u64 = (1 << BITS) - 1;

fn pack(i: [i64; 3]) -> u64 {
    (i[0] as u64 & MASK) | ((i[1] as u64 & MASK) << BITS) | ((i[2] as u64 & MASK) << (2 * BITS))
}
fn unpack(k: u64) -> [i64; 3] {
    [(k & MASK) as i64, ((k >> BITS) & MASK) as i64, ((k >> (2 * BITS)) & MASK) as i64]
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RemeshStats {
    pub cell_mm: f64,
    pub band_cells: usize,
    pub grown_cells: usize,
    pub field_points: usize,
    pub seconds_field: f64,
    pub seconds_contour: f64,
    /// Grid columns whose face crossings do not balance (open surface): their points are signed
    /// by the winding number.
    pub leaky_columns: usize,
    /// Triangles the contour had before simplifying ([`remesh_simplified`]).
    pub raw_triangles: usize,
}

/// Grid points (or cells) with a value each, sorted by key: built and searched in parallel. Later
/// additions (a few, as the band grows) go to a side table until [`Sorted::settle`].
#[derive(Default)]
struct Sorted {
    keys: Vec<u64>,
    vals: Vec<f32>,
    extra: FxHashMap<u64, f32>,
}

impl Sorted {
    fn from_sorted(v: Vec<(u64, f32)>) -> Self {
        let (keys, vals) = v.into_iter().unzip();
        Sorted { keys, vals, extra: FxHashMap::default() }
    }
    fn len(&self) -> usize {
        self.keys.len() + self.extra.len()
    }
    fn contains(&self, k: u64) -> bool {
        self.keys.binary_search(&k).is_ok() || self.extra.contains_key(&k)
    }
    fn get(&self, k: u64) -> f32 {
        match self.keys.binary_search(&k) {
            Ok(i) => self.vals[i],
            Err(_) => self.extra[&k],
        }
    }
    /// Adds entries not already present.
    fn merge(&mut self, add: Vec<(u64, f32)>) {
        if self.keys.is_empty() && self.extra.is_empty() {
            *self = Sorted::from_sorted(add);
        } else {
            self.extra.extend(add);
        }
    }
    /// Folds the side table into the sorted lists.
    fn settle(&mut self) {
        if self.extra.is_empty() {
            return;
        }
        let mut all: Vec<(u64, f32)> = self.keys.iter().copied().zip(self.vals.iter().copied()).collect();
        all.extend(self.extra.drain());
        all.par_sort_unstable_by_key(|e| e.0);
        let (keys, vals) = all.into_iter().unzip();
        self.keys = keys;
        self.vals = vals;
    }
}

struct Grid {
    o: P,
    h: f64,
}

impl Grid {
    fn at(&self, i: [i64; 3]) -> P {
        [0, 1, 2].map(|k| self.o[k] + i[k] as f64 * self.h)
    }
    fn cell_of(&self, p: P) -> [i64; 3] {
        [0, 1, 2].map(|k| ((p[k] - self.o[k]) / self.h).floor() as i64)
    }
}

/// Every grid column's face crossings (height and direction), for the winding number of a grid
/// point by counting the faces below it. A column crossing an edge or corner exactly is counted
/// once (the top-left rule).
struct Columns {
    /// Column key to its run in `z` and `up`.
    runs: FxHashMap<u64, (u32, u32)>,
    z: Vec<f64>,
    /// The winding number just above each crossing.
    after: Vec<i32>,
    /// Columns whose crossings do not sum to nought.
    bad: FxHashSet<u64>,
    leaky: usize,
}

impl Columns {
    fn new(tris: &[[P; 3]], g: &Grid) -> Self {
        Self::within(tris, g, [i64::MIN, i64::MIN], [i64::MAX, i64::MAX])
    }

    /// Only the columns from `from` to `to` (inclusive grid indices).
    fn within(tris: &[[P; 3]], g: &Grid, from: [i64; 2], to: [i64; 2]) -> Self {
        let mut ev: Vec<(u64, f64, i8)> = tris
            .par_iter()
            .flat_map_iter(|t| {
                let mut out = Vec::new();
                let nz = (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0]);
                if nz == 0.0 {
                    return out;
                }
                // Counter-clockwise in plan; the crossing raises the winding when the face looks down.
                let (a, b, c) = if nz > 0.0 { (t[0], t[1], t[2]) } else { (t[0], t[2], t[1]) };
                let up: i8 = if nz > 0.0 { -1 } else { 1 };
                let lo = [0, 1].map(|k| a[k].min(b[k]).min(c[k]));
                let hi = [0, 1].map(|k| a[k].max(b[k]).max(c[k]));
                let i0 = (((lo[0] - g.o[0]) / g.h).ceil() as i64).max(from[0]);
                let i1 = (((hi[0] - g.o[0]) / g.h).floor() as i64).min(to[0]);
                let j0 = (((lo[1] - g.o[1]) / g.h).ceil() as i64).max(from[1]);
                let j1 = (((hi[1] - g.o[1]) / g.h).floor() as i64).min(to[1]);
                let edge = |p: P, q: P, x: f64, y: f64| (q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0]);
                // Top-left: an edge going left (or straight down) owns the points on it.
                let owns = |p: P, q: P| (q[1] < p[1]) || (q[1] == p[1] && q[0] < p[0]);
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let (x, y) = (g.o[0] + i as f64 * g.h, g.o[1] + j as f64 * g.h);
                        let w = [edge(b, c, x, y), edge(c, a, x, y), edge(a, b, x, y)];
                        let es = [(b, c), (c, a), (a, b)];
                        if (0..3).all(|k| w[k] > 0.0 || (w[k] == 0.0 && owns(es[k].0, es[k].1))) {
                            let s = w[0] + w[1] + w[2];
                            let z = (w[0] * a[2] + w[1] * b[2] + w[2] * c[2]) / s;
                            out.push((pack([i, j, 0]), z, up));
                        }
                    }
                }
                out
            })
            .collect();
        ev.par_sort_unstable_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
        let mut runs = FxHashMap::default();
        let mut after = Vec::with_capacity(ev.len());
        let mut bad = FxHashSet::default();
        let mut start = 0;
        while start < ev.len() {
            let mut end = start;
            let mut w = 0;
            while end < ev.len() && ev[end].0 == ev[start].0 {
                w += i32::from(ev[end].2);
                after.push(w);
                end += 1;
            }
            if w != 0 {
                bad.insert(ev[start].0);
            }
            runs.insert(ev[start].0, (start as u32, end as u32));
            start = end;
        }
        let leaky = bad.len();
        Columns { runs, z: ev.iter().map(|e| e.1).collect(), after, bad, leaky }
    }

    /// The winding number at grid point `i` (height `z`), or None in a column that leaks.
    fn winding(&self, i: [i64; 3], z: f64) -> Option<i32> {
        let key = pack([i[0], i[1], 0]);
        if self.bad.contains(&key) {
            return None;
        }
        let Some(&(a, b)) = self.runs.get(&key) else { return Some(0) };
        let zs = &self.z[a as usize..b as usize];
        let n = zs.partition_point(|&x| x < z);
        Some(if n == 0 { 0 } else { self.after[a as usize + n - 1] })
    }
}

/// Cells a triangle passes through (its box, after splitting it until the box is small).
fn cells_of(g: &Grid, t: [P; 3], out: &mut Vec<u64>) {
    let lo = [0, 1, 2].map(|k| t[0][k].min(t[1][k]).min(t[2][k]));
    let hi = [0, 1, 2].map(|k| t[0][k].max(t[1][k]).max(t[2][k]));
    let (a, b) = (g.cell_of(lo), g.cell_of(hi));
    if (0..3).any(|k| b[k] - a[k] > 2) {
        let m = |x: P, y: P| [0, 1, 2].map(|k| (x[k] + y[k]) * 0.5);
        let (ab, bc, ca) = (m(t[0], t[1]), m(t[1], t[2]), m(t[2], t[0]));
        for s in [[t[0], ab, ca], [ab, t[1], bc], [ca, bc, t[2]], [ab, bc, ca]] {
            cells_of(g, s, out);
        }
        return;
    }
    for x in a[0]..=b[0] {
        for y in a[1]..=b[1] {
            for z in a[2]..=b[2] {
                out.push(pack([x, y, z]));
            }
        }
    }
}

/// Stage timings, at debug level.
pub(crate) fn trace(s: &str) {
    tracing::debug!(target: "findamodel.Watertight", "{s}");
}

/// The level-set remesh of a triangle soup on cells `h` mm wide.
pub fn remesh(tris: &[[P; 3]], h: f64) -> (Indexed, RemeshStats) {
    let (field, cells, g, mut st) = field_of(tris, h);
    let t1 = std::time::Instant::now();
    let m = contour(&field, &cells, &g);
    st.seconds_contour = t1.elapsed().as_secs_f64();
    (m, st)
}

/// Marching tetrahedra over the cells, welded by grid edge.
fn contour(field: &Sorted, cells: &[u64], g: &Grid) -> Indexed {
    let tri_edges: Vec<[Edge; 3]> = cells
        .par_iter()
        .fold(Vec::new, |mut out, &c| {
            cell_tris(c, field, g, &mut out);
            out
        })
        .flatten()
        .collect();
    let mut keys: Vec<Edge> = tri_edges.par_iter().flat_map_iter(|t| t.iter().copied()).collect();
    keys.par_sort_unstable();
    keys.dedup();
    let verts: Vec<P> = keys.par_iter().map(|&e| edge_point(e, field, g)).collect();
    let faces: Vec<[u32; 3]> = tri_edges
        .par_iter()
        .map(|t| t.map(|e| keys.binary_search(&e).expect("every edge keyed") as u32))
        .filter(|f| f[0] != f[1] && f[1] != f[2] && f[2] != f[0])
        .collect();
    Indexed { verts, faces }
}

/// The solid `tris` bound (closed) inside the box `lo`-`hi`, remeshed on cells `h` mm wide and
/// simplified to `tol` mm: the level set of the solid and the box together, so it is closed, and
/// its faces on the box lie on the box (the box's planes fall midway between grid planes, where
/// the box's distance interpolates exactly). The box's far side is rounded up to whole cells; the
/// box used is returned.
pub fn remesh_in_box(tris: &[[P; 3]], lo: P, hi: P, h: f64, tol: f64) -> (Indexed, (P, P)) {
    let n = [0, 1, 2].map(|k| ((hi[k] - lo[k]) / h).ceil().max(1.0) as i64);
    let hi = [0, 1, 2].map(|k| lo[k] + n[k] as f64 * h);
    let g = Grid { o: lo.map(|x| x - 2.5 * h), h };
    let reach = 2.0 * h;
    // Faces near the box for distances; faces over or under it for the winding up its columns.
    let pad = 3.0 * h;
    let near_box = |t: &[P; 3], zall: bool| {
        (0..3).all(|k| {
            let (a, b) = (t[0][k].min(t[1][k]).min(t[2][k]), t[0][k].max(t[1][k]).max(t[2][k]));
            (zall && k == 2) || (b >= lo[k] - pad && a <= hi[k] + pad)
        })
    };
    let local: Vec<[P; 3]> = tris.iter().filter(|t| near_box(t, false)).copied().collect();
    let column: Vec<[P; 3]> = tris.iter().filter(|t| near_box(t, true)).copied().collect();
    let cells_n = n.map(|x| x + 4);
    let columns = Columns::within(&column, &g, [0, 0], [cells_n[0], cells_n[1]]);
    let index = BoxIndex::new(&local, (4.0 * h).max(0.2));
    let wind = (columns.leaky > 0).then(|| Wind::new(column.clone()));
    let mut pts: Vec<u64> = Vec::new();
    for z in 0..=cells_n[2] {
        for y in 0..=cells_n[1] {
            for x in 0..=cells_n[0] {
                pts.push(pack([x, y, z]));
            }
        }
    }
    pts.sort_unstable();
    let vals: Vec<(u64, f32)> = pts
        .par_iter()
        .map_init(Vec::new, |near, &k| {
            let i = unpack(k);
            let p = g.at(i);
            index.query_into(sub(p, [reach; 3]), [p[0] + reach, p[1] + reach, p[2] + reach], near);
            let d = near
                .iter()
                .map(|&t| dist(p, closest_on_tri(p, &local[t as usize])))
                .fold(reach, f64::min)
                .max(1e-6 * h);
            let inside = match columns.winding(i, p[2]) {
                Some(w) => w > 0,
                None => wind.as_ref().is_some_and(|w| w.w(p) > 0.5),
            };
            let fr = if inside { -d } else { d };
            // The box's signed distance.
            let q = [0, 1, 2].map(|k| (lo[k] - p[k]).max(p[k] - hi[k]));
            let outside = len(q.map(|x| x.max(0.0)));
            let fb = outside + q[0].max(q[1]).max(q[2]).min(0.0);
            (k, fr.max(fb) as f32)
        })
        .collect();
    let field = Sorted::from_sorted(vals);
    let mut cells: Vec<u64> = Vec::new();
    for z in 0..cells_n[2] {
        for y in 0..cells_n[1] {
            for x in 0..cells_n[0] {
                cells.push(pack([x, y, z]));
            }
        }
    }
    let m = contour(&field, &cells, &g);
    // Points on the box stay on it, for an exact seam with the surface cut at the box.
    let on_box = |p: &P| (0..3).any(|k| (p[k] - lo[k]).abs() < 1e-9 || (p[k] - hi[k]).abs() < 1e-9);
    let locked: Vec<bool> = m.verts.iter().map(on_box).collect();
    (super::simplify::simplify_locked(&m, &locked, tol), (lo, hi))
}

/// Cells per block side for [`remesh_simplified`].
const BLOCK: i64 = 48;

/// The remesh simplified to `tol` mm as it is built: the grid is contoured in blocks, each
/// simplified on its own thread with the points it shares with its neighbours held, and the whole
/// is then simplified once more across the block borders. The full-resolution surface is never
/// held at once.
pub fn remesh_simplified(tris: &[[P; 3]], h: f64, tol: f64) -> (Indexed, RemeshStats) {
    let (field, cells, g, mut st) = field_of(tris, h);
    let t1 = std::time::Instant::now();
    let block_of = |c: u64| {
        let i = unpack(c);
        pack(i.map(|x| x.div_euclid(BLOCK)))
    };
    let mut keyed: Vec<(u64, u64)> = cells.par_iter().map(|&c| (block_of(c), c)).collect();
    keyed.par_sort_unstable();
    let mut runs: Vec<&[(u64, u64)]> = Vec::new();
    let mut start = 0;
    for i in 1..=keyed.len() {
        if i == keyed.len() || keyed[i].0 != keyed[start].0 {
            runs.push(&keyed[start..i]);
            start = i;
        }
    }
    // Each block: its faces as either a shared edge key or a vertex of its own.
    #[derive(Clone, Copy)]
    enum Ref {
        Shared(Edge),
        Own(u32),
    }
    let shared = |e: Edge| {
        let (a, b) = (unpack(e.0), unpack(e.1));
        (0..3).any(|k| a[k] == b[k] && a[k].rem_euclid(BLOCK) == 0)
    };
    let pieces: Vec<(Vec<[Ref; 3]>, Vec<P>, usize)> = runs
        .par_iter()
        .map(|run| {
            let mut te = Vec::new();
            for &(_, c) in run.iter() {
                cell_tris(c, &field, &g, &mut te);
            }
            let raw = te.len();
            let mut keys: Vec<Edge> = te.iter().flatten().copied().collect();
            keys.sort_unstable();
            keys.dedup();
            let verts: Vec<P> = keys.iter().map(|&e| edge_point(e, &field, &g)).collect();
            let faces: Vec<[u32; 3]> = te
                .iter()
                .map(|t| t.map(|e| keys.binary_search(&e).expect("every edge keyed") as u32))
                .filter(|f| f[0] != f[1] && f[1] != f[2] && f[2] != f[0])
                .collect();
            let locked: Vec<bool> = keys.iter().map(|&e| shared(e)).collect();
            let m = super::simplify::simplify_locked(&Indexed { verts, faces }, &locked, tol);
            // Renumber: shared points by key, the rest by their own list.
            let mut own = vec![u32::MAX; m.verts.len()];
            let mut own_verts = Vec::new();
            let refs = m
                .faces
                .iter()
                .map(|f| {
                    f.map(|v| {
                        if v < keys.len() as u32 && locked[v as usize] {
                            Ref::Shared(keys[v as usize])
                        } else {
                            if own[v as usize] == u32::MAX {
                                own[v as usize] = own_verts.len() as u32;
                                own_verts.push(m.verts[v as usize]);
                            }
                            Ref::Own(own[v as usize])
                        }
                    })
                })
                .collect();
            (refs, own_verts, raw)
        })
        .collect();
    st.raw_triangles = pieces.iter().map(|p| p.2).sum();
    let mut shared_keys: Vec<Edge> = pieces
        .iter()
        .flat_map(|p| p.0.iter().flatten())
        .filter_map(|r| if let Ref::Shared(e) = r { Some(*e) } else { None })
        .collect();
    shared_keys.par_sort_unstable();
    shared_keys.dedup();
    let mut verts: Vec<P> = shared_keys.par_iter().map(|&e| edge_point(e, &field, &g)).collect();
    let mut faces = Vec::new();
    for (refs, own, _) in pieces {
        let base = verts.len() as u32;
        verts.extend(own);
        faces.extend(refs.into_iter().map(|f| {
            f.map(|r| match r {
                Ref::Shared(e) => shared_keys.binary_search(&e).expect("a shared key") as u32,
                Ref::Own(i) => base + i,
            })
        }));
    }
    trace(&format!("blocks {} -> {} tris {:.1}s", st.raw_triangles, faces.len(), t1.elapsed().as_secs_f64()));
    let (m, _) = super::simplify::simplify(&Indexed { verts, faces }, tol);
    st.seconds_contour = t1.elapsed().as_secs_f64();
    (m, st)
}

/// The signed distance on the band (grown over holes), the band's cells and the grid.
fn field_of(tris: &[[P; 3]], h: f64) -> (Sorted, Vec<u64>, Grid, RemeshStats) {
    let mut st = RemeshStats { cell_mm: h, ..Default::default() };
    let t0 = std::time::Instant::now();
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for t in tris {
        for v in t {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
    }
    // Off the model's box by an odd fraction of a cell, so flat faces on round coordinates do not
    // lie on grid planes.
    let g = Grid { o: lo.map(|x| x - 4.0 * h + 0.137 * h), h };
    let span = (0..3).map(|k| ((hi[k] - lo[k]) / h) as u64 + 16).max().unwrap_or(0);
    assert!(span < (1 << BITS), "grid too fine for the model");
    // Index cells a few faces wide: a face spanning too many cells goes on the index's list
    // checked by every query.
    let mut edges: Vec<f64> = tris.iter().step_by((tris.len() / 4096).max(1)).map(|t| dist(t[0], t[1])).collect();
    let mid = edges.len() / 2;
    let typical = if edges.is_empty() { h } else { *edges.select_nth_unstable_by(mid, f64::total_cmp).1 };
    let index_cell = (4.0 * h).max(2.0 * typical).max(0.2);
    let (columns, index) = rayon::join(|| Columns::new(tris, &g), || BoxIndex::new(tris, index_cell));
    st.leaky_columns = columns.leaky;
    // The winding number only where a column's crossings do not balance (an open mesh).
    let wind = (columns.leaky > 0).then(|| Wind::new(tris.to_vec()));
    let reach = 2.0 * h;
    let value = |near: &mut Vec<u32>, i: [i64; 3]| -> f32 {
        let p = g.at(i);
        index.query_into(sub(p, [reach; 3]), [p[0] + reach, p[1] + reach, p[2] + reach], near);
        let d = near.iter().map(|&t| dist(p, closest_on_tri(p, &tris[t as usize]))).fold(reach, f64::min).max(1e-6 * h);
        let inside = match columns.winding(i, p[2]) {
            Some(w) => w > 0,
            None => wind.as_ref().is_some_and(|w| w.w(p) > 0.5),
        };
        if inside { -d as f32 } else { d as f32 }
    };

    let mut cells: Vec<u64> = tris
        .par_iter()
        .fold(Vec::new, |mut acc, t| {
            cells_of(&g, *t, &mut acc);
            acc
        })
        .flatten()
        .collect();
    cells.par_sort_unstable();
    cells.dedup();
    st.band_cells = cells.len();
    trace(&format!("band {} cells {:.1}s", cells.len(), t0.elapsed().as_secs_f64()));
    let mut in_band = Sorted::from_sorted(cells.iter().map(|&c| (c, 0.0)).collect());
    let mut field = Sorted::default();
    let mut frontier = cells.clone();
    while !frontier.is_empty() {
        let mut need: Vec<u64> = frontier
            .par_iter()
            .flat_map_iter(|&c| {
                let i = unpack(c);
                (0..8).map(move |b| pack([i[0] + (b & 1), i[1] + ((b >> 1) & 1), i[2] + ((b >> 2) & 1)]))
            })
            .filter(|&k| !field.contains(k))
            .collect();
        need.par_sort_unstable();
        need.dedup();
        let vals: Vec<(u64, f32)> =
            need.par_iter().map_init(Vec::new, |near, &k| (k, value(near, unpack(k)))).collect();
        trace(&format!("values {} {:.1}s", vals.len(), t0.elapsed().as_secs_f64()));
        field.merge(vals);
        trace(&format!("stored {:.1}s", t0.elapsed().as_secs_f64()));
        // Faces of the frontier cells with a sign change across them lead to new cells.
        let mut grown: Vec<u64> = frontier
            .par_iter()
            .flat_map_iter(|&c| {
                let i = unpack(c);
                let corner =
                    |b: i64| field.get(pack([i[0] + (b & 1), i[1] + ((b >> 1) & 1), i[2] + ((b >> 2) & 1)])) < 0.0;
                let mut out = Vec::new();
                for axis in 0..3 {
                    for side in 0..2i64 {
                        let bits: Vec<i64> = (0..8).filter(|b| (b >> axis) & 1 == side).collect();
                        let s: Vec<bool> = bits.iter().map(|&b| corner(b)).collect();
                        if s.iter().any(|&x| x) && s.iter().any(|&x| !x) {
                            let mut n = i;
                            n[axis] += if side == 1 { 1 } else { -1 };
                            out.push(pack(n));
                        }
                    }
                }
                out
            })
            .filter(|&k| !in_band.contains(k))
            .collect();
        grown.par_sort_unstable();
        grown.dedup();
        st.grown_cells += grown.len();
        in_band.merge(grown.iter().map(|&c| (c, 0.0)).collect());
        cells.extend(grown.iter().copied());
        frontier = grown;
    }
    field.settle();
    st.field_points = field.len();
    st.seconds_field = t0.elapsed().as_secs_f64();
    (field, cells, g, st)
}

type Edge = (u64, u64);
const TETS: [[i64; 4]; 6] = [[0, 1, 3, 7], [0, 1, 5, 7], [0, 2, 3, 7], [0, 2, 6, 7], [0, 4, 5, 7], [0, 4, 6, 7]];

/// Marching tetrahedra in one cell: its triangles as edge keys (grid point pairs), facing out.
fn cell_tris(c: u64, field: &Sorted, g: &Grid, out: &mut Vec<[Edge; 3]>) {
    let i = unpack(c);
    let key = |b: i64| pack([i[0] + (b & 1), i[1] + ((b >> 1) & 1), i[2] + ((b >> 2) & 1)]);
    for tet in TETS {
        let k = tet.map(key);
        let f = k.map(|x| field.get(x));
        let ins: Vec<usize> = (0..4).filter(|&j| f[j] < 0.0).collect();
        let outs: Vec<usize> = (0..4).filter(|&j| f[j] >= 0.0).collect();
        if ins.is_empty() || outs.is_empty() {
            continue;
        }
        let e = |a: usize, b: usize| (k[a].min(k[b]), k[a].max(k[b]));
        let pos = |a: usize, b: usize| {
            let (pa, pb) = (g.at(unpack(k[a])), g.at(unpack(k[b])));
            let t = f64::from(f[a] / (f[a] - f[b])).clamp(EDGE_MARGIN, 1.0 - EDGE_MARGIN);
            [0, 1, 2].map(|q| pa[q] + (pb[q] - pa[q]) * t)
        };
        let toward = {
            let ci =
                ins.iter().map(|&j| g.at(unpack(k[j]))).fold([0.0; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]]);
            let co =
                outs.iter().map(|&j| g.at(unpack(k[j]))).fold([0.0; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]]);
            let (ni, no) = (ins.len() as f64, outs.len() as f64);
            [0, 1, 2].map(|q| co[q] / no - ci[q] / ni)
        };
        let mut emit = |mut t: [(usize, usize); 3]| {
            let p = t.map(|(a, b)| pos(a, b));
            if dot(cross(sub(p[1], p[0]), sub(p[2], p[0])), toward) < 0.0 {
                t.swap(1, 2);
            }
            out.push(t.map(|(a, b)| e(a, b)));
        };
        match (ins.len(), outs.len()) {
            (1, 3) => emit([(ins[0], outs[0]), (ins[0], outs[1]), (ins[0], outs[2])]),
            (3, 1) => emit([(ins[0], outs[0]), (ins[1], outs[0]), (ins[2], outs[0])]),
            _ => {
                let (a, b, c, d) = (ins[0], ins[1], outs[0], outs[1]);
                emit([(a, c), (a, d), (b, d)]);
                emit([(a, c), (b, d), (b, c)]);
            }
        }
    }
}

/// The surface point on a grid edge.
fn edge_point(e: Edge, field: &Sorted, g: &Grid) -> P {
    let (fa, fb) = (field.get(e.0), field.get(e.1));
    let (pa, pb) = (g.at(unpack(e.0)), g.at(unpack(e.1)));
    let t = f64::from(fa / (fa - fb)).clamp(EDGE_MARGIN, 1.0 - EDGE_MARGIN);
    [0, 1, 2].map(|q| pa[q] + (pb[q] - pa[q]) * t)
}

#[cfg(test)]
mod tests {
    use super::super::mesh::{self, tests::cube};
    use super::*;

    #[test]
    fn overlapping_cubes_remesh_to_one_closed_shell_of_their_union_volume() {
        let mut t = cube([0.0; 3], [2.0; 3]);
        t.extend(cube([1.0; 3], [3.0; 3]));
        let (m, _) = remesh(&t, 0.1);
        let c = mesh::check(&m, true);
        assert!(c.watertight(), "{c:?}");
        assert_eq!(c.shells, 1);
        assert!((c.volume - 15.0).abs() < 0.3, "{}", c.volume);
    }

    #[test]
    fn a_cube_with_a_missing_face_is_closed_over() {
        let mut t = cube([0.0; 3], [2.0; 3]);
        t.truncate(10);
        let (m, st) = remesh(&t, 0.1);
        let c = mesh::check(&m, true);
        assert!(c.watertight() && c.shells == 1, "{c:?}");
        assert!(st.grown_cells > 0);
        assert!((c.volume - 8.0).abs() < 0.5, "{}", c.volume);
    }

    #[test]
    fn the_blockwise_remesh_is_watertight_across_its_blocks_and_far_smaller() {
        // 6 mm at 0.05 mm is 120 cells a side: three blocks of 48 each way.
        let mut t = cube([0.0; 3], [6.0; 3]);
        t.extend(cube([3.0, 3.0, 3.0], [7.3, 6.5, 6.7]));
        let (full, _) = remesh(&t, 0.05);
        let (m, st) = remesh_simplified(&t, 0.05, 0.005);
        let c = mesh::check(&m, true);
        assert!(c.watertight() && c.shells == 1, "{c:?}");
        assert_eq!(st.raw_triangles, full.faces.len());
        assert!(m.faces.len() * 8 < full.faces.len(), "{} of {}", m.faces.len(), full.faces.len());
        let union = 216.0 + 4.3 * 3.5 * 3.7 - 3.0 * 3.0 * 3.0;
        assert!((c.volume - union).abs() < 1.0, "{} vs {union}", c.volume);
    }

    #[test]
    fn the_solid_inside_a_box_is_closed_and_flat_on_the_box() {
        let t = cube([0.0; 3], [4.0; 3]);
        let (m, (lo, hi)) = remesh_in_box(&t, [1.0; 3], [3.0, 3.0, 5.0], 0.1, 0.005);
        let c = mesh::check(&m, true);
        assert!(c.watertight() && c.shells == 1, "{c:?}");
        let expected = (hi[0] - lo[0]) * (hi[1] - lo[1]) * (4.0 - lo[2]);
        assert!((c.volume - expected).abs() < 0.05, "{} vs {expected}", c.volume);
    }

    #[test]
    fn a_hollow_cube_keeps_its_void() {
        let mut t = cube([0.0; 3], [4.0; 3]);
        let mut inner = cube([1.0; 3], [3.0; 3]);
        for f in &mut inner {
            f.swap(1, 2);
        }
        t.extend(inner);
        let (m, _) = remesh(&t, 0.1);
        let c = mesh::check(&m, true);
        assert!(c.watertight() && c.shells == 2, "{c:?}");
        assert!((c.volume - 56.0).abs() < 1.0, "{}", c.volume);
    }
}
