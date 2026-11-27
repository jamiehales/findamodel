//! Stage 5: everything no route uses (posts, legs, braces, roots, sleeves, loose and floating
//! pieces), then every piece's role.

use std::collections::{BTreeMap, BTreeSet};

use super::super::math::{P, add, dist, dot, mul, sub};
use super::super::pieces::{PieceKind, SectionKind};
use super::{
    End, Exception, FOOT_PILLAR_COS, KNEE_DEG, KNOT_REACH_MM, LEVEL, MICRO_D, Owner, PAD_REACH_MM, PAD_START_MM,
    ROOT_TOP, Reader, Role, SLEEVE_COS, SLEEVE_OFF, STUB_PAST_MM, Step,
};

/// A free piece's join to a claimed piece: (t on the free piece, the claim's owner, the point on
/// the claimed piece, the claimed piece).
pub(crate) type Host = (f64, Owner, P, usize);

pub(crate) type Chain = Vec<Step>;

#[derive(Clone)]
pub(crate) struct Kick {
    pub chain: Chain,
    pub host: Owner,
    pub knot: P,
}

#[derive(Clone)]
pub(crate) struct Brace {
    pub piece: usize,
    pub lower: Host,
    pub upper: Host,
    pub d: f64,
}

/// The median as numpy takes it (the mean of the middle two of an even count).
pub(super) fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

impl Reader<'_> {
    /// The owner of the claim on piece `q` nearest `t`.
    pub(super) fn owner_at(&self, q: usize, t: f64) -> Option<Owner> {
        let mut best: Option<(f64, Owner)> = None;
        for &(lo, hi, o) in &self.p[q].claims {
            let k = if lo - 0.2 <= t && t <= hi + 0.2 { 0.0 } else { (t - lo).abs().min((t - hi).abs()) };
            if best.is_none_or(|b| k < b.0) {
                best = Some((k, o));
            }
        }
        best.map(|b| b.1)
    }

    /// Claimed pieces a free piece attaches to, through joint balls.
    pub(super) fn hosts_of(&self, q: usize) -> Vec<Host> {
        let mut out = Vec::new();
        for a in &self.p[q].att {
            if a.weak {
                continue;
            }
            for (r2, tr, _) in self.hub(q as isize, a.q, a.at, false) {
                if r2 != q
                    && !self.p[r2].claims.is_empty()
                    && let Some(o) = self.owner_at(r2, tr)
                {
                    out.push((a.t, o, self.p[r2].at(tr), r2));
                }
            }
        }
        out
    }

    fn is_free(&self, i: usize) -> bool {
        let p = &self.p[i];
        !p.raft && !p.ball && p.claims.is_empty() && !matches!(p.role, Some(Role::Root) | Some(Role::Sleeve))
    }

    /// Only a piece standing on the raft itself, or wholly under the root top (from the plate), whose
    /// low end joins a routed support: a brace between supports (a slanted brace landing on a pad at
    /// a post's foot), not a leg or post. A piece standing on a root at a trunk's foot is still a leg or
    /// post, which DragonFruit builds from the root top along its line.
    fn low_hosted(&self, q: usize) -> bool {
        let p = &self.p[q];
        if !(p.on_raft || p.hi[2] <= ROOT_TOP) {
            return false;
        }
        self.hosts_of(q).iter().any(|h| h.0 <= 0.6f64.max(2.5 * p.r))
    }

    /// Base stubs: a free piece on the line of a routed rod it joins, running on past that rod's end
    /// into another support piece, is the rod's own base and part of its support (a sleeve), not a
    /// brace from the support to its host and not a leg. Lychee builds a branch's base as a short
    /// capsule from the knot on the host, overlapping the branch's shaft along its line; the route
    /// joins the host from the shaft, which leaves the capsule to no route (real cases b01_branch,
    /// b05_branch_depth3, b09_branch_route, e02_elf_branches: 0.4 to 0.7 mm of overlap, lines
    /// 0.00 mm apart).
    fn base_stubs(&mut self) {
        for q in 0..self.p.len() {
            if !self.is_free(q) || self.p[q].l < 1e-6 || !self.on_of(q).is_empty() {
                continue;
            }
            let pq = &self.p[q];
            let mut found: Option<(usize, f64)> = None;
            for a in pq.att.iter().filter(|a| !a.weak) {
                let ph = &self.p[a.q];
                if ph.ball || ph.raft || ph.claims.is_empty() || ph.l < 1e-6 || dot(pq.d, ph.d).abs() < SLEEVE_COS {
                    continue;
                }
                // Both ends on the rod's line, and past exactly one of its ends.
                let along = |x: P| dot(sub(x, ph.a), ph.d);
                let off = |x: P| dist(x, add(ph.a, mul(ph.d, along(x))));
                if off(pq.a).max(off(pq.b)) > SLEEVE_OFF * ph.r {
                    continue;
                }
                let (far, near) = if along(pq.a) < along(pq.b) { (pq.a, pq.b) } else { (pq.b, pq.a) };
                let (far, near, t_end) = match (along(far) < -STUB_PAST_MM, along(near) > ph.l + STUB_PAST_MM) {
                    (true, false) => (far, near, 0.0),
                    (false, true) => (near, far, ph.l),
                    _ => continue,
                };
                if !(-STUB_PAST_MM..=ph.l + STUB_PAST_MM).contains(&along(near)) {
                    continue;
                }
                // The far end lies in another piece of the same support, or in the support the
                // rod's own hangs from. A brace carrying on in a branch's line to another support
                // (l01_lean_shaft) ends elsewhere. No higher than a piece standing on a pad may
                // start, the base of the last rod of a support on the plate is its root whatever
                // it lands in (branches from inside a trunk's foot that stand on the plate
                // themselves, b03_tree).
                let own = self.owner_at(a.q, t_end);
                let route = own.and_then(Owner::support).map(|s| &self.supports[s]);
                let parent = route.and_then(|r| match r.end {
                    End::Branch(o, _) => Some(o),
                    _ => None,
                });
                let in_root = far[2] <= ROOT_TOP + PAD_START_MM
                    && route.is_some_and(|r| r.end == End::Plate && r.path.last().is_some_and(|x| x.0 == a.q));
                let lands = in_root
                    || pq.att.iter().any(|b| {
                        let pb = &self.p[b.q];
                        if b.weak || b.q == a.q || pb.raft || pb.ball {
                            return false;
                        }
                        let tb = pb.t_of(far);
                        let hosts: Vec<Owner> = if pb.role == Some(Role::Root) {
                            pb.owner.into_iter().collect()
                        } else {
                            pb.claims.iter().filter(|c| c.0 - 0.2 <= tb && tb <= c.1 + 0.2).map(|c| c.2).collect()
                        };
                        dist(far, pb.at(tb)) <= pb.r.max(pb.r_at(tb)) + 0.15
                            && hosts.iter().any(|&o| Some(o) == own || Some(o) == parent)
                    });
                if lands {
                    found = Some((a.q, t_end));
                    break;
                }
            }
            if let Some((h, t)) = found {
                let o = self.owner_at(h, t);
                self.p[q].role = Some(Role::Sleeve);
                self.p[q].owner = o.filter(|o| matches!(o, Owner::Support(_)));
                self.p[q].why = format!("the base of piece {h}'s rod, on its line past its end");
            }
        }
    }

    /// Whether `i` is an upright piece on the plate that supports own from its foot up, with a
    /// top stretch of at least 1 mm (and its own width) that nothing owns.
    fn free_top(&self, i: usize) -> bool {
        let p = &self.p[i];
        let top = p.claims.iter().map(|c| c.0.max(c.1)).fold(f64::NEG_INFINITY, f64::max);
        p.plate
            && !p.raft
            && !p.ball
            && !matches!(p.role, Some(Role::Root) | Some(Role::Sleeve))
            && p.d[2] >= FOOT_PILLAR_COS
            && !p.claims.is_empty()
            && p.claims.iter().all(|c| matches!(c.2, Owner::Support(_)))
            && top <= p.l - 1.0f64.max(2.0 * p.r)
    }

    /// The free pieces that lean up off the top of `q` at a knee: joined at its top end, rising and
    /// turning by at most [`KNEE_DEG`].
    fn leans_off(&self, q: usize, seen: &BTreeSet<usize>) -> Vec<super::route::Way> {
        self.ways(q, 0.0, 1.0, self.p[q].d, true, false, None, false)
            .into_iter()
            .filter(|w| {
                w.0 == 0
                    && w.5 > 0.0
                    && w.2 <= KNEE_DEG
                    && self.p[w.3].d[2] > LEVEL
                    && !seen.contains(&w.3)
                    && self.is_free(w.3)
            })
            .collect()
    }

    /// The branch whose own knot both ends of a would-be brace sit at: one host hangs from the
    /// other, and both ends are within [`KNOT_REACH_MM`] of where it does. Such a run is a second,
    /// short way from the branch to the support it already stands on (the base and the short shaft
    /// of a Lychee branch whose cone reaches its host itself, real cases b02_branch_fan,
    /// p13_tips_close_missed: 0.7 to 1.2 mm long), not a brace.
    fn at_one_knot(&self, lo: &Host, up: &Host) -> Option<Owner> {
        [(lo, up), (up, lo)].into_iter().find_map(|(x, y)| {
            let s = x.1.support()?;
            match self.supports[s].end {
                End::Branch(o, at) if o == y.1 && dist(x.2, at).max(dist(y.2, at)) <= KNOT_REACH_MM => Some(x.1),
                _ => None,
            }
        })
    }

    pub(super) fn rest(&mut self) {
        // Roots: a short free piece on the plate under a support that reaches the plate (its foot
        // cone or pad).
        for i in 0..self.p.len() {
            if !self.p[i].pad {
                continue;
            }
            self.p[i].role = Some(Role::Root);
            let p = &self.p[i];
            let hs: Vec<Host> = self
                .hosts_of(i)
                .into_iter()
                .filter(|h| self.p[h.3].plate || self.p[h.3].lo[2] <= p.hi[2] + 0.3)
                .collect();
            if !hs.is_empty() {
                let m = p.mid();
                let mut best = hs[0];
                for h in &hs[1..] {
                    let dxy = |x: &Host| ((x.2[0] - m[0]).powi(2) + (x.2[1] - m[1]).powi(2)).sqrt();
                    if dxy(h) < dxy(&best) {
                        best = *h;
                    }
                }
                self.p[i].owner = Some(best.1);
                self.root_p.entry(best.1).or_default().push(i);
            }
        }
        // Sleeves belong to their host's support.
        for i in 0..self.p.len() {
            if let Some(h) = self.p[i].sleeve
                && self.p[i].claims.is_empty()
            {
                let o = self.owner_at(h, self.p[h].t_of(self.p[i].mid()));
                self.p[i].role = Some(Role::Sleeve);
                self.p[i].owner = o.filter(|o| matches!(o, Owner::Support(_)));
            }
        }
        self.base_stubs();
        // Plate-standing chains, straight up through free pieces. A leg can also start on a root
        // piece (the pad or foot cone under another support) and lean off it. Upright pieces first,
        // so a post is claimed before a leg leaning on it looks for its host.
        let mut pending: Vec<usize> = (0..self.p.len()).filter(|&i| self.is_free(i) && self.p[i].plate).collect();
        pending.sort_by(|&a, &b| self.p[b].d[2].total_cmp(&self.p[a].d[2]));
        // A pillar whose foot a support owns and whose top no route uses (a branch leaves it lower
        // down: a Lychee branch hanging from a kickstand, b13_branch_on_kickstand). Its top can
        // still be a leg's knee: the leg then stands in the same place as the support, up the same
        // pillar, since DragonFruit's kickstand carries no branch.
        let spurs: BTreeSet<usize> = (0..self.p.len()).filter(|&i| self.free_top(i)).collect();
        pending.extend(spurs.iter().copied());
        pending.extend((0..self.p.len()).filter(|&i| self.p[i].role == Some(Role::Root)));
        let mut tried: BTreeSet<usize> = BTreeSet::new();
        let mut deferred: BTreeSet<usize> = BTreeSet::new();
        let mut changed = true;
        while changed {
            changed = false;
            for &q0 in &pending {
                let is_spur = spurs.contains(&q0);
                if (!self.p[q0].claims.is_empty() && !is_spur) || tried.contains(&q0) {
                    continue;
                }
                let is_root = self.p[q0].role == Some(Role::Root);
                if !is_root && self.low_hosted(q0) {
                    tried.insert(q0);
                    continue;
                }
                let mut chain: Chain = vec![(q0, 0.0, self.p[q0].l)];
                let mut seen = BTreeSet::from([q0]);
                let mut top_host: Option<Host> = None;
                // The chain's length where it turned through a knee, if it did.
                let mut knee: Option<usize> = None;
                loop {
                    let (q, _, t1) = *chain.last().expect("a chain has a piece");
                    let first = is_root && chain.len() == 1;
                    let reach = 0.6f64.max(2.5 * self.p[q].r);
                    let hs: Vec<Host> = if first {
                        Vec::new()
                    } else {
                        self.hosts_of(q).into_iter().filter(|h| (h.0 - t1).abs() <= reach).collect()
                    };
                    if !hs.is_empty() {
                        // A support or post before a leg: where a run's top meets a support at
                        // the knot of a leg already leaning there, it leans on the support too
                        // (two legs from one foot pad, each onto its own trunk; real model
                        // 31676804). Left on the leg it would be a post, or a brace from the
                        // root zone that DragonFruit cannot knot there.
                        let leg = |x: &Host| matches!(x.1, Owner::Kick(_));
                        let mut h = hs[0];
                        for x in &hs[1..] {
                            if (leg(x), (x.0 - t1).abs()) < (leg(&h), (h.0 - t1).abs()) {
                                h = *x;
                            }
                        }
                        let (tq2, tr2) = self.axis_cross(q, h.3, h.0, self.p[h.3].t_of(h.2));
                        top_host = Some((tq2, h.1, self.p[h.3].at(tr2), h.3));
                        let last = chain.len() - 1;
                        chain[last] = (q, chain[last].1, tq2);
                        break;
                    }
                    let nxt: Vec<super::route::Way> = self
                        .ways(q, 0.0, 1.0, self.p[q].d, true, false, None, false)
                        .into_iter()
                        .filter(|w| {
                            (first || w.0 == 0)
                                && w.5 > 0.0
                                && w.2 <= if first { 80.0 } else { 35.0 }
                                && self.p[w.3].d[2] > LEVEL
                                && !seen.contains(&w.3)
                                && self.is_free(w.3)
                                && !(first && self.low_hosted(w.3))
                        })
                        .collect();
                    if nxt.is_empty() {
                        // A knee: nothing goes straight on from the top, and one free piece alone
                        // leans up off it (a kickstand's lean onto the support it holds up).
                        let lean = if first || knee.is_some() { Vec::new() } else { self.leans_off(q, &seen) };
                        if let [w] = lean[..] {
                            // The pillar ends where the lean's line leaves it, not at its own
                            // top (a cap above the knee would bend the line back on itself).
                            let (tq, tr) = self.axis_cross(q, w.3, w.6, w.4);
                            let last = chain.len() - 1;
                            chain[last].2 = tq;
                            knee = Some(chain.len());
                            seen.insert(w.3);
                            chain.push((w.3, tr, self.p[w.3].l));
                            continue;
                        }
                        break;
                    }
                    let mut w = nxt[0];
                    for x in &nxt[1..] {
                        if x.2 < w.2 {
                            w = *x;
                        }
                    }
                    seen.insert(w.3);
                    chain.push((w.3, w.4, self.p[w.3].l));
                }
                // A lean that reaches no host yet: where its top meets a piece still waiting to be
                // a post, the chain waits for it (once); otherwise the lean is no part of it (a
                // brace, as before).
                let on_a_leg = top_host.is_some_and(|h| matches!(h.1, Owner::Kick(_)));
                if let Some(n) = knee.filter(|_| top_host.is_none() || on_a_leg) {
                    let (q, _, t1) = *chain.last().expect("a chain has a piece");
                    let reach = 0.6f64.max(2.5 * self.p[q].r);
                    let waits = self.p[q].att.iter().any(|a| {
                        !a.weak
                            && (a.t - t1).abs() <= reach
                            && self.hub(q as isize, a.q, a.at, false).iter().any(|&(r2, _, _)| {
                                r2 != q && self.is_free(r2) && pending.contains(&r2) && !tried.contains(&r2)
                            })
                    });
                    if waits && !on_a_leg && deferred.insert(q0) {
                        changed = true;
                        continue;
                    }
                    chain.truncate(n);
                    let last = chain.len() - 1;
                    chain[last].2 = self.p[chain[last].0].l;
                    top_host = None;
                }
                tried.insert(q0);
                if is_root && (top_host.is_none() || chain.len() == 1) {
                    continue;
                }
                // A pillar's free top is only ever a leg through a knee onto another support or a
                // post; otherwise it stays as it was.
                if is_spur
                    && (knee.is_none()
                        || top_host.is_none_or(|h| {
                            matches!(h.1, Owner::Kick(_)) || self.p[q0].claims.iter().any(|c| c.2 == h.1)
                        }))
                {
                    continue;
                }
                // A leg cannot lean on another leg (DragonFruit's kickstands hold up supports and
                // posts only): it stands as a post of its own. One whose low end joins a
                // support is left to be a brace from that support to the leg.
                if top_host.is_some_and(|h| matches!(h.1, Owner::Kick(_))) {
                    // The first free piece of the chain must itself join a support at its low end:
                    // one that only stands on a root pad would be written as nothing.
                    let q1 = chain[usize::from(is_root)].0;
                    let low = 0.6f64.max(2.5 * self.p[q1].r);
                    if self.hosts_of(q1).iter().any(|h| h.0 <= low && !matches!(h.1, Owner::Kick(_))) {
                        continue;
                    }
                    top_host = None;
                }
                // A pillar that meets a support on the plate at that support's own foot, no
                // higher than a pad may reach, is the support's root, not a leg holding it up
                // (the upright under a tip cone that starts in the raft band, t02_trunk_wall_raft).
                if let Some(h) = top_host
                    && let Owner::Support(s) = h.1
                    && !is_root
                    && chain.len() == 1
                    && self.p[q0].d[2].abs() >= FOOT_PILLAR_COS
                    && dist([h.2[0], h.2[1], 0.0], [self.p[q0].b[0], self.p[q0].b[1], 0.0]) <= self.p[q0].r + 0.05
                    && h.2[2] <= ROOT_TOP + PAD_REACH_MM
                    && self.supports[s].end == End::Plate
                    && self.supports[s].path.last().is_some_and(|x| x.0 == h.3)
                {
                    for &(q, _, _) in &chain {
                        self.p[q].role = Some(Role::Root);
                        self.p[q].owner = Some(h.1);
                        self.root_p.entry(h.1).or_default().push(q);
                    }
                    changed = true;
                    continue;
                }
                match top_host {
                    None => {
                        let k = self.posts.len();
                        for &(q, t0, t1) in &chain {
                            self.p[q].claims.push((t0, t1, Owner::Post(k)));
                        }
                        self.posts.push(chain);
                    }
                    Some(h) => {
                        let k = self.kicks.len();
                        for &(q, t0, t1) in &chain[usize::from(is_root)..] {
                            self.p[q].claims.push((t0, t1, Owner::Kick(k)));
                        }
                        self.kicks.push(Kick { chain, host: h.1, knot: h.2 });
                    }
                }
                changed = true;
            }
        }
        // Root pieces under a post or a leg (not under a routed support, so not owned above) widen
        // that post's or leg's root.
        for i in 0..self.p.len() {
            if self.p[i].role != Some(Role::Root) || self.p[i].owner.is_some() {
                continue;
            }
            for a in self.p[i].att.clone() {
                if a.weak || self.p[a.q].role == Some(Role::Root) {
                    continue;
                }
                let pq = &self.p[a.q];
                let own: BTreeSet<Owner> = pq
                    .claims
                    .iter()
                    .filter(|c| !matches!(c.2, Owner::Support(_)) && c.0 <= 0.6f64.max(2.5 * pq.r))
                    .map(|c| c.2)
                    .collect();
                if own.len() == 1 {
                    let o = *own.iter().next().expect("one owner");
                    let v = self.root_p.entry(o).or_default();
                    if !v.contains(&i) {
                        v.push(i);
                    }
                }
            }
        }
        self.braces_and_loose();
    }

    /// Braces. Free pieces first join end to end into straight runs (each end goes on into at most
    /// one piece, the one that best continues it within 35 degrees, and only when that piece
    /// agrees), so every piece is in exactly one run. A run is a brace between each pair of
    /// neighbouring hosts along it (a support, post or leg).
    fn braces_and_loose(&mut self) {
        let free_ids: Vec<usize> = (0..self.p.len()).filter(|&i| self.is_free(i)).collect();
        let mut cand: BTreeMap<(usize, i8), (usize, i8)> = BTreeMap::new();
        for &q in &free_ids {
            let pq = &self.p[q];
            for sgn in [-1.0f64, 1.0] {
                let ws: Vec<super::route::Way> = self
                    .ways(q, pq.l / 2.0, sgn, mul(pq.d, sgn), true, false, None, false)
                    .into_iter()
                    .filter(|w| w.0 == 0 && w.2 <= 35.0 && self.is_free(w.3))
                    .collect();
                if let Some(mut w) = ws.first().copied() {
                    for x in &ws[1..] {
                        if x.2 < w.2 {
                            w = *x;
                        }
                    }
                    cand.insert((q, sgn as i8), (w.3, w.5 as i8));
                }
            }
        }
        let link: BTreeMap<(usize, i8), (usize, i8)> =
            cand.iter().filter(|(k, v)| cand.get(&(v.0, -v.1)) == Some(&(k.0, -k.1))).map(|(k, v)| (*k, *v)).collect();
        let mut in_run = BTreeSet::new();
        let mut runs: Vec<Vec<(usize, i8)>> = Vec::new();
        for &q in &free_ids {
            if in_run.contains(&q) {
                continue;
            }
            // Back to the start of the run, then forward.
            let (mut cur, mut sgn, mut guard) = (q, -1i8, 0);
            while let Some(&(nq, ns)) = link.get(&(cur, sgn)) {
                if guard >= 10000 || nq == q {
                    break;
                }
                cur = nq;
                sgn = ns;
                guard += 1;
            }
            let mut run = Vec::new();
            sgn = -sgn;
            while !in_run.contains(&cur) {
                run.push((cur, sgn));
                in_run.insert(cur);
                match link.get(&(cur, sgn)) {
                    Some(&(nq, ns)) => {
                        cur = nq;
                        sgn = ns;
                    }
                    None => break,
                }
            }
            runs.push(run);
        }
        for run in runs {
            let mut hs: Vec<(f64, f64, Host)> = Vec::new();
            let mut off = 0.0;
            for &(q, sgn) in &run {
                let pq = &self.p[q];
                for h in self.hosts_of(q) {
                    let along = off + if sgn > 0 { h.0 } else { pq.l - h.0 };
                    // The brace's end on its host: where its own centre line passes the host rod's.
                    let ph = &self.p[h.3];
                    let w0 = sub(pq.a, ph.a);
                    let b_ = dot(pq.d, ph.d);
                    let den = 1.0 - b_ * b_;
                    let mut pt = h.2;
                    if den > 0.03 && !ph.ball && ph.l > 1e-6 {
                        let t_host = ((dot(ph.d, w0) - b_ * dot(pq.d, w0)) / den).clamp(0.0, ph.l);
                        let cand_pt = ph.at(t_host);
                        if dist(cand_pt, h.2) <= 1.0f64.max(2.0 * ph.r + 2.0 * pq.r) {
                            pt = cand_pt;
                        }
                    }
                    let h = (h.0, h.1, pt, h.3);
                    hs.push((along, dist(h.2, pq.at(h.0)), h));
                }
                off += pq.l;
            }
            hs.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut joints: Vec<(f64, f64, Host)> = Vec::new();
            for (along, near, h) in hs {
                if let Some(last) = joints.last_mut()
                    && along - last.0 <= 0.6
                {
                    if near < last.1 {
                        *last = (last.0, near, h);
                    }
                } else {
                    joints.push((along, near, h));
                }
            }
            let mut seq: Vec<(f64, f64, Host)> = Vec::new();
            for j in &joints {
                if seq.last().is_some_and(|s| s.2.1 == j.2.1) {
                    continue;
                }
                seq.push(*j);
            }
            let pieces: Vec<usize> = run.iter().map(|x| x.0).collect();
            if seq.len() >= 2 {
                let mut lens: Vec<(f64, f64)> = pieces.iter().map(|&q| (2.0 * self.p[q].r, self.p[q].l)).collect();
                lens.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
                let half = lens.iter().map(|x| x.1).sum::<f64>() / 2.0;
                let (mut acc, mut d) = (0.0, lens[lens.len() - 1].0);
                for &(w, l) in &lens {
                    acc += l;
                    if acc >= half {
                        d = w;
                        break;
                    }
                }
                // A rod lying with both ends below the root top (a web of rods in the raft band) is
                // no brace; one low end alone is still a brace (a brace landing on a pad).
                let all: Vec<(Host, Host)> = seq
                    .windows(2)
                    .filter(|w| w[0].2.2[2].max(w[1].2.2[2]) >= ROOT_TOP)
                    .map(|w| (w[0].2, w[1].2))
                    .collect();
                // A run from a branch to its own parent with both ends at the branch's knot is the
                // branch's base, not a brace.
                // Only a run between those two alone: one that goes on to a third host is a brace
                // passing the knot, and stays whole.
                let knots: Vec<Owner> = if seq.len() == 2 {
                    all.iter().filter_map(|(lo, up)| self.at_one_knot(lo, up)).collect()
                } else {
                    Vec::new()
                };
                let pairs: Vec<(Host, Host)> = if knots.is_empty() { all } else { Vec::new() };
                for &(lo, up) in &pairs {
                    self.braces.push(Brace { piece: pieces[0], lower: lo, upper: up, d });
                }
                for &q in &pieces {
                    if !pairs.is_empty() {
                        self.p[q].role = Some(Role::Brace);
                    } else if let Some(&o) = knots.first() {
                        self.p[q].role = Some(Role::Sleeve);
                        self.p[q].owner = Some(o);
                        self.p[q].why =
                            "between a branch and its parent at the branch's knot: the branch's base".into();
                    } else {
                        self.p[q].role = Some(Role::Loose);
                        self.p[q].why =
                            "a run between supports lying below the root top (a web in the raft band): not a brace"
                                .into();
                    }
                }
            } else if !joints.is_empty() {
                for &q in &pieces {
                    let pq = &self.p[q];
                    // A sleeve: a piece lying along the rods of the one support it touches (a
                    // coupling round a joint, a doubled stretch), holding no contact of its own.
                    let hs_q = self.hosts_of(q);
                    let mut along = !hs_q.is_empty();
                    for x in [pq.a, pq.mid(), pq.b] {
                        let best = hs_q
                            .iter()
                            .map(|h| dist(x, self.p[h.3].at(self.p[h.3].t_of(x))) - self.p[h.3].r.max(pq.r))
                            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v))))
                            .unwrap_or(9.0);
                        along &= best <= 0.15;
                    }
                    if along && self.on_of(q).is_empty() {
                        let o = hs_q[0].1;
                        self.p[q].role = Some(Role::Sleeve);
                        self.p[q].owner = Some(o).filter(|o| matches!(o, Owner::Support(_)));
                    } else {
                        self.p[q].role = Some(Role::Loose);
                        self.p[q].why = "a run on one support only (one end free, or both ends on the same support): not writable as a brace".into();
                    }
                }
            } else {
                for &q in &pieces {
                    let on_free = self.p[q].att.iter().any(|a| !a.weak && !self.p[a.q].ball && !self.p[a.q].raft);
                    self.p[q].role = Some(Role::Floating);
                    self.p[q].why =
                        if on_free { "joins only pieces no route uses" } else { "touches no other support piece" }
                            .into();
                }
            }
        }
        // A piece joined only to braces or loose pieces.
        for i in 0..self.p.len() {
            if self.p[i].role == Some(Role::Floating) && self.p[i].why.starts_with("joins only") {
                let brace =
                    self.p[i].att.iter().any(|a| !a.weak && !self.p[a.q].ball && self.p[a.q].role == Some(Role::Brace));
                if brace {
                    self.p[i].role = Some(Role::BraceOnBrace);
                    self.p[i].why = "its ends sit on braces, which DragonFruit braces cannot host".into();
                }
            }
        }
    }

    pub(super) fn roles(&mut self) {
        for si in 0..self.supports.len() {
            let s = &self.supports[si];
            let mut base = match s.end {
                End::Plate => Role::Trunk,
                End::Branch(..) => Role::Branch,
                End::Foot(_) => Role::Strut,
            };
            match s.end {
                End::Foot(_) => {
                    if median(s.path.iter().map(|x| 2.0 * self.p[x.0].r).collect()) <= MICRO_D {
                        base = Role::Micro;
                    }
                }
                End::Branch(..) => {
                    // A thin blunt rod from the model onto a pillar is a micro support too.
                    let own: Vec<usize> = s
                        .path
                        .iter()
                        .map(|x| x.0)
                        .filter(|&q| !self.p[q].claims.iter().any(|c| c.2 != Owner::Support(si)))
                        .collect();
                    let blunt = !own.iter().any(|&q| {
                        self.p[q]
                            .secs
                            .iter()
                            .any(|sec| sec.k == SectionKind::Cone && sec.r0.max(sec.r1) > 1.15 * sec.r0.min(sec.r1))
                    });
                    if !own.is_empty() && blunt && median(own.iter().map(|&q| 2.0 * self.p[q].r).collect()) <= MICRO_D {
                        base = Role::Micro;
                    }
                }
                End::Plate => {}
            }
            let path = s.path.clone();
            self.supports[si].role = base;
            let last = path.len() - 1;
            for (k, &(q, _, _)) in path.iter().enumerate() {
                if self.p[q].role.is_none() {
                    // A strut's piece holding its lower contact is a tip as well.
                    self.p[q].role =
                        Some(if k == 0 || (base == Role::Strut && k == last && last > 0) { Role::Tip } else { base });
                    self.p[q].owner = Some(Owner::Support(si));
                }
            }
        }
        for post in &self.posts {
            for &(q, _, _) in post {
                self.p[q].role.get_or_insert(Role::Post);
            }
        }
        for kick in &self.kicks {
            for &(q, _, _) in &kick.chain {
                self.p[q].role.get_or_insert(Role::Leg);
            }
        }
        // A ball the model cuts at a routed contact is part of that contact's tip (a ball sunk at a
        // cone's end): its loop is the tip's loop and sizes it, and it is never a joint (only tips
        // enter the model). The contact piece of such a tip may be the ball itself.
        let mut ball_tip: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        for (ci, hits) in self.contact_pieces.iter().enumerate() {
            let Some(&own) = self.sup_of.get(&ci).or_else(|| self.consumed.get(&ci)) else { continue };
            for h in hits {
                if self.p[h.0].ball && !self.p[h.0].raft && (h.1 || self.cpiece.get(&ci) == Some(&h.0)) {
                    ball_tip.entry(h.0).or_insert((ci, own));
                }
            }
        }
        let cpieces: BTreeSet<usize> = self.cpiece.values().copied().collect();
        for i in 0..self.p.len() {
            let p = &self.p[i];
            if p.raft {
                self.p[i].role = Some(Role::Raft);
            } else if p.ball && p.role.is_none() {
                if let Some(&(ci, own)) = ball_tip.get(&i) {
                    self.p[i].role = Some(Role::Tip);
                    self.p[i].owner = Some(Owner::Support(own));
                    self.p[i].why = format!("a ball at contact {ci}: part of its tip (its loop sizes the tip)");
                } else if cpieces.contains(&i)
                    && self.on_of(i).iter().any(|c| self.sup_of.contains_key(c) || self.consumed.contains_key(c))
                {
                    self.p[i].role = Some(Role::Tip);
                } else if p
                    .att
                    .iter()
                    .any(|a| !a.weak && (!self.p[a.q].claims.is_empty() || self.p[a.q].role == Some(Role::Brace)))
                {
                    self.p[i].role = Some(Role::Knot);
                } else {
                    self.p[i].role = Some(Role::Floating);
                    self.p[i].why = "a ball on nothing routed".into();
                }
            } else if p.role.is_none() {
                self.p[i].role = Some(Role::Floating);
                self.p[i].why = "unclassified".into();
            }
        }
        for p in &self.p {
            if let Some(role @ (Role::Loose | Role::Floating | Role::BraceOnBrace)) = p.role {
                let unread = if p.kind == PieceKind::Unread { " (unread)" } else { "" };
                self.exceptions.push(Exception {
                    kind: "piece",
                    at: mul(add(p.a, p.b), 0.5),
                    why: format!("{}{unread}", p.why),
                    contact: None,
                    piece: Some(p.i),
                    role: Some(role),
                });
            }
        }
    }
}
