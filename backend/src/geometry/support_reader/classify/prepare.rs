//! Stage 3a: the pieces made ready for classification. Split solids become one piece per straight
//! stretch, a tip welded in line with a brace is cut at the post it meets, a pad welded under a
//! post is cut off as raft, noisy profiles are smoothed; then the raft, the pieces standing on the
//! plate, and every piece's joins (its attachments) are found.

use super::super::contacts::Contact;
use super::super::math::{P, add, dist, dot, len, mul, sub};
use super::super::pieces::{ConnectionKind, PieceKind, Pieces, Section, SectionKind, Segment};
use super::{Att, LEVEL, NEAR_VERTICAL, PLATE_Z, Piece, RAFT_MIN_XY, ROOT_TOP, seg_closest, seg_dist};

/// A pad welded under a post counts as raft up to this height above the plate.
const PAD_BAND: f64 = 3.0;
/// Profile points within this of a straighter profile are noise (see [`smooth_secs`]).
const PROFILE_TOL: f64 = 0.06;

/// A piece as the pieces stage read it, before classification.
#[derive(Debug, Clone)]
pub(super) struct RawPiece {
    pub id: usize,
    pub kind: PieceKind,
    pub a: P,
    pub b: P,
    pub width: f64,
    pub sections: Vec<Section>,
    pub lo: P,
    pub hi: P,
    pub volume: f64,
    pub ray_volume: f64,
    pub in_model: f64,
    pub in_model_at: Option<P>,
    pub tris: usize,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
pub(crate) struct RawConn {
    pub a: usize,
    pub b: usize,
    pub kind: ConnectionKind,
    pub at: P,
    pub overlap: f64,
}

/// A piece a contact lies in (or near): (piece, inside, distance).
pub(super) type Hit = (usize, bool, f64);

pub(super) struct Raw {
    pub pieces: Vec<RawPiece>,
    pub conns: Vec<RawConn>,
    pub contact_pieces: Vec<Vec<Hit>>,
}

impl Raw {
    pub fn from(p: &Pieces) -> Self {
        Raw {
            pieces: p
                .pieces
                .iter()
                .map(|x| RawPiece {
                    id: x.id,
                    kind: x.kind,
                    a: x.a,
                    b: x.b,
                    width: x.width,
                    sections: x.sections.clone(),
                    lo: x.lo,
                    hi: x.hi,
                    volume: x.volume,
                    ray_volume: x.ray_volume,
                    in_model: x.in_model,
                    in_model_at: x.in_model_at,
                    tris: x.tris,
                    segments: x.segments.clone(),
                })
                .collect(),
            conns: p
                .connections
                .iter()
                .map(|c| RawConn { a: c.a, b: c.b, kind: c.kind, at: c.at, overlap: c.overlap })
                .collect(),
            contact_pieces: p.contact_pieces.iter().map(|h| h.iter().map(|x| (x.0, x.1, x.2)).collect()).collect(),
        }
    }
}

fn axis_of(a: P, b: P) -> (f64, P) {
    let l = dist(a, b);
    (l, if l > 1e-12 { super::super::math::div(sub(b, a), l) } else { [0.0; 3] })
}

/// A solid the pieces stage split into straight stretches (a bent tip, a leg or tip unioned onto
/// its shaft) becomes one piece per stretch. Its joins to other pieces and its contacts go to the
/// stretch nearest them; the stretches join each other where their centre lines come within their
/// radii (plus 0.3 mm). A join goes to every stretch whose axis passes within both radii of the
/// other piece (a welded X's join with a post, which both its arms enter), including in the root
/// zone where the other piece comes down into it from above.
pub(super) fn split_pieces(d: Raw, contacts: &[Contact]) -> Raw {
    let pieces = &d.pieces;
    let mut out = pieces.clone();
    // Per split piece: its stretches' ids and (a, b, radius).
    // Per split piece: its stretches' ids and (a, b, radius).
    type Stretches = (Vec<usize>, Vec<(P, P, f64)>);
    let mut new_of: std::collections::BTreeMap<usize, Stretches> = Default::default();
    for p in pieces {
        let segs = &p.segments;
        if segs.len() < 2 {
            continue;
        }
        let mut ids = Vec::new();
        for (k, g) in segs.iter().enumerate() {
            let id = if k == 0 { p.id } else { out.len() };
            let q = RawPiece {
                id,
                kind: PieceKind::Axi,
                a: g.a,
                b: g.b,
                width: g.width,
                sections: g.sections.clone(),
                lo: g.lo,
                hi: g.hi,
                volume: g.volume,
                ray_volume: g.volume,
                in_model: 0.0,
                in_model_at: None,
                tris: g.tris,
                segments: Vec::new(),
            };
            if k == 0 {
                out[p.id] = q;
            } else {
                out.push(q);
            }
            ids.push(id);
        }
        let geo: Vec<(P, P, f64)> = segs.iter().map(|g| (g.a, g.b, g.width / 2.0)).collect();
        if p.in_model > 0.0
            && let Some(x) = p.in_model_at
        {
            let k = argmin((0..segs.len()).map(|k| seg_dist(geo[k].0, geo[k].1, x).0 - geo[k].2));
            out[ids[k]].in_model = p.in_model;
        }
        new_of.insert(p.id, (ids, geo));
    }
    if new_of.is_empty() {
        return d;
    }
    let pick = |pid: usize, x: P| -> usize {
        let (ids, geo) = &new_of[&pid];
        ids[argmin((0..ids.len()).map(|k| seg_dist(geo[k].0, geo[k].1, x).0 - geo[k].2))]
    };
    let touching = |pid: usize, q: &RawPiece| -> Vec<(usize, P)> {
        let (ids, geo) = &new_of[&pid];
        // Low solids, a web of bars on the plate among them, keep the nearest rule, unless the
        // other piece comes down into them from above the root zone.
        if q.width > 6.0 || (pieces[pid].lo[2] <= ROOT_TOP + 0.5 && q.hi[2] <= ROOT_TOP + 0.5) {
            return Vec::new();
        }
        let mut v = Vec::new();
        for k in 0..ids.len() {
            let (dd, x, y) = seg_closest(geo[k].0, geo[k].1, q.a, q.b);
            if dd <= geo[k].2 + q.width / 2.0 + 0.05 {
                v.push((ids[k], mul(add(x, y), 0.5)));
            }
        }
        v
    };
    let mut conns = Vec::new();
    for c in &d.conns {
        let (ea, eb) = (new_of.contains_key(&c.a), new_of.contains_key(&c.b));
        if ea && eb {
            let mut pairs = Vec::new();
            let (pa, pb) = (&pieces[c.a], &pieces[c.b]);
            if pa.lo[2].min(pb.lo[2]) > ROOT_TOP + 0.5
                || (pa.hi[2].max(pb.hi[2]) > ROOT_TOP + 0.5 && pa.width.max(pb.width) <= 6.0)
            {
                let (ia, ga) = &new_of[&c.a];
                let (ib, gb) = &new_of[&c.b];
                for (i, gi) in ia.iter().zip(ga) {
                    for (j, gj) in ib.iter().zip(gb) {
                        let (dd, x, y) = seg_closest(gi.0, gi.1, gj.0, gj.1);
                        if dd <= gi.2 + gj.2 + 0.05 {
                            pairs.push((*i, *j, mul(add(x, y), 0.5)));
                        }
                    }
                }
            }
            if pairs.is_empty() {
                pairs.push((pick(c.a, c.at), pick(c.b, c.at), c.at));
            }
            for (i, j, at) in pairs {
                if i != j {
                    conns.push(RawConn { a: i, b: j, at, ..c.clone() });
                }
            }
            continue;
        }
        if ea || eb {
            let (e, o) = if ea { (c.a, c.b) } else { (c.b, c.a) };
            let mut hits = touching(e, &pieces[o]);
            if hits.is_empty() {
                hits.push((pick(e, c.at), c.at));
            }
            for (nid, at) in hits {
                if nid != o {
                    let mut n = c.clone();
                    n.at = at;
                    if ea {
                        n.a = nid;
                    } else {
                        n.b = nid;
                    }
                    conns.push(n);
                }
            }
            continue;
        }
        if c.a != c.b {
            conns.push(c.clone());
        }
    }
    for (ids, geo) in new_of.values() {
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                let (dd, x, y) = seg_closest(geo[i].0, geo[i].1, geo[j].0, geo[j].1);
                if dd <= geo[i].2 + geo[j].2 + 0.3 {
                    conns.push(RawConn {
                        a: ids[i],
                        b: ids[j],
                        kind: ConnectionKind::Overlap,
                        at: mul(add(x, y), 0.5),
                        overlap: 1e-3,
                    });
                }
            }
        }
    }
    let contact_pieces = d
        .contact_pieces
        .iter()
        .enumerate()
        .map(|(ci, hits)| {
            hits.iter()
                .map(|h| (if new_of.contains_key(&h.0) { pick(h.0, contacts[ci].at) } else { h.0 }, h.1, h.2))
                .collect()
        })
        .collect();
    Raw { pieces: out, conns, contact_pieces }
}

/// The index of the first smallest value.
pub(super) fn argmin(values: impl Iterator<Item = f64>) -> usize {
    let mut best = (0, f64::INFINITY);
    for (i, v) in values.enumerate() {
        if v < best.1 {
            best = (i, v);
        }
    }
    best.0
}

/// A leaning tip cone welded in line with a brace that carries on past a post's top reads as one
/// straight solid (tip, then brace). Where the cone's wide end meets the top end of a near-vertical
/// piece going down (a post), and the solid carries on past it not straight down, it is split
/// there into the tip and the rest (handed to [`split_pieces`] as two stretches): the tip then
/// follows the post down and the rest is a brace. A tip welded to its own shaft has no other piece
/// ending at its base and is left whole.
pub(super) fn split_inline(mut d: Raw, contacts: &[Contact]) -> Raw {
    let mut near: Vec<std::collections::BTreeSet<usize>> = vec![Default::default(); d.pieces.len()];
    for c in &d.conns {
        if c.kind != ConnectionKind::Near {
            near[c.a].insert(c.b);
            near[c.b].insert(c.a);
        }
    }
    let mut done = std::collections::BTreeSet::new();
    for (ci, contact) in contacts.iter().enumerate().take(d.contact_pieces.len()) {
        for hi in 0..d.contact_pieces[ci].len() {
            let h = d.contact_pieces[ci][hi];
            let pid = h.0;
            let p = &d.pieces[pid];
            if !h.1 || done.contains(&pid) || p.kind != PieceKind::Axi || p.segments.len() >= 2 || p.sections.is_empty()
            {
                continue;
            }
            let (l, u) = axis_of(p.a, p.b);
            if l < 2.0 {
                continue;
            }
            let tc = dot(sub(contact.at, p.a), u).clamp(0.0, l);
            let sg = if tc > l / 2.0 { -1.0 } else { 1.0 };
            let mut secs: Vec<&Section> = p.sections.iter().collect();
            secs.sort_by(|x, y| (x.t0 * sg).total_cmp(&(y.t0 * sg)));
            let mut tw = None;
            for sec in secs {
                let (near_t, far_t) = if sg > 0.0 { (sec.t0, sec.t1) } else { (sec.t1, sec.t0) };
                let (rn, rf) = if sg > 0.0 { (sec.r0, sec.r1) } else { (sec.r1, sec.r0) };
                if (far_t - tc) * sg <= 0.05 {
                    continue;
                }
                if sec.k == SectionKind::Cone && rf > rn * 1.15 && (near_t - tc).abs() <= 2.0 {
                    tw = Some((far_t, rf));
                }
                break;
            }
            let Some((tw, rw)) = tw else { continue };
            let rem = if sg > 0.0 { l - tw } else { tw };
            let v = mul(u, sg);
            // Only a tip at the top whose rest runs on down at a slant, as a brace does: not one
            // going straight down its own shaft, nor a foot cone whose rest goes up.
            if rem < 1.0 || v[2] <= -NEAR_VERTICAL || v[2] >= -LEVEL {
                continue;
            }
            let x = add(p.a, mul(u, tw));
            let mut post = None;
            for &qid in &near[pid] {
                let q = &d.pieces[qid];
                if q.kind != PieceKind::Axi {
                    continue;
                }
                let ql = dist(q.a, q.b);
                if ql < 2.0 {
                    continue;
                }
                let (top, bot) = if q.a[2] > q.b[2] { (q.a, q.b) } else { (q.b, q.a) };
                if (top[2] - bot[2]).abs() / ql < NEAR_VERTICAL
                    || dist(top, x) > 0.6f64.max(q.width)
                    || seg_dist(q.a, q.b, x).0 > rw + q.width / 2.0 + 0.1
                {
                    continue;
                }
                post = Some(qid);
                break;
            }
            if post.is_none() {
                continue;
            }
            // The tip's span first (keeps the id).
            let spans = if sg > 0.0 { [(0.0, tw), (tw, l)] } else { [(tw, l), (0.0, tw)] };
            let mut segs = Vec::new();
            for (t0, t1) in spans {
                let secs: Vec<Section> = p
                    .sections
                    .iter()
                    .filter(|x| x.t1 > t0 + 1e-6 && x.t0 < t1 - 1e-6)
                    .map(|x| Section { t0: x.t0.max(t0) - t0, t1: x.t1.min(t1) - t0, ..x.clone() })
                    .collect();
                let rmax = secs
                    .iter()
                    .filter(|x| x.k != SectionKind::Ball)
                    .map(|x| x.r0.max(x.r1))
                    .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
                    .unwrap_or(p.width / 2.0);
                let (sa, sb) = (add(p.a, mul(u, t0)), add(p.a, mul(u, t1)));
                let vol: f64 = secs
                    .iter()
                    .filter(|x| x.k != SectionKind::Ball)
                    .map(|x| std::f64::consts::PI * (x.t1 - x.t0) * (x.r0 * x.r0 + x.r0 * x.r1 + x.r1 * x.r1) / 3.0)
                    .sum();
                let lo = [0, 1, 2].map(|k| sa[k].min(sb[k]) - rmax);
                let hi = [0, 1, 2].map(|k| sa[k].max(sb[k]) + rmax);
                segs.push(Segment {
                    a: sa,
                    b: sb,
                    width: 2.0 * rmax,
                    sections: secs,
                    volume: vol,
                    tris: 1usize.max((p.tris as f64 * (t1 - t0) / l) as usize),
                    lo,
                    hi,
                });
            }
            d.pieces[pid].segments = segs;
            done.insert(pid);
        }
    }
    d
}

/// A wide pad on the plate in one shell with the post standing on it (a solid of revolution whose
/// first sections are at least [`RAFT_MIN_XY`] across and end low, under a much narrower rod) is
/// split at the pad's top: the pad is raft like a pad of its own, the rest is a piece standing on
/// it.
pub(super) fn split_pads(d: Raw, contacts: &[Contact]) -> Raw {
    let mut out = d.pieces.clone();
    let mut cuts: std::collections::BTreeMap<usize, (usize, P, P, f64)> = Default::default();
    let frustum = |x: &Section| std::f64::consts::PI * (x.t1 - x.t0) / 3.0 * (x.r0 * x.r0 + x.r0 * x.r1 + x.r1 * x.r1);
    for p in &d.pieces {
        if p.kind != PieceKind::Axi || p.lo[2] > PLATE_Z || p.sections.is_empty() {
            continue;
        }
        let (l, u) = axis_of(p.a, p.b);
        if l < 1e-6 {
            continue;
        }
        let a = p.a;
        if u[2] < 0.97 || a[2] > PLATE_Z + 0.05 {
            continue;
        }
        let mut secs = p.sections.clone();
        secs.sort_by(|x, y| x.t0.total_cmp(&y.t0));
        let mut k = 0;
        while k < secs.len() && secs[k].k != SectionKind::Ball && secs[k].r0.max(secs[k].r1) >= RAFT_MIN_XY / 2.0 {
            k += 1;
        }
        if k == 0 || k == secs.len() {
            continue;
        }
        let tp = secs[k - 1].t1;
        let rest = &secs[k..];
        let rr = rest.iter().map(|x| x.r0.max(x.r1)).fold(f64::MIN, f64::max);
        if a[2] + tp * u[2] > PAD_BAND || rr > 0.5 * secs[k - 1].r0.min(secs[k - 1].r1) || l - tp < 1.0 {
            continue;
        }
        let top = add(a, mul(u, tp));
        let rpad = secs[..k].iter().map(|x| x.r0.max(x.r1)).fold(f64::MIN, f64::max);
        let pad_vol: f64 = secs[..k].iter().map(frustum).sum();
        let rod_id = out.len();
        let pad = RawPiece {
            b: top,
            sections: secs[..k].to_vec(),
            hi: [p.hi[0], p.hi[1], top[2]],
            width: 2.0 * rpad,
            volume: pad_vol,
            ray_volume: pad_vol,
            in_model: 0.0,
            ..p.clone()
        };
        let rv: f64 = rest.iter().map(frustum).sum();
        let rod = RawPiece {
            id: rod_id,
            a: top,
            sections: rest.iter().map(|x| Section { t0: x.t0 - tp, t1: x.t1 - tp, ..x.clone() }).collect(),
            width: 2.0 * rr,
            lo: [top[0].min(p.b[0]) - rr, top[1].min(p.b[1]) - rr, top[2]],
            volume: rv,
            ray_volume: rv,
            ..p.clone()
        };
        out[p.id] = pad;
        out.push(rod);
        cuts.insert(p.id, (rod_id, a, u, tp));
    }
    if cuts.is_empty() {
        return Raw { pieces: d.pieces, conns: d.conns, contact_pieces: d.contact_pieces };
    }
    let mut conns = d.conns.clone();
    for c in conns.iter_mut() {
        for end in [&mut c.a, &mut c.b] {
            if let Some(&(rid, a, u, tp)) = cuts.get(end)
                && dot(sub(c.at, a), u) > tp + 0.05
            {
                *end = rid;
            }
        }
    }
    for (&pid, &(rid, a, u, tp)) in &cuts {
        conns.push(RawConn { a: pid, b: rid, kind: ConnectionKind::Abut, at: add(a, mul(u, tp)), overlap: 0.0 });
    }
    let contact_pieces = d
        .contact_pieces
        .iter()
        .enumerate()
        .map(|(ci, hits)| {
            let x = contacts[ci].at;
            hits.iter()
                .map(|h| match cuts.get(&h.0) {
                    Some(&(rid, a, u, tp)) if dot(sub(x, a), u) > tp => (rid, h.1, h.2),
                    _ => *h,
                })
                .collect()
        })
        .collect();
    Raw { pieces: out, conns: conns.into_iter().filter(|c| c.a != c.b).collect(), contact_pieces }
}

/// A profile read from a few-sided prism's stretch (an 8-sided welded shaft split by the pieces
/// stage) comes as many short rod and cone sections whose radius wobbles between the prism's inner
/// and outer radius; the first small dip then reads as the tip cone. Runs of 6 or more rod and cone
/// sections are smoothed (Douglas-Peucker on the radius profile, [`PROFILE_TOL`]) back into the few
/// sections they stand for; shorter profiles (every turned Chitubox part) are left as read.
pub(super) fn smooth_secs(secs: &[Section]) -> Vec<Section> {
    let mut out = Vec::new();
    let mut run: Vec<Section> = Vec::new();
    let flush = |run: &mut Vec<Section>, out: &mut Vec<Section>| {
        if run.len() < 6 {
            out.append(run);
            return;
        }
        let mut pts = vec![(run[0].t0, run[0].r0)];
        pts.extend(run.iter().map(|x| (x.t1, x.r1)));
        let mut keep = std::collections::BTreeSet::from([0, pts.len() - 1]);
        let mut stack = vec![(0usize, pts.len() - 1)];
        while let Some((i, j)) = stack.pop() {
            let ((ta, ra), (tb, rb)) = (pts[i], pts[j]);
            let (mut best, mut bk) = (0.0, None);
            for (k, &(tk, rk)) in pts.iter().enumerate().take(j).skip(i + 1) {
                let w = (tk - ta) / (tb - ta).max(1e-9);
                let e = (rk - (ra + (rb - ra) * w)).abs();
                if e > best {
                    best = e;
                    bk = Some(k);
                }
            }
            if let Some(bk) = bk
                && best > PROFILE_TOL
            {
                keep.insert(bk);
                stack.push((i, bk));
                stack.push((bk, j));
            }
        }
        let ks: Vec<usize> = keep.into_iter().collect();
        for w in ks.windows(2) {
            let ((ta, ra), (tb, rb)) = (pts[w[0]], pts[w[1]]);
            let k = if (ra - rb).abs() < 0.03 { SectionKind::Rod } else { SectionKind::Cone };
            out.push(Section { k, t0: ta, t1: tb, r0: ra, r1: rb, tc: None, big_r: None });
        }
        run.clear();
    };
    for x in secs {
        if x.k != SectionKind::Ball {
            run.push(x.clone());
        } else {
            flush(&mut run, &mut out);
            out.push(x.clone());
        }
    }
    flush(&mut run, &mut out);
    out
}

/// The classifier's pieces, the raft's top, and the connections, from the pieces stage's output.
pub(super) fn load(pieces: &Pieces, contacts: &[Contact]) -> (Vec<Piece>, f64, Vec<RawConn>, Vec<Vec<Hit>>) {
    let d = Raw::from(pieces);
    let d = split_pads(split_pieces(split_inline(split_pieces(d, contacts), contacts), contacts), contacts);
    let mut ps: Vec<Piece> = d.pieces.iter().map(Piece::new).collect();
    // Raft: wide pieces on the plate, and flat pieces lying on them.
    for p in ps.iter_mut() {
        let xy = (p.hi[0] - p.lo[0]).max(p.hi[1] - p.lo[1]);
        if p.lo[2] <= PLATE_Z && xy >= RAFT_MIN_XY && p.hi[2] - p.lo[2] <= 0.5 * xy && p.hi[2] <= 6.0 {
            p.raft = true;
        }
    }
    let conns = d.conns;
    // Flat pads on the plate that join into a sheet at least RAFT_MIN_XY across are raft too (a
    // raft laid as tiles).
    let flat: Vec<bool> = ps
        .iter()
        .map(|p| {
            !p.raft
                && p.lo[2] <= PLATE_Z
                && p.hi[2] <= 1.5
                && p.hi[2] - p.lo[2] <= 0.35 * (p.hi[0] - p.lo[0]).max(p.hi[1] - p.lo[1])
        })
        .collect();
    let mut par: Vec<usize> = (0..ps.len()).collect();
    fn root(par: &mut [usize], mut x: usize) -> usize {
        while par[x] != x {
            par[x] = par[par[x]];
            x = par[x];
        }
        x
    }
    for c in &conns {
        if c.kind != ConnectionKind::Near && flat[c.a] && flat[c.b] {
            let (x, y) = (root(&mut par, c.a), root(&mut par, c.b));
            par[x] = y;
        }
    }
    let mut boxes: std::collections::BTreeMap<usize, ([f64; 2], [f64; 2])> = Default::default();
    for i in (0..ps.len()).filter(|&i| flat[i]) {
        let r = root(&mut par, i);
        let e = boxes.entry(r).or_insert(([ps[i].lo[0], ps[i].lo[1]], [ps[i].hi[0], ps[i].hi[1]]));
        for k in 0..2 {
            e.0[k] = e.0[k].min(ps[i].lo[k]);
            e.1[k] = e.1[k].max(ps[i].hi[k]);
        }
    }
    for i in (0..ps.len()).filter(|&i| flat[i]) {
        let (lo, hi) = boxes[&root(&mut par, i)];
        if (hi[0] - lo[0]).max(hi[1] - lo[1]) >= RAFT_MIN_XY {
            ps[i].raft = true;
        }
    }
    let raft_max = |ps: &[Piece]| {
        ps.iter()
            .filter(|p| p.raft)
            .map(|p| p.hi[2])
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
            .unwrap_or(0.0)
    };
    let mut raft_top = raft_max(&ps);
    if raft_top > 0.0 {
        let mut grew = true;
        while grew {
            grew = false;
            for c in &conns {
                if c.kind == ConnectionKind::Near {
                    continue;
                }
                for (x, y) in [(c.a, c.b), (c.b, c.a)] {
                    let (p, q) = (&ps[x], &ps[y]);
                    let xy = (p.hi[0] - p.lo[0]).max(p.hi[1] - p.lo[1]);
                    if q.raft
                        && !p.raft
                        && p.hi[2] - p.lo[2] <= 0.25 * xy
                        && xy >= 4.0
                        && p.lo[2] <= raft_top + 0.6
                        && p.hi[2] <= 6.0
                    {
                        ps[x].raft = true;
                        grew = true;
                    }
                }
            }
        }
        raft_top = raft_max(&ps);
        for p in ps.iter_mut() {
            if !p.raft && p.hi[2] <= raft_top + 0.01 {
                p.raft = true;
            }
        }
    }
    for p in ps.iter_mut() {
        // On the plate, or standing flush on the raft's top.
        if !p.raft && (p.lo[2] <= PLATE_Z || (raft_top > 0.0 && p.lo[2] <= raft_top + 0.05)) {
            p.plate = true;
        }
    }
    for c in &conns {
        let weak = c.kind == ConnectionKind::Near;
        if ps[c.a].raft || ps[c.b].raft {
            if !weak {
                for (x, y) in [(c.a, c.b), (c.b, c.a)] {
                    if ps[y].raft && !ps[x].raft {
                        if ps[x].lo[2] <= raft_top + 0.5 {
                            ps[x].plate = true;
                        }
                        ps[x].on_raft = true;
                    }
                }
            }
            continue;
        }
        let (ta, tb) = (ps[c.a].t_of(c.at), ps[c.b].t_of(c.at));
        ps[c.a].att.push(Att { t: ta, q: c.b, at: c.at, weak });
        ps[c.b].att.push(Att { t: tb, q: c.a, at: c.at, weak });
    }
    let _ = len;
    (ps, raft_top, conns, d.contact_pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(k: SectionKind, t0: f64, t1: f64, r0: f64, r1: f64) -> Section {
        Section { k, t0, t1, r0, r1, tc: None, big_r: None }
    }

    #[test]
    fn a_wobbling_prism_profile_is_smoothed_and_a_short_one_kept() {
        // Eight short sections wobbling 0.03 mm about 0.5 mm: one rod.
        let wobble: Vec<Section> =
            (0..8).map(|i| sec(SectionKind::Rod, i as f64, i as f64 + 1.0, 0.5 + 0.03 * (i % 2) as f64, 0.5)).collect();
        let s = smooth_secs(&wobble);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].k, SectionKind::Rod);
        let short = vec![sec(SectionKind::Cone, 0.0, 1.0, 0.1, 0.4), sec(SectionKind::Rod, 1.0, 5.0, 0.4, 0.4)];
        assert_eq!(smooth_secs(&short), short);
    }

    #[test]
    fn argmin_takes_the_first_of_equal_values() {
        assert_eq!(argmin([3.0, 1.0, 1.0, 2.0].into_iter()), 1);
    }
}
