//! Stage 6a: shaping. Each tip's contact, socket and widths, sphere-tip data, shaft and root
//! widths, and the line helpers the layout uses.

use super::super::math::{P, add, closest_on_tri, cross, dist, dot, len, mul, sub, unit_div};
use super::super::pieces::{PieceKind, SectionKind};
use super::layout::{LTip, Lay};
use super::{BLUNT_MM, DRAWN_AREA_SHARE, MAX_CD, MIN_CD, Owner, ROOT_TOP, Reader, STRAIGHT_MM, STUB_MM, Step};

/// The point `d` along the polyline (its end when past it).
pub(super) fn point_along(pts: &[P], d: f64) -> P {
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = dist(w[0], w[1]);
        if acc + l >= d && l > 1e-9 {
            return add(w[0], mul(sub(w[1], w[0]), (d - acc) / l));
        }
        acc += l;
    }
    pts[pts.len() - 1]
}

/// The polyline from `d` along it to its end.
pub(super) fn cut_from(pts: &[P], d: f64) -> Vec<P> {
    let mut acc = 0.0;
    for (k, w) in pts.windows(2).enumerate() {
        let l = dist(w[0], w[1]);
        if acc + l >= d && l > 1e-9 {
            let mut out = vec![add(w[0], mul(sub(w[1], w[0]), (d - acc) / l))];
            out.extend_from_slice(&pts[k + 1..]);
            return out;
        }
        acc += l;
    }
    vec![pts[pts.len() - 1]]
}

pub(super) fn polyline_len(pts: &[P]) -> f64 {
    pts.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// Douglas-Peucker: the line's bends that leave the straight run by more than [`STRAIGHT_MM`].
pub(super) fn simplify(pts: &[P]) -> Vec<P> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let d = sub(b, a);
    let dd = dot(d, d).max(1e-12);
    let mut k = 1;
    let mut far = f64::NEG_INFINITY;
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let off = dist(*p, add(a, mul(d, (dot(sub(*p, a), d) / dd).clamp(0.0, 1.0))));
        if off > far {
            far = off;
            k = i;
        }
    }
    if far <= STRAIGHT_MM {
        return vec![a, b];
    }
    let mut left = simplify(&pts[..=k]);
    left.pop();
    left.extend(simplify(&pts[k..]));
    left
}

/// A line rising from the plate: where it crosses the root top, and its points above that.
pub(super) fn above_root(pts: &[P]) -> (P, Vec<P>) {
    let above: Vec<P> = pts.iter().copied().filter(|p| p[2] > ROOT_TOP + 0.05).collect();
    let k = pts.len() - above.len();
    if above.is_empty() || k == 0 {
        return ([pts[0][0], pts[0][1], ROOT_TOP], above);
    }
    let (a, b) = (pts[k - 1], pts[k]);
    let w = (ROOT_TOP - a[2]) / (b[2] - a[2]).max(1e-9);
    let x = add(a, mul(sub(b, a), w.clamp(0.0, 1.0)));
    ([x[0], x[1], ROOT_TOP], above)
}

/// Points spread evenly over the unit sphere (Fibonacci) and each one's 6 nearest neighbours.
fn sphere_points() -> (Vec<P>, Vec<[usize; 6]>) {
    let n = 500;
    let u: Vec<P> = (0..n)
        .map(|i| {
            let k = i as f64 + 0.5;
            let phi = (1.0 - 2.0 * k / n as f64).acos();
            let th = std::f64::consts::PI * (1.0 + 5f64.sqrt()) * k;
            [th.cos() * phi.sin(), th.sin() * phi.sin(), phi.cos()]
        })
        .collect();
    let nb = u
        .iter()
        .map(|a| {
            let mut idx: Vec<usize> = (0..n).collect();
            idx.sort_by(|&i, &j| (-dot(*a, u[i])).total_cmp(&-dot(*a, u[j])));
            [idx[1], idx[2], idx[3], idx[4], idx[5], idx[6]]
        })
        .collect();
    (u, nb)
}

impl Reader<'_> {
    /// Socket and body of the tip: the first cone along the route that widens away from the
    /// contact, else a blunt tip as wide as its rod with the socket [`BLUNT_MM`] down the route.
    /// Returns the tip and the walked length where the shaft starts.
    pub(super) fn tip_of(&self, ci: usize, path: &[Step], pts: &[P]) -> (LTip, f64) {
        let c = &self.c[ci];
        let at = c.at;
        let mut walked = 0.0;
        let mut found: Option<(f64, f64, P)> = None;
        // The radius of the section the contact sits in (a ball end's own radius).
        let mut r_contact: f64 = 0.0;
        let mut blunt = false;
        for &(q, t0, t1) in path {
            let pq = &self.p[q];
            let s = if t1 >= t0 { 1.0 } else { -1.0 };
            let mut secs = pq.secs.clone();
            secs.sort_by(|x, y| (x.t0 * s).total_cmp(&(y.t0 * s)));
            for sec in &secs {
                let (lo, hi) = (sec.t0, sec.t1);
                let (near, far) = if s > 0.0 { (lo, hi) } else { (hi, lo) };
                let (rn, rf) = if s > 0.0 { (sec.r0, sec.r1) } else { (sec.r1, sec.r0) };
                if (far - t0) * s <= 0.05 {
                    continue; // behind the contact
                }
                if walked == 0.0 && (near - t0) * s <= 0.05 {
                    // The section holding the contact. A cone narrowing away from the contact is a
                    // blunt wide end in the model (a micro rod's or strut's lower end), not a tip
                    // cone further on.
                    let rs =
                        if sec.k == SectionKind::Ball { sec.r0.max(sec.r1).max((hi - lo) / 2.0) } else { pq.r_at(t0) };
                    r_contact = r_contact.max(rs);
                    if sec.k == SectionKind::Cone && rn > rf * 1.15 && (far - t0) * s > 0.2 {
                        blunt = true;
                        break;
                    }
                }
                let start = walked + ((near - t0) * s).max(0.0);
                if sec.k == SectionKind::Cone && rf > rn * 1.15 && start <= 2.0 {
                    let far_c = if (t1 - far) * s >= -1e-6 { far } else { t1 };
                    found =
                        Some((walked + (far_c - t0) * s, if far_c == far { rf } else { pq.r_at(far_c) }, pq.at(far_c)));
                    break;
                }
                if sec.k == SectionKind::Rod && (far - (near * s).max(t0 * s) * s) * s > 1.0 && start <= 2.0 {
                    break;
                }
            }
            if found.is_some() || blunt || walked + (t1 - t0).abs() > 2.5 {
                break;
            }
            walked += (t1 - t0).abs();
        }
        let total: f64 = path.iter().map(|x| (x.2 - x.1).abs()).sum();
        let q0 = &self.p[path[0].0];
        let (dist_, body_r, socket) = match found {
            Some((mut d, body_r, mut socket)) => {
                if d > 0.9 * total && total > 0.1 {
                    // The route ends where the cone does (a tip straight onto another support's
                    // rod): the socket sits a little short of the end so a stub of shaft is left to
                    // hang from the knot.
                    d = total - (0.1 * total).min(STUB_MM);
                    socket = point_along(pts, d);
                }
                (d, body_r, socket)
            }
            None => {
                // An unread piece (shape unknown: a bent or welded solid) keeps 1 mm.
                let d = (if q0.kind == PieceKind::Unread { 1.0 } else { BLUNT_MM }).min(0.5 * total);
                let body_r = if q0.ball { q0.width / 2.0 } else { q0.r_at(path[0].1) };
                (d, body_r, point_along(pts, d))
            }
        };
        let mut n = mul(unit_div(c.dir), -1.0);
        let ax = unit_div(sub(socket, at));
        if dot(n, ax) < -0.3 {
            n = mul(n, -1.0);
        }
        // DragonFruit stands the tip's disk off the surface along the surface normal, so the loop it
        // leaves is as wide as the contact whatever the lean of the tip: the loop's own width.
        let loop_d = 2.0 * (c.area / std::f64::consts::PI).sqrt();
        let mut d = (loop_d / DRAWN_AREA_SHARE.sqrt()).clamp(MIN_CD, MAX_CD);
        // A ball end in the model is as wide as its ball; a loop round the ends of several welded
        // rods as wide as the widest of them.
        for (&(c3, q2), &inside) in &self.c_in {
            if c3 == ci && inside && !self.p[q2].ball {
                r_contact = r_contact.max(self.p[q2].r_at(self.p[q2].t_of(at)));
            }
        }
        let cap = body_r.max(r_contact);
        // A blunt rod end is a large tip matching the rod, sized from the loop: a rod with no cone
        // that ends in the model at a slant (or deep) leaves a loop wider than its section. Its
        // contact keeps the loop's width and its body the rod's; the writer keeps the cone the
        // rod's width and sinks it until its loop has that area. A cone tip's contact stays within
        // its body.
        let blunt_end =
            found.is_none() && q0.kind != PieceKind::Unread && !q0.ball && d > 2.0 * body_r && 2.0 * body_r > MIN_CD;
        if !blunt_end && 2.0 * cap > MIN_CD {
            d = d.min(2.0 * cap);
        }
        let mut tip = LTip {
            contact: at,
            normal: n,
            socket,
            contact_d: d,
            body_d: if blunt_end { 2.0 * body_r } else { (2.0 * body_r).max(d) },
            ball_d: None,
            cone_end_d: None,
            ball_depth: None,
            rounded: false,
            ball_c: None,
        };
        // A separate ball the model cuts at this contact: its diameter, for a writer that can build
        // a sphere tip. The ball must hold the contact (its centre within its radius of the loop's
        // centre) and be wider than the loop.
        let hits = &self.contact_pieces[ci];
        let mut ball: Option<&super::Piece> = None;
        for h in hits {
            let b = &self.p[h.0];
            if b.ball
                && !b.raft
                && dist(mul(add(b.lo, b.hi), 0.5), at) <= b.width / 2.0 + 0.05
                && b.width >= loop_d
                && ball.is_none_or(|x| b.width > x.width)
            {
                ball = Some(b);
            }
        }
        if let Some(b) = ball {
            tip.ball_d = Some(b.width);
            // The cone's narrow end at the ball's centre: the widest rod or cone at the contact,
            // read at the ball's centre.
            let mid = mul(add(b.lo, b.hi), 0.5);
            let ends: Vec<f64> = hits
                .iter()
                .map(|h| &self.p[h.0])
                .filter(|p| !p.ball && !p.raft && p.l > 1e-6 && dist(p.at(p.t_of(mid)), mid) <= b.width / 2.0)
                .map(|p| 2.0 * p.r_at(p.t_of(mid)))
                .filter(|&e| 0.0 < e && e < b.width)
                .collect();
            if !ends.is_empty() {
                tip.cone_end_d = Some(ends.iter().copied().fold(f64::INFINITY, f64::min));
            }
            tip.ball_c = Some(mid);
        } else if let Some((r, centre)) = self.rounded_end(q0, at, c.area) {
            // A rounded rod end the model cuts is a tip: a sphere tip of the rod's width seated at
            // the dome's own depth. Not where the loop is wider than the rod (a slanted end stays a
            // sunk blunt end).
            tip.ball_d = Some(2.0 * r);
            tip.cone_end_d = Some(2.0 * r);
            tip.body_d = tip.body_d.max(2.0 * r);
            tip.rounded = true;
            tip.ball_c = Some(centre);
        }
        (tip, dist_)
    }

    /// (cap radius, dome centre) when piece `q` ends at the contact in a ball cap about as wide as
    /// the rod next to it (its tip radius under 0.35 R, a rod section within 20% of R, the cap's end
    /// within R + 0.3 mm of the contact) and the loop is no wider than the rod.
    fn rounded_end(&self, q: &super::Piece, at: P, area: f64) -> Option<(f64, P)> {
        if q.ball || q.l < 1e-6 || q.secs.len() < 2 {
            return None;
        }
        let mut secs = q.secs.clone();
        secs.sort_by(|x, y| x.t0.total_cmp(&y.t0));
        let u = super::super::math::div(sub(q.b, q.a), q.l);
        let n = secs.len();
        for (cap, nxt, end_b) in [(&secs[n - 1], &secs[n - 2], true), (&secs[0], &secs[1], false)] {
            if cap.k != SectionKind::Ball {
                continue;
            }
            let r = cap.big_r.unwrap_or(cap.r0.max(cap.r1));
            if (if end_b { cap.r1 } else { cap.r0 }) > 0.35 * r || nxt.k != SectionKind::Rod {
                continue;
            }
            let rn = nxt.r0.max(nxt.r1);
            if (r / rn.max(1e-9) - 1.0).abs() > 0.2 {
                continue;
            }
            let endpt = add(q.a, mul(u, if end_b { cap.t1 } else { cap.t0 }));
            if dist(endpt, at) > r + 0.3 {
                continue;
            }
            if 2.0 * (area / std::f64::consts::PI).sqrt() > 2.0 * r {
                return None;
            }
            let tc = cap.tc.unwrap_or(if end_b { cap.t1 - r } else { cap.t0 + r });
            return Some((r, add(q.a, mul(u, tc))));
        }
        None
    }

    /// A sphere tip is seated at the original ball's (or dome's) own depth, so its sunk part is
    /// rebuilt, unless a ball there would cross the model's surface a second time (more than one
    /// loop): then the writer keeps DragonFruit's own seat. Sets `ball_depth` (the original
    /// centre's depth below the surface, negative outside) on the tips that may take the depth
    /// seat. The ball is placed as the writer will place it: crossing the surface in the loop,
    /// centre on the original's side, along the surface normal.
    pub(super) fn ball_seats(&mut self, lay: &mut Lay) {
        let mut tips: Vec<&mut LTip> = Vec::new();
        for s in lay.supports.iter_mut() {
            tips.push(&mut s.tip);
            if let super::layout::LBase::Lower(low) = &mut s.base {
                tips.push(low);
            }
        }
        let todo: Vec<&mut LTip> = tips.into_iter().filter(|t| t.ball_c.is_some()).collect();
        if todo.is_empty() || self.model.is_empty() {
            for t in todo {
                t.ball_c = None;
            }
            return;
        }
        let (su, snb) = sphere_points();
        let tn: Vec<P> = self.model.iter().map(|t| cross(sub(t[1], t[0]), sub(t[2], t[0]))).collect();
        for t in todo {
            let cb = t.ball_c.take().expect("filtered on ball_c");
            let at = t.contact;
            let r = t.ball_d.unwrap_or(0.0) / 2.0;
            let f = t.contact_d * DRAWN_AREA_SHARE.sqrt() / 2.0;
            if !t.rounded && f < 0.6 * r {
                // A loop much narrower than a separate ball is not the ball's own crossing.
                self.seats.1 += 1;
                continue;
            }
            let lo = [0, 1, 2].map(|k| at[k] - r * 2.0 - 0.1);
            let hi = [0, 1, 2].map(|k| at[k] + r * 2.0 + 0.1);
            let sel: Vec<usize> = self.model_boxes().query(lo, hi).into_iter().map(|i| i as usize).collect();
            if sel.is_empty() {
                continue;
            }
            // Outward normal at the contact: the faces within the loop, turned toward the socket.
            let mut n = [0.0; 3];
            let mut any = false;
            for &i in &sel {
                if dist(at, closest_on_tri(at, &self.model[i])) <= f.max(0.05) {
                    n = add(n, tn[i]);
                    any = true;
                }
            }
            if !any {
                n = t.normal;
            }
            n = super::super::math::div(n, len(n).max(1e-12));
            if dot(n, sub(t.socket, at)) < 0.0 {
                n = mul(n, -1.0);
            }
            let depth = dot(sub(at, cb), n);
            let side = if depth >= 0.0 { 1.0 } else { -1.0 };
            let geo = (r * r - f * f).max(0.0).sqrt();
            // As the writer seats it: a rounded rod end whose dome sits deeper than its loop needs
            // keeps its own depth.
            let centre = sub(at, mul(n, if t.rounded && depth > geo { depth } else { side * geo }));
            let inside: Vec<bool> = su
                .iter()
                .map(|u| {
                    let p = add(centre, mul(*u, r));
                    let qs: Vec<(f64, P, usize)> = sel
                        .iter()
                        .map(|&i| {
                            let q = closest_on_tri(p, &self.model[i]);
                            (dist(p, q), q, i)
                        })
                        .collect();
                    let mut order: Vec<usize> = (0..qs.len()).collect();
                    order.sort_by(|&a, &b| qs[a].0.total_cmp(&qs[b].0));
                    let j = &order[..order.len().min(3)];
                    // The nearest face's side; among faces as near (an edge or corner), the one the
                    // point is most clearly off.
                    let mut best = j[0];
                    let mut score = f64::NEG_INFINITY;
                    for &k in j {
                        let s = dot(sub(p, qs[k].1), tn[qs[k].2]).abs()
                            * if qs[k].0 <= qs[j[0]].0 + 1e-6 { 1.0 } else { 0.0 };
                        if s > score {
                            score = s;
                            best = k;
                        }
                    }
                    dot(sub(p, qs[best].1), tn[qs[best].2]) < 0.0
                })
                .collect();
            let mut loops = 0;
            for want in [true, false] {
                let mut seen = vec![false; su.len()];
                let mut comps = 0;
                for k0 in 0..su.len() {
                    if inside[k0] != want || seen[k0] {
                        continue;
                    }
                    let mut stack = vec![k0];
                    seen[k0] = true;
                    let mut size = 0;
                    while let Some(k) = stack.pop() {
                        size += 1;
                        for &m in &snb[k] {
                            if !seen[m] && inside[m] == want {
                                seen[m] = true;
                                stack.push(m);
                            }
                        }
                    }
                    if size >= 3 {
                        comps += 1;
                    }
                }
                loops += comps;
            }
            if loops <= 2 {
                t.ball_depth = Some(depth);
                self.seats.0 += 1;
            } else {
                self.seats.1 += 1;
            }
        }
    }

    /// Length-weighted median width of what the route walks past the tip: rods at their width,
    /// long tapers (a trunk that narrows all the way up) at the width of a rod of the same volume.
    pub(super) fn shaft_d(&self, path: &[Step], skip: f64) -> Option<f64> {
        let mut items: Vec<(f64, f64)> = Vec::new();
        let mut walked = 0.0;
        for &(q, t0, t1) in path {
            let pq = &self.p[q];
            let sgn = if t1 >= t0 { 1.0 } else { -1.0 };
            let span = (t1 - t0).abs();
            for sec in &pq.secs {
                if sec.k == SectionKind::Ball {
                    continue;
                }
                let (a, b) = ((sec.t0 - t0) * sgn, (sec.t1 - t0) * sgn);
                let (x0, x1) = (a.min(b).max(0.0), a.max(b).min(span));
                if x1 - x0 <= 0.05 || walked + x0 < skip - 0.05 {
                    continue;
                }
                if sec.k == SectionKind::Rod {
                    items.push((sec.r0 + sec.r1, x1 - x0));
                } else if x1 - x0 > 1.0 {
                    items.push((2.0 * ((sec.r0 * sec.r0 + sec.r0 * sec.r1 + sec.r1 * sec.r1) / 3.0).sqrt(), x1 - x0));
                }
            }
            if pq.kind == PieceKind::Unread && walked + span > skip {
                items.push((pq.width, span));
            }
            walked += span;
        }
        if items.is_empty() {
            return None;
        }
        items.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let half = items.iter().map(|x| x.1).sum::<f64>() / 2.0;
        let mut acc = 0.0;
        for &(w, l) in &items {
            acc += l;
            if acc >= half {
                return Some(w);
            }
        }
        Some(items[items.len() - 1].0)
    }

    /// A piece's radius by height, as (z0, z1, r0, r1) stretches, scaled so a square pad counts by
    /// its area.
    fn profile_of(&self, p: &super::Piece) -> Vec<(f64, f64, f64, f64)> {
        let rev: f64 = p
            .secs
            .iter()
            .filter(|s| s.k != SectionKind::Ball)
            .map(|s| std::f64::consts::PI * (s.t1 - s.t0) / 3.0 * (s.r0 * s.r0 + s.r0 * s.r1 + s.r1 * s.r1))
            .sum();
        let k = if rev > 1e-9 && p.kind != PieceKind::Bar { (p.vol / rev).clamp(0.3, 1.2).sqrt() } else { 1.0 };
        let dz = p.d[2].max(0.2);
        p.secs
            .iter()
            .filter(|s| s.k != SectionKind::Ball)
            .map(|s| (p.a[2] + s.t0 * dz, p.a[2] + s.t1 * dz, s.r0 * k, s.r1 * k))
            .collect()
    }

    /// The root's width. DragonFruit's root is a 0.5 mm disk under a 1.5 mm cone up to the shaft at
    /// the 2 mm root top. The width is the one whose root best overlaps the original foot above the
    /// raft (the root pieces under the support and the foot piece's own flare, taken as round about
    /// one axis): the largest intersection over union, never narrower than the shaft.
    pub(super) fn root_d(&self, q: usize, shaft: f64, owner: Owner) -> f64 {
        let r = shaft / 2.0;
        let mut prof = self.profile_of(&self.p[q]);
        for &i in self.root_p.get(&owner).map(|v| v.as_slice()).unwrap_or(&[]) {
            prof.extend(self.profile_of(&self.p[i]));
        }
        let z0 = self.raft_top.min(ROOT_TOP - 0.1);
        let zs = super::super::math::linspace(z0, ROOT_TOP, 61);
        let mut ro = vec![r; zs.len()];
        for &(a, b, r0, r1) in &prof {
            if b > a {
                for (k, &z) in zs.iter().enumerate() {
                    if z >= a - 1e-6 && z <= b + 1e-6 {
                        ro[k] = ro[k].max(r0 + (r1 - r0) * (z - a) / (b - a));
                    }
                }
            }
        }
        let wide = ro.iter().copied().fold(f64::MIN, f64::max);
        let mut best = (-1.0, r * 1.05);
        for big in super::super::math::linspace(r * 1.05, wide.max(r * 1.05), 40) {
            let (mut num, mut den) = (Vec::with_capacity(zs.len()), Vec::with_capacity(zs.len()));
            for (k, &z) in zs.iter().enumerate() {
                let rd = if z <= 0.5 { big } else { big + (r - big) * (z - 0.5) / 1.5 };
                num.push(ro[k].min(rd).powi(2));
                den.push(ro[k].max(rd).powi(2));
            }
            let iou = super::super::math::pairwise_sum(&num) / super::super::math::pairwise_sum(&den);
            if iou > best.0 + 1e-9 {
                best = (iou, big);
            }
        }
        2.0 * best.1
    }

    /// The read radius of the route at `x` (the route piece whose axis passes nearest).
    pub(super) fn radius_on(&self, path: &[Step], x: P) -> Option<f64> {
        let mut best: Option<(f64, f64)> = None;
        for &(q, _, _) in path {
            let pq = &self.p[q];
            if pq.ball {
                continue;
            }
            let t = pq.t_of(x);
            let dd = dist(x, pq.at(t));
            if best.is_none_or(|b| dd < b.0) {
                best = Some((dd, if pq.kind == PieceKind::Unread { pq.width / 2.0 } else { pq.r_at(t) }));
            }
        }
        best.map(|b| b.1)
    }

    /// (stick, twig): a strut whose end pieces both carry a cone is a stick, one where neither
    /// does a twig.
    pub(super) fn strut_kind(&self, path: &[Step]) -> (bool, bool) {
        let cone = |q: usize| {
            self.p[q].secs.iter().any(|s| s.k == SectionKind::Cone && s.r0.max(s.r1) > 1.15 * s.r0.min(s.r1))
        };
        let (a, b) = (cone(path[0].0), cone(path[path.len() - 1].0));
        (a && b, !a && !b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_along_a_polyline_and_the_line_cut_from_them() {
        let pts = [[0.0, 0.0, 0.0], [0.0, 0.0, 2.0], [2.0, 0.0, 2.0]];
        assert_eq!(point_along(&pts, 1.0), [0.0, 0.0, 1.0]);
        assert_eq!(point_along(&pts, 3.0), [1.0, 0.0, 2.0]);
        assert_eq!(point_along(&pts, 9.0), [2.0, 0.0, 2.0]);
        assert_eq!(cut_from(&pts, 3.0), vec![[1.0, 0.0, 2.0], [2.0, 0.0, 2.0]]);
        assert_eq!(polyline_len(&pts), 4.0);
    }

    #[test]
    fn simplify_keeps_only_bends_past_the_straight_tolerance() {
        let wiggle = [[0.0, 0.0, 0.0], [0.0, 0.05, 5.0], [0.0, 0.0, 10.0]];
        assert_eq!(simplify(&wiggle).len(), 2);
        let bend = [[0.0, 0.0, 0.0], [0.0, 1.0, 5.0], [0.0, 0.0, 10.0]];
        assert_eq!(simplify(&bend).len(), 3);
    }

    #[test]
    fn a_line_from_the_plate_starts_at_the_root_top() {
        let (foot, above) = above_root(&[[0.0, 0.0, 0.0], [0.0, 0.0, 10.0]]);
        assert_eq!(foot, [0.0, 0.0, ROOT_TOP]);
        assert_eq!(above, vec![[0.0, 0.0, 10.0]]);
    }

    #[test]
    fn sphere_points_cover_the_sphere_with_neighbours() {
        let (u, nb) = sphere_points();
        assert_eq!(u.len(), 500);
        assert!(u.iter().all(|p| (len(*p) - 1.0).abs() < 1e-12));
        assert!(nb.iter().enumerate().all(|(i, n)| !n.contains(&i)));
    }
}
