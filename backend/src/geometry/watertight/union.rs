//! Method 1, the boolean self-union: the exact surface kept, overlapping and crossing shells
//! merged by a robust mesh boolean (Manifold, through the manifold-csg bindings), internal faces
//! dropped. Sealed voids (and inner skins facing the wrong way) are filled: a void with no way out
//! traps resin; a hollow that drains is part of its solid's shell and stays. Buried parts are left
//! out (they change nothing), and open or non-manifold pieces cannot enter a boolean: they are left
//! out and counted (the hybrid remeshes them).

use manifold_csg::Manifold;
use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::mesh::{self, Indexed};
use super::shells::{self, Kind, Nest};
use crate::geometry::support_reader::math::P;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct UnionStats {
    pub solids: usize,
    pub sealed_voids: usize,
    pub inverted_skins_turned: usize,
    pub inside_out_turned: usize,
    pub buried_left_out: usize,
    /// Volume of the sealed voids filled (mm3).
    pub voids_filled_mm3: f64,
    /// Boxes mended in the passes after the first.
    pub follow_up_boxes: usize,
    /// Specks and sheets ([`mesh::negligible`]), left out.
    pub specks_left_out: usize,
    pub open_left_out: usize,
    pub open_area_left_out: f64,
    /// Groups the boolean would not take (not manifold after welding).
    pub rejected: usize,
    pub rejected_reasons: Vec<String>,
    pub faces_flipped_to_agree: usize,
    pub closing: mesh::Closing,
    pub self_crossing_shells: usize,
    /// For the hybrid: solids remeshed, and their faces before and after.
    pub remeshed_groups: usize,
    pub remeshed_faces_in: usize,
    pub remeshed_faces_out: usize,
    /// Booleans run (parts whose solids cross); the other parts are kept as they are.
    pub booleans: usize,
    /// Booleans that failed (their smaller operand is lost).
    pub booleans_failed: usize,
    /// Solids crossing themselves mended in boxes, and the boxes.
    pub mended_groups: usize,
    pub mend_boxes: usize,
    /// Small solids taken into a larger one's boxes.
    pub small_absorbed: usize,
    /// What was repaired where (kind, place), for showing the repairs.
    #[serde(skip)]
    pub spots: Vec<(&'static str, P)>,
}

/// The shells made consistent and classified: the welded mesh with inner skins and inside-out
/// shells turned, its shells and their kinds.
pub struct Prepared {
    pub mesh: Indexed,
    pub shells: Vec<mesh::Shell>,
    pub nests: Vec<Nest>,
    pub flipped: usize,
    pub closing: mesh::Closing,
    /// Shells crossing themselves (a boolean cannot take them as they are).
    pub self_crossing: Vec<usize>,
    /// Pairs of shells whose faces cross each other.
    pub crossing: rustc_hash::FxHashSet<(usize, usize)>,
    /// Pairs of faces that cross.
    pub pairs: Vec<(u32, u32)>,
}

pub fn prepare(tris: &[[P; 3]]) -> Prepared {
    prepare_welded(Indexed::weld(tris), None)
}

/// [`prepare`] of an indexed mesh whose crossing face pairs are known already (`pairs`). They are
/// used when welding its faces gives the same mesh (corners at distinct positions, every face kept)
/// and closing it changes nothing.
fn prepare_known(m: &Indexed, pairs: Vec<(u32, u32)>) -> Prepared {
    let tris = m.tris();
    let w = Indexed::weld(&tris);
    let mut used = vec![false; m.verts.len()];
    for f in &m.faces {
        for &v in f {
            used[v as usize] = true;
        }
    }
    let same = w.faces.len() == m.faces.len() && w.verts.len() == used.iter().filter(|&&u| u).count();
    prepare_welded(w, same.then_some(pairs))
}

/// [`prepare`] of a welded mesh, with its crossing face pairs when they are known already.
fn prepare_welded(mut m: Indexed, known_pairs: Option<Vec<(u32, u32)>>) -> Prepared {
    let (mut closing, mut e) = mesh::close_holes_edges(&mut m);
    let mut changed = closing.corners_split + closing.non_manifold_faces_removed + closing.holes_filled > 0;
    // Filling a hole beside a pinched corner can make a new non-manifold edge: close again.
    for _ in 0..4 {
        if e.non_manifold.is_empty() && e.boundary.is_empty() {
            break;
        }
        let (more, e2) = mesh::close_holes_edges(&mut m);
        e = e2;
        changed |= more.corners_split + more.non_manifold_faces_removed + more.holes_filled > 0;
        closing.non_manifold_faces_removed += more.non_manifold_faces_removed;
        closing.holes_filled += more.holes_filled;
        closing.longest_hole = closing.longest_hole.max(more.longest_hole);
        closing.boundary_left = more.boundary_left;
        closing.hole_centres.extend(more.hole_centres);
    }
    let (flipped, _) = mesh::orient_with(&mut m, &e);
    if flipped > 0 {
        e = mesh::edges(&m);
    }
    let shells = mesh::shell_summaries(&m, &e);
    // Pairs found on the input hold while closing changed nothing (the faces are the same).
    let pairs = match known_pairs {
        Some(p) if !changed => p,
        _ => super::intersect::pairs(&m),
    };
    let crossing = shells::crossing_shells(&m, &shells, &pairs);
    let self_crossing = shells::self_crossing_shells(&m, &shells, &pairs);
    let nests = shells::nest(&m, &shells, &crossing);
    for (s, n) in shells.iter().zip(&nests) {
        if matches!(n.kind, Kind::InvertedSkin | Kind::InsideOut) {
            mesh::flip(&mut m, &s.faces);
        }
    }
    // Volumes follow the turns.
    let shells = shells
        .into_iter()
        .zip(&nests)
        .map(|(mut s, n)| {
            if matches!(n.kind, Kind::InvertedSkin | Kind::InsideOut) {
                s.volume = -s.volume;
            }
            s
        })
        .collect();
    Prepared { mesh: m, shells, nests, flipped, closing, self_crossing, crossing, pairs }
}

fn centre(s: &mesh::Shell) -> P {
    [0, 1, 2].map(|k| (s.lo[k] + s.hi[k]) / 2.0)
}

/// A boolean's result, or why it failed.
fn checked(m: Manifold, what: &str) -> Result<Manifold, String> {
    m.status().map(|_| m).map_err(|e| format!("{what}: {e}"))
}

/// A part's triangle count.
fn tri_count(m: &Manifold) -> usize {
    m.num_tri()
}

/// A part's box (min, max).
fn bounds(m: &Manifold) -> (P, P) {
    m.bounding_box().map_or(([0.0; 3], [0.0; 3]), |b| (b.min(), b.max()))
}

/// The mesh of a boolean's result.
fn indexed_of(m: &Manifold) -> Indexed {
    let (v, n, i) = m.to_mesh_f64();
    Indexed {
        verts: v.chunks(n).map(|c| [c[0], c[1], c[2]]).collect(),
        faces: i.chunks(3).map(|c| [c[0] as u32, c[1] as u32, c[2] as u32]).collect(),
    }
}

fn manifold_of(m: &Indexed, faces: &[u32]) -> Result<Manifold, String> {
    // Only the corners these faces use, renumbered.
    let mut at = vec![u32::MAX; m.verts.len()];
    let mut pos: Vec<f64> = Vec::new();
    let mut idx: Vec<u64> = Vec::with_capacity(faces.len() * 3);
    for &f in faces {
        for v in m.faces[f as usize] {
            if at[v as usize] == u32::MAX {
                at[v as usize] = (pos.len() / 3) as u32;
                pos.extend_from_slice(&m.verts[v as usize]);
            }
            idx.push(u64::from(at[v as usize]));
        }
    }
    Manifold::from_mesh_f64(&pos, 3, &idx).map_err(|e| e.to_string())
}

/// The self-union of a triangle soup.
pub fn union(tris: &[[P; 3]]) -> (Indexed, UnionStats) {
    union_with(prepare(tris), None, true)
}

/// The self-union, with solids a boolean cannot take (crossing themselves, or left open) first
/// remeshed on cells `h` mm wide and simplified to `tol` mm: the exact surface everywhere else.
pub fn hybrid(tris: &[[P; 3]], h: f64, tol: f64) -> (Indexed, UnionStats) {
    let t = std::time::Instant::now();
    let (mut out, mut st) = union_with(prepare(tris), Some((h, tol)), true);
    super::remesh::trace(&format!("first pass done at {:.2}", t.elapsed().as_secs_f64()));
    // Rounding to single precision (as STL and VOXL store corners) can fold a mended seam over
    // itself: the rounded result is checked and its crossings mended again, a few times.
    for _ in 0..FOLLOW_UP_PASSES {
        let (rounded, _) = super::mesh::finish(&out);
        super::remesh::trace(&format!("follow-up: rounded at {:.2}", t.elapsed().as_secs_f64()));
        let pairs = super::intersect::pairs(&rounded);
        super::remesh::trace(&format!("follow-up: {} crossings at {:.2}", pairs.len(), t.elapsed().as_secs_f64()));
        if pairs.is_empty() {
            break;
        }
        let (again, more) = union_with(prepare_known(&rounded, pairs), Some((h, tol)), false);
        if more.mended_groups == 0 {
            break;
        }
        st.follow_up_boxes += more.mend_boxes;
        out = again;
        super::remesh::trace(&format!("follow-up pass done at {:.2}", t.elapsed().as_secs_f64()));
    }
    // Pockets closed off where parts overlapped show as inner shells only now: fill those too.
    let (filled, n, volume) = fill_sealed(&out);
    st.sealed_voids += n;
    st.voids_filled_mm3 += volume;
    super::remesh::trace(&format!("sealed voids filled at {:.2}", t.elapsed().as_secs_f64()));
    (filled, st)
}

/// The mesh without its sealed voids (inner shells facing inward with no way out): the number
/// filled and their volume.
pub fn fill_sealed(m: &Indexed) -> (Indexed, usize, f64) {
    let e = mesh::edges(m);
    let shells = mesh::shell_summaries(m, &e);
    if shells.len() < 2 {
        return (m.clone(), 0, 0.0);
    }
    // Only crossings between shells matter here: the largest shell's own are not looked for.
    let mut group = vec![0usize; m.faces.len()];
    for (i, s) in shells.iter().enumerate() {
        for &f in &s.faces {
            group[f as usize] = i;
        }
    }
    let largest = (0..shells.len()).max_by_key(|&i| shells[i].faces.len()).unwrap_or(0);
    let pairs = super::intersect::pairs_across(m, &group, largest);
    let crossing = shells::crossing_shells(m, &shells, &pairs);
    let nests = shells::nest(m, &shells, &crossing);
    let mut keep = vec![true; m.faces.len()];
    let (mut n, mut volume) = (0, 0.0);
    for (s, nest) in shells.iter().zip(&nests) {
        if nest.kind == Kind::SealedVoid {
            n += 1;
            volume += s.volume.abs();
            for &f in &s.faces {
                keep[f as usize] = false;
            }
        }
    }
    let faces = m.faces.iter().zip(&keep).filter(|(_, k)| **k).map(|(f, _)| *f).collect();
    (mesh::compact(&Indexed { verts: m.verts.clone(), faces }), n, volume)
}

/// Mending passes after the first, on the rounded result.
const FOLLOW_UP_PASSES: usize = 3;

/// `first`: the first pass also absorbs small solids and remeshes whole any solid it cannot mend;
/// a follow-up pass only mends crossings.
fn union_with(p: Prepared, remesh_bad: Option<(f64, f64)>, first: bool) -> (Indexed, UnionStats) {
    let t0 = std::time::Instant::now();
    let mut st = UnionStats {
        faces_flipped_to_agree: p.flipped,
        closing: p.closing.clone(),
        self_crossing_shells: p.self_crossing.len(),
        ..Default::default()
    };
    st.spots.extend(p.closing.hole_centres.iter().map(|&c| ("hole closed", c)));
    // Each solid with the voids directly inside it.
    let mut groups: Vec<Vec<u32>> = Vec::new();
    let mut group_vol: Vec<f64> = Vec::new();
    let mut group_of = vec![usize::MAX; p.shells.len()];
    for (i, n) in p.nests.iter().enumerate() {
        match n.kind {
            Kind::Solid | Kind::InsideOut
                if mesh::negligible(p.shells[i].volume, p.shells[i].area) && !p.self_crossing.contains(&i) =>
            {
                // Below anything a printer makes, and degenerate enough to stall a boolean.
                st.specks_left_out += 1;
                st.spots.push(("speck or sheet dropped", centre(&p.shells[i])));
            }
            Kind::Solid | Kind::InsideOut => {
                st.solids += 1;
                st.inside_out_turned += usize::from(n.kind == Kind::InsideOut);
                group_of[i] = groups.len();
                groups.push(p.shells[i].faces.clone());
                group_vol.push(p.shells[i].volume.abs());
            }
            Kind::Buried => st.buried_left_out += 1,
            Kind::Open => {
                st.open_left_out += 1;
                st.open_area_left_out += p.shells[i].area;
            }
            _ => {}
        }
    }
    for (i, n) in p.nests.iter().enumerate() {
        if matches!(n.kind, Kind::SealedVoid | Kind::InvertedSkin) {
            st.sealed_voids += 1;
            st.inverted_skins_turned += usize::from(n.kind == Kind::InvertedSkin);
            // A void with no way out traps resin: it is filled (its shell left out). A hollow
            // that drains is part of the outer shell and never comes here.
            let kind = if n.kind == Kind::InvertedSkin { "sealed inner skin filled" } else { "sealed void filled" };
            st.spots.push((kind, centre(&p.shells[i])));
            st.voids_filled_mm3 += p.shells[i].volume.abs();
        }
    }
    // The solid (group) each shell belongs to, for which solids cross which.
    let shell_group = |i: usize| match p.nests[i].kind {
        Kind::Solid | Kind::InsideOut => Some(group_of[i]).filter(|&g| g != usize::MAX),
        _ => None,
    };
    // Parts going into the boolean, each with the groups it stands for.
    let mut parts: Vec<(Manifold, Vec<usize>)> = Vec::new();
    let mut bad_group = vec![false; groups.len()];
    let mut done_group = vec![false; groups.len()];
    if let Some((h, tol)) = remesh_bad {
        // Groups with a shell crossing itself, and the open pieces, are remeshed together.
        for &s in &p.self_crossing {
            if let Some(g) = shell_group(s) {
                bad_group[g] = true;
            }
        }
        // A solid crossing itself is first mended locally: boxes round its crossings are cut out
        // of it and filled with a remesh of what was inside them. Where that fails the whole
        // solid is remeshed.
        let mut shell_of = vec![usize::MAX; p.mesh.faces.len()];
        for (i, s) in p.shells.iter().enumerate() {
            for &f in &s.faces {
                shell_of[f as usize] = i;
            }
        }
        let mut boxes_of: Vec<Vec<(P, P)>> = vec![Vec::new(); groups.len()];
        let mut crossings_of = vec![0usize; groups.len()];
        let margin = (6.0 * h).max(0.3);
        for &(a, b) in &p.pairs {
            let (sa, sb) = (shell_of[a as usize], shell_of[b as usize]);
            if sa != sb {
                continue;
            }
            if let Some(g) = shell_group(sa).filter(|&g| bad_group[g]) {
                let (ta, tb) = (p.mesh.tri(a as usize), p.mesh.tri(b as usize));
                let mut pts = super::intersect::crossing_points(&ta, &tb);
                // Faces crossing from a shared corner cross along a segment that starts there.
                let (fa, fb) = (p.mesh.faces[a as usize], p.mesh.faces[b as usize]);
                pts.extend(fa.iter().filter(|v| fb.contains(v)).map(|&v| p.mesh.verts[v as usize]));
                if pts.is_empty() {
                    // Crossing at a shared corner: the two faces' common corner and the far sides.
                    pts = ta.iter().chain(&tb).copied().collect();
                }
                let mid = pts.iter().fold([0.0; 3], |acc, v| [acc[0] + v[0], acc[1] + v[1], acc[2] + v[2]]);
                st.spots.push(("self-crossing mended", mid.map(|x| x / pts.len() as f64)));
                let lo = [0, 1, 2].map(|k| pts.iter().map(|v| v[k]).fold(f64::MAX, f64::min) - margin);
                let hi = [0, 1, 2].map(|k| pts.iter().map(|v| v[k]).fold(f64::MIN, f64::max) + margin);
                boxes_of[g].push((lo, hi));
                crossings_of[g] += 1;
            }
        }
        // Small solids touching a larger one go into a box of the larger one, filled by the remesh
        // of both: a boolean between a speck and a large surface it lies on is slow and fragile.
        let group_box: Vec<(P, P)> = groups
            .iter()
            .map(|faces| {
                let mut lo = [f64::MAX; 3];
                let mut hi = [f64::MIN; 3];
                for &f in faces {
                    for v in p.mesh.tri(f as usize) {
                        for k in 0..3 {
                            lo[k] = lo[k].min(v[k]);
                            hi[k] = hi[k].max(v[k]);
                        }
                    }
                }
                (lo, hi)
            })
            .collect();
        let mut absorbed: Vec<Vec<usize>> = vec![Vec::new(); groups.len()];
        for g in 0..groups.len() {
            if group_vol[g] >= SMALL_MM3 || !first {
                continue;
            }
            let (lo, hi) = group_box[g];
            let host = (0..groups.len())
                .filter(|&b| group_vol[b] >= SMALL_MM3)
                .filter(|&b| (0..3).all(|k| group_box[b].0[k] <= hi[k] && lo[k] <= group_box[b].1[k]))
                .max_by(|&a, &b| group_vol[a].total_cmp(&group_vol[b]));
            if let Some(b) = host {
                boxes_of[b].push((lo.map(|x| x - margin), hi.map(|x| x + margin)));
                absorbed[b].push(g);
                bad_group[b] = true;
                bad_group[g] = false;
                done_group[g] = true;
                st.small_absorbed += 1;
                st.spots.push(("small piece merged", [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0)));
            }
        }
        // One solid at a time (each mend runs in parallel inside): every Manifold made takes the
        // next id of a counter Manifold keeps for the whole process, and the order the ids go
        // out in steers how its booleans triangulate. Made in a fixed order, the result is the
        // same on every run, whatever else is running.
        let mended: Vec<(usize, Result<Manifold, String>, usize)> = (0..groups.len())
            .filter(|&g| bad_group[g] && !boxes_of[g].is_empty())
            .map(|g| {
                let boxes = merge_boxes(boxes_of[g].clone(), 2.0 * h);
                let cells: f64 =
                    boxes.iter().map(|(lo, hi)| (0..3).map(|k| (hi[k] - lo[k]) / h + 5.0).product::<f64>()).sum();
                if cells > MEND_MAX_CELLS {
                    return (g, Err(format!("boxes too large ({cells:.0} cells)")), boxes.len());
                }
                let extra: Vec<u32> = absorbed[g].iter().flat_map(|&a| groups[a].iter().copied()).collect();
                {
                    // A mend that leaves as many crossings as it found (an input folded through
                    // itself over a wide region) counts as failed for a small solid, which the
                    // first pass then remeshes whole; a large one keeps the mend for the follow-up
                    // passes (remeshing it whole costs minutes).
                    let mended = mend(&p.mesh, &groups[g], &extra, &boxes, h, tol).and_then(|m| {
                        if groups[g].len() > WHOLE_REMESH_MAX_FACES {
                            // Kept whatever is left: no need to count.
                            return Ok(m);
                        }
                        let left = super::intersect::pairs(&indexed_of(&m)).len();
                        if left > 0 && left >= crossings_of[g] {
                            Err(format!("{left} crossings left of {}", crossings_of[g]))
                        } else {
                            Ok(m)
                        }
                    });
                    (g, mended, boxes.len())
                }
            })
            .collect();
        super::remesh::trace(&format!("mended in {:.2}", t0.elapsed().as_secs_f64()));
        super::remesh::trace(&format!(
            "mended {:?}",
            mended.iter().map(|m| (m.2, m.1.as_ref().map(tri_count).map_err(|e| e.clone()))).collect::<Vec<_>>()
        ));
        for (g, m, n) in mended {
            match m {
                Ok(m) => {
                    bad_group[g] = false;
                    st.mended_groups += 1;
                    st.mend_boxes += n;
                    parts.push((m, vec![g]));
                    done_group[g] = true;
                }
                Err(e) => st.rejected_reasons.push(format!("mend: {e}")),
            }
        }
        if !first {
            // What a follow-up pass cannot mend stays as the first pass left it.
            bad_group.iter_mut().for_each(|b| *b = false);
        }
        let mut bad: Vec<[P; 3]> = Vec::new();
        for (g, faces) in groups.iter().enumerate() {
            if bad_group[g] {
                bad.extend(faces.iter().map(|&f| p.mesh.tri(f as usize)));
            }
        }
        for (s, n) in p.shells.iter().zip(&p.nests) {
            if n.kind == Kind::Open {
                bad.extend(s.faces.iter().map(|&f| p.mesh.tri(f as usize)));
            }
        }
        st.remeshed_groups = bad_group.iter().filter(|&&b| b).count();
        st.remeshed_faces_in = bad.len();
        if !bad.is_empty() {
            let (r, _) = super::remesh::remesh_simplified(&bad, h, tol);
            st.remeshed_faces_out = r.faces.len();
            let all: Vec<u32> = (0..r.faces.len() as u32).collect();
            let stands_for: Vec<usize> = (0..groups.len()).filter(|&g| bad_group[g]).collect();
            match manifold_of(&r, &all) {
                Ok(x) => parts.push((x, stands_for)),
                Err(e) => {
                    st.rejected += 1;
                    st.rejected_reasons.push(format!("remeshed: {e}"));
                }
            }
        }
    }
    // In order, for the same ids on every run (see the mends above).
    let built: Vec<(usize, Result<Manifold, String>)> = groups
        .iter()
        .enumerate()
        .filter(|(g, _)| !bad_group[*g] && !done_group[*g])
        .map(|(g, faces)| (g, manifold_of(&p.mesh, faces)))
        .collect();
    for (g, b) in built {
        match b {
            Ok(x) => parts.push((x, vec![g])),
            Err(e) => {
                st.rejected += 1;
                if st.rejected_reasons.len() < 5 {
                    st.rejected_reasons.push(e);
                }
            }
        }
    }
    // Parts whose solids cross go through the boolean together; the rest are kept as they are.
    let mut parent: Vec<u32> = (0..parts.len() as u32).collect();
    let mut join = |x: u32, y: u32| {
        let (a, b) = (
            crate::geometry::support_reader::math::find(&mut parent, x),
            crate::geometry::support_reader::math::find(&mut parent, y),
        );
        parent[a.max(b) as usize] = a.min(b);
    };
    // Parts whose boxes overlap: a crossing test misses faces meeting exactly at an edge or corner
    // (common in aligned parts), so overlap is the safe test.
    let boxes: Vec<(P, P)> = parts.iter().map(|(m, _)| bounds(m)).collect();
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| boxes[a].0[0].total_cmp(&boxes[b].0[0]));
    for (k, &a) in order.iter().enumerate() {
        for &b in &order[k + 1..] {
            if boxes[b].0[0] > boxes[a].1[0] {
                break;
            }
            if (0..3).all(|q| boxes[a].0[q] <= boxes[b].1[q] && boxes[b].0[q] <= boxes[a].1[q]) {
                join(a as u32, b as u32);
            }
        }
    }
    let mut comps: rustc_hash::FxHashMap<u32, Vec<Manifold>> = rustc_hash::FxHashMap::default();
    for (k, (m, _)) in parts.into_iter().enumerate() {
        let r = crate::geometry::support_reader::math::find(&mut parent, k as u32);
        comps.entry(r).or_default().push(m);
    }
    st.booleans = comps.values().map(|c| c.len().saturating_sub(1)).sum();
    for c in comps.values().filter(|c| c.len() > 1) {
        let largest = c.iter().map(tri_count).max().unwrap_or(0);
        for m in c.iter().filter(|m| tri_count(m) < largest) {
            let (lo, hi) = bounds(m);
            st.spots.push(("overlapping solids joined", [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0)));
        }
    }
    let failed = AtomicUsize::new(0);
    let results: Vec<Option<Manifold>> = comps
        .into_values()
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|mut c| {
            // The small parts together first, then the largest once.
            c.sort_by_key(tri_count);
            let largest = c.pop();
            let t = std::time::Instant::now();
            let n = c.len();
            let small = union_all(c, &failed);
            super::remesh::trace(&format!("{n} small parts joined in {:.1}s", t.elapsed().as_secs_f64()));
            match (largest, small) {
                (Some(l), Some(r)) => {
                    let (nl, nr) = (tri_count(&l), tri_count(&r));
                    let out = add(l, r, &failed);
                    super::remesh::trace(&format!("largest {nl} + {nr} in {:.1}s", t.elapsed().as_secs_f64()));
                    Some(out)
                }
                (l, _) => l,
            }
        })
        .collect();
    st.booleans_failed = failed.load(Ordering::Relaxed);
    super::remesh::trace(&format!("booleans done in {:.2}", t0.elapsed().as_secs_f64()));
    let mut out = Indexed::default();
    for r in results.into_iter().flatten() {
        let base = out.verts.len() as u32;
        let m = indexed_of(&r);
        out.verts.extend(m.verts);
        out.faces.extend(m.faces.into_iter().map(|f| f.map(|v| v + base)));
    }
    (out, st)
}

/// Solids under this volume (mm3) touching a larger one are taken into it by mending.
const SMALL_MM3: f64 = 1.0;

/// How far (in grid cells) a mending fill reaches past the box cut out for it.
const FILL_OVERLAP_CELLS: f64 = 0.0;

/// A solid a mend cannot help is remeshed whole only up to this many faces.
const WHOLE_REMESH_MAX_FACES: usize = 100_000;

/// Mending gives up (and the whole solid is remeshed) beyond this many grid cells in its boxes.
const MEND_MAX_CELLS: f64 = 60e6;

/// Merges boxes that overlap or come within `gap` of each other.
fn merge_boxes(mut boxes: Vec<(P, P)>, gap: f64) -> Vec<(P, P)> {
    loop {
        let mut merged = false;
        let mut out: Vec<(P, P)> = Vec::new();
        for b in boxes {
            if let Some(o) = out.iter_mut().find(|o| (0..3).all(|k| o.0[k] <= b.1[k] + gap && b.0[k] <= o.1[k] + gap)) {
                for k in 0..3 {
                    o.0[k] = o.0[k].min(b.0[k]);
                    o.1[k] = o.1[k].max(b.1[k]);
                }
                merged = true;
            } else {
                out.push(b);
            }
        }
        boxes = out;
        if !merged {
            return boxes;
        }
    }
}

/// A solid mended in boxes: the boxes cut out of it, each filled with the remesh of the solid
/// (and the small solids `extra` it takes in) inside it, all joined. The step that failed, if one does.
fn mend(m: &Indexed, faces: &[u32], extra: &[u32], boxes: &[(P, P)], h: f64, tol: f64) -> Result<Manifold, String> {
    // The fills see the solid and the small solids it has taken in.
    let all_faces: Vec<u32> = faces.iter().chain(extra).copied().collect();
    let tris: Vec<[P; 3]> = all_faces.par_iter().map(|&f| m.tri(f as usize)).collect();
    // Each fill reaches past its box, so the fill and the cut solid overlap rather than touch:
    // solids that only touch keep both faces of the contact a hair apart, and single precision
    // then folds those faces through each other.
    let reach = FILL_OVERLAP_CELLS * h;
    let t = std::time::Instant::now();
    let fill_boxes: Vec<(P, P)> =
        boxes.iter().map(|&(lo, hi)| (lo.map(|x| x - reach), hi.map(|x| x + reach))).collect();
    let all = |n: usize| (0..n as u32).collect::<Vec<u32>>();
    let failed = AtomicUsize::new(0);
    // The fills, and the solid cut by the boxes (which does not wait for them), side by side. The
    // fills become Manifolds only after the solid and the boxes, one by one in order: each
    // Manifold made takes the next id, and the ids steer how a boolean triangulates.
    let (fills, cut) = rayon::join(
        || -> Vec<Indexed> {
            // Each fill sees only the faces over or under its box (and near it), in their order.
            let seen = faces_over_boxes(&tris, &fill_boxes, super::remesh::BOX_PAD_CELLS * h + h);
            let fills: Vec<Indexed> = fill_boxes
                .par_iter()
                .zip(&seen)
                .map(|(&(lo, hi), seen)| {
                    let local: Vec<[P; 3]> = seen.iter().map(|&i| tris[i as usize]).collect();
                    super::remesh::remesh_in_box(&local, lo, hi, h, tol).0
                })
                .filter(|f| !f.faces.is_empty())
                .collect();
            super::remesh::trace(&format!("{} fills in {:.2}", boxes.len(), t.elapsed().as_secs_f64()));
            fills
        },
        || -> Result<Manifold, String> {
            // The boxes as one mesh (they are apart).
            let mut cutter: Vec<[P; 3]> = Vec::new();
            for &(lo, hi) in boxes {
                cutter.extend(super::mesh::box_tris(lo, hi));
            }
            let cut_m = Indexed::weld(&cutter);
            let solid = manifold_of(m, faces).map_err(|e| format!("solid: {e}"))?;
            let cutter = manifold_of(&cut_m, &all(cut_m.faces.len())).map_err(|e| format!("boxes: {e}"))?;
            super::remesh::trace(&format!("solid and boxes built in {:.2}", t.elapsed().as_secs_f64()));
            let cut = checked(solid.difference(&cutter), "cut");
            super::remesh::trace(&format!("solid cut in {:.2}", t.elapsed().as_secs_f64()));
            cut
        },
    );
    let cut = cut?;
    let parts: Vec<Manifold> = fills
        .iter()
        .map(|f| manifold_of(f, &all(f.faces.len())).map_err(|e| format!("fill: {e}")))
        .collect::<Result<_, _>>()?;
    let fill = union_all(parts, &failed);
    super::remesh::trace(&format!("fills joined in {:.2}", t.elapsed().as_secs_f64()));
    if failed.load(Ordering::Relaxed) > 0 {
        return Err("fill union".into());
    }
    match fill {
        Some(f) => checked(cut.union(&f), "join"),
        None => Ok(cut),
    }
}

/// For each box, the faces whose plan (x, y) extent comes within `pad` of the box's, ascending.
fn faces_over_boxes(tris: &[[P; 3]], boxes: &[(P, P)], pad: f64) -> Vec<Vec<u32>> {
    if boxes.is_empty() {
        return Vec::new();
    }
    let grown: Vec<(P, P)> = boxes.iter().map(|&(lo, hi)| (lo.map(|x| x - pad), hi.map(|x| x + pad))).collect();
    // The boxes on a coarse plan grid, for the faces to look up.
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    let mut extent = 0.0f64;
    for (a, b) in &grown {
        for k in 0..2 {
            lo[k] = lo[k].min(a[k]);
            hi[k] = hi[k].max(b[k]);
            extent = extent.max(b[k] - a[k]);
        }
    }
    let cell = extent.max(1e-3);
    let dims = [0, 1].map(|k| (((hi[k] - lo[k]) / cell).floor() as usize + 1).min(4096));
    let cell_of = |x: f64, k: usize| (((x - lo[k]) / cell).floor().max(0.0) as usize).min(dims[k] - 1);
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); dims[0] * dims[1]];
    for (b, (a, c)) in grown.iter().enumerate() {
        for x in cell_of(a[0], 0)..=cell_of(c[0], 0) {
            for y in cell_of(a[1], 1)..=cell_of(c[1], 1) {
                grid[y * dims[0] + x].push(b as u32);
            }
        }
    }
    let mut hits: Vec<(u32, u32)> = tris
        .par_iter()
        .enumerate()
        .fold(
            || (Vec::new(), Vec::new()),
            |(mut acc, mut near): (Vec<(u32, u32)>, Vec<u32>), (i, t)| {
                let tlo = [0, 1].map(|k| t[0][k].min(t[1][k]).min(t[2][k]));
                let thi = [0, 1].map(|k| t[0][k].max(t[1][k]).max(t[2][k]));
                if (0..2).any(|k| thi[k] < lo[k] || tlo[k] > hi[k]) {
                    return (acc, near);
                }
                near.clear();
                let (x0, x1, y0, y1) = (cell_of(tlo[0], 0), cell_of(thi[0], 0), cell_of(tlo[1], 1), cell_of(thi[1], 1));
                if (x1 - x0 + 1) * (y1 - y0 + 1) > 64 {
                    // A face over many cells: every box is tried.
                    near.extend(0..grown.len() as u32);
                } else {
                    for x in x0..=x1 {
                        for y in y0..=y1 {
                            near.extend_from_slice(&grid[y * dims[0] + x]);
                        }
                    }
                    near.sort_unstable();
                    near.dedup();
                }
                for &b in &near {
                    let (a, c) = grown[b as usize];
                    if (0..2).all(|k| thi[k] >= a[k] && tlo[k] <= c[k]) {
                        acc.push((b, i as u32));
                    }
                }
                (acc, near)
            },
        )
        .map(|(acc, _)| acc)
        .reduce(Vec::new, |mut a, b| {
            a.extend(b);
            a
        });
    hits.par_sort_unstable();
    let mut out = vec![Vec::new(); boxes.len()];
    for (b, i) in hits {
        out[b as usize].push(i);
    }
    out
}

/// Unions in a balanced tree, halves in parallel. A failing boolean keeps its larger operand and
/// is counted in `failed`.
fn union_all(mut parts: Vec<Manifold>, failed: &AtomicUsize) -> Option<Manifold> {
    match parts.len() {
        0 => None,
        1 => parts.pop(),
        n => {
            let rest = parts.split_off(n / 2);
            let (a, b) = rayon::join(|| union_all(parts, failed), || union_all(rest, failed));
            match (a, b) {
                (Some(a), Some(b)) => Some(add(a, b, failed)),
                (a, b) => a.or(b),
            }
        }
    }
}

fn add(a: Manifold, b: Manifold, failed: &AtomicUsize) -> Manifold {
    checked(a.union(&b), "union").unwrap_or_else(|_| {
        failed.fetch_add(1, Ordering::Relaxed);
        if tri_count(&a) >= tri_count(&b) { a } else { b }
    })
}

#[cfg(test)]
mod tests {
    use super::super::mesh::tests::cube;
    use super::*;

    #[test]
    fn each_box_sees_the_faces_over_or_under_it() {
        let tris = super::super::mesh::tests::soup(3000, 5);
        let boxes =
            vec![([-0.5, -0.5, -0.5], [0.0, 0.2, 0.1]), ([0.3, 0.3, -1.0], [0.6, 0.9, 1.0]), ([5.0; 3], [6.0; 3])];
        let pad = 0.05;
        let seen = faces_over_boxes(&tris, &boxes, pad);
        for (b, (lo, hi)) in boxes.iter().enumerate() {
            let want: Vec<u32> = (0..tris.len() as u32)
                .filter(|&i| {
                    let t = &tris[i as usize];
                    (0..2).all(|k| {
                        let (a, c) = (t[0][k].min(t[1][k]).min(t[2][k]), t[0][k].max(t[1][k]).max(t[2][k]));
                        c >= lo[k] - pad && a <= hi[k] + pad
                    })
                })
                .collect();
            assert_eq!(seen[b], want, "box {b}");
        }
    }

    #[test]
    fn two_overlapping_cubes_union_to_one_watertight_shell() {
        let mut t = cube([0.0; 3], [2.0; 3]);
        t.extend(cube([1.0; 3], [3.0; 3]));
        let (m, st) = union(&t);
        let c = mesh::check(&m, true);
        assert!(c.watertight(), "{c:?}");
        assert_eq!((c.shells, st.solids), (1, 2));
        assert!((c.volume - 15.0).abs() < 1e-9, "{}", c.volume);
    }

    #[test]
    fn a_solid_crossing_itself_is_remeshed_and_joined_to_the_clean_one() {
        // A cube with its top corner pulled down through its base crosses itself.
        let mut t: Vec<[P; 3]> = cube([0.0; 3], [2.0; 3])
            .into_iter()
            .map(|f| f.map(|v| if v == [2.0; 3] { [0.5, 0.5, -1.0] } else { v }))
            .collect();
        t.extend(cube([10.0; 3], [11.0; 3]));
        let (plain, _) = union(&t);
        assert!(!mesh::check(&plain, true).watertight());
        let (out, st) = hybrid(&t, 0.1, 0.002);
        let c = mesh::check(&out, true);
        assert!(c.watertight(), "{c:?} {st:?}");
        assert_eq!(st.mended_groups + st.remeshed_groups, 1, "{st:?}");
        assert!(c.shells >= 2, "{c:?}");
    }

    #[test]
    fn a_sealed_void_and_a_wrong_way_inner_skin_are_filled() {
        for flip in [false, true] {
            let mut t = cube([0.0; 3], [10.0; 3]);
            let mut inner = cube([1.0; 3], [9.0; 3]);
            if flip {
                for f in &mut inner {
                    f.swap(1, 2);
                }
            }
            t.extend(inner);
            let (m, st) = union(&t);
            let c = mesh::check(&m, true);
            assert_eq!((st.sealed_voids, st.inverted_skins_turned), (1, usize::from(!flip)));
            assert!((st.voids_filled_mm3 - 512.0).abs() < 1e-9, "{st:?}");
            assert!(c.watertight() && c.shells == 1, "{c:?}");
            assert!((c.volume - 1000.0).abs() < 1e-9, "{}", c.volume);
        }
    }

    #[test]
    fn a_drained_cavity_keeps_its_hollow_and_its_drain() {
        // A 10 mm box hollowed to a 6 mm cavity, drained by a 2 mm hole through its floor.
        let solid = Manifold::cube(10.0, 10.0, 10.0, false)
            .difference(&Manifold::cube(6.0, 6.0, 6.0, false).translate(2.0, 2.0, 2.0))
            .difference(&Manifold::cube(2.0, 2.0, 2.5, false).translate(4.0, 4.0, -0.25));
        let m = indexed_of(&solid);
        let expected = 1000.0 - 216.0 - 2.0 * 2.0 * 2.0;
        for (out, st) in [union(&m.tris()), hybrid(&m.tris(), 0.1, 0.005)] {
            let c = mesh::check(&out, true);
            assert!(c.watertight() && c.shells == 1, "{c:?}");
            assert!((c.volume - expected).abs() < 1e-6, "{} vs {expected}", c.volume);
            assert_eq!(st.sealed_voids, 0);
        }
    }

    #[test]
    fn a_pocket_closed_off_by_overlapping_parts_is_filled() {
        // Six 1 mm slabs overlapping at the edges of a 10 mm box close off an 8 mm pocket.
        let mut t = Vec::new();
        for k in 0..3 {
            for side in [0.0, 9.0] {
                let mut lo = [0.0; 3];
                let mut hi = [10.0; 3];
                lo[k] = side;
                hi[k] = side + 1.0;
                t.extend(cube(lo, hi));
            }
        }
        let (plain, _) = union(&t);
        assert!((mesh::check(&plain, false).volume - 488.0).abs() < 1e-6, "the union keeps the pocket");
        let (out, st) = hybrid(&t, 0.1, 0.005);
        let c = mesh::check(&out, true);
        assert!(c.watertight() && c.shells == 1, "{c:?}");
        assert!((c.volume - 1000.0).abs() < 1e-6, "{}", c.volume);
        assert!((st.voids_filled_mm3 - 512.0).abs() < 1e-6, "{st:?}");
    }
}
