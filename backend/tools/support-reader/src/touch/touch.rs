// ------------------------------------------------------------- winding number

/// Below this, a support is in the raft band and is not judged.
const RAFT_BAND: f64 = 3.5;
/// A loop smaller than this in area is float noise.
const MIN_LOOP_AREA: f64 = 1e-6;
/// The two sides of a loop's model face are tried this far off it.
const SIDE_STEP: f64 = 0.005;
/// A tip resting flush: a shell with no loop this near the model.
const FLUSH_GAP: f64 = 0.03;
/// A loop this much longer than round (perimeter squared over 4 pi area) is elongated.
const ELONGATED: f64 = 2.5;
/// A pass-through's two loops this close, each this small, with the support ending this soon past
/// the far one: a tip through a thin wall.
const THIN_WALL_SPACING: f64 = 2.0;
const THIN_WALL_LOOP_D: f64 = 1.5;
const THIN_WALL_END: f64 = 3.0;
/// The owner's ruling (2026-11-10): a tip poking through a thin wall is still a tip.
const THIN_WALL_IS_TOUCH: bool = true;
/// Two loops of one patch this close in centre and width are one loop on a doubled surface.
const COINCIDENT_MM: f64 = 0.03;

fn area_vec(t: &[P; 3]) -> P {
    mul(cross(sub(t[1], t[0]), sub(t[2], t[0])), 0.5)
}

/// The solid angle a triangle subtends at a point (Van Oosterom and Strackee).
fn solid_angle(t: &[P; 3], p: P) -> f64 {
    let (a, b, c) = (sub(t[0], p), sub(t[1], p), sub(t[2], p));
    let (la, lb, lc) = (len(a), len(b), len(c));
    let num = dot(a, cross(b, c));
    let den = la * lb * lc + dot(a, b) * lc + dot(b, c) * la + dot(c, a) * lb;
    2.0 * num.atan2(den)
}

struct Cell {
    ids: Vec<u32>,
    c: P,
    n: P,
    r: f64,
}

/// The generalised winding number of a triangle soup: exact near, dipoles of cells far away.
struct Wind {
    tris: Vec<[P; 3]>,
    fine: Vec<Cell>,
    coarse: Vec<(Cell, Vec<usize>)>,
    lo: P,
    hi: P,
}

const FINE: f64 = 2.0;
const COARSE: f64 = 16.0;
const FAR: f64 = 3.0;

impl Wind {
    fn new(tris: Vec<[P; 3]>) -> Self {
        let key = |p: P, s: f64| p.map(|x| (x / s).floor() as i32);
        let mut by: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for (i, t) in tris.iter().enumerate() {
            let c = mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0);
            by.entry(key(c, FINE)).or_default().push(i as u32);
            for v in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        let mut fine = Vec::new();
        for (_, ids) in by {
            let (mut c, mut w, mut n) = ([0.0; 3], 0.0, [0.0; 3]);
            for &i in &ids {
                let t = &tris[i as usize];
                let a = area_vec(t);
                let m = len(a).max(1e-12);
                c = add(c, mul(add(add(t[0], t[1]), t[2]), m / 3.0));
                w += m;
                n = add(n, a);
            }
            let c = mul(c, 1.0 / w);
            let r = ids.iter().flat_map(|&i| tris[i as usize]).map(|v| dist(v, c)).fold(0.0, f64::max);
            fine.push(Cell { ids, c, n, r });
        }
        let mut groups: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
        for (k, f) in fine.iter().enumerate() {
            groups.entry(key(f.c, COARSE)).or_default().push(k);
        }
        let coarse = groups
            .into_values()
            .map(|kids| {
                let (mut c, mut w, mut n) = ([0.0; 3], 0.0, [0.0; 3]);
                for &k in &kids {
                    let m = (fine[k].ids.len() as f64).max(1.0);
                    c = add(c, mul(fine[k].c, m));
                    w += m;
                    n = add(n, fine[k].n);
                }
                let c = mul(c, 1.0 / w);
                let r = kids.iter().map(|&k| dist(fine[k].c, c) + fine[k].r).fold(0.0, f64::max);
                (Cell { ids: Vec::new(), c, n, r }, kids)
            })
            .collect();
        Wind { tris, fine, coarse, lo, hi }
    }

    fn w(&self, p: P) -> f64 {
        if (0..3).any(|k| p[k] < self.lo[k] - 0.01 || p[k] > self.hi[k] + 0.01) {
            return 0.0;
        }
        let dipole = |cell: &Cell| {
            let d = sub(cell.c, p);
            let l = len(d);
            dot(cell.n, d) / (l * l * l)
        };
        let mut s = 0.0;
        for (cc, kids) in &self.coarse {
            if dist(p, cc.c) > FAR * cc.r + 1e-9 {
                s += dipole(cc);
                continue;
            }
            for &k in kids {
                let f = &self.fine[k];
                if dist(p, f.c) > FAR * f.r + 1e-9 {
                    s += dipole(f);
                } else {
                    for &i in &f.ids {
                        s += solid_angle(&self.tris[i as usize], p);
                    }
                }
            }
        }
        s / (4.0 * std::f64::consts::PI)
    }

    fn inside(&self, p: P) -> bool {
        self.w(p) > 0.5
    }
}

// ------------------------------------------------------------- loops with their segments

type Key = (bool, u32, u32, u32);

struct Seg {
    a: Key,
    pa: P,
    b: Key,
    pb: P,
    /// The support triangle and the model triangle it lies on.
    ti: u32,
    mi: u32,
}

fn segments_of(tris: &[[P; 3]], ids: &[[u32; 3]], lo: P, hi: P, model: &Model) -> Vec<Seg> {
    let mut out = Vec::new();
    let candidates = model.near(lo, hi);
    for (ti, t) in tris.iter().enumerate() {
        let tlo: P = [0, 1, 2].map(|k| t.iter().map(|v| v[k]).fold(f64::MAX, f64::min));
        let thi: P = [0, 1, 2].map(|k| t.iter().map(|v| v[k]).fold(f64::MIN, f64::max));
        for &b in &candidates {
            let u = &model.tris[b as usize];
            if (0..3).any(|k| {
                u.iter().map(|v| v[k]).fold(f64::MIN, f64::max) < tlo[k] || u.iter().map(|v| v[k]).fold(f64::MAX, f64::min) > thi[k]
            }) {
                continue;
            }
            if let Some(((pa, fa), (pb, fb))) = cross_segment(t, &ids[ti], u, &model.ids[b as usize]) {
                let name = |f: (bool, u32, u32)| (f.0, f.1, f.2, if f.0 { ti as u32 } else { b });
                out.push(Seg { a: name(fa), pa, b: name(fb), pb, ti: ti as u32, mi: b });
            }
        }
    }
    out
}

/// One loop: its points, whether it closed, and its segments.
struct Walk {
    points: Vec<P>,
    closed: bool,
    joined: bool,
    segs: Vec<usize>,
}

fn walks_of(segments: &[Seg]) -> Vec<Walk> {
    let mut at: HashMap<Key, Vec<u32>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        at.entry(s.a).or_default().push(i as u32);
        at.entry(s.b).or_default().push(i as u32);
    }
    let mut used = vec![false; segments.len()];
    let walk = |start: Key, used: &mut Vec<bool>| -> Option<Walk> {
        let mut node = start;
        let (mut points, mut segs) = (Vec::new(), Vec::new());
        loop {
            let Some(i) = at[&node].iter().copied().find(|&i| !used[i as usize]) else { break };
            used[i as usize] = true;
            segs.push(i as usize);
            let s = &segments[i as usize];
            let (from, to, target) = if s.a == node { (s.pa, s.pb, s.b) } else { (s.pb, s.pa, s.a) };
            if points.is_empty() {
                points.push(from);
            }
            points.push(to);
            node = target;
            if node == start {
                return Some(Walk { points, closed: true, joined: false, segs });
            }
        }
        (!points.is_empty()).then_some(Walk { points, closed: false, joined: false, segs })
    };
    let mut walks = Vec::new();
    let ends: Vec<Key> = at.iter().filter(|(_, l)| l.len() % 2 == 1).map(|(k, _)| *k).collect();
    for start in ends {
        if let Some(w) = walk(start, &mut used) {
            walks.push(w);
        }
    }
    for i in 0..segments.len() {
        if !used[i] {
            if let Some(w) = walk(segments[i].a, &mut used) {
                walks.push(w);
            }
        }
    }
    // Numeric cracks: join open chains whose ends meet.
    let mut i = 0;
    while i < walks.len() {
        if walks[i].closed {
            i += 1;
            continue;
        }
        let (first, last) = (walks[i].points[0], *walks[i].points.last().unwrap());
        if dist(first, last) <= JOIN_GAP {
            walks[i].closed = true;
            walks[i].joined = true;
            continue;
        }
        let mut merged = false;
        for j in 0..walks.len() {
            if j == i || walks[j].closed {
                continue;
            }
            let (f2, l2) = (walks[j].points[0], *walks[j].points.last().unwrap());
            let rev = if dist(last, f2) <= JOIN_GAP {
                Some(false)
            } else if dist(last, l2) <= JOIN_GAP {
                Some(true)
            } else {
                None
            };
            if let Some(rev) = rev {
                let other = walks.remove(j);
                let k = if j < i { i - 1 } else { i };
                let mut pts = other.points;
                if rev {
                    pts.reverse();
                }
                walks[k].points.extend(pts);
                walks[k].segs.extend(other.segs);
                walks[k].joined = true;
                i = k;
                merged = true;
                break;
            }
        }
        if !merged {
            i += 1;
        }
    }
    walks
}

/// Area, unit normal, centre and perimeter of a loop's points.
fn loop_stats(points: &[P], closed: bool) -> (f64, P, P, f64) {
    let (mut vector, mut perimeter, mut centre) = ([0.0; 3], 0.0, [0.0; 3]);
    let origin = points[0];
    for k in 0..points.len() {
        let (a, b) = (points[k], points[(k + 1) % points.len()]);
        vector = add(vector, cross(sub(a, origin), sub(b, origin)));
        if k + 1 < points.len() || closed {
            let l = dist(a, b);
            perimeter += l;
            centre = add(centre, mul(add(a, b), l / 2.0));
        }
    }
    (len(vector) / 2.0, unit(vector), mul(centre, 1.0 / perimeter.max(1e-12)), perimeter)
}

fn eq_d(area: f64) -> f64 {
    2.0 * (area / std::f64::consts::PI).sqrt()
}

// ------------------------------------------------------------- patches

struct LoopOut {
    points: Vec<P>,
    area: f64,
    normal: P,
    centre: P,
    perimeter: f64,
    closed: bool,
}

/// One inside patch of a shell.
struct Patch {
    loops: Vec<usize>,
    tris: Vec<usize>,
    inside_verts: Vec<P>,
}

struct ShellCall {
    tris: Vec<[P; 3]>,
    /// Per triangle: its patch, if it has any part inside.
    patch_of: Vec<Option<usize>>,
    patches: Vec<Patch>,
    loops: Vec<LoopOut>,
    /// Loops on a surface that does not divide inside from outside.
    inner_loops: usize,
    /// Loops lying on another of the same patch: a doubled surface.
    coincident: usize,
    buried: bool,
    lo: P,
    hi: P,
    verts: Vec<P>,
    inside: Vec<bool>,
}

fn find(p: &mut [usize], mut x: usize) -> usize {
    while p[x] != x {
        p[x] = p[p[x]];
        x = p[x];
    }
    x
}

fn call_shell(tris: Vec<[P; 3]>, model: &Model, wind: &Wind) -> ShellCall {
    let (ids, verts) = weld_ids(&tris);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for v in &verts {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let inside: Vec<bool> = verts.iter().map(|v| wind.inside(*v)).collect();
    let segments = segments_of(&tris, &ids, lo, hi, model);
    let walks = walks_of(&segments);
    // The support's own solid: where overlapping primitives were joined into one shell, a primitive's
    // surface inside another is not the support's surface, and its loops are not where it meets the model.
    // Each closed solid of the shell on its own, so primitives wound either way still count.
    let own: Vec<Wind> = if walks.is_empty() { Vec::new() } else { solids(&tris).into_iter().map(Wind::new).collect() };
    let in_own = |p: P| own.iter().any(|w| w.w(p).abs() > 0.5);
    // A loop is a boundary when the model face it lies on has inside on one side and outside on the other.
    let mut boundary = vec![false; walks.len()];
    for (k, w) in walks.iter().enumerate() {
        let picks: Vec<usize> = (0..3).map(|j| w.segs[j * w.segs.len() / 3]).collect();
        let mut votes = 0;
        for &s in &picks {
            let seg = &segments[s];
            let m = mul(add(seg.pa, seg.pb), 0.5);
            let n = unit(area_vec(&model.tris[seg.mi as usize]));
            let (a, b) = (wind.inside(add(m, mul(n, SIDE_STEP))), wind.inside(sub(m, mul(n, SIDE_STEP))));
            let ns = unit(area_vec(&tris[seg.ti as usize]));
            let (c, d) = (in_own(add(m, mul(ns, SIDE_STEP))), in_own(sub(m, mul(ns, SIDE_STEP))));
            if a != b && c != d {
                votes += 1;
            }
        }
        boundary[k] = votes * 2 > picks.len();
    }
    // Pieces of the inside: inside vertices and the loops' ends, joined where the inside runs between
    // them. A triangle crossing the surface is split by its segments, so one triangle can hold parts of
    // two patches.
    let nv = verts.len();
    let mut item: HashMap<Key, usize> = HashMap::new();
    let mut points: Vec<P> = Vec::new();
    let mut on_edge: HashMap<(u32, u32), Vec<(f64, usize)>> = HashMap::new();
    for (k, w) in walks.iter().enumerate() {
        if !boundary[k] {
            continue;
        }
        for &s in &w.segs {
            let seg = &segments[s];
            for (key, at) in [(seg.a, seg.pa), (seg.b, seg.pb)] {
                if item.contains_key(&key) {
                    continue;
                }
                let id = nv + points.len();
                item.insert(key, id);
                points.push(at);
                // On a support edge: by its name, or, where it fell on a model edge too, by where it is.
                let tri = ids[seg.ti as usize];
                let edges: Vec<(u32, u32)> = if !key.0 {
                    vec![(key.1, key.2)]
                } else {
                    [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])]
                        .into_iter()
                        .map(|(a, b)| (a.min(b), a.max(b)))
                        .filter(|&(a, b)| seg_dist(at, verts[a as usize], verts[b as usize]) < 1e-7)
                        .collect()
                };
                for (u, v) in edges {
                    let t = dot(sub(at, verts[u as usize]), sub(verts[v as usize], verts[u as usize]));
                    on_edge.entry((u, v)).or_default().push((t, id));
                }
            }
        }
    }
    let mut parent: Vec<usize> = (0..nv + points.len()).collect();
    let mut join = |parent: &mut Vec<usize>, a: usize, b: usize| {
        let (x, y) = (find(parent, a), find(parent, b));
        parent[x] = y;
    };
    let mut seen_edges: HashSet<(u32, u32)> = HashSet::new();
    for id in &ids {
        for (a, b) in [(id[0], id[1]), (id[1], id[2]), (id[2], id[0])] {
            let (u, v) = (a.min(b), a.max(b));
            if !seen_edges.insert((u, v)) {
                continue;
            }
            // Along the edge from u to v: inside and outside alternate at each crossing.
            let mut cuts = on_edge.get(&(u, v)).cloned().unwrap_or_default();
            cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
            // One crossing named twice (on a model edge, from both its triangles) is one cut.
            let scale = dist(verts[u as usize], verts[v as usize]).powi(2).max(1e-12);
            let mut merged: Vec<(f64, usize)> = Vec::new();
            for c in cuts {
                match merged.last() {
                    Some(&(t, id)) if (c.0 - t).abs() <= 1e-9 * scale => join(&mut parent, id, c.1),
                    _ => merged.push(c),
                }
            }
            let mut chain: Vec<usize> = vec![u as usize];
            chain.extend(merged.iter().map(|c| c.1));
            chain.push(v as usize);
            let mut state = inside[u as usize];
            for w in chain.windows(2) {
                if state {
                    join(&mut parent, w[0], w[1]);
                }
                state = !state;
            }
        }
    }
    for (k, w) in walks.iter().enumerate() {
        if !boundary[k] {
            continue;
        }
        for &s in &w.segs {
            join(&mut parent, item[&segments[s].a], item[&segments[s].b]);
        }
    }
    let mut slot: HashMap<usize, usize> = HashMap::new();
    let mut patches: Vec<Patch> = Vec::new();
    let mut patch_at = |parent: &mut Vec<usize>, x: usize, patches: &mut Vec<Patch>| -> usize {
        let r = find(parent, x);
        *slot.entry(r).or_insert_with(|| {
            patches.push(Patch { loops: Vec::new(), tris: Vec::new(), inside_verts: Vec::new() });
            patches.len() - 1
        })
    };
    for v in 0..nv {
        if inside[v] {
            let p = patch_at(&mut parent, v, &mut patches);
            patches[p].inside_verts.push(verts[v]);
        }
    }
    let mut loops: Vec<LoopOut> = Vec::new();
    let mut inner_loops = 0;
    let mut coincident = 0;
    let mut loop_patch: Vec<Option<usize>> = vec![None; walks.len()];
    for (k, w) in walks.iter().enumerate() {
        let (area, normal, centre, perimeter) = loop_stats(&w.points, w.closed);
        if area < MIN_LOOP_AREA && w.closed {
            continue;
        }
        if !boundary[k] {
            inner_loops += 1;
            continue;
        }
        let p = patch_at(&mut parent, item[&segments[w.segs[0]].a], &mut patches);
        loop_patch[k] = Some(p);
        // The same loop twice: a surface doubled in the model or the support (two coincident sheets),
        // not an entry and an exit.
        let twin = patches[p].loops.iter().any(|&o| {
            let x: &LoopOut = &loops[o];
            dist(x.centre, centre) <= COINCIDENT_MM && (eq_d(x.area) - eq_d(area)).abs() <= COINCIDENT_MM
        });
        if twin {
            coincident += 1;
            continue;
        }
        patches[p].loops.push(loops.len());
        loops.push(LoopOut { points: w.points.clone(), area, normal, centre, perimeter, closed: w.closed });
    }
    let mut patch_of = vec![None; tris.len()];
    for (t, id) in ids.iter().enumerate() {
        if let Some(&v) = id.iter().find(|&&v| inside[v as usize]) {
            let p = patch_at(&mut parent, v as usize, &mut patches);
            patch_of[t] = Some(p);
            patches[p].tris.push(t);
        }
    }
    for (k, w) in walks.iter().enumerate() {
        if let Some(p) = loop_patch[k] {
            for &s in &w.segs {
                let t = segments[s].ti as usize;
                if patch_of[t].is_none() {
                    patch_of[t] = Some(p);
                    patches[p].tris.push(t);
                }
            }
        }
    }
    let buried = loops.is_empty() && inside.iter().all(|&x| x);
    ShellCall { tris, patch_of, patches, loops, inner_loops, coincident, buried, lo, hi, verts, inside }
}

/// The deepest of up to `n` of the points, by distance to the surface.
fn depth_of(points: &[P], model: &Model, n: usize) -> f64 {
    let step = (points.len() / n.max(1)).max(1);
    points.iter().step_by(step).map(|p| model.depth(*p)).fold(0.0, f64::max)
}

fn extent(points: &[P]) -> f64 {
    let step = (points.len() / 300).max(1);
    let s: Vec<P> = points.iter().step_by(step).copied().collect();
    let mut best: f64 = 0.0;
    for a in &s {
        for b in &s {
            best = best.max(dist(*a, *b));
        }
    }
    best
}

/// The direction away from the model just outside a loop: from its centre to the shell's outside
/// vertices near it.
fn outward(shell: &ShellCall, l: &LoopOut, wind: &Wind) -> P {
    let r = eq_d(l.area) / 2.0;
    let reach = 2.0 * r + 0.5;
    let mut s = [0.0; 3];
    for (v, inside) in shell.verts.iter().zip(&shell.inside) {
        let d = dist(*v, l.centre);
        if !inside && d <= reach && d > 1e-9 {
            s = add(s, mul(sub(*v, l.centre), 1.0 / d));
        }
    }
    if len(s) > 1e-9 {
        return unit(s);
    }
    if wind.inside(add(l.centre, mul(l.normal, 0.05))) { mul(l.normal, -1.0) } else { l.normal }
}

/// measure: the shape of one contact's loops taken together: the second moments of their polygons (area-weighted, in
/// the plane of their mean normal, about their joint centroid). Returns (area, centre, elongation = major over minor
/// axis, major axis direction).
fn loop_shape(ls: &[&LoopOut]) -> Option<(f64, P, f64, P)> {
    let mut nsum = [0.0; 3];
    for l in ls {
        nsum = add(nsum, mul(l.normal, l.area.max(1e-12)));
    }
    if len(nsum) < 1e-12 {
        return None;
    }
    let n = unit(nsum);
    let seed = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = unit(cross(n, seed));
    let v = cross(n, u);
    let o = ls[0].centre;
    // Moments about o: area, first and second.
    let (mut a, mut sx, mut sy, mut xx, mut yy, mut xy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for l in ls {
        let pts: Vec<(f64, f64)> = l.points.iter().map(|q| { let r = sub(*q, o); (dot(r, u), dot(r, v)) }).collect();
        if pts.len() < 3 {
            continue;
        }
        let (mut la, mut lsx, mut lsy, mut lxx, mut lyy, mut lxy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for i in 0..pts.len() {
            let (x0, y0) = pts[i];
            let (x1, y1) = pts[(i + 1) % pts.len()];
            let c = x0 * y1 - x1 * y0;
            la += c / 2.0;
            lsx += (x0 + x1) * c / 6.0;
            lsy += (y0 + y1) * c / 6.0;
            lxx += (x0 * x0 + x0 * x1 + x1 * x1) * c / 12.0;
            lyy += (y0 * y0 + y0 * y1 + y1 * y1) * c / 12.0;
            lxy += (x0 * y1 + 2.0 * x0 * y0 + 2.0 * x1 * y1 + x1 * y0) * c / 24.0;
        }
        let sg = if la < 0.0 { -1.0 } else { 1.0 };
        a += sg * la; sx += sg * lsx; sy += sg * lsy; xx += sg * lxx; yy += sg * lyy; xy += sg * lxy;
    }
    if a <= 1e-12 {
        return None;
    }
    let (cx, cy) = (sx / a, sy / a);
    let (cxx, cyy, cxy) = (xx / a - cx * cx, yy / a - cy * cy, xy / a - cx * cy);
    let tr = cxx + cyy;
    let det = cxx * cyy - cxy * cxy;
    let disc = (tr * tr / 4.0 - det).max(0.0).sqrt();
    let (l1, l2) = (tr / 2.0 + disc, (tr / 2.0 - disc).max(1e-18));
    let ang = 0.5 * (2.0 * cxy).atan2(cxx - cyy);
    let major = add(mul(u, ang.cos()), mul(v, ang.sin()));
    Some((a, add(o, add(mul(u, cx), mul(v, cy))), (l1 / l2).sqrt(), major))
}

fn elongation(l: &LoopOut) -> f64 {
    l.perimeter * l.perimeter / (4.0 * std::f64::consts::PI * l.area.max(1e-12))
}

/// A graze: one elongated loop, with the shell going on past both of its long ends near its plane.
fn grazes(shell: &ShellCall, l: &LoopOut) -> bool {
    if elongation(l) < ELONGATED || l.points.len() < 3 {
        return false;
    }
    // The loop's long direction.
    let mut axis = unit(sub(l.points[l.points.len() / 2], l.points[0]));
    for _ in 0..8 {
        let mut next = [0.0; 3];
        for p in &l.points {
            let d = sub(*p, l.centre);
            next = add(next, mul(d, dot(d, axis)));
        }
        if len(next) < 1e-12 {
            break;
        }
        axis = unit(next);
    }
    let half = l.points.iter().map(|p| dot(sub(*p, l.centre), axis).abs()).fold(0.0, f64::max);
    let (mut plus, mut minus) = (false, false);
    for (v, inside) in shell.verts.iter().zip(&shell.inside) {
        if *inside || dot(sub(*v, l.centre), l.normal).abs() > 0.5 + half {
            continue;
        }
        let t = dot(sub(*v, l.centre), axis);
        plus |= t > half + 0.2;
        minus |= t < -half - 0.2;
    }
    plus && minus
}

/// The winding number inside a patch: about 1 on a closed body, between 0.5 and 1 where a stray sheet
/// or a hole in an open body is all that makes it inside.
fn patch_wind(patch: &Patch, ls: &[&LoopOut], wind: &Wind) -> f64 {
    let pts: Vec<P> = if patch.inside_verts.is_empty() {
        let c = ls.iter().fold([0.0; 3], |s, l| add(s, l.centre));
        vec![mul(c, 1.0 / ls.len() as f64)]
    } else {
        patch.inside_verts.iter().step_by((patch.inside_verts.len() / 16).max(1)).copied().collect()
    };
    pts.iter().map(|p| wind.w(*p)).sum::<f64>() / pts.len() as f64
}

/// A tip through a thin wall: its entry loop (on the side the support goes on from) and its exit loop.
fn thin_wall_entry(shell: &ShellCall, patch: &Patch) -> (usize, usize) {
    let (ka, kb) = (patch.loops[0], patch.loops[1]);
    let (a, b) = (shell.loops[ka].centre, shell.loops[kb].centre);
    let spacing = dist(a, b);
    let axis = unit(sub(b, a));
    let (mut before, mut after) = (0.0f64, 0.0f64);
    for (v, inside) in shell.verts.iter().zip(&shell.inside) {
        if !*inside {
            let t = dot(sub(*v, a), axis);
            before = before.max(-t);
            after = after.max(t - spacing);
        }
    }
    if before >= after { (ka, kb) } else { (kb, ka) }
}

/// How a pass-through goes: a tip through a thin wall, a graze, or a part through.
fn through_type(shell: &ShellCall, patch: &Patch) -> &'static str {
    let ls: Vec<&LoopOut> = patch.loops.iter().map(|&k| &shell.loops[k]).collect();
    if ls.iter().any(|l| elongation(l) >= ELONGATED) && ls.len() == 2 {
        let spacing = dist(ls[0].centre, ls[1].centre);
        if spacing < 0.5 * ls.iter().map(|l| l.perimeter).fold(0.0, f64::max) {
            return "graze";
        }
    }
    if ls.len() == 2 {
        let (a, b) = (ls[0].centre, ls[1].centre);
        let spacing = dist(a, b);
        let small = ls.iter().all(|l| eq_d(l.area) <= THIN_WALL_LOOP_D);
        if spacing <= THIN_WALL_SPACING && small && spacing > 1e-6 {
            let axis = unit(sub(b, a));
            let (mut before, mut after) = (0.0f64, 0.0f64);
            for (v, inside) in shell.verts.iter().zip(&shell.inside) {
                if *inside {
                    continue;
                }
                let t = dot(sub(*v, a), axis);
                before = before.max(-t);
                after = after.max(t - spacing);
            }
            if before.min(after) <= THIN_WALL_END {
                return "tip_thin_wall";
            }
        }
        return "through";
    }
    "through_many"
}

/// Union area of loops, rasterised on their mean plane.
fn union_area(ls: &[&LoopOut], dirs: &[P]) -> f64 {
    let mut n = [0.0; 3];
    for d in dirs {
        n = add(n, *d);
    }
    let n = if len(n) < 1e-9 { ls[0].normal } else { unit(n) };
    let helper = if n[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let u = unit(cross(n, helper));
    let v = cross(n, u);
    let polys: Vec<Vec<(f64, f64)>> = ls.iter().map(|l| l.points.iter().map(|p| (dot(*p, u), dot(*p, v))).collect()).collect();
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in polys.iter().flatten() {
        x0 = x0.min(p.0);
        x1 = x1.max(p.0);
        y0 = y0.min(p.1);
        y1 = y1.max(p.1);
    }
    let step = (((x1 - x0) * (y1 - y0)) / 40000.0).sqrt().max(1e-4);
    let inside = |poly: &[(f64, f64)], x: f64, y: f64| {
        let mut c = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[j]);
            if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                c = !c;
            }
            j = i;
        }
        c
    };
    let mut count = 0usize;
    let mut y = y0 + step / 2.0;
    while y < y1 {
        let mut x = x0 + step / 2.0;
        while x < x1 {
            if polys.iter().any(|p| inside(p, x, y)) {
                count += 1;
            }
            x += step;
        }
        y += step;
    }
    count as f64 * step * step
}

fn shells_of(tris: &[[P; 3]]) -> Vec<Vec<[P; 3]>> {
    shells_at(tris, 1e4).into_iter().map(|s| dedupe(&s)).collect()
}

/// The original mesh's truth.
fn run_orig(dir: &Path, id: &str) -> Value {
    let model_tris = read_stl(&dir.join(format!("{id}.orig_model.stl")));
    let support_tris = read_stl(&dir.join(format!("{id}.orig_support.stl")));
    let model = Model::new(model_tris.clone());
    let wind = Wind::new(model_tris);
    let shells: Vec<Vec<[P; 3]>> = shells_of(&support_tris);
    let calls: Vec<(usize, ShellCall)> =
        shells.into_par_iter().enumerate().map(|(i, s)| (i, call_shell(s, &model, &wind))).collect();
    let mut touches = Vec::new();
    let mut throughs = Vec::new();
    let mut buried = Vec::new();
    let mut flush = Vec::new();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let judged: Vec<&(usize, ShellCall)> = calls.iter().filter(|(_, c)| c.hi[2] > RAFT_BAND).collect();
    *counts.entry("shells").or_insert(0) += calls.len();
    *counts.entry("shells_judged").or_insert(0) += judged.len();
    for (i, c) in &judged {
        *counts.entry("inner_loops").or_insert(0) += c.inner_loops;
        *counts.entry("coincident_loops").or_insert(0) += c.coincident;
        *counts.entry("open_loops").or_insert(0) += c.loops.iter().filter(|l| !l.closed).count();
        if c.buried {
            let cen = mul(add(c.lo, c.hi), 0.5);
            buried.push(json!({ "shell": i, "at": cen, "depth": depth_of(&c.verts, &model, 64), "size": dist(c.lo, c.hi) }));
            continue;
        }
        if c.loops.is_empty() {
            // Flush: no loop, and the model right there.
            let mut gap = f64::MAX;
            let mut at = [0.0; 3];
            for v in &c.verts {
                let r = [FLUSH_GAP; 3];
                for &t in &model.near(sub(*v, r), add(*v, r)) {
                    let d = dist(*v, closest_on_tri(*v, &model.tris[t as usize]));
                    if d < gap {
                        gap = d;
                        at = *v;
                    }
                }
            }
            if gap <= FLUSH_GAP {
                // Its footprint: the shell's vertices within the tolerance of the model.
                let near: Vec<P> = c
                    .verts
                    .iter()
                    .copied()
                    .filter(|v| {
                        let r = [FLUSH_GAP; 3];
                        model.near(sub(*v, r), add(*v, r)).iter().any(|&t| dist(*v, closest_on_tri(*v, &model.tris[t as usize])) <= FLUSH_GAP)
                    })
                    .collect();
                let cen = mul(near.iter().fold([0.0; 3], |s, v| add(s, *v)), 1.0 / near.len().max(1) as f64);
                let d = (2.0 * near.iter().map(|v| dist(*v, cen)).fold(0.0, f64::max)).max(0.05);
                let mid = mul(add(c.lo, c.hi), 0.5);
                flush.push(json!({ "shell": i, "at": cen, "gap": gap, "d": d, "dir": unit(sub(mid, cen)) }));
            }
            if c.patches.iter().any(|p| !p.inside_verts.is_empty()) {
                *counts.entry("inside_no_loop").or_insert(0) += 1;
            }
            continue;
        }
        for p in &c.patches {
            match p.loops.len() {
                0 => *counts.entry("patch_no_loop").or_insert(0) += 1,
                1 => {
                    let l = &c.loops[p.loops[0]];
                    let dir = outward(c, l, &wind);
                    touches.push((
                        *i,
                        p.loops[0],
                        dir,
                        depth_of(&p.inside_verts, &model, 32),
                        grazes(c, l),
                        None,
                    ));
                    if patch_wind(p, &[l], &wind) < 0.8 {
                        *counts.entry("touch_low_wind").or_insert(0) += 1;
                    }
                }
                _ if THIN_WALL_IS_TOUCH && through_type(c, p) == "tip_thin_wall" => {
                    // The owner's ruling: a tip poking through a thin wall is a tip. Its contact is
                    // the loop on the side its shaft comes from; the exit loop is neither size nor clip.
                    let (entry, exit) = thin_wall_entry(c, p);
                    let l = &c.loops[entry];
                    touches.push((*i, entry, outward(c, l, &wind), depth_of(&p.inside_verts, &model, 32), false, Some(exit)));
                    *counts.entry("thin_wall_touch").or_insert(0) += 1;
                }
                _ => {
                    let ls: Vec<&LoopOut> = p.loops.iter().map(|&k| &c.loops[k]).collect();
                    let mut pts: Vec<P> = p.inside_verts.clone();
                    for l in &ls {
                        pts.extend(l.points.iter().copied());
                    }
                    throughs.push(json!({
                        "shell": i,
                        "type": through_type(c, p),
                        "loops": ls.iter().map(|l| json!({ "at": l.centre, "area": l.area, "d": eq_d(l.area), "closed": l.closed, "elong": elongation(l) })).collect::<Vec<_>>(),
                        "at": ls[0].centre,
                        "depth": depth_of(&p.inside_verts, &model, 32),
                        "length": extent(&pts),
                        "shell_len": dist(c.lo, c.hi),
                        "w": patch_wind(p, &ls, &wind),
                    }));
                }
            }
        }
    }
    // Merge overlapping touches into one contact, sized by their union.
    let n = touches.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let lp = |t: &(usize, usize, P, f64, bool, Option<usize>)| -> &LoopOut { &calls[t.0].1.loops[t.1] };
    for a in 0..n {
        for b in a + 1..n {
            let (la, lb) = (lp(&touches[a]), lp(&touches[b]));
            if dist(la.centre, lb.centre) < (eq_d(la.area) + eq_d(lb.area)) / 2.0 {
                let (x, y) = (find(&mut parent, a), find(&mut parent, b));
                parent[x] = y;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for a in 0..n {
        groups.entry(find(&mut parent, a)).or_default().push(a);
    }
    let contacts: Vec<Value> = groups
        .values()
        .map(|g| {
            let ls: Vec<&LoopOut> = g.iter().map(|&a| lp(&touches[a])).collect();
            let dirs: Vec<P> = g.iter().map(|&a| touches[a].2).collect();
            let area = if g.len() == 1 { ls[0].area } else { union_area(&ls, &dirs) };
            let w: f64 = ls.iter().map(|l| l.area).sum::<f64>().max(1e-12);
            let at = ls.iter().fold([0.0; 3], |s, l| add(s, mul(l.centre, l.area / w)));
            let mut dir = [0.0; 3];
            for d in &dirs {
                dir = add(dir, *d);
            }
            let depth = g.iter().map(|&a| touches[a].3).fold(0.0, f64::max);
            let graze = g.len() == 1 && touches[g[0]].4;
            let exits: Vec<P> = g.iter().filter_map(|&a| touches[a].5.map(|e| calls[touches[a].0].1.loops[e].centre)).collect();
            json!({
                "at": at, "area": area, "d": eq_d(area), "dir": unit(dir), "n": g.len(), "depth": depth,
                "graze": graze, "open": ls.iter().any(|l| !l.closed),
                "thin_wall": !exits.is_empty(), "exits": exits,
                "shape": loop_shape(&ls).map(|(a, c, e, m)| json!({ "area": a, "centre": c, "elong": e, "major": m })),
                "shells": g.iter().map(|&a| touches[a].0).collect::<Vec<_>>(),
                "loops": g.iter().map(|&a| touches[a].1).collect::<Vec<_>>(),
            })
        })
        .collect();
    counts.insert("touch_loops", n);
    // The owner's rulings (2026-11-10): a side graze is clipping, not a tip; a tip resting flush is a tip.
    let (grazes, mut contacts): (Vec<Value>, Vec<Value>) = contacts.into_iter().partition(|c| c["graze"].as_bool() == Some(true));
    for f in &flush {
        let at = v3j(&f["at"]);
        let d = f["d"].as_f64().unwrap();
        if contacts.iter().any(|c| dist(v3j(&c["at"]), at) < (c["d"].as_f64().unwrap() + d) / 2.0) {
            continue;
        }
        contacts.push(json!({ "at": at, "area": std::f64::consts::PI * d * d / 4.0, "d": d, "dir": f["dir"], "n": 0,
                              "depth": 0.0, "graze": false, "open": false, "thin_wall": false, "exits": [], "flush": true,
                              "shells": [f["shell"]], "loops": [] }));
    }
    counts.insert("grazes", grazes.len());
    json!({ "id": id, "contacts": contacts, "grazes": grazes, "throughs": throughs, "buried": buried, "flush": flush, "counts": counts,
            "body_open_edges": model.open_edges, "body_nonmanifold": model.nonmanifold_edges })
}

// ------------------------------------------------------------- built mesh

fn v3j(v: &Value) -> P {
    [v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()]
}

/// The built mesh against the truth: each tip, its size at the 0.05 offset, and each patch's call.
fn run_built(dir: &Path, id: &str, truth: &Value) -> Value {
    let model_tris = read_stl(&dir.join(format!("{id}.orig_model.stl")));
    let model = Model::new(model_tris.clone());
    let wind = Wind::new(model_tris);
    let (parts, tips, counts) = built_parts(dir, id);
    let contacts: Vec<(P, f64)> = truth["contacts"]
        .as_array()
        .map(|a| a.iter().map(|c| (v3j(&c["at"]), c["d"].as_f64().unwrap() / 2.0)).collect())
        .unwrap_or_default();
    let at_touch = |l: &LoopOut| {
        let r = eq_d(l.area) / 2.0;
        contacts.iter().any(|(c, rc)| dist(*c, l.centre) <= rc + r + 0.1)
    };
    // Where a true tip pokes through a thin wall: its entry and exit, for a built patch doing the same.
    let walls: Vec<(P, f64, Vec<P>)> = truth["contacts"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|c| c["thin_wall"].as_bool() == Some(true))
                .map(|c| (v3j(&c["at"]), c["d"].as_f64().unwrap() / 2.0, c["exits"].as_array().unwrap().iter().map(v3j).collect()))
                .collect()
        })
        .unwrap_or_default();
    let through_wall = |ls: &[&LoopOut]| {
        ls.len() == 2
            && walls.iter().any(|(at, rc, exits)| {
                let near = |l: &LoopOut, p: P, r: f64| dist(l.centre, p) <= r + eq_d(l.area) / 2.0 + 0.1;
                exits.iter().any(|e| {
                    (near(ls[0], *at, *rc) && near(ls[1], *e, *rc)) || (near(ls[1], *at, *rc) && near(ls[0], *e, *rc))
                })
            })
    };
    let rows: Vec<Value> = parts
        .par_iter()
        .enumerate()
        .flat_map_iter(|(i, part)| {
            if part.hi[2] <= RAFT_BAND {
                return Vec::new();
            }
            let c = call_shell(part.tris.clone(), &model, &wind);
            let mut out = Vec::new();
            for p in &c.patches {
                let ls: Vec<&LoopOut> = p.loops.iter().map(|&k| &c.loops[k]).collect();
                if ls.is_empty() {
                    continue;
                }
                let clip = if ls.len() >= 2 { !through_wall(&ls) } else { !at_touch(ls[0]) };
                out.push(json!({
                    "part": i, "kind": part.kind, "n": ls.len(), "clip": clip,
                    "area": ls.iter().map(|l| l.area).sum::<f64>(),
                    "at": ls[0].centre, "depth": depth_of(&p.inside_verts, &model, 16),
                }));
            }
            if c.buried && part.kind != "tip" {
                out.push(json!({ "part": i, "kind": part.kind, "n": 0, "clip": true, "buried": true, "area": 0.0,
                                  "at": mul(add(c.lo, c.hi), 0.5), "depth": depth_of(&c.verts, &model, 32) }));
            }
            out
        })
        .collect();
    // Each tip's size at the 0.05 offset: its cone run on into the model.
    let tip_rows: Vec<Value> = tips
        .par_iter()
        .map(|t| {
            // merge r7: a sphere tip's ball (cball) is the part that crosses the surface: taken before the cone.
            let sphere = parts
                .iter()
                .filter(|p| p.kind == "tip")
                .filter(|p| p.sphere_like().is_some_and(|(c, r)| dist(c, t.pos) <= r + 0.15 && dist(c, t.pos) < dist(c, t.socket)))
                .min_by(|x, y| {
                    let d = |p: &Part| p.sphere_like().map_or(f64::MAX, |(c, _)| dist(c, t.pos));
                    d(x).total_cmp(&d(y))
                });
            let nearest = parts
                .iter()
                .filter(|p| p.kind == "tip")
                .filter(|p| p.tris.iter().flatten().any(|v| dist(*v, t.pos) <= t.radius + 0.1))
                .min_by(|x, y| {
                    let d = |p: &Part| p.tris.iter().flatten().map(|v| dist(*v, t.pos)).fold(f64::MAX, f64::min);
                    d(x).total_cmp(&d(y))
                });
            // merge (owner decision 9): TOUCH_PEN (default 0.05) is how far the built cone is run on into the model before
            // its loop is taken; 0 for a build made at DragonFruit's own 0.05 mm tip offset (the loop as built).
            let pen: f64 = std::env::var("TOUCH_PEN").ok().and_then(|v| v.parse().ok()).unwrap_or(0.05);
            // Of the ball and the nearest tip part, the one leaving the larger loop (a ball sunk past its cone's end).
            let area_of = |p: &Part| {
                let deep = if pen > 0.0 { with_penetration(p, t, t.normal, pen) } else { p.clone() };
                loops_of(&deep, &model).iter().map(|l| l.area).fold(0.0, f64::max)
            };
            let own = match (sphere, nearest) {
                (Some(a), Some(b)) if !std::ptr::eq(a, b) => if area_of(a) > area_of(b) { Some(a) } else { Some(b) },
                (s, n) => n.or(s),
            };
            let d05 = own.map(|p| {
                let deep = if pen > 0.0 { with_penetration(p, t, t.normal, pen) } else { p.clone() };
                loops_of(&deep, &model).iter().map(|l| l.area).fold(0.0, f64::max)
            });
            // measure: the built tip's own loop at the 0.05 offset (the largest of its loops), its centre and shape.
            let loop05 = own.and_then(|p| {
                let deep = if pen > 0.0 { with_penetration(p, t, t.normal, pen) } else { p.clone() };
                let c = call_shell(deep.tris.clone(), &model, &wind);
                let best = c.loops.iter().max_by(|x, y| x.area.total_cmp(&y.area))?;
                loop_shape(&[best]).map(|(a, cen, e, m)| json!({ "area": a, "centre": cen, "elong": e, "major": m }))
            });
            json!({ "pos": t.pos, "contact_d": t.contact_d, "area05": d05, "disk": t.disk, "loop05": loop05 })
        })
        .collect();
    json!({ "id": id, "patches": rows, "tips": tip_rows,
            "counts": counts.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>() })
}

// ------------------------------------------------------------- panels

/// Splits a triangle until its longest edge is at most `h`.
fn split_to(t: [P; 3], h: f64, out: &mut Vec<[P; 3]>) {
    let e = [dist(t[0], t[1]), dist(t[1], t[2]), dist(t[2], t[0])];
    let k = (0..3).max_by(|&a, &b| e[a].total_cmp(&e[b])).unwrap();
    if e[k] <= h || out.len() > 400_000 {
        out.push(t);
        return;
    }
    let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
    let m = mul(add(a, b), 0.5);
    split_to([a, m, c], h, out);
    split_to([m, b, c], h, out);
}

/// The triangles near a point, by call, for drawing: support pieces split finely and each coloured by
/// its patch's call where it is inside the model, the model's triangles, the loops, and which cells of
/// the cut plane (through the point, spanned by `right` and `up`) are inside the model.
fn run_panel(dir: &Path, id: &str, which: &str, at: P, r: f64, up: P, right: P, grid: usize) -> Value {
    let model_tris = read_stl(&dir.join(format!("{id}.orig_model.stl")));
    let model = Model::new(model_tris.clone());
    let wind = Wind::new(model_tris.clone());
    let support = if which == "built" {
        read_stl(&dir.join(format!("{id}.df_support.stl")))
    } else {
        read_stl(&dir.join(format!("{id}.orig_support.stl")))
    };
    let near = |t: &[P; 3]| t.iter().any(|v| dist(*v, at) <= r) || dist(mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0), at) <= r;
    let shells: Vec<Vec<[P; 3]>> = shells_of(&support).into_iter().filter(|s| s.iter().any(near)).collect();
    let h = r / 60.0;
    let mut sup = Vec::new();
    let mut loops = Vec::new();
    for s in shells {
        let c = call_shell(s, &model, &wind);
        for (t, tri) in c.tris.iter().enumerate() {
            if !near(tri) {
                continue;
            }
            let call = match c.patch_of[t] {
                _ if c.buried => "buried",
                None => "out",
                Some(p) => match c.patches[p].loops.len() {
                    0 => "noloop",
                    1 => {
                        if grazes(&c, &c.loops[c.patches[p].loops[0]]) { "flag" } else { "touch" }
                    }
                    _ => match through_type(&c, &c.patches[p]) {
                        "through" | "through_many" => "through",
                        _ => "flag",
                    },
                },
            };
            if call == "out" || call == "buried" {
                sup.push(json!({ "t": tri, "c": call }));
                continue;
            }
            let mut pieces = Vec::new();
            split_to(*tri, h, &mut pieces);
            let calls: Vec<bool> = pieces.par_iter().map(|q| wind.inside(mul(add(add(q[0], q[1]), q[2]), 1.0 / 3.0))).collect();
            for (q, inside) in pieces.iter().zip(calls) {
                sup.push(json!({ "t": q, "c": if inside { call } else { "out" } }));
            }
        }
        for l in &c.loops {
            if dist(l.centre, at) <= r {
                loops.push(json!(l.points));
            }
        }
    }
    let m: Vec<Value> = model_tris.iter().filter(|t| near(t)).map(|t| json!(t)).collect();
    let half = 1.1 * r;
    let cells: Vec<Vec<u8>> = (0..grid)
        .into_par_iter()
        .map(|row| {
            let y = half - (row as f64 + 0.5) / grid as f64 * 2.0 * half;
            (0..grid)
                .map(|col| {
                    let x = -half + (col as f64 + 0.5) / grid as f64 * 2.0 * half;
                    u8::from(wind.inside(add(at, add(mul(right, x), mul(up, y)))))
                })
                .collect()
        })
        .collect();
    json!({ "support": sup, "model": m, "loops": loops, "section": cells, "half": half })
}

// ------------------------------------------------------------- self test

fn scene_check(name: &str, model: Vec<[P; 3]>, support: Vec<[P; 3]>, want: (usize, usize, usize)) -> bool {
    let m = Model::new(model.clone());
    let w = Wind::new(model);
    let mut got = (0, 0, 0);
    for s in shells_of(&support) {
        let c = call_shell(s, &m, &w);
        if c.buried {
            got.2 += 1;
            continue;
        }
        if std::env::var("TDBG").is_ok() {
            for p in &c.patches {
                println!("  patch loops {:?} verts {}", p.loops.iter().map(|&k| (c.loops[k].centre, c.loops[k].area)).collect::<Vec<_>>(), p.inside_verts.len());
            }
            println!("  inner {} inside {}/{}", c.inner_loops, c.inside.iter().filter(|x| **x).count(), c.inside.len());
        }
        for p in &c.patches {
            match p.loops.len() {
                0 => {}
                1 => got.0 += 1,
                _ => got.1 += 1,
            }
        }
    }
    let ok = got == want;
    println!("{} {name}: touch/through/buried {got:?}, want {want:?}", if ok { "ok  " } else { "FAIL" });
    ok
}

fn tframe(a: P, b: P, ra: f64, rb: f64) -> Vec<[P; 3]> {
    // A closed frustum with outward winding.
    let sides = 16;
    let axis = unit(sub(b, a));
    let helper = if axis[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let u = unit(cross(axis, helper));
    let v = cross(axis, u);
    let ring = |c: P, r: f64, k: usize| {
        let t = std::f64::consts::TAU * k as f64 / sides as f64;
        add(c, add(mul(u, r * t.cos()), mul(v, r * t.sin())))
    };
    let mut out = Vec::new();
    for k in 0..sides {
        let (p0, p1, q0, q1) = (ring(a, ra, k), ring(a, ra, (k + 1) % sides), ring(b, rb, k), ring(b, rb, (k + 1) % sides));
        out.push([p0, p1, q1]);
        out.push([p0, q1, q0]);
        out.push([a, p1, p0]);
        out.push([b, q0, q1]);
    }
    out
}

fn ball(c: P, r: f64) -> Vec<[P; 3]> {
    let n = 12;
    let point = |i: usize, j: usize| {
        let (th, ph) = (std::f64::consts::PI * i as f64 / n as f64, std::f64::consts::TAU * j as f64 / (2 * n) as f64);
        add(c, [r * th.sin() * ph.cos(), r * th.sin() * ph.sin(), r * th.cos()])
    };
    let mut out = Vec::new();
    for i in 0..n {
        for j in 0..2 * n {
            let (a, b, d, e) = (point(i, j), point(i, (j + 1) % (2 * n)), point(i + 1, j), point(i + 1, (j + 1) % (2 * n)));
            if i > 0 {
                out.push([a, b, d]);
            }
            if i + 1 < n {
                out.push([b, e, d]);
            }
        }
    }
    out
}

/// A box, wound outward, finely gridded on its faces.
fn boxm(lo: P, hi: P) -> Vec<[P; 3]> {
    let mut out = Vec::new();
    let n = 20;
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for side in [0, 1] {
            let w = if side == 0 { lo[axis] } else { hi[axis] };
            for i in 0..n {
                for j in 0..n {
                    let pt = |a: usize, b: usize| {
                        let mut p = [0.0; 3];
                        p[axis] = w;
                        p[u] = lo[u] + (hi[u] - lo[u]) * a as f64 / n as f64;
                        p[v] = lo[v] + (hi[v] - lo[v]) * b as f64 / n as f64;
                        p
                    };
                    let (a, b, c, d) = (pt(i, j), pt(i + 1, j), pt(i + 1, j + 1), pt(i, j + 1));
                    if side == 1 {
                        out.push([a, b, c]);
                        out.push([a, c, d]);
                    } else {
                        out.push([a, c, b]);
                        out.push([a, d, c]);
                    }
                }
            }
        }
    }
    out
}

fn selftest() {
    let mut all = true;
    let slab = || boxm([-10.0, -10.0, 10.0], [10.0, 10.0, 14.0]);
    // Every tip type, ending inside: one touch each.
    all &= scene_check("cone", slab(), tframe([0.0, 0.0, 5.0], [0.0, 0.0, 10.3], 0.5, 0.15), (1, 0, 0));
    all &= scene_check("slanted cone", slab(), tframe([-3.0, 0.0, 5.0], [0.2, 0.3, 10.4], 0.5, 0.15), (1, 0, 0));
    all &= scene_check("blunt rod", slab(), tframe([0.0, 0.0, 5.0], [0.0, 0.0, 10.5], 1.2, 1.2), (1, 0, 0));
    all &= scene_check("ball", slab(), ball([0.0, 0.0, 10.2], 0.4), (1, 0, 0));
    all &= scene_check("stub", slab(), tframe([0.0, 0.0, 9.0], [0.0, 0.0, 10.4], 0.3, 0.3), (1, 0, 0));
    // A cone with a ball on it: two touch loops (merged later into one contact).
    let mut cb = tframe([0.0, 0.0, 5.0], [0.0, 0.0, 10.2], 0.5, 0.2);
    cb.extend(ball([0.0, 0.0, 10.2], 0.3));
    all &= scene_check("cone with ball", slab(), cb, (2, 0, 0));
    // A cone and a ball joined into one shell (sharing a vertex), the cone running on inside the ball:
    // one touch, the cone's surface inside the ball is not the support's surface.
    let mut welded = tframe([0.0, 0.0, 5.0], [0.0, 0.0, 10.25], 0.5, 0.12);
    let mut b = ball([0.0, 0.0, 10.2], 0.3);
    b.push([[0.0, 0.0, 10.25], [0.0, 0.0, 10.5], [0.1, 0.0, 10.4]]);
    welded.extend(b);
    all &= scene_check("cone welded into a ball", slab(), welded, (1, 0, 0));
    // A fan: three tips meeting below.
    let mut fan = Vec::new();
    for k in 0..3 {
        let x = -0.4 + 0.4 * k as f64;
        fan.extend(tframe([0.0, 0.0, 7.0], [x, 0.0, 10.3], 0.3, 0.12));
    }
    all &= scene_check("fan", slab(), fan, (3, 0, 0));
    // A rod through the whole slab: one pass-through. A tip through a thin wall: one pass-through.
    all &= scene_check("rod through", slab(), tframe([0.0, 0.0, 5.0], [0.0, 0.0, 16.0], 0.5, 0.5), (0, 1, 0));
    let wall = boxm([-10.0, -10.0, 10.0], [10.0, 10.0, 10.4]);
    all &= scene_check("tip through a thin wall", wall, tframe([0.0, 0.0, 7.0], [0.0, 0.0, 11.0], 0.5, 0.15), (0, 1, 0));
    // A hollow strut: both ends in the model, one shell, two touches.
    let mut cav = boxm([-10.0, -10.0, 0.0], [10.0, 10.0, 20.0]);
    let inner: Vec<[P; 3]> = boxm([-5.0, -5.0, 5.0], [5.0, 5.0, 15.0]).into_iter().map(|t| [t[0], t[2], t[1]]).collect();
    cav.extend(inner);
    all &= scene_check("hollow strut", cav.clone(), tframe([0.0, 0.0, 4.6], [0.0, 0.0, 15.4], 0.4, 0.4), (2, 0, 0));
    // Inside a cavity, touching nothing: no call. Buried in the wall: buried.
    all &= scene_check("in a cavity", cav.clone(), tframe([0.0, 0.0, 7.0], [0.0, 0.0, 12.0], 0.4, 0.4), (0, 0, 0));
    all &= scene_check("buried", slab(), tframe([0.0, 0.0, 11.0], [0.0, 0.0, 13.0], 0.4, 0.4), (0, 0, 1));
    // A side graze: one loop.
    all &= scene_check("graze", slab(), tframe([-4.0, 0.0, 9.8], [4.0, 0.0, 9.8], 0.3, 0.3), (1, 0, 0));
    // Flush: no loop.
    all &= scene_check("flush", slab(), tframe([0.0, 0.0, 6.0], [0.0, 0.0, 10.0], 0.5, 0.2), (0, 0, 0));
    // An open body: a hole cut out of the slab's underside away from the tip.
    let open: Vec<[P; 3]> = slab()
        .into_iter()
        .filter(|t| {
            let c = mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0);
            !((c[2] - 10.0).abs() < 1e-9 && c[0] > 3.0 && c[0] < 5.0 && c[1].abs() < 1.0)
        })
        .collect();
    all &= scene_check("open body", open, tframe([0.0, 0.0, 5.0], [0.0, 0.0, 10.3], 0.5, 0.15), (1, 0, 0));
    // A non-manifold body: two boxes sharing a face edge; and overlapping shells (an inner face).
    let mut nm = slab();
    nm.extend(boxm([10.0, -10.0, 10.0], [20.0, 10.0, 14.0]));
    all &= scene_check("non-manifold", nm, tframe([9.0, 0.0, 5.0], [9.0, 0.0, 10.3], 0.5, 0.15), (1, 0, 0));
    let mut over = slab();
    over.extend(boxm([-10.0, -10.0, 10.2], [10.0, 10.0, 12.0]));
    all &= scene_check("overlapping model shells", over, tframe([0.0, 0.0, 5.0], [0.0, 0.0, 10.6], 0.5, 0.15), (1, 0, 0));
    // The model's surface doubled (the same shell twice): one touch, not a pass-through.
    let mut doubled = slab();
    doubled.extend(slab());
    all &= scene_check("doubled model surface", doubled, tframe([0.0, 0.0, 5.0], [0.0, 0.0, 10.3], 0.5, 0.15), (1, 0, 0));
    println!("{}", if all { "SELFTEST PASSED" } else { "SELFTEST FAILED" });
    std::process::exit(i32::from(!all));
}

/// The `touch` subcommand: `args` are the arguments after the subcommand name.
pub fn run(args: &[String]) {
    let out = match args[0].as_str() {
        "selftest" => {
            selftest();
            return;
        }
        "orig" => run_orig(Path::new(&args[1]), &args[2]),
        "built" => {
            let truth: Value = serde_json::from_slice(&std::fs::read(&args[3]).unwrap()).unwrap();
            run_built(Path::new(&args[1]), &args[2], &truth)
        }
        "panel" => {
            let f = |k: usize| args[k].parse::<f64>().unwrap();
            run_panel(
                Path::new(&args[1]),
                &args[2],
                &args[3],
                [f(4), f(5), f(6)],
                f(7),
                [f(8), f(9), f(10)],
                [f(11), f(12), f(13)],
                f(14) as usize,
            )
        }
        "wind" => {
            let model = read_stl(&Path::new(&args[1]).join(format!("{}.orig_model.stl", args[2])));
            let m = Model::new(model.clone());
            let w = Wind::new(model);
            let mut out = Vec::new();
            for line in std::io::stdin().lines() {
                let v: Vec<f64> = line.unwrap().split_whitespace().map(|x| x.parse().unwrap()).collect();
                let p = [v[0], v[1], v[2]];
                out.push(json!([w.w(p), m.inside(p), m.depth(p)]));
            }
            json!(out)
        }
        _ => panic!("unknown mode"),
    };
    println!("{out}");
}
