//! Port of gen.Scene.truth: the truth.json structure, built as serde_json values in the same shape
//! (keys, nesting and value types) as the Python writes.

use crate::scene::{Base, Line, PRef, RAFT, Scene, Tip, lst};
use crate::vec::v3;
use serde_json::{Map, Value, json};

impl Scene {
    /// The index truth gives a support or post: supports first, then the posts still in the scene.
    pub fn pillar_index(&self, r: PRef) -> usize {
        match r {
            PRef::Sup(i) => i,
            PRef::Post(i) => self.supports.len() + self.posts[..i].iter().filter(|p| !p.removed).count(),
        }
    }

    fn tip_json(&self, t: &Tip) -> Value {
        json!({"contact": lst(self.turn(t.contact)), "normal": lst(self.turn(t.normal)), "socket": lst(self.turn(t.socket)),
               "contact_d": t.contact_d, "body_d": t.body_d, "name": t.name})
    }

    fn pts_json(&self, pts: &[crate::vec::V3]) -> Value {
        Value::Array(pts.iter().map(|&p| lst(self.turn(p))).collect())
    }

    fn with_seg(&self, mut m: Map<String, Value>, l: &Line) -> Value {
        if let Some(s) = &l.seg_d
            && !s.is_empty()
        {
            m.insert("seg_d".into(), json!(s));
        }
        Value::Object(m)
    }

    pub fn truth(&self, cid: &str, title: &str, real: &str, params: Value) -> Value {
        let supports: Vec<Value> = self
            .supports
            .iter()
            .map(|s| {
                let base = match &s.base {
                    Base::Plate { xy, root_d } => {
                        let p = self.turn(v3(xy[0], xy[1], 0.0));
                        json!({"plate": [p[0], p[1]], "root_d": root_d})
                    }
                    Base::Parent { host, at } => {
                        json!({"parent": self.pillar_index(*host), "at": lst(self.turn(*at)), "parent_name": self.name_of(*host)})
                    }
                    Base::Lower { tip } => json!({"lower": self.tip_json(&self.tips[*tip])}),
                };
                let mut m = json!({"name": s.name, "tip": self.tip_json(&self.tips[s.tip]), "base": base, "joints": self.pts_json(&s.l.joints),
                               "shaft_d": s.shaft_d, "line": self.pts_json(&s.l.line)});
                // How a strut is to be written (a twig or a stick), where the case says.
                if let Some((_, w)) = self.written.iter().find(|w| w.0 == s.name) {
                    m["written"] = json!(w);
                }
                self.with_seg(m.as_object().unwrap().clone(), &s.l)
            })
            .collect();
        let posts: Vec<Value> = self
            .posts
            .iter()
            .filter(|p| !p.removed)
            .map(|p| {
                let m = json!({"name": p.name, "foot": lst(self.turn(p.foot)), "top": lst(self.turn(p.top)), "joints": self.pts_json(&p.l.joints),
                               "d": p.d, "root_d": p.root_d, "line": self.pts_json(&p.l.line)});
                self.with_seg(m.as_object().unwrap().clone(), &p.l)
            })
            .collect();
        let braces: Vec<Value> = self
            .braces
            .iter()
            .map(|b| {
                let mut ends = [(b.pa, b.a), (b.pb, b.b)];
                // A stable sort by height of the end point.
                if ends[1].1[2] < ends[0].1[2] {
                    ends.swap(0, 1);
                }
                json!({"name": b.name, "part": b.part,
                       "lower": [self.pillar_index(ends[0].0), null, lst(self.turn(ends[0].1))],
                       "upper": [self.pillar_index(ends[1].0), null, lst(self.turn(ends[1].1))],
                       "lower_name": self.name_of(ends[0].0), "upper_name": self.name_of(ends[1].0), "d": b.d, "hosts_any": b.hosts_any})
            })
            .collect();
        let kicks: Vec<Value> = self
            .kicks
            .iter()
            .map(|k| {
                let m = json!({"name": k.name, "support": self.pillar_index(k.host), "host_name": self.name_of(k.host), "knot": lst(self.turn(k.knot)),
                               "joints": self.pts_json(&k.l.joints), "foot": lst(self.turn(k.foot)), "root_d": k.root_d, "d": k.d});
                self.with_seg(m.as_object().unwrap().clone(), &k.l)
            })
            .collect();
        let tips: Vec<Value> = self
            .tips
            .iter()
            .map(|t| {
                json!({"name": t.name, "contact": lst(self.turn(t.contact)), "normal": lst(self.turn(t.normal)), "dir": lst(self.turn(t.dir)),
                       "contact_d": t.contact_d, "kind": t.kind, "part": t.part, "exported": true})
            })
            .collect();
        let parts: Vec<Value> = self
            .parts
            .iter()
            .map(|p| {
                json!({"name": p.name, "type": p.kind, "a": lst(self.turn(p.a)), "b": lst(self.turn(p.b)), "ra": p.ra, "rb": p.rb,
                       "required": p.required, "expect": p.expect})
            })
            .collect();
        let count = |f: fn(&Base) -> bool| self.supports.iter().filter(|s| f(&s.base)).count();
        let counts = json!({
            "trunks": count(|b| matches!(b, Base::Plate { .. })),
            "branches": count(|b| matches!(b, Base::Parent { .. })),
            "struts": count(|b| matches!(b, Base::Lower { .. })),
            "posts": self.posts.iter().filter(|p| !p.removed).count(),
            "kickstands": self.kicks.len(),
            "braces": self.braces.len(),
        });
        json!({
            "id": cid, "title": title, "real": real, "params": params, "raft": RAFT, "rot": self.rot, "shapes": self.shapes,
            "parts": parts, "tips": tips, "counts": counts, "counts_alt": self.counts_alt, "flex": self.flex, "tree_free": self.tree_free,
            "passes_through": self.passes_through, "notes": self.notes, "model_open": self.model_open,
            "layout": {"supports": supports, "posts": posts, "braces": braces, "kickstands": kicks},
        })
    }
}
