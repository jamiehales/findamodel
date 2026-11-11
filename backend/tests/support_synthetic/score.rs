//! Port of the DragonFruit-free stages of synthetic/score.py: the generator's truth self-check
//! (`validate`), stage 1 (reader labels), stage 2 (layout) and stage 4's clip and loop checks
//! (`stage4`, run on any built support mesh). Inputs and outputs are serde_json::Value in the
//! shapes the Python reads and writes. Tolerances are score.py's, unchanged. Floating-point
//! evaluation follows numpy's order where it matters (see crate::vec), and error strings are
//! formatted as Python's f-strings format them.

use crate::measure::Seg;
use crate::shapes::inside_shapes;
use crate::vec::{V3, gemm_row, gemv_rows, pairwise_sum};
use serde_json::{Value, json};

pub const AXIS_DEG: f64 = 3.0;
pub const WIDTH_MM: f64 = 0.1;
pub const CONTACT_POS_MM: f64 = 0.2;
pub const LOOP_POS_MM: f64 = 0.15;
pub const AREA_SHARE: f64 = 0.2;
pub const RECALL_MM: f64 = 0.3;
pub const RECALL_MIN: f64 = 0.9;
pub const PRECISION_MIN: f64 = 0.95;
pub const CLIP_MM: f64 = 0.05;
pub const EXTRA_MM: f64 = 0.3;
pub const BRACE_END_MM: f64 = 0.75;
pub const LINE_MM: f64 = 0.3;
pub const TIE_MM: f64 = 1e-6;

// ---- Value and Python helpers ----

pub fn arr(x: &Value) -> V3 {
    V3([0, 1, 2].map(|k| x[k].as_f64().unwrap()))
}
fn f(x: &Value) -> f64 {
    x.as_f64().unwrap()
}
fn s(x: &Value) -> &str {
    x.as_str().unwrap_or("")
}
fn list(x: &Value) -> &[Value] {
    x.as_array().map(|a| a.as_slice()).unwrap_or(&[])
}
/// Python truthiness of a JSON value (None for a missing key).
pub fn truthy(x: Option<&Value>) -> bool {
    match x {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}
fn get<'a>(x: &'a Value, k: &str) -> Option<&'a Value> {
    x.as_object().and_then(|o| o.get(k))
}
fn is_dict(x: &Value) -> bool {
    x.is_object()
}
fn has(x: &Value, k: &str) -> bool {
    get(x, k).is_some()
}

/// Python's repr of a float.
pub fn py_repr(x: f64) -> String {
    let d = format!("{x:?}");
    if let Some(i) = d.find('e') {
        let (m, e) = d.split_at(i);
        let e = &e[1..];
        let (sign, digits) = if let Some(r) = e.strip_prefix('-') { ("-", r) } else { ("+", e) };
        format!("{m}e{sign}{digits:0>2}")
    } else {
        d
    }
}
/// Python's str of a JSON value as score.py prints it (ints as ints, None, floats by repr).
fn py_str(x: &Value) -> String {
    match x {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::Number(n) => {
            if n.is_f64() {
                py_repr(n.as_f64().unwrap())
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
fn py_quote(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}
/// `sorted(set_of_str)` as Python prints a list of strings.
fn py_str_list(mut v: Vec<String>) -> String {
    v.sort();
    v.dedup();
    format!("[{}]", v.iter().map(|x| py_quote(x)).collect::<Vec<_>>().join(", "))
}
/// numpy round to `dec` decimals: rint(x * 10^dec) / 10^dec.
fn np_round(x: f64, dec: i32) -> f64 {
    let p = 10f64.powi(dec);
    (x * p).round_ties_even() / p
}
/// `np.round(v, 1).tolist()` as Python prints it.
fn round_list(v: V3) -> String {
    format!("[{}]", v.0.iter().map(|&x| py_repr(np_round(x, 1))).collect::<Vec<_>>().join(", "))
}
/// np.linspace(a, b, n).
fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    let step = (b - a) / (n - 1) as f64;
    let mut y: Vec<f64> = (0..n).map(|i| i as f64 * step + a).collect();
    y[n - 1] = b;
    y
}
#[allow(clippy::manual_clamp)]
fn clip01(x: f64) -> f64 {
    x.max(0.0).min(1.0)
}
fn argmin(v: &[f64]) -> usize {
    let mut best = 0;
    for (i, &x) in v.iter().enumerate() {
        if x < v[best] {
            best = i;
        }
    }
    best
}

// ---- geometry ----

/// seg_dist for an (N, 3) array of points.
pub fn seg_dist(pts: &[V3], a: V3, b: V3) -> Vec<f64> {
    let d = b - a;
    let den = d.dot(d).max(1e-12);
    let rel: Vec<V3> = pts.iter().map(|&p| p - a).collect();
    gemv_rows(&rel, d)
        .iter()
        .zip(pts)
        .map(|(&pr, &p)| {
            let t = clip01(pr / den);
            (p - (a + d * t)).norm()
        })
        .collect()
}
/// seg_dist for one 1-D point.
pub fn seg_dist1(p: V3, a: V3, b: V3) -> f64 {
    let d = b - a;
    let t = clip01((p - a).dot(d) / d.dot(d).max(1e-12));
    (p - (a + d * t)).norm()
}
pub fn poly_dist(pts: &[V3], line: &[V3]) -> Vec<f64> {
    let mut best = vec![1e9; pts.len()];
    for w in line.windows(2) {
        for (b, x) in best.iter_mut().zip(seg_dist(pts, w[0], w[1])) {
            *b = f64::min(*b, x);
        }
    }
    best
}
/// Distance outside a truth part (0 inside), with round walls.
pub fn part_gap(pts: &[V3], p: &Value) -> Vec<f64> {
    let (a, b) = (arr(&p["a"]), arr(&p["b"]));
    let (ra, rb) = (f(&p["ra"]), f(&p["rb"]));
    let d = b - a;
    let ll = d.dot(d);
    if ll < 1e-12 {
        return pts.iter().map(|&q| ((q - a).norm() - ra).max(0.0)).collect();
    }
    let rel: Vec<V3> = pts.iter().map(|&q| q - a).collect();
    gemv_rows(&rel, d)
        .iter()
        .zip(&rel)
        .map(|(&pr, &q)| {
            let raw = pr / ll;
            let t = clip01(raw);
            let r = ra + (rb - ra) * t;
            let radial = ((q - d * raw).norm() - r).max(0.0);
            let beyond = (raw - t).abs() * ll.sqrt();
            radial.hypot(beyond)
        })
        .collect()
}
/// geom.frame.
pub fn frame(axis: V3) -> (V3, V3, V3) {
    let axis = axis.unit();
    let helper = if axis[2].abs() < 0.9 { V3([0.0, 0.0, 1.0]) } else { V3([1.0, 0.0, 0.0]) };
    let u = axis.cross(helper).unit();
    (axis, u, axis.cross(u))
}
/// The width with the area of the loop the truth cone makes on the surface.
pub fn loop_d(tip: &Value) -> f64 {
    let n = arr(&tip["normal"]);
    let d = arr(&tip["contact"]) - arr(&tip["socket"]);
    let cos = n.dot(d).abs() / (n.norm() * d.norm()).max(1e-12);
    let cd = f(&tip["contact_d"]);
    if cos < 0.999 { cd / cos.max(0.3).sqrt() } else { cd }
}

/// Convex hull perimeter and centre (of the hull's outline) of 2D points.
pub fn hull_perimeter(pts: &[[f64; 2]]) -> (f64, [f64; 2]) {
    let mut ps: Vec<[f64; 2]> = Vec::new();
    for p in pts {
        let q = [np_round(p[0], 5), np_round(p[1], 5)];
        if !ps.contains(&q) {
            ps.push(q);
        }
    }
    ps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if ps.len() < 3 {
        return (0.0, ps.first().copied().unwrap_or([0.0, 0.0]));
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let (mut lo, mut up): (Vec<[f64; 2]>, Vec<[f64; 2]>) = (vec![], vec![]);
    for &p in &ps {
        while lo.len() >= 2 && cross(lo[lo.len() - 2], lo[lo.len() - 1], p) <= 0.0 {
            lo.pop();
        }
        lo.push(p);
    }
    for &p in ps.iter().rev() {
        while up.len() >= 2 && cross(up[up.len() - 2], up[up.len() - 1], p) <= 0.0 {
            up.pop();
        }
        up.push(p);
    }
    let mut hull = lo[..lo.len() - 1].to_vec();
    hull.extend_from_slice(&up[..up.len() - 1]);
    if hull.len() < 3 {
        let (a, b) = (ps[0], ps[ps.len() - 1]);
        let l = ((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1])).sqrt() * 2.0;
        let mut m = [0.0, 0.0];
        for p in &ps {
            m[0] += p[0];
            m[1] += p[1];
        }
        let n = ps.len() as f64;
        return (l, [m[0] / n, m[1] / n]);
    }
    let h = hull.len();
    let mut ln = Vec::with_capacity(h);
    let mut acc = [0.0, 0.0];
    for i in 0..h {
        let (p, q) = (hull[i], hull[(i + 1) % h]);
        let e = [q[0] - p[0], q[1] - p[1]];
        let l = (e[0] * e[0] + e[1] * e[1]).sqrt();
        ln.push(l);
        let mid = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
        acc[0] += mid[0] * l;
        acc[1] += mid[1] * l;
    }
    let total = pairwise_sum(&ln);
    (total, [acc[0] / total, acc[1] / total])
}

/// A contact loop: its size (hull perimeter / pi), centre and which segments it took.
#[derive(Clone, Debug)]
pub struct Loop {
    pub d: f64,
    pub centre: V3,
    pub mask: Vec<bool>,
}

/// The loop of intersection segments around one contact.
pub fn tip_loop(tip: &Value, mids: &[V3], a: &[V3], b: &[V3], all_contacts: &[V3]) -> Option<Loop> {
    let (c, n) = (arr(&tip["contact"]), arr(&tip["normal"]));
    if mids.is_empty() {
        return None;
    }
    let rel: Vec<V3> = mids.iter().map(|&m| m - c).collect();
    let along = gemv_rows(&rel, n);
    let cos = arr(&tip["dir"]).dot(n).abs();
    let reach = f(&tip["contact_d"]) / 2.0 / cos.max(0.3) + 0.8;
    let lim = 0.3 + reach * (1.0 - cos * cos).max(0.0).sqrt();
    let mut q: Vec<bool> =
        rel.iter().zip(&along).map(|(&r, &al)| al.abs() <= lim && (r - n * al).norm() <= reach).collect();
    if all_contacts.len() > 1 {
        let me = all_contacts.iter().position(|x| *x == c).unwrap();
        for (i, m) in mids.iter().enumerate() {
            let ds: Vec<f64> = all_contacts.iter().map(|&x| (*m - x).norm()).collect();
            q[i] &= argmin(&ds) == me;
        }
    }
    if !q.iter().any(|&x| x) {
        return None;
    }
    let (_, u, v) = frame(n);
    let mut pts: Vec<V3> = a.iter().zip(&q).filter(|x| *x.1).map(|x| *x.0 - c).collect();
    pts.extend(b.iter().zip(&q).filter(|x| *x.1).map(|x| *x.0 - c));
    let (pu, pv) = (gemv_rows(&pts, u), gemv_rows(&pts, v));
    let flat: Vec<[f64; 2]> = pu.iter().zip(&pv).map(|(&x, &y)| [x, y]).collect();
    let (per, c2) = hull_perimeter(&flat);
    let centre = c + u * c2[0] + v * c2[1];
    Some(Loop { d: per / std::f64::consts::PI, centre, mask: q })
}

fn seg_ends(segs: &[Seg]) -> (Vec<V3>, Vec<V3>) {
    let w = |p: [f32; 3]| V3(p.map(f64::from));
    (segs.iter().map(|s| w(s.1)).collect(), segs.iter().map(|s| w(s.2)).collect())
}
fn mids_lens(a: &[V3], b: &[V3]) -> (Vec<V3>, Vec<f64>) {
    (a.iter().zip(b).map(|(&x, &y)| (x + y) / 2.0).collect(), a.iter().zip(b).map(|(&x, &y)| (y - x).norm()).collect())
}

// ---- Stage 1: reader labels ----

pub fn stage1(t: &Value, rd: &Value) -> Value {
    let els = list(&rd["elements"]);
    let ea: Vec<V3> = els.iter().map(|e| arr(&e["a"])).collect();
    let eb: Vec<V3> = els.iter().map(|e| arr(&e["b"])).collect();
    let mut errors: Vec<String> = Vec::new();
    let rods: Vec<usize> =
        (0..els.len()).filter(|&i| !els[i]["ball"].as_bool().unwrap_or(false) && f(&els[i]["length"]) > 0.0).collect();
    let cos15 = 15f64.to_radians().cos();
    for p in list(&t["parts"]) {
        let expect: Vec<String> = list(&p["expect"]).iter().map(|x| s(x).to_string()).collect();
        if expect.is_empty() {
            continue;
        }
        let (a, b) = (arr(&p["a"]), arr(&p["b"]));
        let (pra, prb) = (f(&p["ra"]), f(&p["rb"]));
        let name = s(&p["name"]);
        let length = (b - a).norm();
        if length < 1e-6 {
            continue;
        }
        let u = (b - a) / length;
        let ts = linspace(0.08, 0.92, 13);
        let pts: Vec<V3> = ts.iter().map(|&x| a + (b - a) * x).collect();
        let rs: Vec<f64> = ts.iter().map(|&x| pra + (prb - pra) * x).collect();
        let np_ = pts.len();
        let mut best_i: Vec<i64> = vec![-1; np_];
        let mut best_d = vec![1e9; np_];
        let mut best_r = vec![1e9; np_];
        for (i, &ri) in rods.iter().enumerate() {
            let e = &els[ri];
            let ed = eb[ri] - ea[ri];
            let cos = u.dot(ed).abs() / ed.norm().max(1e-9);
            if cos < cos15 {
                continue;
            }
            let ds = seg_dist(&pts, ea[ri], eb[ri]);
            let el = ed.norm();
            let rel: Vec<V3> = pts.iter().map(|&q| q - ea[ri]).collect();
            let proj = gemv_rows(&rel, ed / el.max(1e-9));
            let (era, erb) = (f(&e["ra"]), f(&e["rb"]));
            for k in 0..np_ {
                if !(-0.1 <= proj[k] && proj[k] <= el + 0.1 && ds[k] <= 0.25f64.max(0.6 * rs[k])) {
                    continue;
                }
                let rgap = (era + (erb - era) * clip01(proj[k] / el.max(1e-9)) - rs[k]).abs();
                if ds[k] < best_d[k] - TIE_MM || ((ds[k] - best_d[k]).abs() <= TIE_MM && rgap < best_r[k]) {
                    best_d[k] = ds[k];
                    best_i[k] = i as i64;
                    best_r[k] = rgap;
                }
            }
        }
        let cover = best_i.iter().filter(|&&x| x >= 0).count() as f64 / np_ as f64;
        if cover < 0.7 {
            let mid = pts[np_ / 2];
            let mut near: Vec<(f64, usize)> = (0..els.len())
                .filter(|&i| f(&els[i]["length"]) > 0.0)
                .map(|i| (seg_dist1(mid, ea[i], eb[i]), i))
                .collect();
            near.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
            let what = match near.first() {
                Some(&(d, i)) if d < 1.0 => py_str(&els[i]["kind"]),
                _ => "nothing".into(),
            };
            errors.push(format!("{name} missing (cover {cover:.2}, near {what})"));
            continue;
        }
        let mut weights: Vec<(String, usize)> = Vec::new();
        for &i in &best_i {
            if i >= 0 {
                let k = py_str(&els[rods[i as usize]]["kind"]);
                match weights.iter_mut().find(|w| w.0 == k) {
                    Some(w) => w.1 += 1,
                    None => weights.push((k, 1)),
                }
            }
        }
        let mut kind = &weights[0];
        for w in &weights {
            if w.1 > kind.1 {
                kind = w;
            }
        }
        let kind = kind.0.clone();
        if !expect.contains(&kind) {
            errors.push(format!("{name} read as {kind}, is {}", s(&p["type"])));
        }
        let mut used: Vec<usize> = best_i.iter().filter(|&&x| x >= 0).map(|&x| x as usize).collect();
        used.sort();
        used.dedup();
        let (mut angle, mut width, mut overrun) = (0.0f64, 0.0f64, 0.0f64);
        for &i in &used {
            let ri = rods[i];
            let e = &els[ri];
            let ed = eb[ri] - ea[ri];
            let ang = (u.dot(ed).abs() / ed.norm().max(1e-9)).min(1.0).acos().to_degrees();
            angle = angle.max(ang);
            let (era, erb) = (f(&e["ra"]), f(&e["rb"]));
            if pra == prb {
                width = width.max((2.0 * f(&e["r"]) - 2.0 * pra).abs());
            } else {
                width = width.max((2.0 * era.max(erb) - 2.0 * pra.max(prb)).abs());
                if e["cone"].as_bool().unwrap_or(false) {
                    width = width.max((2.0 * era.min(erb) - 2.0 * pra.min(prb)).abs());
                }
            }
            for q in [ea[ri], eb[ri]] {
                let al = (q - a).dot(u);
                overrun = overrun.max(-al).max(al - length);
            }
        }
        if angle > AXIS_DEG {
            errors.push(format!("{name} axis off by {angle:.1} deg"));
        }
        if width > WIDTH_MM {
            errors.push(format!("{name} width off by {width:.2} mm"));
        }
        if overrun > 1.0 {
            errors.push(format!("{name} read {overrun:.1} mm past its end"));
        }
    }
    for (i, e) in els.iter().enumerate() {
        if e["ball"].as_bool().unwrap_or(false) && e["kind"].as_str() != Some("ball") {
            errors.push(format!("ball labelled {}", py_str(&e["kind"])));
        }
        for j in i + 1..els.len() {
            let g = &els[j];
            if f(&e["length"]) > 0.0 && f(&g["length"]) > 0.0 && (f(&e["r"]) - f(&g["r"])).abs() < 0.05 {
                let same = (ea[i] - ea[j]).norm().max((eb[i] - eb[j]).norm());
                let flipped = (ea[i] - eb[j]).norm().max((eb[i] - ea[j]).norm());
                if same.min(flipped) < 0.05 {
                    errors.push(format!("duplicate {} element", py_str(&e["kind"])));
                }
            }
        }
    }
    let mut phantoms: Vec<String> = Vec::new();
    for (i, e) in els.iter().enumerate() {
        let pts: Vec<V3> = if f(&e["length"]) > 0.0 {
            linspace(0.1, 0.9, 7).iter().map(|&x| ea[i] + (eb[i] - ea[i]) * x).collect()
        } else {
            vec![ea[i]]
        };
        let mut ok = vec![false; pts.len()];
        for p in list(&t["parts"]) {
            let lim = f(&p["ra"]).max(f(&p["rb"])) + 0.3;
            for (o, d) in ok.iter_mut().zip(seg_dist(&pts, arr(&p["a"]), arr(&p["b"]))) {
                *o |= d <= lim;
            }
        }
        if (ok.iter().filter(|&&x| x).count() as f64 / ok.len() as f64) < 0.5 {
            phantoms.push(format!("{} r={:.2} at {}", py_str(&e["kind"]), f(&e["r"]), round_list(pts[pts.len() / 2])));
        }
    }
    if !phantoms.is_empty() {
        errors.push(format!("{} phantom: {}", phantoms.len(), phantoms[0]));
    }
    json!({"pass": errors.is_empty(), "errors": errors, "unread": rd["unread"], "elements": els.len()})
}

// ---- Stage 2: layout ----

pub fn widths_check(name: &str, want: Option<&Value>, got: Option<&Value>) -> Vec<String> {
    let fl =
        |x: Option<&Value>| -> Vec<f64> { if truthy(x) { list(x.unwrap()).iter().map(f).collect() } else { vec![] } };
    let (want, got) = (fl(want), fl(got));
    if want.len() != got.len() && want.len().max(got.len()) > 1 {
        return vec![format!("{name} in {} width steps, is {}", got.len().max(1), want.len().max(1))];
    }
    let bad = got.iter().zip(&want).any(|(x, y)| (x - y).abs() > WIDTH_MM + 0.05);
    if bad {
        let j = |v: &[f64]| v.iter().map(|x| format!("{x:.2}")).collect::<Vec<_>>().join(" ");
        vec![format!("{name} step widths {} mm, are {}", j(&got), j(&want))]
    } else {
        vec![]
    }
}

const COUNT_KEYS: [&str; 6] = ["trunks", "branches", "struts", "posts", "kickstands", "braces"];

fn xy(v: &Value) -> [f64; 2] {
    [f(&v[0]), f(&v[1])]
}
fn norm2(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (x, y) = (a[0] - b[0], a[1] - b[1]);
    (x * x + y * y).sqrt()
}

pub fn stage2(t: &Value, lay: &Value) -> Value {
    let tl = &t["layout"];
    let mut errors: Vec<String> = Vec::new();
    let sup = list(&lay["supports"]);
    let mut used: Vec<usize> = Vec::new();
    let mut names: Vec<(usize, String)> = Vec::new();
    let mut alt = false;
    let base_has = |b: &Value, k: &str| is_dict(b) && has(b, k);
    let got: Vec<i64> = vec![
        sup.iter().filter(|x| base_has(&x["base"], "plate")).count() as i64,
        sup.iter().filter(|x| base_has(&x["base"], "parent")).count() as i64,
        sup.iter().filter(|x| x["base"].as_str() == Some("model")).count() as i64,
        list(&lay["posts"]).len() as i64,
        list(&lay["kickstands"]).len() as i64,
        list(&lay["braces"]).len() as i64,
    ];
    let want: Vec<i64> = COUNT_KEYS.iter().map(|k| t["counts"][*k].as_i64().unwrap()).collect();
    let as_counts = |c: &Value| -> Vec<i64> { COUNT_KEYS.iter().map(|k| c[*k].as_i64().unwrap_or(i64::MIN)).collect() };
    let mut counts_ok = got == want;
    if truthy(get(t, "counts_alt"))
        && list(&t["counts_alt"]).iter().any(|c| as_counts(c) == got && c.as_object().map(|o| o.len()) == Some(6))
    {
        counts_ok = true;
        alt = true;
    }
    if truthy(get(t, "flex")) {
        let total = |c: &[i64]| c[3] + c[4] + c[5];
        let flex_total = total(&got) - total(&want);
        counts_ok = got[..3] == want[..3] && 0 <= flex_total && flex_total <= t["flex"].as_i64().unwrap();
    }
    if !counts_ok {
        let parts: Vec<String> = (0..6)
            .filter(|&k| got[k] != want[k])
            .map(|k| format!("{} {}/{}", COUNT_KEYS[k], got[k], want[k]))
            .collect();
        errors.push(format!("counts {}", parts.join(" ")));
    }
    let tree_free = truthy(get(t, "tree_free"));
    for ts in list(&tl["supports"]) {
        let c = arr(&ts["tip"]["contact"]);
        let tname = s(&ts["name"]);
        let (mut best, mut bd, mut swapped): (Option<usize>, f64, bool) = (None, 1e9, false);
        for (i, sp) in sup.iter().enumerate() {
            if used.contains(&i) {
                continue;
            }
            let d = (arr(&sp["tip"]["contact"]) - c).norm();
            if d < bd {
                (best, bd, swapped) = (Some(i), d, false);
            }
            if truthy(get(sp, "lower")) {
                let d = (arr(&sp["lower"]["contact"]) - c).norm();
                if d < bd {
                    (best, bd, swapped) = (Some(i), d, true);
                }
            }
        }
        let Some(bi) = best.filter(|_| bd <= 1.0) else {
            errors.push(format!("tip {tname} missing"));
            continue;
        };
        used.push(bi);
        match names.iter_mut().find(|x| x.0 == bi) {
            Some(x) => x.1 = tname.to_string(),
            None => names.push((bi, tname.to_string())),
        }
        let sp = &sup[bi];
        let ltip = if swapped { &sp["lower"] } else { &sp["tip"] };
        if bd > CONTACT_POS_MM {
            errors.push(format!("tip {tname} contact off by {bd:.2} mm"));
        }
        let sk = (arr(&ltip["socket"]) - arr(&ts["tip"]["socket"])).norm();
        if sk > 0.4 && f(&ts["tip"]["body_d"]) != f(&ts["tip"]["contact_d"]) {
            errors.push(format!("tip {tname} socket off by {sk:.2} mm"));
        }
        let lcd = f(&ltip["contact_d"]);
        let ld = loop_d(&ts["tip"]);
        let r = lcd / ld;
        if (r * r - 1.0).abs() > AREA_SHARE {
            errors.push(format!("tip {tname} contact {lcd:.2} mm, is {ld:.2}"));
        }
        let lower_truth =
            if is_dict(&ts["base"]) { get(&ts["base"], "lower").filter(|x| truthy(Some(x))) } else { None };
        if let Some(lt) = lower_truth {
            let other = if swapped { Some(&sp["tip"]) } else { get(sp, "lower") }.filter(|x| truthy(Some(x)));
            match other {
                None => errors.push(format!("{tname} has no lower tip")),
                Some(o) => {
                    let dd = (arr(&o["contact"]) - arr(&lt["contact"])).norm();
                    if dd > CONTACT_POS_MM {
                        errors.push(format!("{tname} lower contact off by {dd:.2} mm"));
                    }
                    let (ocd, lld) = (f(&o["contact_d"]), loop_d(lt));
                    let r = ocd / lld;
                    if (r * r - 1.0).abs() > AREA_SHARE {
                        errors.push(format!("{tname} lower contact {ocd:.2} mm, is {lld:.2}"));
                    }
                }
            }
        }
        let (ssd, tsd) = (f(&sp["shaft_d"]), f(&ts["shaft_d"]));
        let sd = (ssd - tsd).abs();
        if sd > WIDTH_MM + 0.05 && !alt && !tree_free {
            errors.push(format!("{tname} shaft {ssd:.2} mm, is {tsd:.2}"));
        }
        if !alt && !tree_free {
            errors.extend(widths_check(tname, get(ts, "seg_d"), get(sp, "seg_d")));
        }
    }
    let n_sup = sup.len();
    let mut post_names: Vec<(usize, String)> = Vec::new();
    let tposts = list(&tl["posts"]);
    for (i, p) in list(&lay["posts"]).iter().enumerate() {
        for tp in tposts {
            let tn = s(&tp["name"]);
            if post_names.iter().any(|x| x.1 == tn) {
                continue;
            }
            if norm2(xy(&p["foot"]), xy(&tp["foot"])) <= 0.6 && (f(&p["top"][2]) - f(&tp["top"][2])).abs() <= 1.5 {
                post_names.push((i, tn.to_string()));
                break;
            }
        }
    }
    for tp in tposts {
        let tn = s(&tp["name"]);
        if !post_names.iter().any(|x| x.1 == tn) && !alt {
            errors.push(format!("post {tn} missing"));
        }
    }
    for (i, nm) in &post_names {
        let tp = tposts.iter().find(|x| s(&x["name"]) == nm).unwrap();
        if !alt {
            errors.extend(widths_check(&format!("post {nm}"), get(tp, "seg_d"), get(&lay["posts"][*i], "seg_d")));
        }
    }
    let pillar_name = |i: usize| -> String {
        if i < n_sup {
            names.iter().find(|x| x.0 == i).map(|x| x.1.clone()).unwrap_or(format!("?{i}"))
        } else {
            post_names.iter().find(|x| x.0 == i - n_sup).map(|x| x.1.clone()).unwrap_or(format!("?post{}", i - n_sup))
        }
    };
    for ts in list(&tl["supports"]) {
        let tname = s(&ts["name"]);
        let li = names.iter().find(|x| x.1 == tname).map(|x| x.0);
        let Some(li) = li else { continue };
        if alt || tree_free {
            continue;
        }
        let sp = &sup[li];
        let b = &ts["base"];
        let lb = &sp["base"];
        let pidx = |v: &Value| v["parent"].as_u64().unwrap() as usize;
        if has(b, "plate") {
            if !base_has(lb, "plate") {
                let has_ = if lb.as_str() == Some("model") {
                    "model foot".to_string()
                } else {
                    format!("parent {}", pillar_name(pidx(lb)))
                };
                errors.push(format!("{tname} should stand on the plate, has {has_}"));
            } else {
                let d = norm2(xy(&lb["plate"]), xy(&b["plate"]));
                if d > 0.5 {
                    errors.push(format!("{tname} foot off by {d:.2} mm"));
                }
            }
        } else if has(b, "parent") {
            if !base_has(lb, "parent") {
                errors.push(format!(
                    "{tname} should hang from {}, has {}",
                    s(&b["parent_name"]),
                    if is_dict(lb) { "plate" } else { "model foot" }
                ));
            } else if pillar_name(pidx(lb)) != s(&b["parent_name"]) {
                errors.push(format!(
                    "{tname} hangs from {}, should be {}",
                    pillar_name(pidx(lb)),
                    s(&b["parent_name"])
                ));
            } else {
                let d = (arr(&lb["at"]) - arr(&b["at"])).norm();
                if d > 1.0 {
                    errors.push(format!("{tname} attach off by {d:.2} mm"));
                }
            }
        } else if lb.as_str() != Some("model") {
            errors.push(format!("{tname} should be a strut"));
        }
        let line: Vec<V3> = list(&ts["line"]).iter().map(arr).collect();
        let lsp = linspace(0.0, 1.0, 9);
        let pts: Vec<V3> = line.windows(2).flat_map(|w| lsp.iter().map(move |&x| w[0] + (w[1] - w[0]) * x)).collect();
        let mut lline: Vec<V3> = Vec::new();
        if base_has(lb, "plate") {
            lline.push(V3([f(&lb["plate"][0]), f(&lb["plate"][1]), 2.0]));
        } else if base_has(lb, "at") {
            lline.push(arr(&lb["at"]));
        } else if truthy(get(sp, "lower")) {
            lline.push(arr(&sp["lower"]["socket"]));
        }
        lline.extend(list(&sp["joints"]).iter().map(arr));
        lline.push(arr(&sp["tip"]["socket"]));
        if lline.len() >= 2 {
            let dev = poly_dist(&pts, &lline).into_iter().fold(f64::NEG_INFINITY, f64::max);
            if dev > LINE_MM + 0.1 {
                errors.push(format!("{tname} line off by {dev:.2} mm"));
            }
        }
    }
    let extra: Vec<usize> = (0..n_sup).filter(|i| !used.contains(i)).collect();
    if !extra.is_empty() {
        errors.push(format!(
            "{} support(s) not in truth, contact {}",
            extra.len(),
            round_list(arr(&sup[extra[0]]["tip"]["contact"]))
        ));
    }
    let lbr = list(&lay["braces"]);
    let mut lb_used: Vec<usize> = Vec::new();
    for tb in list(&tl["braces"]) {
        let bname = s(&tb["name"]);
        let (a, b) = (arr(&tb["lower"][2]), arr(&tb["upper"][2]));
        let (mut best, mut bd): (Option<usize>, f64) = (None, 1e9);
        for (i, lb) in lbr.iter().enumerate() {
            if lb_used.contains(&i) {
                continue;
            }
            let (la, lb2) = (arr(&lb["lower"][2]), arr(&lb["upper"][2]));
            let d = (la - a).norm().max((lb2 - b).norm()).min((la - b).norm().max((lb2 - a).norm()));
            if d < bd {
                (best, bd) = (Some(i), d);
            }
        }
        let hosts_any = truthy(get(tb, "hosts_any"));
        let reach = if hosts_any { 2.5 } else { BRACE_END_MM };
        let Some(bi) = best.filter(|_| bd <= reach) else {
            let near = if best.is_some() { format!(" (nearest {bd:.2} mm off)") } else { String::new() };
            errors.push(format!("brace {bname} missing{near}"));
            continue;
        };
        lb_used.push(bi);
        let lb = &lbr[bi];
        let hosts = vec![
            pillar_name(lb["lower"][0].as_u64().unwrap() as usize),
            pillar_name(lb["upper"][0].as_u64().unwrap() as usize),
        ];
        let (ln, un) = (s(&tb["lower_name"]).to_string(), s(&tb["upper_name"]).to_string());
        let wanted = vec![ln.clone(), un.clone()];
        let ok = if hosts_any {
            let any: Vec<String> = list(&tb["hosts_any"]).iter().map(|x| s(x).to_string()).collect();
            (hosts.contains(&ln) || hosts.contains(&un)) && hosts.iter().any(|h| any.contains(h))
        } else {
            let mut h = hosts.clone();
            let mut w = wanted.clone();
            h.sort();
            h.dedup();
            w.sort();
            w.dedup();
            h == w
        };
        if !ok {
            errors.push(format!("brace {bname} joins {}, should be {}", py_str_list(hosts), py_str_list(wanted)));
        }
        let (ld, td) = (f(&lb["d"]), f(&tb["d"]));
        if (ld - td).abs() > WIDTH_MM {
            errors.push(format!("brace {bname} {ld:.2} mm wide, is {td:.2}"));
        }
    }
    for tk in list(&tl["kickstands"]) {
        let kname = s(&tk["name"]);
        let hit = list(&lay["kickstands"]).iter().find(|lk| norm2(xy(&lk["foot"]), xy(&tk["foot"])) <= 0.75);
        let Some(hit) = hit else {
            errors.push(format!("kickstand {kname} missing"));
            continue;
        };
        let holder = pillar_name(hit["support"].as_u64().unwrap() as usize);
        if holder != s(&tk["host_name"]) {
            errors.push(format!("kickstand {kname} holds {holder}, should be {}", s(&tk["host_name"])));
        }
        errors.extend(widths_check(&format!("kickstand {kname}"), get(tk, "seg_d"), get(hit, "seg_d")));
    }
    let counts: serde_json::Map<String, Value> =
        COUNT_KEYS.iter().zip(&got).map(|(k, v)| (k.to_string(), json!(v))).collect();
    json!({"pass": errors.is_empty(), "errors": errors, "counts": counts, "want": t["counts"],
           "names": names.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>()})
}

// ---- Generator validation: truth against the mesh it made ----

/// What validate hands on to stage 4.
pub struct Validated {
    pub errors: Vec<String>,
    pub loops: Vec<(String, Loop)>,
    pub tmids: Vec<V3>,
    pub truth_clip_mm: f64,
}

/// score.validate. `t` gets "truth_clip_mm" set, as the Python does. isect, touch and dist are
/// the harness measure outputs for the truth support mesh.
pub fn validate(t: &mut Value, isect: &[Seg], touch: &[Seg], dist: &[f32]) -> Validated {
    let mut errors: Vec<String> = Vec::new();
    let far =
        if dist.is_empty() { 0.0 } else { dist.iter().filter(|&&d| d > 0.05f32).count() as f64 / dist.len() as f64 };
    if far > 0.02 {
        errors.push(format!("truth surface {:.1}% off the mesh", 100.0 * far));
    }
    let report = t.get("mesh_report").cloned().unwrap_or(Value::Null);
    if truthy(get(&report, "union_open_edges")) {
        errors.push(format!("union has {} open edges", py_str(&report["union_open_edges"])));
    }
    if let Some(o) = report.as_object() {
        for (k, v) in o {
            if k.starts_with("weld:") && truthy(get(v, "open")) {
                errors.push(format!("{k} has {} open edges", py_str(&v["open"])));
            }
        }
    }
    let (a, bb) = seg_ends(isect);
    let (mids, lens) = mids_lens(&a, &bb);
    let allowed = ["tip", "micro", "contact_rod", "floating", "ball"];
    let parts = list(&t["parts"]).to_vec();
    let mut clip = 0.0;
    if !isect.is_empty() {
        let gaps: Vec<Vec<f64>> = parts.iter().map(|p| part_gap(&mids, p)).collect();
        for i in 0..isect.len() {
            let row: Vec<f64> = gaps.iter().map(|g| g[i]).collect();
            let nearest = argmin(&row);
            if !allowed.contains(&s(&parts[nearest]["type"])) {
                let near_tip = parts.iter().enumerate().any(|(k, p)| allowed.contains(&s(&p["type"])) && row[k] <= 0.1);
                if !near_tip {
                    clip += lens[i];
                }
            }
        }
    }
    t["truth_clip_mm"] = json!(clip);
    if clip > CLIP_MM && !truthy(get(t, "passes_through")) {
        errors.push(format!("truth's non-tip parts clip the model: {clip:.2} mm"));
    }
    let tips: Vec<Value> = list(&t["tips"]).iter().filter(|x| truthy(get(x, "exported"))).cloned().collect();
    let contacts: Vec<V3> = tips.iter().map(|x| arr(&x["contact"])).collect();
    let (ta, tb) = seg_ends(touch);
    let (tmids, _) = mids_lens(&ta, &tb);
    let mut loops = Vec::new();
    let sides = t["params"]["sides"].as_f64().unwrap();
    for tip in &tips {
        let name = s(&tip["name"]);
        let lp = tip_loop(tip, &tmids, &ta, &tb, &contacts);
        let Some(lp) = lp.filter(|l| l.d >= 0.05) else {
            if truthy(get(tip, "contact_d")) && s(&t["params"]["base"]) != "p07_bury0" {
                errors.push(format!("truth tip {name} has no loop"));
            }
            continue;
        };
        let cos = arr(&tip["dir"]).dot(arr(&tip["normal"])).abs();
        let factor = sides * (std::f64::consts::PI / sides).sin() / std::f64::consts::PI;
        let cd = f(&tip["contact_d"]);
        if cos > 0.999 && (lp.d - cd * factor).abs() > 0.06 {
            errors.push(format!("truth tip {name} loop {:.3} mm against analytic {:.3}", lp.d, cd * factor));
        }
        if (lp.centre - arr(&tip["contact"])).norm() > 0.03 + if cos < 0.999 { 0.3 } else { 0.0 } {
            errors.push(format!("truth tip {name} loop centre off"));
        }
        loops.push((name.to_string(), lp));
    }
    Validated { errors, loops, tmids, truth_clip_mm: clip }
}

// ---- Stage 4: the owner's two checks, on any built support mesh ----

/// score.turn_back: points of the turned case back to the generator's frame.
pub fn turn_back(t: &Value, p: V3) -> V3 {
    let a = f(&t["rot"]).to_radians();
    let (c, s) = (a.cos(), a.sin());
    gemm_row(p, &[[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]])
}

/// Stage 4 (4a clips, 4b loops) on a build given as its triangles plus their `.isect` and `.touch`
/// segments against the case model. `labels` names the build element of each triangle for the
/// clip report (pass and fail do not depend on it).
pub fn stage4(
    t: &Value,
    tris: &[[V3; 3]],
    isect: &[Seg],
    touch: &[Seg],
    labels: &dyn Fn(u32) -> String,
    v: &Validated,
) -> Value {
    let (a, bb) = seg_ends(isect);
    let (mids, lens) = mids_lens(&a, &bb);
    let parts = list(&t["parts"]);
    let tips_all = list(&t["tips"]);
    let mut tip_parts: Vec<&Value> = tips_all
        .iter()
        .filter(|x| !x["part"].is_null())
        .map(|x| &parts[x["part"].as_u64().unwrap() as usize])
        .collect();
    let tip_balls: Vec<String> = tips_all.iter().map(|x| format!("{}-ball", s(&x["name"]))).collect();
    tip_parts.extend(parts.iter().filter(|p| s(&p["type"]) == "ball" && tip_balls.iter().any(|b| b == s(&p["name"]))));
    let micro_parts: Vec<&Value> =
        parts.iter().filter(|p| ["micro", "contact_rod", "floating"].contains(&s(&p["type"]))).collect();
    let bare: Vec<(V3, V3, f64)> = tips_all
        .iter()
        .filter(|x| x["part"].is_null())
        .map(|x| (arr(&x["contact"]), arr(&x["normal"]), f(&x["contact_d"]) / 2.0))
        .collect();
    let mut clip = vec![false; isect.len()];
    if !isect.is_empty() {
        let mut near = vec![false; isect.len()];
        for p in &tip_parts {
            for (n, g) in near.iter_mut().zip(part_gap(&mids, p)) {
                *n |= g <= 0.1;
            }
        }
        for p in &micro_parts {
            for (n, g) in near.iter_mut().zip(part_gap(&mids, p)) {
                *n |= g <= 0.15;
            }
        }
        for &(c0, n0, r0) in &bare {
            let rel: Vec<V3> = mids.iter().map(|&m| m - c0).collect();
            let al = gemv_rows(&rel, n0);
            for i in 0..near.len() {
                near[i] |= al[i].abs() <= 0.5 && (rel[i] - n0 * al[i]).norm() <= r0 + 0.1;
            }
        }
        clip = near.iter().map(|x| !x).collect();
    }
    let clip_lens: Vec<f64> = lens.iter().zip(&clip).filter(|x| *x.1).map(|x| *x.0).collect();
    let clip_mm = pairwise_sum(&clip_lens);
    // Built parts wholly inside the model cross nothing, so count their area.
    let shapes = &t["shapes"];
    let mut buried = Vec::new();
    {
        let corners: Vec<V3> = tris.iter().flat_map(|tr| tr.iter().map(|&p| turn_back(t, p))).collect();
        let inside = inside_shapes(shapes, &corners);
        let ins: Vec<&[V3; 3]> = tris
            .iter()
            .enumerate()
            .filter(|(k, _)| inside[3 * k] && inside[3 * k + 1] && inside[3 * k + 2])
            .map(|x| x.1)
            .collect();
        let mid3: Vec<V3> = ins.iter().map(|tr| (tr[0] + tr[1] + tr[2]) / 3.0).collect();
        let mut keep = vec![true; ins.len()];
        if !ins.is_empty() {
            for p in &tip_parts {
                for (k, g) in keep.iter_mut().zip(part_gap(&mid3, p)) {
                    *k &= g > 0.1;
                }
            }
            for p in &micro_parts {
                for (k, g) in keep.iter_mut().zip(part_gap(&mid3, p)) {
                    *k &= g > 0.15;
                }
            }
            for &(c0, _, r0) in &bare {
                for (k, m) in keep.iter_mut().zip(&mid3) {
                    *k &= (*m - c0).norm() > r0 + 0.5;
                }
            }
        }
        for (tr, k) in ins.iter().zip(&keep) {
            if *k {
                buried.push((tr[1] - tr[0]).cross(tr[2] - tr[0]).norm());
            }
        }
    }
    let buried_area = if buried.is_empty() { 0.0 } else { pairwise_sum(&buried) / 2.0 };
    let mut errors: Vec<String> = Vec::new();
    let mut who: Vec<(String, f64)> = Vec::new();
    for i in (0..isect.len()).filter(|&i| clip[i]) {
        let e = labels(isect[i].0);
        match who.iter_mut().find(|w| w.0 == e) {
            Some(w) => w.1 += lens[i],
            None => who.push((e, lens[i])),
        }
    }
    let allowed = v.truth_clip_mm;
    let tba = t.get("truth_buried_area").and_then(|x| x.as_f64()).unwrap_or(0.0);
    let clip_pass = clip_mm <= CLIP_MM + allowed * 1.2 && buried_area <= 0.05 + tba * 1.2;
    if !clip_pass {
        let mut top = "buried".to_string();
        let mut best = f64::NEG_INFINITY;
        for (k, x) in &who {
            if *x > best {
                best = *x;
                top = k.clone();
            }
        }
        let at = clip.iter().position(|&x| x).map(|i| round_list(mids[i])).unwrap_or("None".into());
        errors.push(format!("clips the model: {clip_mm:.2} mm of {top} near {at}, {buried_area:.2} mm2 buried"));
    }
    let (ta, tb) = seg_ends(touch);
    let (tmids, tlens) = mids_lens(&ta, &tb);
    let tips: Vec<&Value> = tips_all.iter().filter(|x| truthy(get(x, "exported"))).collect();
    let contacts: Vec<V3> = tips.iter().map(|x| arr(&x["contact"])).collect();
    let (mut worst_pos, mut worst_size, mut missing) = (0.0f64, 0.0f64, 0usize);
    let mut assigned = vec![false; touch.len()];
    let mut loops = serde_json::Map::new();
    for tip in &tips {
        let name = s(&tip["name"]);
        let lp = tip_loop(tip, &tmids, &ta, &tb, &contacts);
        let tl = v.loops.iter().find(|x| x.0 == name).map(|x| &x.1);
        let td = tl.map(|l| l.d).unwrap_or(f(&tip["contact_d"]));
        let tc = tl.map(|l| l.centre).unwrap_or(arr(&tip["contact"]));
        let Some(lp) = lp.filter(|l| l.d >= 0.05) else {
            missing += 1;
            errors.push(format!("tip {name}: no contact loop"));
            continue;
        };
        for (x, m) in assigned.iter_mut().zip(&lp.mask) {
            *x |= *m;
        }
        let pos = (lp.centre - tc).norm();
        let size = lp.d - td;
        loops.insert(name.into(), json!({"d": lp.d, "truth_d": td, "pos": pos}));
        worst_pos = worst_pos.max(pos);
        if size.abs() > worst_size.abs() {
            worst_size = size;
        }
        if pos > LOOP_POS_MM {
            errors.push(format!("tip {name}: contact {pos:.2} mm off"));
        }
        let r = lp.d / td;
        if (r * r - 1.0).abs() > AREA_SHARE {
            errors.push(format!("tip {name}: loop {:.2} mm, is {td:.2}", lp.d));
        }
    }
    let mut extra = 0.0;
    if !touch.is_empty() && assigned.iter().any(|x| !x) {
        let clipped: Vec<V3> = mids.iter().zip(&clip).filter(|x| *x.1).map(|x| *x.0).collect();
        let near = |set: &[V3], m: V3| {
            !set.is_empty() && set.iter().map(|&x| (x - m).norm()).fold(f64::INFINITY, f64::min) <= 0.3
        };
        let far: Vec<f64> = (0..touch.len())
            .filter(|&i| !assigned[i] && !(near(&v.tmids, tmids[i]) || near(&clipped, tmids[i])))
            .map(|i| tlens[i])
            .collect();
        extra = pairwise_sum(&far) / 2.0;
    }
    if extra > EXTRA_MM {
        errors.push(format!("{extra:.2} mm of contact truth doesn't have"));
    }
    let loop_pass =
        missing == 0 && worst_pos <= LOOP_POS_MM && !errors.iter().any(|x| x.contains("loop")) && extra <= EXTRA_MM;
    json!({"pass": clip_pass && loop_pass, "clip_pass": clip_pass, "loop_pass": loop_pass, "errors": errors, "clip_mm": clip_mm,
           "buried_area": buried_area, "clip_who": who.iter().map(|(k, x)| (k.clone(), json!(x))).collect::<serde_json::Map<_, _>>(),
           "missing": missing, "worst_pos": worst_pos, "worst_size": worst_size, "extra_mm": extra, "loops": loops})
}
