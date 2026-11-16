//! Twin split: splits a supported file whose supports are welded into the model (one shell, no
//! seam to cut along) by finding the same sculpt without supports elsewhere in the catalogue
//! (its twin), registering the twin onto it, and taking every triangle on the twin's surface as
//! model and the rest as support.
//!
//! The lookup ignores file names, folders, position, rotation and uniform scale:
//!
//! 1. **Keys.** Each triangle with three edge neighbours gets a key from five scale-free shape
//!    measures: its two shorter edges over its longest (in winding order from the longest edge),
//!    and each neighbour's area over its own. They are binned in log space ([`BIN`]) and hashed.
//!    The key carries the triangle's longest edge (log, [`SCALE_STEP`]) so matches can agree on
//!    a scale.
//! 2. **Sketch.** A model's fingerprint is the [`SKETCH_SIZE`] smallest hashes among its keys that
//!    occur once in it (bottom-k sampling), each with its scale. Both files of a twin pair pick the
//!    same sampled triangles, since the choice depends only on the hash.
//! 3. **Index.** Sketches are inverted (hash to model and scale). A supported file probes every one
//!    of its keys (with the neighbouring bins of measures near a bin edge, [`PROBE_MARGIN`]), and a
//!    candidate's score is the number of its sketch entries hit at one consistent scale.
//! 4. **Verify.** For a candidate the twin is loaded, all keys are matched, a similarity transform
//!    (rotation, translation, uniform scale) is found by RANSAC over single matched triangles
//!    and refined on the inliers, and each triangle of the supported file is labelled model when
//!    its corners and centre lie within [`ON_SURFACE_MM`] of the twin's surface. Supported files
//!    are re-exported with their corners moved by up to about 0.01 mm, so keys are coarse and
//!    matching is by shape, not by exact corners.
//! 5. **Guards.** The twin must be covered with no large gap ([`MIN_TWIN_COVERAGE`],
//!    [`MAX_TWIN_GAP`]), so a twin with a part the supported file lacks (a variant with another
//!    head) is refused. Support parts lying inside the twin (a hollowed file's inner wall and
//!    drain holes) are model ([`inner_walls`]). The support side must meet the model only in small
//!    loops ([`support_like`]), so a twin lacking a part the supported file has (a body without
//!    its head) does not turn that part into support.
//!
//! The result is a per-triangle support mask over the supported file's triangles, as a welded
//! split gives one: the cut leaves both sides open along the seams, and caps are rebuilt from the
//! mask on demand.

use crate::geometry::Triangle;
use crate::geometry::support_reader::math::{
    BoxIndex, P, add, closest_on_tri, cross, dist, dot, eig3, len, mul, sub, unit,
};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// Increment when keys, sketches or the twin split change, so stored sketches and the splits
/// made from them are rebuilt.
pub const CURRENT_TWIN_VERSION: i32 = 1;

/// Sketch entries kept per model.
pub const SKETCH_SIZE: usize = 64;
/// Width of a shape measure's bin (natural log units).
pub const BIN: f64 = 0.05;
/// A measure within this share of a bin from a bin edge is also probed in the next bin. Bins and
/// margin tolerate a 1.5% error in a measure: supported files are re-exported with their corners
/// moved by up to about 0.01 mm, so their small triangles' shapes are not exact.
pub const PROBE_MARGIN: f64 = 0.3;
/// Step of a key's stored scale (natural log of the longest edge).
pub const SCALE_STEP: f64 = 1e-3;
/// Sketch hits of one candidate agree on a scale when within this many steps of each other.
pub const SCALE_WINDOW_STEPS: i32 = 15;
/// Edges shorter than this (mm) are degenerate and not keyed.
pub const MIN_KEY_EDGE_MM: f64 = 1e-4;
/// Sketches sample the triangles whose shortest edge is at least this quantile of the model's:
/// the larger triangles, whose shapes the corner noise moves least.
pub const SKETCH_EDGE_QUANTILE: f64 = 0.5;
/// The longest edge must be longer than the next by this share to fix the key's starting edge.
pub const ROTATION_MARGIN: f64 = 0.02;
/// Triangles thinner than this (area over longest edge squared) are not keyed.
pub const MIN_KEY_ROUNDNESS: f64 = 1e-3;
/// A candidate needs this share of its sketch hit at one scale to be verified.
pub const MIN_SKETCH_SHARE: f64 = 0.3;
/// ... and at least this many hits.
pub const MIN_SKETCH_HITS: usize = 6;
/// Candidates verified per supported file, best first.
pub const MAX_CANDIDATES: usize = 4;
/// A supported triangle within this distance (mm, in the supported file's units) of the twin's
/// surface is model.
pub const ON_SURFACE_MM: f64 = 0.005;
/// Matched triangles agree with a transform when their corners land within this (mm).
pub const INLIER_MM: f64 = 0.01;
/// Share of the twin's surface the supported file's model side must cover.
pub const MIN_TWIN_COVERAGE: f64 = 0.9;
/// Largest connected part of the twin's surface (share of its area) the supported file may lack.
/// Supports' contacts, drain holes and keys leave small scattered gaps; a part the supported file
/// lacks (another head, a weapon) is one large gap.
pub const MAX_TWIN_GAP: f64 = 0.01;
/// Fewest matched triangles agreeing on the transform.
pub const MIN_INLIERS: usize = 20;
/// Model islands (edge-connected) of at most this many triangles and this share of the model's
/// area, inside support, are support lying on the twin's surface by chance (contact rims).
pub const ISLAND_MAX_TRIANGLES: usize = 64;
pub const ISLAND_MAX_SHARE: f64 = 1e-4;
/// Support islands no larger than this, inside model, are model the boolean union retriangulated
/// slightly off the surface.
pub const HOLE_MAX_TRIANGLES: usize = 8;
pub const HOLE_MAX_SHARE: f64 = 1e-5;

const NONE: u32 = u32::MAX;

/// A mesh welded by exact corner positions, with each triangle's neighbour across each edge
/// (edge k runs from corner k to k+1; [`NONE`] when open or shared by more than two).
pub struct Mesh {
    pub tris: Vec<[P; 3]>,
    pub corners: Vec<[u32; 3]>,
    pub nb: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn new(triangles: &[Triangle]) -> Self {
        let mut ids: FxHashMap<[u32; 3], u32> = FxHashMap::default();
        ids.reserve(triangles.len() / 2 + 16);
        let mut corners = Vec::with_capacity(triangles.len());
        let mut tris = Vec::with_capacity(triangles.len());
        for t in triangles {
            let vs = [t.v0, t.v1, t.v2];
            corners.push(vs.map(|v| {
                let n = ids.len() as u32;
                *ids.entry([v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]).or_insert(n)
            }));
            tris.push(vs.map(|v| [f64::from(v.x), f64::from(v.y), f64::from(v.z)]));
        }
        let mut edges: FxHashMap<(u32, u32), u32> = FxHashMap::default();
        edges.reserve(triangles.len() * 3);
        for (i, c) in corners.iter().enumerate() {
            for k in 0..3 {
                let e = (c[k], c[(k + 1) % 3]);
                edges.entry(e).and_modify(|x| *x = NONE).or_insert(i as u32);
            }
        }
        let nb = corners
            .iter()
            .enumerate()
            .map(|(i, c)| {
                [0, 1, 2].map(|k| match edges.get(&(c[(k + 1) % 3], c[k])) {
                    Some(&j) if j != NONE && j as usize != i => j,
                    _ => NONE,
                })
            })
            .collect();
        Mesh { tris, corners, nb }
    }

    fn area(&self, i: usize) -> f64 {
        tri_area(&self.tris[i])
    }

    /// Edge-connected groups of triangles with the same flag: (group of each triangle, groups).
    fn groups(&self, flag: &[bool]) -> (Vec<u32>, Vec<Vec<u32>>) {
        let mut of = vec![NONE; self.tris.len()];
        let mut groups = Vec::new();
        let mut stack = Vec::new();
        for s in 0..self.tris.len() {
            if of[s] != NONE {
                continue;
            }
            let g = groups.len() as u32;
            let mut members = vec![s as u32];
            of[s] = g;
            stack.push(s as u32);
            while let Some(i) = stack.pop() {
                for &j in &self.nb[i as usize] {
                    if j != NONE && of[j as usize] == NONE && flag[j as usize] == flag[s] {
                        of[j as usize] = g;
                        members.push(j);
                        stack.push(j);
                    }
                }
            }
            groups.push(members);
        }
        (of, groups)
    }
}

fn tri_area(t: &[P; 3]) -> f64 {
    0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0])))
}

fn centroid(t: &[P; 3]) -> P {
    mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0)
}

/// A triangle's key: the hash of its binned shape, its scale (log of its longest edge in
/// [`SCALE_STEP`]s), the triangle and the corner its key starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub hash: u64,
    pub scale: i32,
    pub tri: u32,
    pub start: u8,
}

fn mix(mut z: u64) -> u64 {
    // splitmix64 finaliser: stable across platforms and versions.
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn hash_bins(bins: &[i32; 5]) -> u64 {
    bins.iter().fold(0x9e37_79b9_7f4a_7c15u64, |h, &b| mix(h ^ u64::from(b as u32)))
}

/// The shape measures of triangle `i` keyed from corner `r`, with its longest edge, or None when
/// it cannot be keyed (an open or thin neighbour, a tiny edge).
fn measures(m: &Mesh, i: usize, r: usize) -> Option<([f64; 5], f64)> {
    let t = &m.tris[i];
    let q = [0, 1, 2].map(|k| t[(r + k) % 3]);
    let l = [0, 1, 2].map(|k| len(sub(q[(k + 1) % 3], q[k])));
    let a = tri_area(t);
    if l.iter().any(|&x| x < MIN_KEY_EDGE_MM) || a < MIN_KEY_ROUNDNESS * l[0] * l[0] {
        return None;
    }
    let mut f = [(l[1] / l[0]).ln(), (l[2] / l[0]).ln(), 0.0, 0.0, 0.0];
    for k in 0..3 {
        let j = m.nb[i][(r + k) % 3];
        if j == NONE {
            return None;
        }
        let an = m.area(j as usize);
        if an < 1e-12 {
            return None;
        }
        f[2 + k] = (an / a).ln();
    }
    Some((f, l[0]))
}

/// Starting corners for triangle `i`: the start of its longest edge, and the start of the next
/// longest when within `margin` of it (None for that when not).
fn starts(t: &[P; 3], margin: f64) -> [Option<usize>; 2] {
    let l = [0, 1, 2].map(|k| len(sub(t[(k + 1) % 3], t[k])));
    let mut o = [0usize, 1, 2];
    o.sort_by(|&a, &b| l[b].total_cmp(&l[a]));
    let close = l[o[1]] > l[o[0]] * (1.0 - margin);
    [Some(o[0]), close.then_some(o[1])]
}

fn scale_of(longest: f64) -> i32 {
    (longest.ln() / SCALE_STEP).round() as i32
}

/// Every triangle's canonical key (no neighbouring bins), skipping triangles whose starting
/// corner is ambiguous.
pub fn canonical_keys(m: &Mesh) -> Vec<Key> {
    (0..m.tris.len())
        .into_par_iter()
        .filter_map(|i| {
            let [Some(r), None] = starts(&m.tris[i], ROTATION_MARGIN) else {
                return None;
            };
            let (f, l0) = measures(m, i, r)?;
            let bins = f.map(|x| (x / BIN).floor() as i32);
            Some(Key { hash: hash_bins(&bins), scale: scale_of(l0), tri: i as u32, start: r as u8 })
        })
        .collect()
}

/// Every key a triangle might have in another copy of the mesh: both starting corners when close,
/// and the neighbouring bin of every measure near a bin edge.
pub fn probe_keys(m: &Mesh) -> Vec<Key> {
    (0..m.tris.len())
        .into_par_iter()
        .flat_map_iter(|i| {
            let mut out = Vec::new();
            let margin = ROTATION_MARGIN * 2.0;
            for r in starts(&m.tris[i], margin).into_iter().flatten() {
                let Some((f, l0)) = measures(m, i, r) else { continue };
                let mut combos: Vec<[i32; 5]> = vec![[0; 5]];
                for (k, &x) in f.iter().enumerate() {
                    let v = x / BIN;
                    let b = v.floor();
                    let fr = v - b;
                    let b = b as i32;
                    let alt = if fr < PROBE_MARGIN {
                        Some(b - 1)
                    } else if fr > 1.0 - PROBE_MARGIN {
                        Some(b + 1)
                    } else {
                        None
                    };
                    let n = combos.len();
                    for c in combos.iter_mut() {
                        c[k] = b;
                    }
                    if let Some(a) = alt {
                        for c in 0..n {
                            let mut x = combos[c];
                            x[k] = a;
                            combos.push(x);
                        }
                    }
                }
                let scale = scale_of(l0);
                out.extend(combos.iter().map(|b| Key { hash: hash_bins(b), scale, tri: i as u32, start: r as u8 }));
            }
            out
        })
        .collect()
}

/// Keys that occur once among `keys`, by hash.
fn unique(keys: &[Key]) -> FxHashMap<u64, Key> {
    let mut map: FxHashMap<u64, Option<Key>> = FxHashMap::default();
    map.reserve(keys.len());
    for k in keys {
        map.entry(k.hash).and_modify(|e| *e = None).or_insert(Some(*k));
    }
    map.into_iter().filter_map(|(h, k)| k.map(|k| (h, k))).collect()
}

/// A model's fingerprint: its smallest unique key hashes with their scales.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sketch {
    pub entries: Vec<(u64, i32)>,
    /// Unique keys the model has (what the entries are sampled from).
    pub keys: u32,
}

impl Sketch {
    pub fn of(m: &Mesh) -> Self {
        let keys = unique(&canonical_keys(m));
        let shortest = |k: &Key| {
            let t = &m.tris[k.tri as usize];
            (0..3).map(|j| len(sub(t[(j + 1) % 3], t[j]))).fold(f64::MAX, f64::min)
        };
        let mut lengths: Vec<f64> = keys.values().map(shortest).collect();
        lengths.sort_unstable_by(f64::total_cmp);
        let floor = lengths.get(((lengths.len().max(1) - 1) as f64 * SKETCH_EDGE_QUANTILE) as usize).copied();
        let mut entries: Vec<(u64, i32)> =
            keys.values().filter(|k| floor.is_none_or(|f| shortest(k) >= f)).map(|k| (k.hash, k.scale)).collect();
        entries.sort_unstable();
        entries.truncate(SKETCH_SIZE);
        Sketch { entries, keys: keys.len() as u32 }
    }

    /// Stored form: version (u8), key count (u32), entry count (u16), then each entry's hash (u64)
    /// and scale (i16), little endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(7 + self.entries.len() * 10);
        b.push(CURRENT_TWIN_VERSION as u8);
        b.extend(self.keys.to_le_bytes());
        b.extend((self.entries.len() as u16).to_le_bytes());
        for &(h, s) in &self.entries {
            b.extend(h.to_le_bytes());
            b.extend((s.clamp(i16::MIN as i32, i16::MAX as i32) as i16).to_le_bytes());
        }
        b
    }

    /// Reads a stored sketch; None when it is malformed or from another version.
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 7 || b[0] != CURRENT_TWIN_VERSION as u8 {
            return None;
        }
        let keys = u32::from_le_bytes(b[1..5].try_into().ok()?);
        let n = u16::from_le_bytes(b[5..7].try_into().ok()?) as usize;
        if b.len() != 7 + n * 10 {
            return None;
        }
        let entries = (0..n)
            .map(|i| {
                let o = 7 + i * 10;
                let h = u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
                let s = i16::from_le_bytes(b[o + 8..o + 10].try_into().unwrap());
                (h, i32::from(s))
            })
            .collect();
        Some(Sketch { entries, keys })
    }
}

/// Stored probe keys keep only hashes below this (1/256 of the hash space): a sketch holds a model's
/// smallest hashes, so a twin with 16,000 or more unique keys has its whole sketch below it, and
/// a smaller one the part of it that is.
pub const STORED_PROBE_BOUND: u64 = 1 << 56;
/// A stored key is its hash shifted down by this, so it fits a u32 (the low 24 bits are dropped:
/// at most a few chance hits more, far below [`MIN_SKETCH_HITS`] at one scale).
const STORED_HASH_SHIFT: u32 = 24;

/// A supported model's probe keys, compacted to keep in the database while it waits for a twin: so
/// it is probed again against new sketches without loading its file ([`Index::candidates_stored`]).
/// Each key below [`STORED_PROBE_BOUND`] as its shifted hash and scale, sorted and deduplicated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredProbes {
    pub keys: Vec<(u32, i16)>,
}

impl StoredProbes {
    pub fn of(probes: &[Key]) -> Self {
        let mut keys: Vec<(u32, i16)> = probes
            .iter()
            .filter(|k| k.hash < STORED_PROBE_BOUND)
            .map(|k| ((k.hash >> STORED_HASH_SHIFT) as u32, k.scale.clamp(i16::MIN as i32, i16::MAX as i32) as i16))
            .collect();
        keys.par_sort_unstable();
        keys.dedup();
        StoredProbes { keys }
    }

    /// Stored form: version (u8), key count (u32), then each key's hash (u32) and scale (i16),
    /// little endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(5 + self.keys.len() * 6);
        b.push(CURRENT_TWIN_VERSION as u8);
        b.extend((self.keys.len() as u32).to_le_bytes());
        for &(h, s) in &self.keys {
            b.extend(h.to_le_bytes());
            b.extend(s.to_le_bytes());
        }
        b
    }

    /// Reads stored keys; None when malformed or from another version.
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 5 || b[0] != CURRENT_TWIN_VERSION as u8 {
            return None;
        }
        let n = u32::from_le_bytes(b[1..5].try_into().ok()?) as usize;
        if b.len() != 5 + n * 6 {
            return None;
        }
        let keys = b[5..]
            .as_chunks::<6>()
            .0
            .iter()
            .map(|c| (u32::from_le_bytes([c[0], c[1], c[2], c[3]]), i16::from_le_bytes([c[4], c[5]])))
            .collect();
        Some(StoredProbes { keys })
    }
}

/// A model found by its sketch: how many of its sketch entries were hit at one scale, and that
/// scale (supported over twin, as a log).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub model: u32,
    pub hits: usize,
    pub sketch_len: usize,
    pub log_scale: f64,
}

impl Candidate {
    pub fn share(&self) -> f64 {
        self.hits as f64 / self.sketch_len.max(1) as f64
    }
}

/// Sketches inverted: each hash's models and scales, as one sorted list (16 bytes a posting, about
/// 100 MB for a catalogue of 100,000 models).
#[derive(Default)]
pub struct Index {
    /// (hash, model, scale), sorted once every sketch is in ([`Index::add`]).
    postings: Vec<(u64, u32, i32)>,
    sorted: bool,
    lens: FxHashMap<u32, usize>,
    /// Each model's sketch entries stored probe keys can hit (below [`STORED_PROBE_BOUND`]).
    lens_stored: FxHashMap<u32, usize>,
}

impl Index {
    pub fn add(&mut self, model: u32, sketch: &Sketch) {
        self.postings.extend(sketch.entries.iter().map(|&(h, s)| (h, model, s)));
        self.sorted = false;
        self.lens.insert(model, sketch.entries.len());
        self.lens_stored.insert(model, sketch.entries.iter().filter(|e| e.0 < STORED_PROBE_BOUND).count());
    }

    pub fn models(&self) -> usize {
        self.lens.len()
    }

    pub fn postings(&self) -> usize {
        self.postings.len()
    }

    /// Whether stored probe keys can find `model`: enough of its sketch lies below
    /// [`STORED_PROBE_BOUND`] (a model with fewer than about 1,500 unique keys may not).
    pub fn reachable_by_stored(&self, model: u32) -> bool {
        self.lens_stored.get(&model).is_some_and(|&n| n >= MIN_SKETCH_HITS)
    }

    /// Sorts the postings once the sketches are in (a candidates call does it when needed).
    pub fn sort(&mut self) {
        if !self.sorted {
            self.postings.par_sort_unstable();
            self.sorted = true;
        }
    }

    fn list(&self, hash: u64) -> &[(u64, u32, i32)] {
        let lo = self.postings.partition_point(|p| p.0 < hash);
        let hi = lo + self.postings[lo..].partition_point(|p| p.0 == hash);
        &self.postings[lo..hi]
    }

    /// Every model with sketch hits from `probes`, best first: hits counted once per sketch entry,
    /// at the scale most of them agree on. All are returned; [`Candidate::share`] and
    /// [`MIN_SKETCH_SHARE`] pick the ones worth verifying.
    pub fn candidates(&mut self, probes: &[Key]) -> Vec<Candidate> {
        self.sort();
        self.candidates_sorted(probes)
    }

    /// [`Index::candidates`] on an index already sorted ([`Index::sort`]), shared between threads.
    pub fn candidates_sorted(&self, probes: &[Key]) -> Vec<Candidate> {
        assert!(self.sorted || self.postings.is_empty(), "the index is sorted before it is probed");
        let mut hits: FxHashMap<u32, Vec<(i32, u64)>> = FxHashMap::default();
        let mut seen: FxHashSet<(u64, i32)> = FxHashSet::default();
        for k in probes {
            let list = self.list(k.hash);
            if list.is_empty() || !seen.insert((k.hash, k.scale)) {
                continue;
            }
            for &(_, model, s) in list {
                hits.entry(model).or_default().push((k.scale - s, k.hash));
            }
        }
        self.rank(hits, &self.lens)
    }

    /// [`Index::candidates_sorted`] for stored probe keys ([`StoredProbes`]): only sketch entries
    /// below [`STORED_PROBE_BOUND`] can be hit, so a candidate's share is of those entries.
    pub fn candidates_stored(&self, probes: &StoredProbes) -> Vec<Candidate> {
        assert!(self.sorted || self.postings.is_empty(), "the index is sorted before it is probed");
        let mut hits: FxHashMap<u32, Vec<(i32, u64)>> = FxHashMap::default();
        // The keys are sorted, so each search gallops on from where the last one ended: a few nearby
        // reads rather than a binary search over the whole index.
        let mut at = 0usize;
        for &(h, scale) in &probes.keys {
            let lo = u64::from(h) << STORED_HASH_SHIFT;
            let hi = lo + (1u64 << STORED_HASH_SHIFT);
            let a = at + gallop(&self.postings[at..], |p| p.0 < lo);
            let b = a + gallop(&self.postings[a..], |p| p.0 < hi);
            at = a;
            for &(hash, model, s) in &self.postings[a..b] {
                hits.entry(model).or_default().push((i32::from(scale) - s, hash));
            }
        }
        self.rank(hits, &self.lens_stored)
    }

    /// Each model's hits ranked: counted once per sketch entry, at the scale most agree on.
    fn rank(&self, hits: FxHashMap<u32, Vec<(i32, u64)>>, lens: &FxHashMap<u32, usize>) -> Vec<Candidate> {
        let mut out: Vec<Candidate> = hits
            .into_iter()
            .map(|(model, mut d)| {
                d.sort_unstable();
                // The window of scales with the most distinct entries.
                let (mut best, mut at, mut lo) = (0usize, 0i32, 0usize);
                for hi in 0..d.len() {
                    while d[hi].0 - d[lo].0 > SCALE_WINDOW_STEPS {
                        lo += 1;
                    }
                    let mut hs: Vec<u64> = d[lo..=hi].iter().map(|x| x.1).collect();
                    hs.sort_unstable();
                    hs.dedup();
                    if hs.len() > best {
                        best = hs.len();
                        at = d[(lo + hi) / 2].0;
                    }
                }
                Candidate {
                    model,
                    hits: best,
                    sketch_len: lens.get(&model).copied().unwrap_or(0),
                    log_scale: f64::from(at) * SCALE_STEP,
                }
            })
            .collect();
        out.sort_by(|a, b| b.share().total_cmp(&a.share()).then(b.hits.cmp(&a.hits)).then(a.model.cmp(&b.model)));
        out
    }
}

/// [`slice::partition_point`] for a predicate usually true only near the start: doubling steps
/// to bracket the point, then a binary search within the bracket.
fn gallop<T>(s: &[T], pred: impl Fn(&T) -> bool) -> usize {
    let mut hi = 1usize;
    while hi < s.len() && pred(&s[hi - 1]) {
        hi *= 2;
    }
    let lo = hi / 2;
    let hi = hi.min(s.len());
    lo + s[lo..hi].partition_point(&pred)
}

/// Whether a candidate is worth verifying.
pub fn promising(c: &Candidate) -> bool {
    c.hits >= MIN_SKETCH_HITS && c.share() >= MIN_SKETCH_SHARE
}

type M3 = [[f64; 3]; 3];

fn mat_vec(r: &M3, v: P) -> P {
    [dot(r[0], v), dot(r[1], v), dot(r[2], v)]
}

/// A similarity transform: `scale * r * x + t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Similarity {
    pub r: M3,
    pub scale: f64,
    pub t: P,
}

impl Similarity {
    pub fn apply(&self, p: P) -> P {
        add(mul(mat_vec(&self.r, p), self.scale), self.t)
    }
}

/// The similarity taking `src` onto `dst` in the least-squares sense (Umeyama, with the rotation
/// from the polar decomposition of the cross-covariance).
pub fn fit(src: &[P], dst: &[P]) -> Option<Similarity> {
    let n = src.len() as f64;
    if src.len() < 3 {
        return None;
    }
    let cs = mul(src.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / n);
    let cd = mul(dst.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / n);
    let mut h = [[0.0; 3]; 3];
    let mut var = 0.0;
    for (a, b) in src.iter().zip(dst) {
        let (x, y) = (sub(*a, cs), sub(*b, cd));
        var += dot(x, x);
        for i in 0..3 {
            for j in 0..3 {
                h[i][j] += x[i] * y[j];
            }
        }
    }
    let mut hth = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            hth[i][j] = (0..3).map(|k| h[k][i] * h[k][j]).sum();
        }
    }
    let (ev, vecs) = eig3(hth);
    if ev[0] < 1e-18 * ev[2].max(1e-300) || var < 1e-18 {
        return None;
    }
    let mut inv = [[0.0; 3]; 3];
    for k in 0..3 {
        let w = 1.0 / ev[k].sqrt();
        for i in 0..3 {
            for j in 0..3 {
                inv[i][j] += w * vecs[k][i] * vecs[k][j];
            }
        }
    }
    let mut qm = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            qm[i][j] = (0..3).map(|k| h[i][k] * inv[k][j]).sum();
        }
    }
    let r: M3 = [0, 1, 2].map(|i| [qm[0][i], qm[1][i], qm[2][i]]);
    if dot(r[0], cross(r[1], r[2])) < 0.0 {
        return None;
    }
    let mut num = 0.0;
    for (a, b) in src.iter().zip(dst) {
        num += dot(sub(*b, cd), mat_vec(&r, sub(*a, cs)));
    }
    let scale = num / var;
    if scale <= 0.0 {
        return None;
    }
    let t = sub(cd, mul(mat_vec(&r, cs), scale));
    Some(Similarity { r, scale, t })
}

/// The similarity taking triangle `src` onto `dst` by their frames and perimeters.
pub fn fit_tri(src: &[P; 3], dst: &[P; 3]) -> Option<Similarity> {
    let frame = |t: &[P; 3]| -> Option<[P; 3]> {
        let e1 = sub(t[1], t[0]);
        let n = cross(e1, sub(t[2], t[0]));
        if len(e1) < 1e-12 || len(n) < 1e-18 {
            return None;
        }
        let (e1, n) = (unit(e1), unit(n));
        Some([e1, cross(n, e1), n])
    };
    let (fs, fd) = (frame(src)?, frame(dst)?);
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| fd[k][i] * fs[k][j]).sum();
        }
    }
    let per = |t: &[P; 3]| (0..3).map(|k| len(sub(t[(k + 1) % 3], t[k]))).sum::<f64>();
    let scale = per(dst) / per(src);
    let t = sub(centroid(dst), mul(mat_vec(&r, centroid(src)), scale));
    Some(Similarity { r, scale, t })
}

fn rotated(t: &[P; 3], start: u8) -> [P; 3] {
    let r = start as usize;
    [t[r], t[(r + 1) % 3], t[(r + 2) % 3]]
}

/// A twin registered onto the supported file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Registration {
    /// Takes the twin onto the supported file.
    pub transform: Similarity,
    pub matches: usize,
    pub inliers: usize,
}

/// Registers twin `u` onto supported `s` from their matched keys (`s_probes` as [`probe_keys`]
/// gives them). `log_scale` narrows the matches to one scale (supported over twin) when known.
pub fn register(s: &Mesh, s_probes: &[Key], u: &Mesh, log_scale: Option<f64>) -> Option<Registration> {
    let u_keys = unique(&canonical_keys(u));
    let mut matches: Vec<(u32, u8, u32, u8)> = s_probes
        .iter()
        .filter_map(|k| {
            let uk = u_keys.get(&k.hash)?;
            let d = f64::from(k.scale - uk.scale) * SCALE_STEP;
            if log_scale.is_some_and(|ls| (d - ls).abs() > 0.02) {
                return None;
            }
            Some((k.tri, k.start, uk.tri, uk.start))
        })
        .collect();
    matches.sort_unstable();
    matches.dedup();
    if matches.len() < MIN_INLIERS {
        return None;
    }
    let pair =
        |&(si, ss, ui, us): &(u32, u8, u32, u8)| (rotated(&u.tris[ui as usize], us), rotated(&s.tris[si as usize], ss));
    let agrees = |x: &Similarity, m: &(u32, u8, u32, u8), eps: f64| {
        let (a, b) = pair(m);
        (0..3).all(|k| dist(x.apply(a[k]), b[k]) < eps)
    };
    let sample: Vec<&(u32, u8, u32, u8)> = matches.iter().step_by((matches.len() / 4000).max(1)).collect();
    let hyps: Vec<usize> = (0..matches.len()).step_by((matches.len() / 256).max(1)).collect();
    let (_, mut best) = hyps
        .par_iter()
        .filter_map(|&h| {
            let (a, b) = pair(&matches[h]);
            let x = fit_tri(&a, &b)?;
            let score = sample.iter().filter(|m| agrees(&x, m, INLIER_MM * 2.0)).count();
            Some((score, x))
        })
        .max_by(|a, b| a.0.cmp(&b.0))?;
    for eps in [INLIER_MM * 2.0, INLIER_MM] {
        let (mut src, mut dst) = (Vec::new(), Vec::new());
        for m in &matches {
            if agrees(&best, m, eps) {
                let (a, b) = pair(m);
                src.extend(a);
                dst.extend(b);
            }
        }
        if let Some(x) = fit(&src, &dst) {
            best = x;
        }
    }
    let inliers = matches.iter().filter(|m| agrees(&best, m, INLIER_MM)).count();
    (inliers >= MIN_INLIERS).then_some(Registration { transform: best, matches: matches.len(), inliers })
}

/// Triangles within `tol` of a point.
struct Near<'a> {
    idx: BoxIndex,
    tris: &'a [[P; 3]],
}

impl<'a> Near<'a> {
    fn new(tris: &'a [[P; 3]]) -> Self {
        let mean = if tris.is_empty() {
            1.0
        } else {
            let step = (tris.len() / 20000).max(1);
            let (sum, n) = tris.iter().step_by(step).fold((0.0, 0usize), |(s, n), t| (s + len(sub(t[1], t[0])), n + 1));
            sum / n as f64
        };
        Near { idx: BoxIndex::new(tris, (mean * 4.0).clamp(0.02, 4.0)), tris }
    }

    fn within(&self, p: P, tol: f64, buf: &mut Vec<u32>) -> bool {
        self.idx.query_into([p[0] - tol, p[1] - tol, p[2] - tol], [p[0] + tol, p[1] + tol, p[2] + tol], buf);
        buf.iter().any(|&i| dist(p, closest_on_tri(p, &self.tris[i as usize])) <= tol)
    }

    fn on(&self, t: &[P; 3], tol: f64, buf: &mut Vec<u32>) -> bool {
        self.within(centroid(t), tol, buf) && t.iter().all(|&p| self.within(p, tol, buf))
    }
}

/// A supported file labelled by one twin.
#[derive(Debug, Clone)]
pub struct Labels {
    /// Per supported triangle: on the twin's surface.
    pub on_twin: Vec<bool>,
    /// Share of the twin's surface the supported file's model side covers.
    pub twin_coverage: f64,
    /// The largest edge-connected part of the twin's surface the supported file's model side
    /// does not cover, as a share of the twin's area.
    pub largest_gap: f64,
    /// Per twin triangle: not covered by the supported file's model side.
    pub twin_uncovered: Vec<bool>,
    /// The twin's triangles moved onto the supported file.
    pub twin: Vec<[P; 3]>,
}

/// Labels each triangle of `s` on twin `u` (registered by `x`) within `tol`, and measures how much
/// of the twin it covers.
pub fn label(s: &Mesh, u: &Mesh, x: &Similarity, tol: f64) -> Labels {
    let moved: Vec<[P; 3]> = u.tris.par_iter().map(|t| t.map(|p| x.apply(p))).collect();
    let near = Near::new(&moved);
    let on_twin: Vec<bool> = s.tris.par_iter().map_init(Vec::new, |buf, t| near.on(t, tol, buf)).collect();
    let model: Vec<[P; 3]> = s.tris.iter().zip(&on_twin).filter(|x| *x.1).map(|x| *x.0).collect();
    let near_s = Near::new(&model);
    // Each twin triangle's covered share: its centre, and when that is uncovered, six more points
    // (a large triangle of a flat base can have a support's contact over its centre).
    let covered: Vec<f64> = moved
        .par_iter()
        .map_init(Vec::new, |buf, t| {
            if near_s.within(centroid(t), tol, buf) {
                return 1.0;
            }
            let at = |w: [f64; 3]| add(add(mul(t[0], w[0]), mul(t[1], w[1])), mul(t[2], w[2]));
            let (a, b) = (2.0 / 3.0, 1.0 / 6.0);
            let points = [[a, b, b], [b, a, b], [b, b, a], [0.5, 0.5, 0.0], [0.0, 0.5, 0.5], [0.5, 0.0, 0.5]];
            points.iter().filter(|&&w| near_s.within(at(w), tol, buf)).count() as f64 / 7.0
        })
        .collect();
    let uncovered: Vec<bool> = covered.iter().map(|&c| c < 0.5).collect();
    let tot: f64 = u.tris.iter().map(tri_area).sum::<f64>().max(1e-30);
    let gap: f64 = (0..u.tris.len()).map(|i| u.area(i) * (1.0 - covered[i])).sum();
    let (_, groups) = u.groups(&uncovered);
    let largest = groups
        .iter()
        .filter(|g| uncovered[g[0] as usize])
        .map(|g| g.iter().map(|&i| u.area(i as usize)).sum::<f64>())
        .fold(0.0, f64::max);
    Labels {
        on_twin,
        twin_coverage: 1.0 - gap / tot,
        largest_gap: largest / tot,
        twin: moved,
        twin_uncovered: uncovered,
    }
}

/// Flips small islands: model islands inside support (support faces lying on the twin by chance,
/// contact rims) become support, and tiny support islands inside model become model.
pub fn clean(s: &Mesh, is_support: &mut [bool]) -> (usize, usize) {
    let model_area: f64 = (0..s.tris.len()).filter(|&i| !is_support[i]).map(|i| s.area(i)).sum();
    let (_, groups) = s.groups(is_support);
    let (mut to_support, mut to_model) = (0, 0);
    for g in &groups {
        let support = is_support[g[0] as usize];
        let area: f64 = g.iter().map(|&i| s.area(i as usize)).sum();
        let (max_n, max_share) =
            if support { (HOLE_MAX_TRIANGLES, HOLE_MAX_SHARE) } else { (ISLAND_MAX_TRIANGLES, ISLAND_MAX_SHARE) };
        if g.len() <= max_n && area <= max_share * model_area {
            // Only islands wholly surrounded by the other label (not touching an open edge).
            let closed = g.iter().all(|&i| s.nb[i as usize].iter().all(|&j| j != NONE));
            if closed {
                for &i in g {
                    is_support[i as usize] = !support;
                }
                if support {
                    to_model += g.len();
                } else {
                    to_support += g.len();
                }
            }
        }
    }
    (to_support, to_model)
}

/// How the support side of a twin split looks.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SupportShape {
    /// Share of the supported file's area on support.
    pub support_share: f64,
    /// Seam loops: edges with support on one side and model on the other, joined by shared
    /// corners. Each is where a support meets the model.
    pub seam_loops: usize,
    /// The longest seam loop (mm).
    pub longest_seam_mm: f64,
}

/// Longest seam loop (mm) a support makes with the model. A support's contact is a small loop (at
/// most 6 mm in the Loot, Hussar and Cast n Play files); a part of the model the twin lacks (a
/// head, an arm) meets the rest along a loop of 20 mm or more, and would be cut off as support.
pub const MAX_SEAM_LOOP_MM: f64 = 15.0;

/// Measures the support side.
pub fn support_shape(s: &Mesh, is_support: &[bool]) -> SupportShape {
    use crate::geometry::support_reader::math::find;
    let total: f64 = (0..s.tris.len()).map(|i| s.area(i)).sum();
    let sup: f64 = (0..s.tris.len()).filter(|&i| is_support[i]).map(|i| s.area(i)).sum();
    let nv = s.corners.iter().flatten().max().map_or(0, |&x| x as usize + 1);
    let mut parent: Vec<u32> = (0..nv as u32).collect();
    let mut seam: Vec<(u32, f64)> = Vec::new();
    for i in (0..s.tris.len()).filter(|&i| is_support[i]) {
        for k in 0..3 {
            let j = s.nb[i][k];
            if j != NONE && !is_support[j as usize] {
                let (a, b) = (s.corners[i][k], s.corners[i][(k + 1) % 3]);
                seam.push((a, dist(s.tris[i][k], s.tris[i][(k + 1) % 3])));
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent[rb as usize] = ra;
                }
            }
        }
    }
    let mut loops: FxHashMap<u32, f64> = FxHashMap::default();
    for (a, l) in seam {
        *loops.entry(find(&mut parent, a)).or_default() += l;
    }
    SupportShape {
        support_share: if total > 0.0 { sup / total } else { 0.0 },
        seam_loops: loops.len(),
        longest_seam_mm: loops.values().copied().fold(0.0, f64::max),
    }
}

/// Fewest seam loops: supports welded into the model meet it at several contacts. A support side
/// with none is not welded supports (a near copy with a part added, a kit piece sharing parts).
pub const MIN_SEAM_LOOPS: usize = 3;
/// Least share of the file's area on support: a sliver of difference between near copies is not
/// supports (the Loot and Hussar files have 30% to 85%).
pub const MIN_SUPPORT_SHARE: f64 = 0.02;

/// Whether the support side looks like welded supports: enough of it, meeting the model in several
/// loops, all small.
pub fn support_like(shape: &SupportShape) -> bool {
    shape.support_share >= MIN_SUPPORT_SHARE
        && shape.seam_loops >= MIN_SEAM_LOOPS
        && shape.longest_seam_mm <= MAX_SEAM_LOOP_MM
}

/// Why a twin was refused, or what it gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Not registered (too few matched triangles agree on a transform).
    NoRegistration,
    /// The supported file covers too little of the twin, or leaves a large part of it uncovered (the
    /// twin has a part the supported file lacks): coverage and largest gap.
    Uncovered(f64, f64),
    /// Accepted: the twin's labels and how well it fitted.
    Twin { registration: Registration, coverage: f64 },
}

/// A supported file split by its twins.
#[derive(Debug, Clone)]
pub struct TwinSplit {
    pub is_support: Vec<bool>,
    pub shape: SupportShape,
    /// Each accepted twin's index in the given list, with its registration and coverage.
    pub twins: Vec<(usize, Registration, f64)>,
    pub cleaned: (usize, usize),
    /// Triangles turned to model as inner walls ([`inner_walls`]).
    pub inner: usize,
    /// The support side looks like supports ([`support_like`]); a split is only used when it does.
    pub supports: bool,
}

/// Verifies one twin against the supported file: registration, labels and coverage.
pub fn verify(s: &Mesh, s_probes: &[Key], u: &Mesh, log_scale: Option<f64>) -> (Verdict, Option<Labels>) {
    let Some(reg) = register(s, s_probes, u, log_scale) else {
        return (Verdict::NoRegistration, None);
    };
    let labels = label(s, u, &reg.transform, ON_SURFACE_MM);
    if labels.twin_coverage < MIN_TWIN_COVERAGE || labels.largest_gap > MAX_TWIN_GAP {
        return (Verdict::Uncovered(labels.twin_coverage, labels.largest_gap), Some(labels));
    }
    let coverage = labels.twin_coverage;
    (Verdict::Twin { registration: reg, coverage }, Some(labels))
}

/// Splits supported `s` by the accepted twins' labels (a union: model where any twin lies), cleans
/// it, and keeps it only when the support side looks like supports. `up` is the up axis index.
pub fn combine(s: &Mesh, accepted: &[(usize, Registration, f64, Labels)]) -> Option<TwinSplit> {
    combine_at(s, accepted, WALL_MAX_DEPTH_MM)
}

/// [`combine`] with inner walls up to `wall_depth_mm` deep ([`WALL_MAX_DEPTH_MM`]).
pub fn combine_at(s: &Mesh, accepted: &[(usize, Registration, f64, Labels)], wall_depth_mm: f64) -> Option<TwinSplit> {
    if accepted.is_empty() {
        return None;
    }
    let mut is_support = vec![true; s.tris.len()];
    for (_, _, _, l) in accepted {
        for (x, &on) in is_support.iter_mut().zip(&l.on_twin) {
            *x &= !on;
        }
    }
    let cleaned = clean(s, &mut is_support);
    let twins: Vec<&[[P; 3]]> = accepted.iter().map(|(_, _, _, l)| l.twin.as_slice()).collect();
    let inner = inner_walls(s, &mut is_support, &twins, wall_depth_mm);
    let again = clean(s, &mut is_support);
    let cleaned = (cleaned.0 + again.0, cleaned.1 + again.1);
    let shape = support_shape(s, &is_support);
    let supports = is_support.contains(&true) && support_like(&shape);
    let twins = accepted.iter().map(|(i, r, c, _)| (*i, *r, *c)).collect();
    Some(TwinSplit { is_support, shape, twins, cleaned, inner, supports })
}

/// Whether `p` is inside a closed mesh, by the parity of crossings of a ray along `d` (nudged off
/// the axes so it misses edges).
fn inside(idx: &BoxIndex, tris: &[[P; 3]], reach: f64, p: P, d: P, buf: &mut Vec<u32>) -> bool {
    use crate::geometry::support_reader::math::ray_hits;
    let q = add(p, mul(d, reach));
    let (lo, hi) = ([0, 1, 2].map(|k| p[k].min(q[k])), [0, 1, 2].map(|k| p[k].max(q[k])));
    idx.query_into(lo, hi, buf);
    buf.iter().filter(|&&i| ray_hits(p, d, &tris[i as usize])).count() % 2 == 1
}

/// Fewest edge-connected wall triangles ([`inner_walls`]) to be a hollowed file's inner wall or
/// cavity rather than a stray patch.
pub const INNER_MIN_TRIANGLES: usize = 256;
/// A wall's back (along its inward normal) reaches the twin's surface within this depth (mm): a
/// hollowed file's wall thickness. Supports standing in an open cavity are further from it. 6 mm
/// (3.5 before) splits 9 more of the 204 keyed Loot files, all hollowed, leaves the files split at
/// both the same, and splits nothing in the guard sets, files without supports or partial twins.
pub const WALL_MAX_DEPTH_MM: f64 = 6.0;
/// Share of a support part's sampled triangles inside a twin for the whole part to be model (a
/// drain hole's wall, supports sealed inside a hollow).
pub const INNER_PART_MIN_INSIDE: f64 = 0.8;
/// Triangles sampled per support part.
pub const INNER_PART_SAMPLES: usize = 63;

/// Distance along the ray from `o` along unit `d` to the first twin triangle it crosses, when one
/// lies within `reach`.
fn first_hit(idx: &BoxIndex, tris: &[[P; 3]], o: P, d: P, reach: f64, buf: &mut Vec<u32>) -> Option<f64> {
    let q = add(o, mul(d, reach));
    let (lo, hi) = ([0, 1, 2].map(|k| o[k].min(q[k])), [0, 1, 2].map(|k| o[k].max(q[k])));
    idx.query_into(lo, hi, buf);
    buf.iter()
        .filter_map(|&i| {
            let t = &tris[i as usize];
            let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
            let h = cross(d, e2);
            let a = dot(e1, h);
            if a.abs() < 1e-14 {
                return None;
            }
            let f = 1.0 / a;
            let sv = sub(o, t[0]);
            let u = f * dot(sv, h);
            let qv = cross(sv, e1);
            let v = f * dot(d, qv);
            let x = f * dot(e2, qv);
            ((0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && x > 0.0 && x <= reach).then_some(x)
        })
        .min_by(f64::total_cmp)
}

/// Turns into model the support triangles a hollowed file has inside its unhollowed twin:
///
/// - walls: triangles inside a twin (two rays, along +x and +y, both say so) whose back reaches
///   its surface within `wall_depth_mm` ([`WALL_MAX_DEPTH_MM`]), [`INNER_MIN_TRIANGLES`] or more together (an inner
///   wall, a hollow base's cavity);
/// - then support parts mostly inside a twin ([`INNER_PART_MIN_INSIDE`]): a drain hole's wall,
///   supports sealed inside the hollow.
///
/// Supports standing in an open cavity (under a hollow base) are inside the twin too, but are
/// neither a wall's depth from its surface nor mostly inside it, so they stay support. Returns how many
/// triangles turned.
pub fn inner_walls(s: &Mesh, is_support: &mut [bool], twins: &[&[[P; 3]]], wall_depth_mm: f64) -> usize {
    let idx: Vec<(BoxIndex, f64)> = twins
        .iter()
        .map(|t| {
            let (a, b) = t.iter().flatten().fold(([f64::MAX; 3], [f64::MIN; 3]), |(a, b), p| {
                ([0, 1, 2].map(|k| a[k].min(p[k])), [0, 1, 2].map(|k| b[k].max(p[k])))
            });
            (BoxIndex::new(t, 1.0), 2.0 * len(sub(b, a)) + 1.0)
        })
        .collect();
    let dirs = [unit([1.0, 1.3e-4, 2.9e-4]), unit([2.3e-4, 1.0, 1.7e-4])];
    let is_inside = |c: P, buf: &mut Vec<u32>| -> Option<usize> {
        idx.iter().zip(twins).position(|((ix, reach), t)| dirs.iter().all(|&d| inside(ix, t, *reach, c, d, buf)))
    };
    let walls: Vec<u32> = (0..s.tris.len() as u32)
        .into_par_iter()
        .filter(|&i| is_support[i as usize])
        .map_init(Vec::new, |buf, i| {
            let t = &s.tris[i as usize];
            let c = centroid(t);
            let wall = is_inside(c, buf).is_some_and(|k| {
                let back = mul(unit(cross(sub(t[1], t[0]), sub(t[2], t[0]))), -1.0);
                first_hit(&idx[k].0, twins[k], c, back, wall_depth_mm, buf).is_some()
            });
            (i, wall)
        })
        .filter(|x| x.1)
        .map(|x| x.0)
        .collect();
    let mut flag = vec![false; s.tris.len()];
    for &i in &walls {
        flag[i as usize] = true;
    }
    let (_, groups) = s.groups(&flag);
    let mut turned = 0;
    for g in groups.iter().filter(|g| flag[g[0] as usize] && g.len() >= INNER_MIN_TRIANGLES) {
        for &i in g {
            is_support[i as usize] = false;
        }
        turned += g.len();
    }
    // Support parts mostly inside.
    let (_, groups) = s.groups(is_support);
    let parts: Vec<&Vec<u32>> = groups.iter().filter(|g| is_support[g[0] as usize]).collect();
    let mut buf = Vec::new();
    for g in parts {
        let step = (g.len() / INNER_PART_SAMPLES).max(1);
        let sample: Vec<u32> = g.iter().copied().step_by(step).collect();
        let n_in = sample.iter().filter(|&&i| is_inside(centroid(&s.tris[i as usize]), &mut buf).is_some()).count();
        if n_in as f64 >= INNER_PART_MIN_INSIDE * sample.len() as f64 {
            for &i in g {
                is_support[i as usize] = false;
            }
            turned += g.len();
        }
    }
    turned
}

/// The whole split for loaded (Y-up) triangles and their candidate twins: verifies each, and
/// combines the accepted ones.
pub fn split(supported: &[Triangle], twins: &[&[Triangle]]) -> Option<TwinSplit> {
    let twins: Vec<(&[Triangle], Option<f64>)> = twins.iter().map(|t| (*t, None)).collect();
    split_scaled(supported, &twins)
}

/// [`split`] with each twin's scale (supported over twin, as a log) when known, as a lookup found
/// it: the split made again from the twins a lookup accepted is the lookup's split.
pub fn split_scaled(supported: &[Triangle], twins: &[(&[Triangle], Option<f64>)]) -> Option<TwinSplit> {
    let s = Mesh::new(supported);
    let probes = probe_keys(&s);
    let accepted: Vec<_> = twins
        .iter()
        .enumerate()
        .filter_map(|(i, (t, log_scale))| {
            let u = Mesh::new(t);
            match verify(&s, &probes, &u, *log_scale) {
                (Verdict::Twin { registration, coverage }, Some(l)) => Some((i, registration, coverage, l)),
                _ => None,
            }
        })
        .collect();
    combine(&s, &accepted).filter(|x| x.supports)
}

/// One candidate a lookup verified, and what came of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub candidate: Candidate,
    pub verdict: Verdict,
}

/// A twin lookup of one supported file: the candidates verified and the split their accepted twins
/// give (None when none was accepted).
#[derive(Debug, Clone)]
pub struct Lookup {
    /// Promising candidates found (all of them, verified or not).
    pub promising: usize,
    pub checked: Vec<Checked>,
    pub split: Option<TwinSplit>,
}

impl Lookup {
    /// The split to use: one whose support side looks like supports.
    pub fn usable(&self) -> Option<&TwinSplit> {
        self.split.as_ref().filter(|x| x.supports && x.is_support.contains(&true))
    }
}

/// Looks up supported `s` (probed by `probes`) among `candidates` (best first, from
/// [`Index::candidates`], without the file itself): verifies up to [`MAX_CANDIDATES`] promising
/// ones, each loaded by `load` (None when it cannot be), and combines the accepted ones.
pub fn lookup(
    s: &Mesh,
    probes: &[Key],
    candidates: &[Candidate],
    mut load: impl FnMut(&Candidate) -> Option<Vec<Triangle>>,
) -> Lookup {
    let promising: Vec<&Candidate> = candidates.iter().filter(|c| promising(c)).collect();
    let mut checked = Vec::new();
    let mut accepted = Vec::new();
    for c in promising.iter().take(MAX_CANDIDATES) {
        let Some(t) = load(c) else { continue };
        let u = Mesh::new(&t);
        let (verdict, labels) = verify(s, probes, &u, Some(c.log_scale));
        if let (Verdict::Twin { registration, coverage }, Some(l)) = (&verdict, labels) {
            // The split's twins are numbered as `checked` lists them.
            accepted.push((checked.len(), *registration, *coverage, l));
        }
        checked.push(Checked { candidate: **c, verdict });
    }
    Lookup { promising: promising.len(), checked, split: combine(s, &accepted) }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::geometry::Vec3;

    /// A bumpy closed sphere: a UV sphere with a deterministic irregular radius, so its triangles
    /// have distinct shapes as a sculpt's do.
    pub(crate) fn blob(n: usize, seed: u64) -> Vec<[P; 3]> {
        let (rings, segs) = (n, 2 * n);
        let r = |i: usize, j: usize| {
            let h = mix(seed ^ ((i as u64) << 32) ^ j as u64);
            10.0 + (h % 1000) as f64 * 1e-3
        };
        let mut verts = vec![[0.0, 0.0, r(0, 0)]];
        for i in 1..rings {
            let th = std::f64::consts::PI * i as f64 / rings as f64;
            for j in 0..segs {
                let ph = 2.0 * std::f64::consts::PI * (j as f64 + 0.37 * (i % 3) as f64) / segs as f64;
                let rr = r(i, j);
                verts.push([rr * th.sin() * ph.cos(), rr * th.sin() * ph.sin(), rr * th.cos()]);
            }
        }
        verts.push([0.0, 0.0, -r(rings, 0)]);
        let at = |i: usize, j: usize| 1 + (i - 1) * segs + j % segs;
        let last = verts.len() - 1;
        let mut tris = Vec::new();
        for j in 0..segs {
            tris.push([verts[0], verts[at(1, j)], verts[at(1, j + 1)]]);
            tris.push([verts[last], verts[at(rings - 1, j + 1)], verts[at(rings - 1, j)]]);
        }
        for i in 1..rings - 1 {
            for j in 0..segs {
                let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j), at(i + 1, j + 1));
                tris.push([verts[a], verts[c], verts[b]]);
                tris.push([verts[b], verts[c], verts[d]]);
            }
        }
        tris
    }

    pub(crate) fn to_tris(t: &[[P; 3]]) -> Vec<Triangle> {
        let v = |p: P| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
        t.iter().map(|t| Triangle::new(v(t[0]), v(t[1]), v(t[2]), Vec3::UP)).collect()
    }

    pub(crate) fn rot(a: f64, b: f64) -> M3 {
        let (ca, sa, cb, sb) = (a.cos(), a.sin(), b.cos(), b.sin());
        let rz = [[ca, -sa, 0.0], [sa, ca, 0.0], [0.0, 0.0, 1.0]];
        let rx = [[1.0, 0.0, 0.0], [0.0, cb, -sb], [0.0, sb, cb]];
        let mut r = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = (0..3).map(|k| rz[i][k] * rx[k][j]).sum();
            }
        }
        r
    }

    fn moved(t: &[[P; 3]], x: &Similarity) -> Vec<[P; 3]> {
        t.iter().map(|t| t.map(|p| x.apply(p))).collect()
    }

    /// The blob moved, with supports welded into it as a boolean union leaves them: a few of its
    /// lowest triangles replaced by long downward spikes on their edges (Y-up). The body's
    /// triangles come first; returns them with the spikes, and how many body triangles there are.
    pub(crate) fn supported(u: &[[P; 3]], x: &Similarity) -> (Vec<[P; 3]>, usize) {
        let body = moved(u, x);
        let mut order: Vec<usize> = (0..body.len()).collect();
        order.sort_by(|&a, &b| centroid(&body[a])[1].total_cmp(&centroid(&body[b])[1]));
        let picked: Vec<usize> = order.iter().take(420).copied().step_by(60).collect();
        let mut s: Vec<[P; 3]> =
            body.iter().enumerate().filter(|(i, _)| !picked.contains(i)).map(|(_, t)| *t).collect();
        let n = s.len();
        for &i in &picked {
            let t = body[i];
            let apex = sub(centroid(&t), [0.0, 12.0, 0.0]);
            s.extend([[t[0], t[1], apex], [t[1], t[2], apex], [t[2], t[0], apex]]);
        }
        (s, n)
    }

    #[test]
    fn sketch_round_trips() {
        let m = Mesh::new(&to_tris(&blob(30, 1)));
        let s = Sketch::of(&m);
        assert_eq!(s.entries.len(), SKETCH_SIZE);
        assert_eq!(Sketch::from_bytes(&s.to_bytes()), Some(s.clone()));
        let mut bad = s.to_bytes();
        bad[0] = bad[0].wrapping_add(1);
        assert_eq!(Sketch::from_bytes(&bad), None);
    }

    #[test]
    fn finds_rotated_and_scaled_twin_and_not_others() {
        let u = blob(40, 7);
        let mut index = Index::default();
        index.add(1, &Sketch::of(&Mesh::new(&to_tris(&u))));
        index.add(2, &Sketch::of(&Mesh::new(&to_tris(&blob(40, 8)))));
        index.add(3, &Sketch::of(&Mesh::new(&to_tris(&blob(36, 7)))));
        for scale in [1.0, 75.0 / 32.0, 0.5] {
            let x = Similarity { r: rot(0.7, 1.1), scale, t: [5.0, 20.0, -3.0] };
            let s = Mesh::new(&to_tris(&supported(&u, &x).0));
            let c = index.candidates(&probe_keys(&s));
            let good: Vec<_> = c.iter().filter(|c| promising(c)).collect();
            assert_eq!(good.len(), 1, "scale {scale}: {c:?}");
            assert_eq!(good[0].model, 1);
            assert!(good[0].share() > 0.8, "{:?}", good[0]);
            assert!((good[0].log_scale - scale.ln()).abs() < 0.01);
        }
    }

    #[test]
    fn gallop_finds_the_partition_point() {
        let v: Vec<u32> = (0..1000).map(|i| i * 3).collect();
        for x in [0, 1, 2, 3, 500, 2996, 2997, 2998, 5000] {
            for from in [0, 1, 400, 999, 1000] {
                let s = &v[from..];
                assert_eq!(gallop(s, |&y| y < x), s.partition_point(|&y| y < x), "{x} from {from}");
            }
        }
        assert_eq!(gallop(&[] as &[u32], |_| true), 0);
    }

    #[test]
    fn stored_probe_keys_find_the_twin_without_the_mesh() {
        let u = blob(80, 7);
        let mut index = Index::default();
        index.add(1, &Sketch::of(&Mesh::new(&to_tris(&u))));
        index.add(2, &Sketch::of(&Mesh::new(&to_tris(&blob(80, 8)))));
        index.add(3, &Sketch::of(&Mesh::new(&to_tris(&blob(40, 7)))));
        index.sort();
        let x = Similarity { r: rot(0.7, 1.1), scale: 75.0 / 32.0, t: [5.0, 20.0, -3.0] };
        let s = Mesh::new(&to_tris(&supported(&u, &x).0));
        let probes = probe_keys(&s);
        let stored = StoredProbes::of(&probes);
        assert!(stored.keys.len() * 50 < probes.len(), "{} of {}", stored.keys.len(), probes.len());
        let stored = StoredProbes::from_bytes(&stored.to_bytes()).expect("round trip");
        let full: Vec<_> = index.candidates_sorted(&probes).into_iter().filter(promising).collect();
        let kept: Vec<_> = index.candidates_stored(&stored).into_iter().filter(promising).collect();
        assert_eq!(full.iter().map(|c| c.model).collect::<Vec<_>>(), [1]);
        assert_eq!(kept.iter().map(|c| c.model).collect::<Vec<_>>(), [1], "{kept:?}");
        assert!((kept[0].log_scale - x.scale.ln()).abs() < 0.01);
        let mut other = stored.to_bytes();
        other[0] = other[0].wrapping_add(1);
        assert_eq!(StoredProbes::from_bytes(&other), None);
    }

    #[test]
    fn labels_supports_by_the_twin() {
        let u = blob(40, 3);
        let x = Similarity { r: rot(2.0, 0.4), scale: 1.5, t: [1.0, 30.0, 2.0] };
        let (st, body_n) = supported(&u, &x);
        let split = split(&to_tris(&st), &[&to_tris(&u)]).expect("split");
        assert_eq!(split.twins.len(), 1);
        let (_, reg, cov) = split.twins[0];
        assert!(cov > 0.99);
        assert!((reg.transform.scale - 1.5).abs() < 1e-4);
        assert!(split.is_support[..body_n].iter().all(|s| !s));
        assert!(split.is_support[body_n..].iter().all(|s| *s));
    }

    #[test]
    fn refuses_twin_with_parts_the_supported_file_lacks() {
        // The twin is the blob plus a second blob beside it (a variant with an extra part).
        let u = blob(40, 3);
        let mut twin = u.clone();
        twin.extend(moved(&blob(20, 9), &Similarity { r: rot(0.0, 0.0), scale: 0.5, t: [30.0, 0.0, 0.0] }));
        let x = Similarity { r: rot(1.0, 0.2), scale: 1.0, t: [0.0, 15.0, 0.0] };
        let (st, _) = supported(&u, &x);
        assert!(split(&to_tris(&st), &[&to_tris(&u)]).is_some());
        assert!(split(&to_tris(&st), &[&to_tris(&twin)]).is_none());
    }

    #[test]
    fn refuses_twin_missing_a_part_the_supported_file_has() {
        // The twin lacks the blob's top (a head): in the supported file that part would be cut
        // off as support along a long seam (its neck), so the split is refused.
        let u = blob(40, 3);
        let headless: Vec<[P; 3]> = u.iter().filter(|t| centroid(t)[2] < 6.0).copied().collect();
        let x = Similarity { r: rot(1.0, 0.2), scale: 1.0, t: [0.0, 15.0, 0.0] };
        let (st, _) = supported(&u, &x);
        let s = Mesh::new(&to_tris(&st));
        let probes = probe_keys(&s);
        let (v, labels) = verify(&s, &probes, &Mesh::new(&to_tris(&headless)), None);
        let Verdict::Twin { registration, coverage } = v else { panic!("{v:?}") };
        let combined = combine(&s, &[(0, registration, coverage, labels.unwrap())]).unwrap();
        assert!(combined.shape.longest_seam_mm > MAX_SEAM_LOOP_MM, "{:?}", combined.shape);
        assert!(!combined.supports);
        assert!(split(&to_tris(&st), &[&to_tris(&headless)]).is_none());
        // The whole twin passes.
        assert!(split(&to_tris(&st), &[&to_tris(&u)]).is_some());
    }

    #[test]
    fn unrelated_model_is_no_twin() {
        let (st, _) = supported(&blob(40, 3), &Similarity { r: rot(0.0, 0.0), scale: 1.0, t: [0.0, 15.0, 0.0] });
        assert!(split(&to_tris(&st), &[&to_tris(&blob(40, 4))]).is_none());
    }

    #[test]
    fn clean_flips_small_islands() {
        // Large enough that one triangle is a tiny share of the surface.
        let m = Mesh::new(&to_tris(&blob(250, 5)));
        let mut is_support = vec![false; m.tris.len()];
        // One triangle marked support inside the model, and a model triangle inside a support
        // patch (three rings of triangles, large enough to stay).
        is_support[100_000] = true;
        for x in &mut is_support[2000..5000] {
            *x = true;
        }
        is_support[3500] = false;
        let (to_support, to_model) = clean(&m, &mut is_support);
        assert!(!is_support[100_000]);
        assert!(is_support[3500]);
        assert!(is_support[2000..5000].iter().all(|x| *x));
        assert_eq!((to_support, to_model), (1, 1));
    }

    #[test]
    fn hollow_inner_wall_is_model() {
        // The supported file is hollowed: an inner wall (a smaller shell facing inwards) inside
        // the body, which the twin lacks. It lies inside the twin a wall's depth from its surface,
        // so it is model; the spikes stay support.
        let u = blob(40, 3);
        let x = Similarity { r: rot(0.3, 0.9), scale: 1.0, t: [0.0, 15.0, 0.0] };
        let (mut st, body_n) = supported(&u, &x);
        let n = st.len();
        let inner = moved(&blob(30, 21), &Similarity { r: rot(0.0, 0.0), scale: 0.82, t: [0.0, 15.0, 0.0] });
        st.extend(inner.iter().map(|t| [t[0], t[2], t[1]]));
        let split = split(&to_tris(&st), &[&to_tris(&u)]).expect("split");
        assert!(split.inner > 0);
        assert!(split.is_support[n..].iter().all(|s| !s));
        assert!(split.is_support[..body_n].iter().all(|s| !s));
        assert!(split.is_support[body_n..n].iter().all(|s| *s));
    }

    #[test]
    fn near_copy_without_welded_supports_is_not_split() {
        // A near copy with a separate small part added (no seam with the model, a sliver of the
        // area) is not a supported file and its twin does not split it.
        let u = blob(40, 3);
        let mut st = moved(&u, &Similarity { r: rot(0.4, 0.1), scale: 1.0, t: [0.0, 15.0, 0.0] });
        st.extend(moved(&blob(8, 2), &Similarity { r: rot(0.0, 0.0), scale: 0.1, t: [0.0, 40.0, 0.0] }));
        let s = Mesh::new(&to_tris(&st));
        let (v, labels) = verify(&s, &probe_keys(&s), &Mesh::new(&to_tris(&u)), None);
        let Verdict::Twin { registration, coverage } = v else { panic!("{v:?}") };
        let combined = combine(&s, &[(0, registration, coverage, labels.unwrap())]).unwrap();
        assert_eq!(combined.shape.seam_loops, 0);
        assert!(!combined.supports);
    }
}
