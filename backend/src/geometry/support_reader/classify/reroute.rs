//! Stage 5b: a second reading. Each route takes the best way on at every joint without knowing
//! what the other routes will need, so two rods that look alike at a joint (a tip's own shaft and
//! a brace ending at the same socket) can be taken for each other. The rest stage then shows the
//! mistake: a pillar left standing as a post with no tip (or as a leg leaning on a socket), or
//! rods left hanging on a rod read as a brace. Where such a rod leaves a joint a route turned
//! away from, the route is told to take it, and the structure is routed again. The second reading
//! is kept only when it leaves fewer pieces unread, no more contacts without a route, and no
//! route with bends it did not have.

use std::collections::{BTreeMap, BTreeSet};

use super::super::math::{dot, mul};
use super::{End, Owner, REROUTE_ROUNDS, Reader, Role, STRAIGHT_ON_COS, Saved};

/// A leg's chain turns by more than a straight run may (35 degrees) at its knee.
const KNEE_COS: f64 = 0.819;

impl Reader<'_> {
    /// A piece the rest stage could make nothing of.
    fn odd(&self, i: usize) -> bool {
        matches!(self.p[i].role, Some(Role::Loose) | Some(Role::Floating) | Some(Role::BraceOnBrace))
    }

    /// What a reading leaves unread: the contacts without a route, and the pieces of posts and
    /// legs and those that are neither part of a support nor a brace. Legs count so that a trunk
    /// is never given up for the kickstand leaning on it (the trunk would stand on as a leg).
    fn left_over(&self) -> (usize, BTreeSet<usize>) {
        let unrouted = self
            .cpiece
            .keys()
            .filter(|c| !self.sup_of.contains_key(c) && !self.consumed.contains_key(c) && !self.through.contains_key(c))
            .count();
        let mut left: BTreeSet<usize> = (0..self.p.len()).filter(|&i| self.odd(i)).collect();
        left.extend(self.posts.iter().flatten().map(|x| x.0));
        left.extend(self.kicks.iter().flat_map(|k| k.chain.iter().map(|x| x.0)));
        (unrouted, left)
    }

    /// The pieces joined to `i` at its end (`sgn` along its axis), through joint balls.
    fn at_end(&self, i: usize, sgn: f64) -> Vec<usize> {
        let p = &self.p[i];
        let t_end = if sgn > 0.0 { p.l } else { 0.0 };
        let tol = 0.5f64.max(2.0 * p.r);
        let mut out = Vec::new();
        for a in p.att.iter().filter(|a| !a.weak && (a.t - t_end).abs() <= tol) {
            for (r, _, _) in self.hub(i as isize, a.q, a.at, false) {
                if r != i && !self.p[r].raft && !out.contains(&r) {
                    out.push(r);
                }
            }
        }
        out
    }

    /// Whether piece `i` is a rod no route, post or leg uses. A piece holding a contact is not
    /// one: no route found it a tip, so it is a collar or a flank in the model (real case
    /// m04_micro_extra_tip), which routing through would build as a rod inside the model.
    fn spare(&self, i: usize) -> bool {
        let p = &self.p[i];
        !p.raft && !p.ball && !p.pad && p.sleeve.is_none() && p.claims.is_empty() && self.on_of(i).is_empty()
    }

    /// Whether a route runs from its stretch `k` on along one straight line to the plate or to
    /// its foot on the model (the foot's own tip piece aside): a pillar, a strut's span. Such a
    /// route keeps its way.
    fn straight_on(&self, route: &super::Route, k: usize) -> bool {
        let last = match route.end {
            End::Plate => route.path.len(),
            End::Foot(_) => route.path.len() - usize::from(route.path.len() > k + 1),
            End::Branch(..) => return false,
        };
        let dir = |x: &super::Step| mul(self.p[x.0].d, (x.2 - x.1).signum());
        let rods: Vec<&super::Step> =
            route.path[k..last].iter().filter(|x| !self.p[x.0].ball && (x.2 - x.1).abs() > 1e-6).collect();
        rods.windows(2).all(|w| dot(dir(w[0]), dir(w[1])) >= STRAIGHT_ON_COS)
    }

    /// From piece `w`'s end (`sgn` along its axis) through spare rods in line with it, to the
    /// socket of a route that leaves its tip piece another way: the ways that route would have to
    /// take to come down `w` instead, as (contact, piece) to the next piece.
    fn wish(&self, w: usize, sgn: f64, out: &mut BTreeMap<(usize, usize), usize>) {
        let mut chain = vec![w];
        let (mut q, mut s) = (w, sgn);
        let mut seen = BTreeSet::from([w]);
        for _ in 0..4 {
            let pq = &self.p[q];
            let t_end = if s > 0.0 { pq.l } else { 0.0 };
            let tol = 0.5f64.max(2.0 * pq.r);
            // A route passing this end and leaving its piece there: the nearest exit wins.
            let mut best: Option<(f64, usize, usize)> = None;
            let mut next: Option<(f64, usize, f64)> = None;
            for a in pq.att.iter().filter(|a| !a.weak && (a.t - t_end).abs() <= tol) {
                for (r, tr, _) in self.hub(q as isize, a.q, a.at, false) {
                    let pr = &self.p[r];
                    if r == q || pr.raft || pr.pad || pr.sleeve.is_some() {
                        continue;
                    }
                    if self.spare(r) {
                        // The same line going on (up to 35 degrees off, as a brace's run).
                        for sr in [-1.0f64, 1.0] {
                            let rem = if sr > 0.0 { pr.l - tr } else { tr };
                            let cos = sr * s * dot(pr.d, pq.d);
                            if !seen.contains(&r) && rem > 0.45 * pr.l && cos >= 0.819 && next.is_none_or(|n| cos > n.0)
                            {
                                next = Some((cos, r, sr));
                            }
                        }
                        continue;
                    }
                    let tol_r = 0.5f64.max(2.0 * pr.r);
                    for &(lo, hi, o) in &pr.claims {
                        let Owner::Support(si) = o else { continue };
                        let route = &self.supports[si];
                        let Some(k) =
                            route.path.iter().position(|x| x.0 == r && x.1.min(x.2) == lo && x.1.max(x.2) == hi)
                        else {
                            continue;
                        };
                        let exit = route.path[k].2;
                        // The route goes on from here (it does not end on this piece), another way.
                        if k + 1 >= route.path.len() || chain.contains(&route.path[k + 1].0) {
                            continue;
                        }
                        // Only at the route's own tip piece, its socket: there a tip's shaft and
                        // a brace look alike. A joint further down moved the knots of branches
                        // along the braces beside their parents (real model 01a23637), and stood
                        // branches on the pillars of posts that are not theirs.
                        if k != 0 {
                            continue;
                        }
                        if self.straight_on(route, k + 1) {
                            continue;
                        }
                        let gap = (exit - tr).abs();
                        if gap <= tol_r && best.is_none_or(|b| gap < b.0) {
                            best = Some((gap, route.contact, r));
                        }
                    }
                }
            }
            if let Some((_, ci, r)) = best {
                let mut from = r;
                for &x in chain.iter().rev() {
                    out.entry((ci, from)).or_insert(x);
                    from = x;
                }
                return;
            }
            let Some((_, r, sr)) = next else { return };
            seen.insert(r);
            chain.push(r);
            (q, s) = (r, sr);
        }
    }

    /// The ways routes should have taken, from what the reading leaves unread.
    fn wishes(&self) -> BTreeMap<(usize, usize), usize> {
        let mut out = BTreeMap::new();
        let mut add = |found: BTreeMap<(usize, usize), usize>| {
            for (k, v) in found {
                out.entry(k).or_insert(v);
            }
        };
        // A post: a pillar on the plate with no tip. A rod read as a brace leaving its top upwards
        // is the knee of a trunk whose route went another way.
        for chain in &self.posts {
            let Some(&(top, _, _)) = chain.last() else { continue };
            for r in self.at_end(top, 1.0) {
                let pr = &self.p[r];
                if !self.spare(r) || pr.role != Some(Role::Brace) {
                    continue;
                }
                let sr = if pr.t_of(self.p[top].b) < pr.l / 2.0 { 1.0 } else { -1.0 };
                if sr * pr.d[2] > 0.0 {
                    let mut found = BTreeMap::new();
                    self.wish(r, sr, &mut found);
                    let on: Vec<usize> = found.iter().filter(|(_, to)| **to == r).map(|(k, _)| k.0).collect();
                    for ci in on {
                        found.entry((ci, r)).or_insert(top);
                    }
                    add(found);
                }
            }
        }
        // A leg with a knee: an upright pillar and its lean onto a support. Where the lean meets a
        // route's socket it is that same knee, read as a kickstand under the trunk it belongs to
        // (real case x12_brace_steep).
        for kick in &self.kicks {
            for w in kick.chain.windows(2) {
                let ((top, t0, t1), (r, r0, r1)) = (w[0], w[1]);
                let (st, sr) = ((t1 - t0).signum(), (r1 - r0).signum());
                if st * sr * dot(self.p[top].d, self.p[r].d) >= KNEE_COS || sr * self.p[r].d[2] <= 0.0 {
                    continue;
                }
                let mut found = BTreeMap::new();
                self.wish(r, sr, &mut found);
                let on: Vec<usize> = found.iter().filter(|(_, to)| **to == r).map(|(k, _)| k.0).collect();
                for ci in on {
                    found.entry((ci, r)).or_insert(top);
                }
                add(found);
            }
        }
        for i in 0..self.p.len() {
            if !self.odd(i) || !self.spare(i) {
                continue;
            }
            let up = if self.p[i].d[2] >= 0.0 { 1.0 } else { -1.0 };
            // A rod with both ends on one support: another way down for that support's route.
            let mut found = BTreeMap::new();
            self.wish(i, up, &mut found);
            if found.is_empty() {
                self.wish(i, -up, &mut found);
            }
            add(found);
            // A rod whose end hangs on a rod read as a brace: a brace cannot host it, a support's
            // shaft can.
            for sgn in [up, -up] {
                let ends = self.at_end(i, sgn);
                if ends.iter().any(|&r| !self.p[r].claims.is_empty()) {
                    continue;
                }
                for r in ends {
                    if self.spare(r) && self.p[r].role == Some(Role::Brace) {
                        let up_r = if self.p[r].d[2] >= 0.0 { 1.0 } else { -1.0 };
                        let mut found = BTreeMap::new();
                        self.wish(r, up_r, &mut found);
                        if found.is_empty() {
                            self.wish(r, -up_r, &mut found);
                        }
                        add(found);
                    }
                }
            }
        }
        out
    }

    /// The contacts whose routes, in this reading or as `before`, run by the pieces of `fresh`.
    fn blamed(&self, fresh: &BTreeSet<usize>, before: &[super::Route]) -> BTreeSet<usize> {
        let mut near: BTreeSet<usize> = fresh.clone();
        for &i in fresh {
            for a in self.p[i].att.iter().filter(|a| !a.weak) {
                near.extend(self.hub(i as isize, a.q, a.at, false).into_iter().map(|x| x.0));
            }
        }
        let told: BTreeSet<usize> = self.prefer.keys().map(|k| k.0).collect();
        self.supports
            .iter()
            .chain(before)
            .filter(|s| told.contains(&s.contact) && s.path.iter().any(|x| near.contains(&x.0)))
            .map(|s| s.contact)
            .collect()
    }

    /// The bends of a route's line, as the layout keeps them (`STRAIGHT_MM`).
    fn bends(&self, route: &super::Route) -> usize {
        super::tips::simplify(&self.line_of(&route.path)).len().saturating_sub(2)
    }

    /// The contacts told a way whose route now has over one bend more than in the first reading
    /// (`first`: contact to its bends and whether it stood on the plate), or no route at all. A
    /// route that now stands on the plate and did not before may have two more: the socket and
    /// the knee of a trunk that was read as a stub hanging from its neighbour. More is a route
    /// wandering through a lattice of braces.
    fn bent(&self, first: &BTreeMap<usize, (usize, bool)>) -> BTreeSet<usize> {
        let told: BTreeSet<usize> = self.prefer.keys().map(|k| k.0).collect();
        told.into_iter()
            .filter(|ci| match (self.sup_of.get(ci), first.get(ci)) {
                (Some(&si), Some(&(was, on_plate))) => {
                    let gains_pillar = !on_plate && self.supports[si].end == End::Plate;
                    self.bends(&self.supports[si]) > was + 1 + usize::from(gains_pillar)
                }
                (None, Some(_)) => true,
                _ => false,
            })
            .collect()
    }

    /// Reads the structure again with the routes told which way to take where the first reading
    /// left posts, or rods hanging on rods read as braces. `unrouted` is the state before
    /// routing. A second reading is kept when it leaves fewer pieces unread and no more contacts
    /// without a route, and no route told a way has grown bends (see `bent`). Routes that have
    /// go back to their own way, as do the routes by whatever is newly unread when the reading
    /// is no better, and the rest is read once more.
    pub(super) fn reroute(&mut self, unrouted: &Saved) {
        let first: BTreeMap<usize, (usize, bool)> =
            self.supports.iter().map(|s| (s.contact, (self.bends(s), s.end == End::Plate))).collect();
        let mut best = (self.save(), self.prefer.clone(), self.left_over());
        let mut refused: BTreeSet<usize> = BTreeSet::new();
        for _ in 0..REROUTE_ROUNDS {
            let mut prefer = self.prefer.clone();
            for (k, v) in self.wishes() {
                if !refused.contains(&k.0) {
                    prefer.entry(k).or_insert(v);
                }
            }
            if prefer == self.prefer {
                break;
            }
            self.prefer = prefer;
            let mut good = false;
            // Read with what is told; take back what made things worse, and read again.
            for _ in 0..REROUTE_ROUNDS {
                let before = std::mem::take(&mut self.supports);
                self.load(unrouted.clone());
                self.route();
                self.rest();
                let now = self.left_over();
                let better = now.0 <= best.2.0 && now.1.len() < best.2.1.len();
                let mut blamed = self.bent(&first);
                if blamed.is_empty() && !better {
                    let fresh: BTreeSet<usize> = now.1.difference(&best.2.1).copied().collect();
                    blamed = self.blamed(&fresh, &before);
                }
                if blamed.is_empty() {
                    good = better;
                    break;
                }
                self.prefer.retain(|k, _| !blamed.contains(&k.0));
                refused.extend(blamed);
                if self.prefer == best.1 {
                    break;
                }
            }
            if !good {
                break;
            }
            best = (self.save(), self.prefer.clone(), self.left_over());
        }
        self.load(best.0);
        self.prefer = best.1;
    }
}
