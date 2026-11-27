//! Stage 4: contacts and routes. Each contact gets its piece; pads (the ground) and sleeves
//! (pieces inside a host's shaft) are found; then each tip follows its own trunk down to the
//! plate, a footing on the model (another contact: a strut) or a rod an earlier route owns (a
//! branch).

use std::collections::BTreeSet;

use super::super::math::{P, dist, dot, mul, unit_div};
use super::super::pieces::{PieceKind, SectionKind};
use super::{
    CONE_BODY, DETOUR_W, End, Exception, LEVEL, NEAR_VERTICAL, Owner, ROOT_TOP, Reader, Route, SAME_ROD_DEG,
    SAME_ROD_OFF, SAME_ROD_WIDTH, SLEEVE_COS, SLEEVE_OFF, SLEEVE_SHARE, Step, VEE_HUB_MM, VEE_MIN_COS,
};

/// A way on from a joint: (group by distance from the piece's end, class (-1 the same rod going
/// on, 0 down, 1 sloping down, 2 level, each plus `PASSING` for a rod passing by the joint and the
/// first two plus `BESIDE` for a rod standing off the end, 2.5 an owned rod, 3 up), turn in
/// degrees, piece, t on it, direction, t on the piece left).
pub(super) type Way = (usize, f64, f64, usize, f64, f64, f64);

/// The class of a way onto a rod an earlier route owns, or onto another tip's own piece.
const OWNED: f64 = 2.5;
/// The class of a way that is the same rod going on.
const SAME_ROD: f64 = -1.0;
/// Added to the class of a way down onto another tip's shaft standing off this rod's end by over
/// a radius.
const BESIDE: f64 = 1.0;
/// Added to the class of a way down or level onto a rod that passes by the joint.
const PASSING: f64 = 0.25;

type Found = (Vec<Step>, End);

/// A piece's read radius at `t`, a ball section by its own round profile.
fn profile_r(p: &super::Piece, t: f64) -> f64 {
    for s in &p.secs {
        if s.t0 - 1e-6 <= t && t <= s.t1 + 1e-6 {
            if s.k != SectionKind::Ball {
                return s.r0 + (s.r1 - s.r0) * (t - s.t0) / (s.t1 - s.t0).max(1e-9);
            }
            let big = s.big_r.unwrap_or(s.r0.max(s.r1));
            let off = t - s.tc.unwrap_or(0.5 * (s.t0 + s.t1));
            return (big * big - off * off).max(0.0).sqrt();
        }
    }
    p.r
}

/// The area two circles of radii `r0` and `r1`, `gap` apart, share.
fn lens_area(r0: f64, r1: f64, gap: f64) -> f64 {
    let (big, small) = (r0.max(r1), r0.min(r1));
    if gap >= big + small {
        return 0.0;
    }
    if gap <= big - small {
        return std::f64::consts::PI * small * small;
    }
    let a0 = ((gap * gap + r0 * r0 - r1 * r1) / (2.0 * gap * r0)).clamp(-1.0, 1.0).acos();
    let a1 = ((gap * gap + r1 * r1 - r0 * r0) / (2.0 * gap * r1)).clamp(-1.0, 1.0).acos();
    let kite = ((-gap + r0 + r1) * (gap + r0 - r1) * (gap - r0 + r1) * (gap + r0 + r1)).max(0.0).sqrt();
    r0 * r0 * a0 + r1 * r1 * a1 - 0.5 * kite
}

impl Reader<'_> {
    /// Contacts' pieces, pads and sleeves.
    pub(super) fn prepare(&mut self) {
        self.contact_piece();
        // Pads: pieces on the plate or raft wholly under the root top (from the plate, as every
        // root-top rule), holding no contact (a foot cone, a disc under a support). They are the
        // ground: what stands on one stands on the plate, and no route runs through one.
        for i in 0..self.p.len() {
            let p = &self.p[i];
            if !p.raft && !p.ball && p.plate && p.hi[2] <= ROOT_TOP + 0.6 && self.on_of(i).is_empty() {
                self.p[i].pad = true;
            }
        }
        for i in 0..self.p.len() {
            if !self.p[i].pad {
                continue;
            }
            for a in self.p[i].att.clone() {
                let q = a.q;
                if !a.weak && !self.p[q].pad && a.at[2] <= ROOT_TOP + 1.0 && self.p[q].lo[2] <= ROOT_TOP + 0.6 {
                    self.p[q].plate = true;
                    // A joint ball on the pad (a short trunk read as a pad, its ball, then the tip
                    // cone): the pieces on the ball stand on the plate too, else no route reaches
                    // the ground (no route runs through a pad and a ball is only a hub).
                    if self.p[q].ball {
                        for (r, _, _) in self.hub(i as isize, q, a.at, false) {
                            let pr = &self.p[r];
                            if !pr.pad && !pr.raft && pr.lo[2] <= ROOT_TOP + 0.6 {
                                self.p[r].plate = true;
                            }
                        }
                    }
                }
            }
        }
        self.find_sleeves();
    }

    /// A piece running inside another support piece along the same line is part of it, a sleeve
    /// that adds nothing to the build (doubled shafts of branches standing in their host's
    /// column). No route runs through a sleeve, so a support reaching it joins the host where it
    /// meets it. Thresholds from 6 real models: sleeves have centre lines within 0.02 to 0.25 of
    /// the host's radius and share 0.73 to 1.00 of their volume with it; the nearest other pieces
    /// share at most 0.53.
    fn find_sleeves(&mut self) {
        let mut ov: std::collections::BTreeMap<(usize, usize), f64> = Default::default();
        for c in &self.conns {
            if c.kind != super::super::pieces::ConnectionKind::Near {
                let v = ov.get(&(c.a, c.b)).copied().unwrap_or(0.0) + c.overlap;
                ov.insert((c.a, c.b), v);
                ov.insert((c.b, c.a), v);
            }
        }
        let ok = |p: &super::Piece| !p.raft && !p.ball && !p.pad && p.l >= 2.0 * p.width && p.l > 1e-6;
        // Largest first, so a host is known not to be a sleeve itself.
        let mut order: Vec<usize> = (0..self.p.len()).collect();
        order.sort_by(|&a, &b| self.p[b].vol.total_cmp(&self.p[a].vol));
        for qi in order {
            let q = &self.p[qi];
            if !ok(q) || !self.on_of(qi).is_empty() {
                continue;
            }
            let mut best: Option<(f64, usize)> = None;
            for a in &q.att {
                let h = a.q;
                let ph = &self.p[h];
                if a.weak || !ok(ph) || ph.sleeve.is_some() || ph.vol < q.vol || dot(q.d, ph.d).abs() < SLEEVE_COS {
                    continue;
                }
                let pts: Vec<P> = (0..9).map(|k| q.at(q.l * k as f64 / 8.0)).collect();
                let along = |x: P| dot(super::super::math::sub(x, ph.a), ph.d);
                let inside: Vec<P> =
                    pts.iter().copied().filter(|&x| (-0.01..=ph.l + 0.01).contains(&along(x))).collect();
                // Wholly inside the host's span, but for a foot in the root zone (a doubled shaft
                // standing on the host's root or the raft).
                if inside.len() < 2
                    || pts.iter().any(|&x| !(-ph.r <= along(x) && along(x) <= ph.l + ph.r) && x[2] > ROOT_TOP)
                {
                    continue;
                }
                let off = inside.iter().map(|&x| dist(x, ph.at(ph.t_of(x)))).fold(f64::MIN, f64::max);
                let share = ov.get(&(qi, h)).copied().unwrap_or(0.0) / q.vol.max(1e-9);
                // ... and its top reaches another piece; a post with a free top standing beside a
                // trunk stays a post.
                let reach = 0.6f64.max(2.5 * q.r);
                let tops = q
                    .att
                    .iter()
                    .any(|b| !b.weak && b.q != h && b.t >= q.l - reach && !self.p[b.q].raft && !self.p[b.q].pad);
                if off <= SLEEVE_OFF * ph.r && share >= SLEEVE_SHARE && tops && best.is_none_or(|b| share > b.0) {
                    best = Some((share, h));
                }
            }
            if let Some((share, h)) = best {
                self.p[qi].sleeve = Some(h);
                self.p[qi].why = format!("inside piece {h}'s shaft along its line ({share:.2} of its volume shared)");
            }
        }
        self.sleeves = self.p.iter().filter(|p| p.sleeve.is_some()).count();
    }

    /// Whether `x` lies well inside the read profile of a cone or rod ending there.
    fn in_profile(p: &super::Piece, x: P) -> bool {
        if p.kind != PieceKind::Axi || p.ball || p.secs.is_empty() || p.l < 1e-6 {
            return false;
        }
        let t = dot(super::super::math::sub(x, p.a), p.d);
        if !(-0.02..=p.l + 0.02).contains(&t) {
            return false;
        }
        let tc = t.clamp(0.0, p.l);
        if tc.min(p.l - tc) > 0.6 {
            return false;
        }
        dist(x, p.at(tc)) <= 0.85 * p.r_at(tc)
    }

    fn contact_piece(&mut self) {
        for (ci, c) in self.c.iter().enumerate() {
            let at = c.at;
            let mut hits: Vec<(usize, bool, f64)> =
                self.contact_pieces[ci].iter().copied().filter(|h| !self.p[h.0].raft).collect();
            if !hits.iter().any(|h| h.1) {
                // A parity ray can run exactly along the edges of a polygonal cone and miss it:
                // when no piece holds the contact by parity, one that ends there with the contact
                // well inside its own read profile does.
                hits = hits.iter().map(|h| (h.0, Self::in_profile(&self.p[h.0], at), h.2)).collect();
            }
            if hits.is_empty() {
                self.exceptions.push(Exception {
                    kind: "contact",
                    at,
                    why: "no support piece at the contact (raft only or none within reach)".into(),
                    contact: Some(ci),
                    piece: None,
                    role: None,
                });
                continue;
            }
            let inside_set: BTreeSet<usize> =
                hits.iter().filter(|h| h.1 || Self::in_profile(&self.p[h.0], at)).map(|h| h.0).collect();
            self.inside_at.insert(ci, inside_set);
            let inside: Vec<(usize, bool, f64)> = hits.iter().copied().filter(|h| h.1).collect();
            let pool = if inside.is_empty() { vec![hits[0]] } else { inside };
            // A rod or cone before a ball; then the one whose end is nearest the contact.
            let key = |h: &(usize, bool, f64)| {
                let p = &self.p[h.0];
                let t = p.t_of(at);
                (p.ball, t.min(p.l - t), h.2)
            };
            let mut h = pool[0];
            for x in &pool[1..] {
                if key(x).partial_cmp(&key(&h)) == Some(std::cmp::Ordering::Less) {
                    h = *x;
                }
            }
            self.cpiece.insert(ci, h.0);
            self.tip_pieces.entry(h.0).or_insert(ci);
            self.ct.insert((ci, h.0), self.p[h.0].t_of(at));
            // Every piece the contact sits in can end a route there (several rods entering one loop).
            for h2 in &hits {
                if (h2.1 || h2.2 <= c.d / 2.0 + 0.05) && !self.p[h2.0].ball {
                    self.on.entry(h2.0).or_default().push(ci);
                    self.c_in.insert((ci, h2.0), h2.1);
                    self.ct.insert((ci, h2.0), self.p[h2.0].t_of(at));
                }
            }
        }
    }

    /// What an attachment leads to: the piece itself, or through joint balls everything else on
    /// them. `from` is the piece the attachment is on (-1 for none).
    pub(super) fn hub(&self, from: isize, q: usize, at: P, weak_ok: bool) -> Vec<(usize, f64, P)> {
        if !self.p[q].ball {
            return vec![(q, self.p[q].t_of(at), at)];
        }
        let mut out = Vec::new();
        let mut seen: BTreeSet<isize> = BTreeSet::from([from]);
        let mut stack = vec![q];
        while let Some(b) = stack.pop() {
            if !seen.insert(b as isize) {
                continue;
            }
            for a in &self.p[b].att {
                let r = a.q;
                if seen.contains(&(r as isize)) || (a.weak && !weak_ok) {
                    continue;
                }
                if self.p[r].ball {
                    stack.push(r);
                } else {
                    out.push((r, self.p[r].t_of(a.at), a.at));
                }
            }
        }
        out
    }

    /// Whether the pieces joined to `r` (not through `block`), up to 8 joins away, hold a contact
    /// not in `skip`.
    fn leads_to_contact(&self, r: usize, block: usize, skip: &[usize]) -> bool {
        let mut seen: BTreeSet<usize> = BTreeSet::from([r, block]);
        let mut frontier = vec![r];
        for _ in 0..8 {
            let mut next = Vec::new();
            for &x in &frontier {
                if self.on_of(x).iter().any(|c| !skip.contains(c)) {
                    return true;
                }
                for a in &self.p[x].att {
                    if !a.weak && !seen.contains(&a.q) && !self.p[a.q].raft {
                        seen.insert(a.q);
                        next.push(a.q);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        false
    }

    /// Where the centre lines of two joined pieces pass each other (parameters along each), in
    /// place of the join's centroid, when the lines are not parallel and the crossing is near the
    /// join.
    pub(super) fn axis_cross(&self, q: usize, r: usize, tq: f64, tr: f64) -> (f64, f64) {
        let (pq, pr) = (&self.p[q], &self.p[r]);
        if pq.ball || pr.ball || pq.l < 1e-6 || pr.l < 1e-6 {
            return (tq, tr);
        }
        let b = dot(pq.d, pr.d);
        let den = 1.0 - b * b;
        if den < 0.03 {
            return (tq, tr);
        }
        let w0 = super::super::math::sub(pq.a, pr.a);
        let (d_, e_) = (dot(pq.d, w0), dot(pr.d, w0));
        let sq = ((b * e_ - d_) / den).clamp(0.0, pq.l);
        let sr = ((e_ - b * d_) / den).clamp(0.0, pr.l);
        let lim = 1.0f64.max(2.0 * (pq.r + pr.r));
        if (sq - tq).abs() > lim || (sr - tr).abs() > lim || dist(pq.at(sq), pr.at(sr)) > pq.r + pr.r + 0.3 {
            return (tq, tr);
        }
        (sq, sr)
    }

    /// Whether piece `r` is piece `p`'s own rod going on past `p`'s end: two rods (plain or
    /// tapering, as a mini support's) on one line (`turn` degrees apart, `p`'s end `off` from
    /// `r`'s line), of one width where they join (`tp` along `p`, `tr` along `r`). A tip's own
    /// piece is never the same rod as what it stands on.
    fn same_rod(&self, p: usize, r: usize, turn: f64, off: f64, tp: f64, tr: f64) -> bool {
        let (pp, pr) = (&self.p[p], &self.p[r]);
        let (rp, rr) = (pp.r_at(tp), pr.r_at(tr));
        !pp.ball
            && !pr.ball
            && self.on_of(p).is_empty()
            && turn <= SAME_ROD_DEG
            && off <= SAME_ROD_OFF * rp.min(rr)
            && (rp - rr).abs() <= SAME_ROD_WIDTH * rp.max(rr)
    }

    /// Whether another tip's own piece (not `from`) sits on rod `r` at `tr`, within the rod's end
    /// reach: the rod is that tip's shaft.
    fn another_tip_s(&self, r: usize, tr: f64, from: usize) -> bool {
        let pr = &self.p[r];
        let reach = 0.5f64.max(2.0 * pr.r);
        pr.att.iter().any(|a| !a.weak && a.q != from && (a.t - tr).abs() <= reach && self.tip_pieces.contains_key(&a.q))
    }

    fn way_class(v: P) -> f64 {
        if v[2] <= -NEAR_VERTICAL {
            0.0
        } else if v[2] < -LEVEL {
            1.0
        } else if v[2] <= LEVEL {
            2.0
        } else {
            3.0
        }
    }

    fn turn_deg(v: P, vin: P) -> f64 {
        dot(v, vin).clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// Ways on from piece `p` (entered at `t`, going `s`), end-most joint first.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ways(
        &self,
        p: usize,
        t: f64,
        s: f64,
        vin: P,
        allow_up: bool,
        weak_ok: bool,
        start_hub: Option<usize>,
        straight: bool,
    ) -> Vec<Way> {
        let mut cands: Vec<Way> = Vec::new();
        let (atts, t_end, slack, end_tol) = match start_hub {
            Some(h) => (vec![super::Att { t: 0.0, q: h, at: self.p[h].a, weak: false }], 0.0, 1e9, 1e9),
            None => {
                let pp = &self.p[p];
                let t_end = if s > 0.0 { pp.l } else { 0.0 };
                (pp.att.clone(), t_end, 0.3f64.max(pp.r), 0.5f64.max(2.0 * pp.r_at(t_end)).max(2.0 * pp.r))
            }
        };
        let mut seen_way: BTreeSet<(usize, i8)> = BTreeSet::new();
        // Ways classed as the same rod going on, with the class they would have otherwise.
        let mut same: Vec<(usize, f64)> = Vec::new();
        for a in &atts {
            if start_hub.is_none() && (a.t - t) * s < -slack {
                continue;
            }
            if a.weak && !weak_ok {
                continue;
            }
            let group = if (a.t - t_end).abs() <= end_tol { 0 } else { 1 + ((a.t - t_end).abs() / 0.5) as usize };
            let from = if start_hub.is_none() { p as isize } else { -1 };
            for (r, tr, _) in self.hub(from, a.q, a.at, weak_ok) {
                if r == p || self.p[r].pad || self.p[r].sleeve.is_some() {
                    continue;
                }
                let pr = &self.p[r];
                for sr in [-1.0f64, 1.0] {
                    let rem = if sr > 0.0 { pr.l - tr } else { tr };
                    if rem < 0.3f64.max(pr.r).min(0.45 * pr.l) || seen_way.contains(&(r, sr as i8)) {
                        continue;
                    }
                    let v = mul(pr.d, sr);
                    let mut cls = Self::way_class(v);
                    let mut turn = Self::turn_deg(v, vin);
                    // A rod an earlier support owns right at this joint can always be joined (a
                    // branch), whichever way it runs; it ranks after every way down or level.
                    let owned = pr.claims.iter().any(|&(lo, hi, _)| lo - 0.3 <= tr && tr <= hi + 0.3);
                    if owned && (cls == 3.0 || turn > 150.0) {
                        cls = OWNED;
                    } else {
                        if cls == 3.0 && !allow_up {
                            continue;
                        }
                        if turn > 150.0 && start_hub.is_none() {
                            continue; // straight back the way it came
                        }
                    }
                    seen_way.insert((r, sr as i8));
                    // Among ways of one kind, the one on this rod's own line comes first: a rod
                    // beside this one counts as a turn, by how far off the line it stands.
                    if start_hub.is_none() && !self.p[a.q].ball {
                        let pp = &self.p[p];
                        let end = pp.at(t_end);
                        let off = dist(end, pr.at(pr.t_of(end)));
                        // The same rod going on: a rod read in two pieces along one line.
                        if group == 0 && cls <= 2.0 && self.same_rod(p, r, turn, off, a.t, tr) {
                            same.push((cands.len(), cls));
                            cls = SAME_ROD;
                        }
                        turn += 40.0 * (off / pr.r.max(pp.r).max(1e-6)).min(2.0);
                        // A rod standing off this rod's end by over a radius, with another tip's
                        // own piece on its end here, is that tip's shaft: a way down it comes after
                        // the ways down the rods that meet this end (the shaft of the strut beside
                        // a tip, steeper than the tip's own rod; real case b14_branch_on_strut).
                        if group == 0
                            && (0.0..1.0 + PASSING).contains(&cls)
                            && off > pr.r.max(pp.r)
                            && self.another_tip_s(r, tr, p)
                        {
                            cls += BESIDE;
                        }
                    }
                    // Among ways of one kind, a rod that starts at this joint (the next stretch of
                    // a line of rods joined end to end) comes before a rod passing by it.
                    if (0.0..=2.0).contains(&cls) && tr.min(pr.l - tr) > 0.3f64.max(pr.r) {
                        cls += PASSING;
                    }
                    cands.push((group, cls, turn, r, tr, sr, a.t));
                }
            }
        }
        // The same rod going on comes first unless it carries on past a way straight down: then it
        // is a brace in line with this rod (a leaning shaft on a post, a brace going on to the
        // next post: l01_lean_shaft). A branch read in two pieces ends inside its parent's rod.
        for (k, cls) in same {
            let (r, sr) = (cands[k].3, cands[k].5);
            let pr = &self.p[r];
            let far = pr.at(if sr > 0.0 { pr.l } else { 0.0 });
            let past = cands.iter().enumerate().any(|(j, w)| {
                let px = &self.p[w.3];
                j != k
                    && w.0 == 0
                    && (0.0..1.0).contains(&w.1)
                    && w.3 != r
                    && dist(far, px.at(px.t_of(far))) > px.r + 0.1
            });
            if past {
                cands[k].1 = cls;
            }
        }
        // A route climbing from a foot keeps to its own rod (the straightest way) rather than the
        // way most nearly down.
        if straight {
            cands.sort_by(|x, y| x.0.cmp(&y.0).then(x.2.total_cmp(&y.2)));
        } else {
            cands.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)).then(x.2.total_cmp(&y.2)));
        }
        cands
    }

    #[allow(clippy::too_many_arguments)]
    fn enter(
        &self,
        ci: usize,
        q: usize,
        t: f64,
        s: f64,
        allow_up: bool,
        weak_ok: bool,
        visited: &mut BTreeSet<(usize, i8)>,
        straight: bool,
    ) -> Option<Found> {
        if !visited.insert((q, s as i8)) {
            return None;
        }
        let pq = &self.p[q];
        let t_end = if s > 0.0 { pq.l } else { 0.0 };
        // A stretch an earlier support owns: this one hangs from it there.
        let mut best: Option<(f64, Owner)> = None;
        for &(lo, hi, o) in &pq.claims {
            if lo - 0.2 <= t && t <= hi + 0.2 {
                best = Some((t, o));
                break;
            }
            let edge = if s > 0.0 { lo } else { hi };
            if (edge - t) * s > 0.0 && best.is_none_or(|b| (edge - t).abs() < (b.0 - t).abs()) {
                best = Some((edge, o));
            }
        }
        if let Some((tb, o)) = best {
            return Some((vec![(q, t, tb)], End::Branch(o, pq.at(tb))));
        }
        if s < 0.0 && pq.plate {
            return Some((vec![(q, t, 0.0)], End::Plate));
        }
        let mut foot: Option<Found> = None;
        let mut foot_c = None;
        for &c2 in self.on_of(q) {
            if c2 == ci {
                continue;
            }
            let t2 = self.ct[&(c2, q)];
            if (t2 - t) * s > 0.02 && (t_end - t2).abs() <= self.c[c2].depth + 0.6 {
                foot = Some((vec![(q, t, t2)], End::Foot(c2)));
                foot_c = Some(c2);
                break;
            }
        }
        let vin = mul(pq.d, s);
        let mut ways = self.ways(q, t, s, vin, allow_up, weak_ok, None, straight);
        // The ways down from the rod's end, for telling a foot from a rod passing through.
        let down: Vec<Way> = ways.iter().filter(|w| w.0 == 0 && w.1 < 2.0).copied().collect();
        // Another tip's own piece (its cone) passing by the joint is that tip's from the start:
        // like a rod an earlier route owns, it can be joined, but only after every way down or
        // level (tip cones lapping each other at their sockets, real model 35ba31e3). This orders
        // the ways only: such a piece below a rod's end is still the structure going on down.
        if !straight {
            let mut moved = false;
            for w in ways.iter_mut() {
                let passing = w.1 < OWNED && (w.1 - w.1.floor() - PASSING).abs() < 1e-9;
                if passing && self.tip_pieces.get(&w.3).is_some_and(|&c| c != ci) && w.3 != self.cpiece[&ci] {
                    w.1 = OWNED;
                    moved = true;
                }
            }
            if moved {
                ways.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)).then(x.2.total_cmp(&y.2)));
            }
        }
        // The way this contact's route is to take first here (a second reading, see `reroute`).
        if let Some(&r) = self.prefer.get(&(ci, q))
            && let Some(k) = ways.iter().position(|w| w.3 == r)
        {
            let w = ways.remove(k);
            ways.insert(0, (0, w.1.min(1.0), w.2, w.3, w.4, w.5, w.6));
        }
        // The rod ends in the model: that is its foot, unless the structure goes on down from this
        // joint (a stray sheet of the model across a joint: a real foot has nothing hanging below
        // it), which is tried first. A way down into another rod that ends in this same loop is not
        // the structure going on down but a second foot cone on the contact (a vee: two struts or
        // branches whose feet meet at one loop); each route ends on its own cone.
        let same_foot = |w: &Way| -> bool {
            let (r, tr, sr) = (w.3, w.4, w.5);
            let Some(c2) = foot_c else { return false };
            // Both cones hold the contact inside them.
            let inside = self.inside_at.get(&c2);
            if !inside.is_some_and(|x| x.contains(&q) && x.contains(&r)) {
                return false;
            }
            // ... their centre lines do not cross deep inside the model (rods running on through
            // the loop to a hub inside the model are one foot with branches off the hub) ...
            let (ta2, _) = self.axis_cross(q, r, w.6, tr);
            let Some(&tq2) = self.ct.get(&(c2, q)) else { return false };
            if (tq2 - ta2) * s < -VEE_HUB_MM {
                return false;
            }
            let Some(&t2r) = self.ct.get(&(c2, r)) else { return false };
            let end_r = if sr > 0.0 { self.p[r].l } else { 0.0 };
            if (end_r - t2r).abs() > self.c[c2].depth + 0.6 {
                return false;
            }
            // ... the two rods really diverge from the loop (real vees are 12.6 to 39 degrees
            // apart; rods on one line are one strut stacked from pieces) ...
            if dot(self.p[r].d, self.p[q].d).abs() > VEE_MIN_COS {
                return false;
            }
            // ... and the other cone leads to a contact of its own, so its own route builds it.
            self.leads_to_contact(r, q, &[c2, ci])
        };
        let through = !down.is_empty() && down.iter().any(|w| !same_foot(w));
        if foot.is_some() && !through {
            return foot;
        }
        for &(_, _, _, r, tr, sr, ta) in &ways {
            let (mut ta2, mut tr2) = self.axis_cross(q, r, ta, tr);
            // The crossing must not turn the way round.
            if (ta2 - t) * s < -1e-6 || (if sr > 0.0 { self.p[r].l - tr2 } else { tr2 }) < 1e-3 {
                (ta2, tr2) = (ta, tr);
            }
            if let Some((path, end)) = self.enter(ci, r, tr2, sr, allow_up, weak_ok, visited, straight) {
                let mut full = vec![(q, t, ta2)];
                full.extend(path);
                return Some((full, end));
            }
        }
        foot
    }

    /// A contact piece too short to have a direction of its own (a tip ball read as a 0.15 mm
    /// stub): it is left like a ball, by its joints, not along its axis.
    fn tiny(&self, q: usize) -> bool {
        let pq = &self.p[q];
        !pq.ball && pq.l <= 0.3 && !pq.att.is_empty()
    }

    fn walk_tiny(
        &self,
        ci: usize,
        q: usize,
        allow_up: bool,
        weak_ok: bool,
        visited: &mut BTreeSet<(usize, i8)>,
        foot_like: bool,
    ) -> Option<Found> {
        let vin = unit_div(self.c[ci].dir);
        let mut cands: Vec<(f64, f64, usize, f64, f64)> = Vec::new();
        for a in &self.p[q].att {
            if a.weak && !weak_ok {
                continue;
            }
            for (r, tr, _) in self.hub(q as isize, a.q, a.at, weak_ok) {
                if r == q || self.p[r].pad || self.p[r].sleeve.is_some() {
                    continue;
                }
                let pr = &self.p[r];
                for sr in [-1.0f64, 1.0] {
                    let rem = if sr > 0.0 { pr.l - tr } else { tr };
                    if rem < 0.3f64.max(pr.r).min(0.45 * pr.l) {
                        continue;
                    }
                    let v = mul(pr.d, sr);
                    let cls = Self::way_class(v);
                    if cls == 3.0 && !allow_up && !pr.claims.iter().any(|&(lo, hi, _)| lo - 0.3 <= tr && tr <= hi + 0.3)
                    {
                        continue;
                    }
                    let turn = Self::turn_deg(v, vin);
                    cands.push(if foot_like { (turn, cls, r, tr, sr) } else { (cls, turn, r, tr, sr) });
                }
            }
        }
        cands.sort_by(|x, y| {
            x.0.total_cmp(&y.0)
                .then(x.1.total_cmp(&y.1))
                .then(x.2.cmp(&y.2))
                .then(x.3.total_cmp(&y.3))
                .then(x.4.total_cmp(&y.4))
        });
        for c in cands {
            if let Some(res) = self.enter(ci, c.2, c.3, c.4, allow_up, weak_ok, visited, foot_like) {
                return Some(res);
            }
        }
        None
    }

    /// The route from contact `ci`, and the pass that found it.
    pub(super) fn walk(&self, ci: usize, foot_like: bool) -> Option<(Vec<Step>, End, usize)> {
        let q = self.cpiece[&ci];
        let pq = &self.p[q];
        let t = self.ct[&(ci, q)];
        let passes: &[(bool, bool)] =
            if foot_like { &[(true, true)] } else { &[(false, false), (false, true), (true, true)] };
        for (k, &(allow_up, weak_ok)) in passes.iter().enumerate() {
            let mut visited = BTreeSet::new();
            if self.tiny(q) {
                visited.insert((q, 1));
                visited.insert((q, -1));
                if let Some((path, end)) = self.walk_tiny(ci, q, allow_up, weak_ok, &mut visited, foot_like) {
                    let mut full = vec![(q, t, t)];
                    full.extend(path);
                    return Some((full, end, k));
                }
                continue;
            }
            if pq.ball {
                let vin = mul(unit_div(self.c[ci].dir), -1.0);
                for (_, _, _, r, tr, sr, _) in self.ways(q, 0.0, 1.0, vin, allow_up, weak_ok, Some(q), foot_like) {
                    if let Some((path, end)) = self.enter(ci, r, tr, sr, allow_up, weak_ok, &mut visited, foot_like) {
                        return Some((path, end, k));
                    }
                }
                continue;
            }
            let s = if t > pq.l / 2.0 { -1.0 } else { 1.0 };
            if let Some((path, end)) = self.enter(ci, q, t, s, allow_up, weak_ok, &mut visited, foot_like) {
                return Some((path, end, k));
            }
        }
        None
    }

    fn start_dir(&self, ci: usize) -> f64 {
        let pq = &self.p[self.cpiece[&ci]];
        if pq.ball {
            return 0.0;
        }
        if self.tiny(pq.i) {
            return self.c[ci].dir[2];
        }
        let s = if self.ct[&(ci, pq.i)] > pq.l / 2.0 { -1.0 } else { 1.0 };
        pq.d[2] * s
    }

    /// The points a route passes, at its stretches' ends.
    pub(super) fn line_of(&self, path: &[Step]) -> Vec<P> {
        let mut pts: Vec<P> = Vec::new();
        for &(q, t0, t1) in path {
            for t in [t0, t1] {
                let x = self.p[q].at(t);
                if pts.last().is_none_or(|l| dist(x, *l) > 0.02) {
                    pts.push(x);
                }
            }
        }
        pts
    }

    pub(super) fn route(&mut self) {
        let mut order: Vec<usize> = self.cpiece.keys().copied().collect();
        order.sort_by(|&a, &b| self.c[b].at[2].total_cmp(&self.c[a].at[2]));
        let mut normal: Vec<usize> = order.iter().copied().filter(|&ci| self.start_dir(ci) <= LEVEL).collect();
        // Tips sharing a trunk: the one that goes most straight down owns it. Each tip's route is
        // first walked on its own (nothing claimed yet), and its detour (route length less the
        // height it drops) lowers its place in the order.
        let mut detour: std::collections::BTreeMap<usize, f64> = Default::default();
        // (a, b): tip a's first walk joins the cone that holds tip b, in the cone's body: past b's
        // contact where the cone is wider than at the contact, and before its socket (where it is
        // still under `CONE_BODY` of its full width).
        let mut joins: BTreeSet<(usize, usize)> = BTreeSet::new();
        // Per piece: the first walks that enter it at one of its ends and those that join it from
        // the side, each with the stretch it runs along.
        let mut end_on: std::collections::BTreeMap<usize, Vec<(usize, f64, f64)>> = Default::default();
        let mut side_on: std::collections::BTreeMap<usize, Vec<(usize, f64)>> = Default::default();
        for &ci in &normal {
            let Some((path, _, _)) = self.walk(ci, false) else { continue };
            for &(q, t0, t1) in path.iter().skip(1) {
                let pq = &self.p[q];
                if !pq.ball && (t1 - t0).abs() > 1e-6 {
                    if t0.min(pq.l - t0) <= 0.3f64.max(pq.r) {
                        end_on.entry(q).or_default().push((ci, t0, t1));
                    } else {
                        side_on.entry(q).or_default().push((ci, t0));
                    }
                }
            }
            for &(q, t0, _) in path.iter().skip(1) {
                let pq = &self.p[q];
                let full = pq.secs.iter().map(|s| s.r0.max(s.r1)).fold(0.0, f64::max);
                if pq.ball || q == self.cpiece[&ci] || pq.r_at(t0) >= CONE_BODY * full {
                    continue;
                }
                for &c2 in &normal {
                    if c2 != ci && self.cpiece[&c2] == q && pq.r_at(t0) > 1.15 * profile_r(pq, self.ct[&(c2, q)]) {
                        joins.insert((ci, c2));
                    }
                }
            }
            let pts = self.line_of(&path);
            if pts.len() >= 2 {
                let length: f64 = pts.windows(2).map(|w| dist(w[0], w[1])).sum();
                detour.insert(ci, (length - (pts[0][2] - pts[pts.len() - 1][2])).max(0.0));
            }
        }
        // A rod is its own tip's: a tip whose first walk joins a rod from the side, where another
        // tip's first walk comes down that rod from its end, is routed after that tip, so it hangs
        // from the rod as a branch instead of owning the rod's lower part and leaving the rod's
        // own tip a stub.
        for (q, sides) in &side_on {
            for &(a, ta) in sides {
                for &(b, t0, t1) in end_on.get(q).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if a != b && t0.min(t1) + 0.3 < ta && ta < t0.max(t1) - 0.3 {
                        joins.insert((a, b));
                    }
                }
            }
        }
        let key = |ci: usize| -(self.c[ci].at[2] - DETOUR_W * detour.get(&ci).copied().unwrap_or(0.0));
        normal.sort_by(|&a, &b| key(a).total_cmp(&key(b)));
        let normal = Self::own_cone_first(&normal, &joins);
        let feet: Vec<usize> = order.iter().copied().filter(|&ci| self.start_dir(ci) > LEVEL).collect();
        let mut todo: Vec<(usize, bool)> =
            normal.iter().map(|&c| (c, false)).chain(feet.iter().map(|&c| (c, true))).collect();
        // Rounds: a contact that found no route is tried again once later routes exist to hang from.
        for rnd in 0..4 {
            let mut left = Vec::new();
            for &(ci, foot_like) in &todo {
                if self.consumed.contains_key(&ci) || self.sup_of.contains_key(&ci) || self.through.contains_key(&ci) {
                    continue;
                }
                let Some((path, end, k)) = self.walk(ci, foot_like) else {
                    left.push((ci, foot_like));
                    continue;
                };
                let idx = self.supports.len();
                if let End::Foot(c2) = end {
                    self.consumed.entry(c2).or_insert(idx);
                }
                let on_path: BTreeSet<usize> = path.iter().map(|x| x.0).collect();
                // A collar round the route: a piece joined to nothing but the route's own pieces
                // (the sleeve of a mini support), with no other contact inside it.
                let collar = |p: usize, c2: usize| {
                    let pp = &self.p[p];
                    !pp.ball
                        && self.on_of(p).iter().all(|&c| c == c2 || self.c_in.get(&(c, p)) != Some(&true))
                        && pp.att.iter().any(|a| !a.weak)
                        && pp.att.iter().filter(|a| !a.weak).all(|a| on_path.contains(&a.q))
                };
                // A contact the route runs through (its own piece on the route, not at either end
                // of it), or one where the route's flank lies in the model: inside a piece of the
                // route, its own piece a collar round the route.
                let mut passed: Vec<(usize, bool)> = Vec::new();
                for &(q, t0, t1) in &path {
                    for &c2 in self.on_of(q) {
                        let own = self.cpiece.get(&c2).is_some_and(|p| on_path.contains(p));
                        let flank = !own
                            && self.c_in.get(&(c2, q)) == Some(&true)
                            && self.cpiece.get(&c2).is_some_and(|&p| collar(p, c2));
                        if c2 != ci
                            && end != End::Foot(c2)
                            && !self.sup_of.contains_key(&c2)
                            && (own || flank)
                            && t0.min(t1) - 0.05 <= self.ct[&(c2, q)]
                            && self.ct[&(c2, q)] <= t0.max(t1) + 0.05
                        {
                            passed.push((c2, flank));
                        }
                    }
                }
                for (c2, flank) in passed {
                    if self.through.contains_key(&c2) {
                        continue;
                    }
                    self.through.insert(c2, idx);
                    // A flank loop overlapping the loop of one of the route's own ends is part of
                    // that end's opening in the model: the end is sized by both.
                    if flank {
                        let ends = [Some(ci), if let End::Foot(f) = end { Some(f) } else { None }];
                        for e in ends.into_iter().flatten() {
                            let (a, b) = (&self.c[e], &self.c[c2]);
                            let gap = dist(a.at, b.at);
                            if gap < (a.d + b.d) / 2.0 {
                                let area = self.loop_area(e) + b.area - lens_area(a.d / 2.0, b.d / 2.0, gap);
                                self.widened.insert(e, area.max(self.loop_area(e)));
                            }
                        }
                    }
                }
                for &(q, t0, t1) in &path {
                    self.p[q].claims.push((t0.min(t1), t0.max(t1), Owner::Support(idx)));
                }
                let _ = rnd;
                self.supports.push(Route {
                    contact: ci,
                    path,
                    end,
                    pass: k,
                    foot_like,
                    role: super::Role::Trunk,
                    tip_len: 0.0,
                });
                self.sup_of.insert(ci, idx);
            }
            if left.len() == todo.len() {
                break;
            }
            todo = left;
        }
        for &ci in self.through.clone().keys() {
            if !self.consumed.contains_key(&ci) {
                self.exceptions.push(Exception {
                    kind: "contact",
                    at: self.c[ci].at,
                    why: "passed through: a routed support runs on through the model here (a stray sheet or thin wall across it), so no tip is built".into(),
                    contact: Some(ci),
                    piece: self.cpiece.get(&ci).copied(),
                    role: None,
                });
            }
        }
        for &(ci, _) in &todo {
            if self.consumed.contains_key(&ci) || self.sup_of.contains_key(&ci) || self.through.contains_key(&ci) {
                continue;
            }
            let why = format!("no route: {}", self.why_no_route(ci));
            self.exceptions.push(Exception {
                kind: "contact",
                at: self.c[ci].at,
                why,
                contact: Some(ci),
                piece: self.cpiece.get(&ci).copied(),
                role: None,
            });
        }
    }

    /// The routing order with each tip's own cone kept its own: a tip whose first walk joins the
    /// cone holding another tip in the cone's body (a mini support standing on a tip cone) goes
    /// after that tip, so it hangs from the cone instead of taking the cone's line down and
    /// leaving its owner a stub of a cone. `joins` holds those pairs (the tip, the cone's tip). Two
    /// tips that each join the other's cone keep their order.
    fn own_cone_first(order: &[usize], joins: &BTreeSet<(usize, usize)>) -> Vec<usize> {
        let through = |a: usize, b: usize| joins.contains(&(a, b));
        let mut out: Vec<usize> = Vec::with_capacity(order.len());
        let mut stack: Vec<usize> = Vec::new();
        fn place(
            ci: usize,
            order: &[usize],
            through: &dyn Fn(usize, usize) -> bool,
            out: &mut Vec<usize>,
            stack: &mut Vec<usize>,
        ) {
            if out.contains(&ci) || stack.contains(&ci) {
                return;
            }
            stack.push(ci);
            for &c2 in order {
                if c2 != ci && through(ci, c2) && !through(c2, ci) {
                    place(c2, order, through, out, stack);
                }
            }
            stack.pop();
            out.push(ci);
        }
        for &ci in order {
            place(ci, order, &through, &mut out, &mut stack);
        }
        out
    }

    /// What the structure around an unrouted contact offers.
    fn why_no_route(&self, ci: usize) -> String {
        let q0 = self.cpiece[&ci];
        let mut seen = BTreeSet::from([q0]);
        let mut stack = vec![q0];
        let (mut plate, mut claimed, mut other) = (false, false, 0);
        while let Some(q) = stack.pop() {
            plate |= self.p[q].plate;
            claimed |= !self.p[q].claims.is_empty();
            other += self.on_of(q).iter().filter(|&&c| c != ci).count();
            for a in &self.p[q].att {
                if seen.insert(a.q) {
                    stack.push(a.q);
                }
            }
        }
        if seen.len() == 1 && self.p[q0].att.is_empty() {
            return "its piece touches no other piece, the plate or a second contact (a stub in the model)".into();
        }
        if !plate && !claimed && other == 0 {
            let balls = seen.iter().filter(|&&q| self.p[q].ball).count();
            return format!(
                "isolated: its structure ({} pieces, {balls} of them balls) touches the model at this one loop and nothing else (a capsule or stub in the model)",
                seen.len()
            );
        }
        format!(
            "structure of {} pieces; reaches plate {plate}, a routed support {claimed}, other contacts {other}; no way on within the rules",
            seen.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tip_joining_another_tip_s_cone_is_routed_after_it() {
        // Contact 0's first walk joins the cone of contact 1 (a mini support on a tip cone).
        let mut joins: BTreeSet<(usize, usize)> = BTreeSet::from([(0, 1)]);
        assert_eq!(Reader::own_cone_first(&[0, 1, 2], &joins), vec![1, 0, 2]);
        // Each joins the other's cone: the order stays.
        joins.insert((1, 0));
        assert_eq!(Reader::own_cone_first(&[0, 1, 2], &joins), vec![0, 1, 2]);
        // A chain: 2 joins 0's cone, 0 joins 1's.
        let joins = BTreeSet::from([(2, 0), (0, 1)]);
        assert_eq!(Reader::own_cone_first(&[2, 0, 1], &joins), vec![1, 0, 2]);
    }

    #[test]
    fn the_lens_two_circles_share() {
        let pi = std::f64::consts::PI;
        assert_eq!(lens_area(0.3, 0.2, 0.6), 0.0);
        assert!((lens_area(0.3, 0.2, 0.05) - pi * 0.04).abs() < 1e-12);
        // Two unit circles one radius apart share 2 pi / 3 - sqrt(3) / 2.
        assert!((lens_area(1.0, 1.0, 1.0) - (2.0 * pi / 3.0 - 3f64.sqrt() / 2.0)).abs() < 1e-12);
    }
}
