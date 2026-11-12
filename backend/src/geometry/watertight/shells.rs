//! Which closed shells sit inside which, and what each one is: a solid, a sealed void (an
//! inward-facing skin with no drain, a resin trap), an inner skin facing the wrong way (a closed
//! shell inside another with the same orientation, winding 2 inside, large enough to be the
//! other's hollow), a part buried in another, or an open or non-manifold piece.

use rayon::prelude::*;
use rustc_hash::FxHashSet;

use super::mesh::{Indexed, Shell};
use crate::geometry::support_reader::math::{BoxIndex, P, ray_hits};

/// A shell inside a same-orientation shell is its inverted inner skin when it holds at least this
/// share of the outer's volume (a buried part is far smaller than the body it is in).
pub const INVERTED_SKIN_SHARE: f64 = 0.25;
/// Points on each shell tried against the others.
const SAMPLES: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Kind {
    /// Outermost, or inside a void: material.
    Solid,
    /// Facing inward inside a solid, not joined to its outside: a sealed void (a resin trap,
    /// filled by the repair).
    SealedVoid,
    /// Facing outward inside a solid of the same orientation, large: an inner skin facing the
    /// wrong way. Flipped, it is a sealed void.
    InvertedSkin,
    /// Facing outward inside a solid, small: a buried part, no effect on the solid.
    Buried,
    /// Facing inward at the top level: a whole shell inside out.
    InsideOut,
    /// Not closed, not manifold or not orientable.
    Open,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Nest {
    pub kind: Kind,
    /// The innermost closed shell holding this one.
    pub parent: Option<usize>,
    pub depth: usize,
}

/// Points on a shell's faces, spread through its face list.
fn samples(m: &Indexed, s: &Shell) -> Vec<P> {
    let n = s.faces.len();
    (0..SAMPLES.min(n))
        .map(|k| {
            let t = m.tri(s.faces[k * n / SAMPLES.min(n)] as usize);
            [0, 1, 2].map(|i| t[0][i] * 0.31 + t[1][i] * 0.33 + t[2][i] * 0.36)
        })
        .collect()
}

/// Classifies every shell. `crossing` holds the pairs of shells whose faces cross.
pub fn nest(m: &Indexed, shells: &[Shell], crossing: &FxHashSet<(usize, usize)>) -> Vec<Nest> {
    let closed: Vec<usize> = (0..shells.len()).filter(|&i| shells[i].closed).collect();
    let mut tris: Vec<[P; 3]> = Vec::new();
    let mut tri_shell: Vec<usize> = Vec::new();
    for &i in &closed {
        for &f in &shells[i].faces {
            tris.push(m.tri(f as usize));
            tri_shell.push(i);
        }
    }
    let zmax = tris.iter().flat_map(|t| t.iter().map(|v| v[2])).fold(f64::MIN, f64::max) + 1.0;
    let index = BoxIndex::new(&tris, 2.0);
    // A slightly tilted ray up from each sample: the shells it leaves an odd number of times.
    let dir = [0.000_731, 0.000_427, 1.0];
    let containers = |p: P| -> FxHashSet<usize> {
        let top = [p[0] + dir[0] * (zmax - p[2]), p[1] + dir[1] * (zmax - p[2]), zmax];
        let lo = [p[0].min(top[0]), p[1].min(top[1]), p[2]];
        let hi = [p[0].max(top[0]), p[1].max(top[1]), zmax];
        let mut odd: FxHashSet<usize> = FxHashSet::default();
        for i in index.query(lo, hi) {
            if ray_hits(p, dir, &tris[i as usize]) {
                let s = tri_shell[i as usize];
                if !odd.remove(&s) {
                    odd.insert(s);
                }
            }
        }
        odd
    };
    let inside: Vec<Vec<usize>> = (0..shells.len())
        .into_par_iter()
        .map(|b| {
            let pts = samples(m, &shells[b]);
            let sets: Vec<FxHashSet<usize>> = pts.iter().map(|&p| containers(p)).collect();
            let mut held: Vec<usize> = closed
                .iter()
                .copied()
                .filter(|&a| a != b)
                .filter(|&a| !crossing.contains(&(a.min(b), a.max(b))))
                .filter(|&a| sets.iter().all(|s| s.contains(&a)))
                .filter(|&a| (0..3).all(|k| shells[a].lo[k] <= shells[b].lo[k] && shells[a].hi[k] >= shells[b].hi[k]))
                .collect();
            held.sort_by(|&x, &y| shells[x].volume.abs().total_cmp(&shells[y].volume.abs()));
            held
        })
        .collect();
    (0..shells.len())
        .map(|b| {
            let parent = inside[b].first().copied();
            let s = &shells[b];
            let kind = if !s.closed {
                Kind::Open
            } else {
                match parent {
                    None if s.volume < 0.0 => Kind::InsideOut,
                    None => Kind::Solid,
                    Some(a) => {
                        let pa = &shells[a];
                        // The parent's own sense: material if it faces out, unless it is itself a
                        // void (an island in a cavity sits in a negative shell).
                        let parent_material = pa.volume > 0.0;
                        match (parent_material, s.volume > 0.0) {
                            (true, false) => Kind::SealedVoid,
                            (true, true) if s.volume >= INVERTED_SKIN_SHARE * pa.volume => Kind::InvertedSkin,
                            (true, true) => Kind::Buried,
                            (false, true) => Kind::Solid,
                            (false, false) => Kind::InsideOut,
                        }
                    }
                }
            };
            Nest { kind, parent, depth: inside[b].len() }
        })
        .collect()
}

/// Shells with faces crossing faces of their own shell.
pub fn self_crossing_shells(m: &Indexed, shells: &[Shell], face_pairs: &[(u32, u32)]) -> Vec<usize> {
    let mut of = vec![0usize; m.faces.len()];
    for (i, s) in shells.iter().enumerate() {
        for &f in &s.faces {
            of[f as usize] = i;
        }
    }
    let mut out: Vec<usize> = face_pairs
        .iter()
        .map(|&(a, b)| (of[a as usize], of[b as usize]))
        .filter(|(x, y)| x == y)
        .map(|x| x.0)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Shell pairs whose faces cross, from crossing face pairs.
pub fn crossing_shells(m: &Indexed, shells: &[Shell], face_pairs: &[(u32, u32)]) -> FxHashSet<(usize, usize)> {
    let mut of = vec![0usize; m.faces.len()];
    for (i, s) in shells.iter().enumerate() {
        for &f in &s.faces {
            of[f as usize] = i;
        }
    }
    face_pairs
        .iter()
        .filter_map(|&(a, b)| {
            let (x, y) = (of[a as usize], of[b as usize]);
            (x != y).then(|| (x.min(y), x.max(y)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::mesh::tests::cube;
    use super::super::mesh::{edges, shell_summaries};
    use super::*;

    fn flipped(mut t: Vec<[P; 3]>) -> Vec<[P; 3]> {
        for f in &mut t {
            f.swap(1, 2);
        }
        t
    }

    fn kinds(tris: &[[P; 3]]) -> Vec<Kind> {
        let m = Indexed::weld(tris);
        let e = edges(&m);
        let shells = shell_summaries(&m, &e);
        let pairs = super::super::intersect::pairs(&m);
        let crossing = crossing_shells(&m, &shells, &pairs);
        let mut k: Vec<(f64, Kind)> =
            nest(&m, &shells, &crossing).iter().zip(&shells).map(|(n, s)| (s.lo[0], n.kind)).collect();
        k.sort_by(|a, b| a.0.total_cmp(&b.0));
        k.into_iter().map(|x| x.1).collect()
    }

    #[test]
    fn a_hollow_box_has_a_sealed_void_and_a_wrong_way_skin_is_found() {
        let outer = cube([0.0; 3], [10.0; 3]);
        let inner = cube([1.0; 3], [9.0; 3]);
        let mut hollow = outer.clone();
        hollow.extend(flipped(inner.clone()));
        assert_eq!(kinds(&hollow), vec![Kind::Solid, Kind::SealedVoid]);
        let mut wrong = outer.clone();
        wrong.extend(inner);
        assert_eq!(kinds(&wrong), vec![Kind::Solid, Kind::InvertedSkin]);
        let mut buried = outer;
        buried.extend(cube([4.0; 3], [5.0; 3]));
        assert_eq!(kinds(&buried), vec![Kind::Solid, Kind::Buried]);
    }

    #[test]
    fn overlapping_parts_are_both_solid() {
        let mut t = cube([0.0; 3], [10.0; 3]);
        t.extend(cube([5.0, 2.0, 2.0], [15.0, 8.0, 8.0]));
        assert_eq!(kinds(&t), vec![Kind::Solid, Kind::Solid]);
    }
}
