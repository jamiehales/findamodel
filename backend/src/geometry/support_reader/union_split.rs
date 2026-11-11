//! Reading a welded union of rods. A closed solid that is no solid of revolution, ball or bar may
//! be several rods unioned into one shell (welded trunks and braces, a tip welded to its shaft).
//! Its faces are then clipped pieces of the rods' own facets, so each rod is found from its
//! facets' planes: the facets of one ring of a regular n-gon rod or cone all make the same angle
//! with the axis (`n.d = s`) and lie at the same offset from it (`n.c - h = K`). Three facets in a
//! row around a ring give the axis; every facet fitting it joins it.

use rustc_hash::FxHashMap;

use super::math::{P, add, cross, dot, len, mul, solve3, sub, unit};

/// Facets of distinct normals a ring must show to be a rod's.
const MIN_RING: usize = 4;
/// Tolerance on a facet's angle to the axis (`n.d`).
const TOL_S: f64 = 2e-3;
/// Tolerance on a facet's offset from the axis.
const TOL_K: f64 = 2e-3;

struct Facet {
    tris: Vec<usize>,
    n: P,
    h: f64,
    area: f64,
    c: P,
}

fn find(p: &mut [usize], mut x: usize) -> usize {
    while p[x] != x {
        p[x] = p[p[x]];
        x = p[x];
    }
    x
}

/// A rod found in a union: its triangles (indices into the solid's), its axis (point and
/// direction) and its profile (position along the axis from the point, radius): each facet's
/// vertices at the radius of its ring's corners at their height.
pub struct Rod {
    pub tris: Vec<usize>,
    pub c: P,
    pub d: P,
    pub prof: Vec<(f64, f64)>,
}

/// The rods of a solid (`tv` its triangles' corners, `ids` their welded vertex ids), or None when
/// the solid does not read as unioned rods.
pub fn split_union(tv: &[[P; 3]], ids: &[[u32; 3]]) -> Option<Vec<Rod>> {
    let nt = tv.len();
    if nt < 24 {
        return None;
    }
    let mut tn = Vec::with_capacity(nt);
    let mut ta = Vec::with_capacity(nt);
    let mut tc = Vec::with_capacity(nt);
    for t in tv {
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        ta.push(0.5 * len(n));
        tn.push(unit(n));
        tc.push(mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0));
    }
    let mut em: FxHashMap<(u32, u32), Vec<usize>> = FxHashMap::default();
    for (i, t) in ids.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            em.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    // Facets: coplanar triangles joined across an edge.
    let mut par: Vec<usize> = (0..nt).collect();
    for l in em.values() {
        if l.len() != 2 {
            continue;
        }
        let (i, j) = (l[0], l[1]);
        if ta[i] < 1e-12 || ta[j] < 1e-12 {
            continue;
        }
        if dot(tn[i], tn[j]) > 1.0 - 1e-6 && dot(tn[i], sub(tc[j], tc[i])).abs() < 2e-4 {
            let (x, y) = (find(&mut par, i), find(&mut par, j));
            par[x] = y;
        }
    }
    let mut fid: FxHashMap<usize, usize> = FxHashMap::default();
    let mut facets: Vec<Facet> = Vec::new();
    let mut fof = vec![0usize; nt];
    for (i, slot) in fof.iter_mut().enumerate() {
        let r = find(&mut par, i);
        let f = *fid.entry(r).or_insert_with(|| {
            facets.push(Facet { tris: vec![], n: [0.0; 3], h: 0.0, area: 0.0, c: [0.0; 3] });
            facets.len() - 1
        });
        facets[f].tris.push(i);
        *slot = f;
    }
    for f in facets.iter_mut() {
        let (mut n, mut c, mut a) = ([0.0; 3], [0.0; 3], 0.0);
        for &i in &f.tris {
            n = add(n, mul(tn[i], ta[i]));
            c = add(c, mul(tc[i], ta[i]));
            a += ta[i];
        }
        f.n = unit(n);
        f.c = mul(c, 1.0 / a.max(1e-30));
        f.area = a;
        f.h = dot(f.n, f.c);
    }
    let nf = facets.len();
    // Facet neighbours with their shared edges, in a fixed order.
    let mut nb: FxHashMap<(usize, usize), Vec<(P, P)>> = FxHashMap::default();
    let vtx = |i: usize, v: u32| -> P {
        let t = ids[i];
        let k = t.iter().position(|&x| x == v).expect("the edge's vertex is the triangle's");
        tv[i][k]
    };
    let mut edge_keys: Vec<&(u32, u32)> = em.keys().collect();
    edge_keys.sort_unstable();
    for &(a, b) in edge_keys {
        let l = &em[&(a, b)];
        if l.len() != 2 {
            continue;
        }
        let (fa, fb) = (fof[l[0]], fof[l[1]]);
        if fa == fb {
            continue;
        }
        let e = (vtx(l[0], a), vtx(l[0], b));
        nb.entry((fa.min(fb), fa.max(fb))).or_default().push(e);
    }
    let mut adj: Vec<Vec<usize>> = vec![vec![]; nf];
    let mut pairs: Vec<&(usize, usize)> = nb.keys().collect();
    pairs.sort_unstable();
    for &(a, b) in pairs {
        adj[a].push(b);
        adj[b].push(a);
    }
    let meridian = |c: P, d: P, es: &Vec<(P, P)>| -> bool {
        // Every shared edge lies in a plane through the axis.
        es.iter().all(|(p, q)| {
            let l = len(sub(*q, *p));
            l < 1e-9 || (dot(cross(sub(*p, c), sub(*q, c)), d) / l).abs() < 2e-3
        })
    };
    let fits = |f: &Facet, c: P, d: P, s: f64, k: f64| -> bool {
        if (dot(f.n, d) - s).abs() > TOL_S || (dot(f.n, c) - f.h - k).abs() > TOL_K {
            return false;
        }
        let u = sub(f.n, mul(d, s));
        let w = sub(f.c, c);
        let w = sub(w, mul(d, dot(w, d)));
        len(u) > 1e-3 && dot(u, w) > 0.0
    };
    // Hypotheses from facets in a row: F - G - H.
    type Hyp = (f64, Vec<usize>, P, P);
    let mut hyps: Vec<Hyp> = Vec::new();
    let mut seen: rustc_hash::FxHashSet<Vec<usize>> = rustc_hash::FxHashSet::default();
    for g in 0..nf {
        let ng = &adj[g];
        for x in 0..ng.len() {
            for y in x + 1..ng.len() {
                let (f, h) = (ng[x], ng[y]);
                let (a, b) = (sub(facets[g].n, facets[f].n), sub(facets[h].n, facets[f].n));
                if len(a) < 1e-2 || len(b) < 1e-2 || len(sub(facets[h].n, facets[g].n)) < 1e-2 {
                    continue;
                }
                let dd = cross(a, b);
                if len(dd) < 1e-5 {
                    continue;
                }
                let mut d = unit(dd);
                if d[2] < 0.0 || (d[2] == 0.0 && (d[1] < 0.0 || (d[1] == 0.0 && d[0] < 0.0))) {
                    d = mul(d, -1.0);
                }
                let rhs = [facets[g].h - facets[f].h, facets[h].h - facets[f].h, dot(d, facets[g].c)];
                let Some(c) = solve3([a, b, d], rhs) else { continue };
                let s = dot(facets[f].n, d);
                let k = dot(facets[f].n, c) - facets[f].h;
                if !meridian(c, d, &nb[&(f.min(g), f.max(g))]) || !meridian(c, d, &nb[&(h.min(g), h.max(g))]) {
                    continue;
                }
                if ![f, g, h].iter().all(|&q| fits(&facets[q], c, d, s, k)) {
                    continue;
                }
                let mem: Vec<usize> = (0..nf).filter(|&q| fits(&facets[q], c, d, s, k)).collect();
                if !seen.insert(mem.clone()) {
                    continue;
                }
                let area: f64 = mem.iter().map(|&q| facets[q].area).sum();
                hyps.push((area, mem, c, d));
            }
        }
    }
    hyps.sort_by(|x, y| y.0.total_cmp(&x.0));
    let distinct = |mem: &[usize]| -> usize {
        let mut ns: Vec<P> = Vec::new();
        for &q in mem {
            if !ns.iter().any(|n| dot(*n, facets[q].n) > 0.999) {
                ns.push(facets[q].n);
            }
        }
        ns.len()
    };
    // Axis groups: (point, direction, facets).
    let mut groups: Vec<(P, P, Vec<usize>)> = Vec::new();
    let mut claim = vec![usize::MAX; nf];
    let mut ring_of = vec![usize::MAX; nf];
    for (hi, (_, mem, c, d)) in hyps.iter().enumerate() {
        let free: Vec<usize> = mem.iter().copied().filter(|&q| claim[q] == usize::MAX).collect();
        if distinct(&free) < MIN_RING.min(distinct(mem)).max(3) || distinct(mem) < MIN_RING {
            continue;
        }
        let gi = groups.iter().position(|(gc, gd, _)| {
            len(cross(*gd, *d)) < 3e-3 && {
                let w = sub(*c, *gc);
                len(sub(w, mul(*gd, dot(w, *gd)))) < 5e-3
            }
        });
        let gi = match gi {
            Some(i) => i,
            None => {
                groups.push((*c, *d, vec![]));
                groups.len() - 1
            }
        };
        for q in free {
            claim[q] = gi;
            ring_of[q] = hi;
            groups[gi].2.push(q);
        }
    }
    if groups.len() < 2 {
        return None;
    }
    // Caps: an unclaimed facet across a group's axis, within its radius, at one of its ends.
    let ext: Vec<(f64, f64, f64)> = groups
        .iter()
        .map(|(c, d, fs)| {
            let (mut lo, mut hi, mut r) = (f64::MAX, f64::MIN, 0.0f64);
            for &q in fs {
                for &i in &facets[q].tris {
                    for v in &tv[i] {
                        let w = sub(*v, *c);
                        let t = dot(w, *d);
                        lo = lo.min(t);
                        hi = hi.max(t);
                        r = r.max(len(sub(w, mul(*d, t))));
                    }
                }
            }
            (lo, hi, r)
        })
        .collect();
    for q in 0..nf {
        if claim[q] != usize::MAX {
            continue;
        }
        let mut best: Option<(f64, usize)> = None;
        for (gi, (c, d, _)) in groups.iter().enumerate() {
            if dot(facets[q].n, *d).abs() < 0.999 {
                continue;
            }
            let (lo, hi, r) = ext[gi];
            let ok = facets[q].tris.iter().all(|&i| {
                tv[i].iter().all(|v| {
                    let w = sub(*v, *c);
                    let t = dot(w, *d);
                    t >= lo - 1e-3 && t <= hi + 1e-3 && len(sub(w, mul(*d, t))) <= r + 1e-3
                })
            });
            if !ok {
                continue;
            }
            let w = sub(facets[q].c, *c);
            let t = dot(w, *d);
            let e = (t - lo).abs().min((t - hi).abs());
            if best.is_none_or(|b| e < b.0) {
                best = Some((e, gi));
            }
        }
        if let Some((_, gi)) = best {
            claim[q] = gi;
            groups[gi].2.push(q);
        }
    }
    // The rest: joined to the group they share the longest edge with.
    let total: f64 = facets.iter().map(|f| f.area).sum();
    loop {
        let mut changed = false;
        for q in 0..nf {
            if claim[q] != usize::MAX {
                continue;
            }
            let mut w: Vec<(usize, f64)> = Vec::new();
            for &o in &adj[q] {
                if claim[o] == usize::MAX {
                    continue;
                }
                let l: f64 = nb[&(q.min(o), q.max(o))].iter().map(|(p, r)| len(sub(*r, *p))).sum();
                match w.iter_mut().find(|x| x.0 == claim[o]) {
                    Some(x) => x.1 += l,
                    None => w.push((claim[o], l)),
                }
            }
            if let Some(&(gi, _)) = w.iter().max_by(|x, y| x.1.total_cmp(&y.1)) {
                claim[q] = gi;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let left: f64 = (0..nf).filter(|&q| claim[q] == usize::MAX).map(|q| facets[q].area).sum();
    if left > 0.02 * total {
        return None;
    }
    // Profiles: a facet's plane gives the ring's in-radius at each height; the ring's corners
    // (vertices on its meridian edges) give the ratio to the corner radius.
    let rho = |f: &Facet, c: P, d: P, t: f64| -> f64 {
        let s = dot(f.n, d);
        let q = (1.0 - s * s).max(1e-12).sqrt();
        -(dot(f.n, add(c, mul(d, t))) - f.h) / q
    };
    let mut ratio: FxHashMap<usize, f64> = FxHashMap::default();
    for q in 0..nf {
        if ring_of[q] == usize::MAX {
            continue;
        }
        let (c, d, _) = &groups[claim[q]];
        for &i in &facets[q].tris {
            for v in &tv[i] {
                let w = sub(*v, *c);
                let t = dot(w, *d);
                let r = len(sub(w, mul(*d, t)));
                let rh = rho(&facets[q], *c, *d, t);
                if rh > 1e-6 {
                    let e = ratio.entry(ring_of[q]).or_insert(1.0);
                    *e = e.max(r / rh);
                }
            }
        }
    }
    let mut out: Vec<Rod> = groups.iter().map(|(c, d, _)| Rod { tris: vec![], c: *c, d: *d, prof: vec![] }).collect();
    for q in 0..nf {
        if claim[q] == usize::MAX {
            continue;
        }
        let g = claim[q];
        out[g].tris.extend(facets[q].tris.iter().copied());
        if ring_of[q] == usize::MAX {
            continue;
        }
        let k = ratio.get(&ring_of[q]).copied().unwrap_or(1.0).min(1.5);
        let (c, d) = (out[g].c, out[g].d);
        for &i in &facets[q].tris {
            for v in &tv[i] {
                let t = dot(sub(*v, c), d);
                let rh = rho(&facets[q], c, d, t);
                if rh > 0.0 {
                    out[g].prof.push((t, rh * k));
                }
            }
        }
    }
    out.retain(|g| !g.tris.is_empty() && g.prof.len() >= 2);
    // A rod whose faces leave a stretch of its axis bare (two rods in line, meeting inside a ball
    // or another rod) is not read here: left to the straight-stretch split.
    for g in &out {
        let mut iv: Vec<(f64, f64)> = g
            .tris
            .iter()
            .map(|&i| {
                let ts: Vec<f64> = tv[i].iter().map(|v| dot(sub(*v, g.c), g.d)).collect();
                (ts.iter().copied().fold(f64::MAX, f64::min), ts.iter().copied().fold(f64::MIN, f64::max))
            })
            .collect();
        iv.sort_by(|x, y| x.0.total_cmp(&y.0));
        let rmax = g.prof.iter().map(|x| x.1).fold(0.0, f64::max);
        let mut reach = iv[0].1;
        for &(a, b) in &iv[1..] {
            if a - reach > (0.5 * rmax).max(0.1) {
                return None;
            }
            reach = reach.max(b);
        }
    }
    if out.len() < 2 { None } else { Some(out) }
}

#[cfg(test)]
mod tests {
    use super::super::contacts::tests::frustum;
    use super::*;

    fn ids_of(tv: &[[P; 3]]) -> Vec<[u32; 3]> {
        let mut seen: FxHashMap<[u64; 3], u32> = FxHashMap::default();
        tv.iter()
            .map(|t| {
                t.map(|v| {
                    let n = seen.len() as u32;
                    *seen.entry(v.map(f64::to_bits)).or_insert(n)
                })
            })
            .collect()
    }

    #[test]
    fn too_few_triangles_or_a_single_rod_is_no_union() {
        let small = frustum([0.0; 3], [0.0, 0.0, 1.0], 0.2, 0.2, 4);
        assert!(split_union(&small, &ids_of(&small)).is_none());
        let rod = frustum([0.0; 3], [0.0, 0.0, 5.0], 0.4, 0.4, 16);
        assert!(split_union(&rod, &ids_of(&rod)).is_none(), "one axis is one rod, not a union");
    }

    #[test]
    fn two_rods_side_by_side_are_found_on_their_own_axes() {
        // Two separate 12-sided rods in one triangle list: each ring's facets give its axis.
        let mut tv = frustum([0.0; 3], [0.0, 0.0, 5.0], 0.4, 0.4, 12);
        tv.extend(frustum([3.0, 0.0, 0.0], [3.0, 0.0, 5.0], 0.4, 0.4, 12));
        let rods = split_union(&tv, &ids_of(&tv)).expect("two rods");
        assert_eq!(rods.len(), 2);
        for rod in &rods {
            assert!(dot(rod.d, [0.0, 0.0, 1.0]).abs() > 0.999);
            assert!(rod.c[0].abs() < 1e-6 || (rod.c[0] - 3.0).abs() < 1e-6, "{:?}", rod.c);
        }
    }
}
