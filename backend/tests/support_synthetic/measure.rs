//! Port of the harness's `measure` (synthetic/harness/src/main.rs): the intersection segments of a
//! support mesh with the case's model, the same against the model moved both ways along its
//! normals (the touch test, `.touch`), and the distance from the truth samples to the support mesh.
//! Triangles come in as f32 (as read from STL) and are widened to f64, as the harness does.

use rayon::prelude::*;

pub type P = [f64; 3];
pub type Tri32 = [[f32; 3]; 3];
/// One segment as written to `.isect` / `.touch`: support triangle index, then both ends (f32).
pub type Seg = (u32, [f32; 3], [f32; 3]);

/// How far the model is moved along its normals, both ways, when looking for contact loops.
pub const TOUCH_MM: f64 = 0.003;

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P, b: P) -> P {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dd(a: P, b: P) -> f64 {
    let d = sub(a, b);
    dot(d, d)
}

/// An STL triangle widened to f64.
pub fn widen(t: &Tri32) -> [P; 3] {
    t.map(|v| v.map(f64::from))
}

/// Where the segment p-q crosses the triangle, if it does.
pub fn seg_tri(p: P, q: P, t: &[P; 3]) -> Option<P> {
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let d = sub(q, p);
    let h = cross(d, e2);
    let a = dot(e1, h);
    if a.abs() < 1e-14 {
        return None;
    }
    let s = sub(p, t[0]);
    let u = dot(s, h) / a;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let qv = cross(s, e1);
    let w = dot(d, qv) / a;
    if w < -1e-9 || u + w > 1.0 + 1e-9 {
        return None;
    }
    let l = dot(e2, qv) / a;
    if !(0.0..=1.0).contains(&l) {
        return None;
    }
    Some([p[0] + d[0] * l, p[1] + d[1] * l, p[2] + d[2] * l])
}

/// The longest segment between the edge crossings of two triangles, if longer than 1e-5.
pub fn tri_tri(a: &[P; 3], b: &[P; 3]) -> Option<(P, P)> {
    let mut pts: Vec<P> = Vec::new();
    for k in 0..3 {
        if let Some(x) = seg_tri(a[k], a[(k + 1) % 3], b) {
            pts.push(x);
        }
        if let Some(x) = seg_tri(b[k], b[(k + 1) % 3], a) {
            pts.push(x);
        }
    }
    if pts.len() < 2 {
        return None;
    }
    let (mut best, mut d) = ((pts[0], pts[1]), -1.0);
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            let x = dd(pts[i], pts[j]);
            if x > d {
                d = x;
                best = (pts[i], pts[j]);
            }
        }
    }
    (d > 1e-10).then_some(best)
}

/// Squared distance from p to a triangle.
pub fn closest_sq(p: P, t: &[P; 3]) -> f64 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return dot(ap, ap);
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return dot(bp, bp);
    }
    let vc = d1 * d4 - d3 * d2;
    let at = |u: f64, w: f64| {
        dd(p, [a[0] + ab[0] * u + ac[0] * w, a[1] + ab[1] * u + ac[1] * w, a[2] + ab[2] * u + ac[2] * w])
    };
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return at(d1 / (d1 - d3), 0.0);
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return dot(cp, cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return at(0.0, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return dd(p, [b[0] + (c[0] - b[0]) * w, b[1] + (c[1] - b[1]) * w, b[2] + (c[2] - b[2]) * w]);
    }
    let den = 1.0 / (va + vb + vc);
    at(vb * den, vc * den)
}

pub fn bbox(t: &[P; 3]) -> (P, P) {
    let mut lo = t[0];
    let mut hi = t[0];
    for p in t {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

fn to_seg(i: usize, a: P, b: P) -> Seg {
    (i as u32, a.map(|x| x as f32), b.map(|x| x as f32))
}

/// Intersection segments of every support triangle with every model triangle whose box it meets,
/// in support order, then model order (as rayon's collect keeps it).
pub fn segments(sup: &[[P; 3]], model: &[[P; 3]]) -> Vec<Seg> {
    let boxes: Vec<_> = model.iter().map(bbox).collect();
    sup.par_iter()
        .enumerate()
        .flat_map_iter(|(i, s)| {
            let (lo, hi) = bbox(s);
            let mut out = Vec::new();
            for (m, (ml, mh)) in model.iter().zip(&boxes) {
                if (0..3).all(|k| lo[k] <= mh[k] && ml[k] <= hi[k])
                    && let Some((a, b)) = tri_tri(s, m)
                {
                    out.push(to_seg(i, a, b));
                }
            }
            out
        })
        .collect()
}

/// The model moved TOUCH_MM along its normals, and back: each triangle twice (+, then -).
pub fn shifted_model(model: &[[P; 3]]) -> Vec<[P; 3]> {
    model
        .iter()
        .flat_map(|t| {
            let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            let l = dot(n, n).sqrt().max(1e-12);
            let n = [n[0] / l * TOUCH_MM, n[1] / l * TOUCH_MM, n[2] / l * TOUCH_MM];
            let mv = |t: &[P; 3], s: f64| t.map(|p| [p[0] + n[0] * s, p[1] + n[1] * s, p[2] + n[2] * s]);
            [mv(t, 1.0), mv(t, -1.0)]
        })
        .collect()
}

/// `.isect`: support against model.
pub fn isect(sup: &[[P; 3]], model: &[[P; 3]]) -> Vec<Seg> {
    segments(sup, model)
}

/// `.touch`: support against the shifted model.
pub fn touch(sup: &[[P; 3]], model: &[[P; 3]]) -> Vec<Seg> {
    segments(sup, &shifted_model(model))
}

/// `.dist`: the distance from each sample (f32 xyz) to the support mesh, capped at 9 mm.
pub fn dist(samples: &[[f32; 3]], sup: &[[P; 3]]) -> Vec<f32> {
    let sboxes: Vec<_> = sup.iter().map(bbox).collect();
    samples
        .par_iter()
        .map(|s| {
            let p = s.map(f64::from);
            let mut best = 9.0f64 * 9.0;
            for (t, (lo, hi)) in sup.iter().zip(&sboxes) {
                let mut lb = 0.0;
                for k in 0..3 {
                    let d = (lo[k] - p[k]).max(p[k] - hi[k]).max(0.0);
                    lb += d * d;
                }
                if lb < best {
                    best = best.min(closest_sq(p, t));
                }
            }
            best.sqrt() as f32
        })
        .collect()
}

/// The three outputs of `measure` for one support mesh.
pub struct Measured {
    pub isect: Vec<Seg>,
    pub touch: Vec<Seg>,
    pub dist: Vec<f32>,
}

pub fn measure(model: &[Tri32], support: &[Tri32], samples: &[[f32; 3]]) -> Measured {
    let model: Vec<[P; 3]> = model.iter().map(widen).collect();
    let sup: Vec<[P; 3]> = support.iter().map(widen).collect();
    Measured { isect: isect(&sup, &model), touch: touch(&sup, &model), dist: dist(samples, &sup) }
}

// ---- file io (the harness's formats) ----

/// Triangles of a binary STL (vertices only).
pub fn read_stl(path: &std::path::Path) -> std::io::Result<Vec<Tri32>> {
    let b = std::fs::read(path)?;
    let n = u32::from_le_bytes(b[80..84].try_into().unwrap()) as usize;
    let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    Ok((0..n)
        .map(|i| {
            let base = 84 + i * 50 + 12;
            [0, 1, 2].map(|k| [f(base + k * 12), f(base + k * 12 + 4), f(base + k * 12 + 8)])
        })
        .collect())
}

pub fn read_segments(path: &std::path::Path) -> std::io::Result<Vec<Seg>> {
    let b = std::fs::read(path)?;
    let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    Ok((0..b.len() / 28)
        .map(|i| {
            let o = i * 28;
            (
                u32::from_le_bytes(b[o..o + 4].try_into().unwrap()),
                [f(o + 4), f(o + 8), f(o + 12)],
                [f(o + 16), f(o + 20), f(o + 24)],
            )
        })
        .collect())
}

pub fn write_segments(path: &std::path::Path, segs: &[Seg]) -> std::io::Result<()> {
    let mut bytes = Vec::with_capacity(segs.len() * 28);
    for (i, a, b) in segs {
        bytes.extend_from_slice(&i.to_le_bytes());
        for c in a.iter().chain(b) {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
    }
    std::fs::write(path, bytes)
}

pub fn read_f32s(path: &std::path::Path) -> std::io::Result<Vec<f32>> {
    let b = std::fs::read(path)?;
    Ok(b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
}

pub fn read_samples(path: &std::path::Path) -> std::io::Result<Vec<[f32; 3]>> {
    let v = read_f32s(path)?;
    Ok(v.as_chunks::<3>().0.to_vec())
}
