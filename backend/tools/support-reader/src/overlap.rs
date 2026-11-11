// Copied from the prototype's overlap tool (scratchpad overlap/src/main.rs) with only its entry point
// changed; the algorithm and tolerances are the prototype's.
//
// overlap: volume overlap between two closed triangle meshes (the original support mesh A and the
// support mesh DragonFruit built, B), split by inside/outside a third mesh (the model M) and by a
// raft band. No voxel grid: rays along each axis in turn on a fine jittered lattice (dexels), the three averaged. Along each ray the solid
// intervals of A, B and M are exact (signed crossings, winding number != 0 is inside, so overlapping
// shells are a union); only XY is sampled, at `pitch` (default 0.025 mm).
//
// usage: support-reader overlap A.stl B.stl M.stl [--pitch 0.025] [--raft Z] [--labels A.u8] [--names a,b,c]
//                [--diff out.json] [--cell 0.25] [--axis all|z|x|y]
//   --raft Z      the band z < Z is reported apart (headline excludes it)
//   --labels      one byte per triangle of A; A's volume and missed volume are reported per label
//   --diff        missed (A not B) and excess (B not A) volume per coarse cell, for rendering
// Output: one JSON object on stdout. Volumes in mm3.
use rayon::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

type P = [f64; 3];

fn read_stl(path: &str) -> Vec<[P; 3]> {
    if path == "-" {
        return Vec::new();
    }
    let b = std::fs::read(Path::new(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    if b.len() < 84 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(b[80..84].try_into().unwrap()) as usize;
    let f = |at: usize| f64::from(f32::from_le_bytes(b[at..at + 4].try_into().unwrap()));
    (0..n)
        .map(|i| {
            let base = 84 + i * 50 + 12;
            [0, 1, 2].map(|v| [f(base + v * 12), f(base + v * 12 + 4), f(base + v * 12 + 8)])
        })
        .collect()
}

#[derive(Clone, Copy)]
struct Ev {
    ray: u32,
    z: f32,
    d: i8,
    src: u8, // 0 A, 1 B, 2 M
    label: u8,
}

const TILE: i64 = 128;
/// Where ray (i, j) sits inside its lattice cell, as fractions: a fixed pseudo-random jitter (stratified sampling),
/// so thin axis-aligned pieces are not aliased by a regular lattice and rays do not run along axis-aligned walls.
#[inline]
fn jit(i: i64, j: i64) -> (f64, f64) {
    let mut h = (i as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ (j as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 32;
    (((h & 0xFFFFFF) as f64 + 0.5) / 16777216.0, (((h >> 24) & 0xFFFFFF) as f64 + 0.5) / 16777216.0)
}

/// Edge function with a canonical vertex order, so the two triangles sharing an edge agree exactly.
fn edge(ax: f64, ay: f64, bx: f64, by: f64, px: f64, py: f64) -> f64 {
    if (ax, ay) <= (bx, by) {
        (bx - ax) * (py - ay) - (by - ay) * (px - ax)
    } else {
        -((ax - bx) * (py - by) - (ay - by) * (px - bx))
    }
}

struct Mesh {
    tris: Vec<[P; 3]>,
    tiles: HashMap<(i64, i64), Vec<u32>>,
}

fn tile_index(tris: &[[P; 3]], pitch: f64) -> HashMap<(i64, i64), Vec<u32>> {
    let mut m: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
    let w = TILE as f64 * pitch;
    for (i, t) in tris.iter().enumerate() {
        let x0 = t.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
        let x1 = t.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
        let y0 = t.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        let y1 = t.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        for tx in ((x0 - pitch) / w - 0.01).floor() as i64..=((x1 + pitch) / w + 0.01).floor() as i64 {
            for ty in ((y0 - pitch) / w - 0.01).floor() as i64..=((y1 + pitch) / w + 0.01).floor() as i64 {
                m.entry((tx, ty)).or_default().push(i as u32);
            }
        }
    }
    m
}

fn raster(t: &[P; 3], tx: i64, ty: i64, pitch: f64, src: u8, label: u8, mask: Option<&[bool]>, out: &mut Vec<Ev>) {
    let area = (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0]);
    if area == 0.0 {
        return;
    }
    let x0 = t.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
    let x1 = t.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
    let y0 = t.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
    let y1 = t.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
    let i0 = ((x0 / pitch).floor() as i64 - 1).max(tx * TILE);
    let i1 = ((x1 / pitch).floor() as i64).min(tx * TILE + TILE - 1);
    let j0 = ((y0 / pitch).floor() as i64 - 1).max(ty * TILE);
    let j1 = ((y1 / pitch).floor() as i64).min(ty * TILE + TILE - 1);
    let s = if area > 0.0 { 1.0 } else { -1.0 };
    // A ray going up leaves the solid through an up-facing triangle (area > 0, counter-clockwise from above).
    let d: i8 = if area > 0.0 { -1 } else { 1 };
    for j in j0..=j1 {
        for i in i0..=i1 {
            let ray = ((j - ty * TILE) * TILE + (i - tx * TILE)) as usize;
            if let Some(m) = mask
                && !m[ray]
            {
                continue;
            }
            let (jx, jy) = jit(i, j);
            let (px, py) = ((i as f64 + jx) * pitch, (j as f64 + jy) * pitch);
            let e0 = edge(t[0][0], t[0][1], t[1][0], t[1][1], px, py) * s;
            let e1 = edge(t[1][0], t[1][1], t[2][0], t[2][1], px, py) * s;
            let e2 = edge(t[2][0], t[2][1], t[0][0], t[0][1], px, py) * s;
            // Half-open: a point exactly on an edge belongs to the triangle whose inside is the positive side of
            // the edge function in canonical vertex order, so the two triangles sharing the edge never both take it.
            let in0 = ((t[0][0], t[0][1]) <= (t[1][0], t[1][1])) == (s > 0.0);
            let in1 = ((t[1][0], t[1][1]) <= (t[2][0], t[2][1])) == (s > 0.0);
            let in2 = ((t[2][0], t[2][1]) <= (t[0][0], t[0][1])) == (s > 0.0);
            if e0 < 0.0 || e1 < 0.0 || e2 < 0.0 {
                continue;
            }
            if (e0 == 0.0 && !in0) || (e1 == 0.0 && !in1) || (e2 == 0.0 && !in2) {
                continue;
            }
            let sum = e0 + e1 + e2;
            if sum <= 0.0 {
                continue;
            }
            // e1 weights vertex 0, e2 vertex 1, e0 vertex 2.
            let z = (e1 * t[0][2] + e2 * t[1][2] + e0 * t[2][2]) / sum;
            out.push(Ev { ray: ray as u32, z: z as f32, d, src, label });
        }
    }
}

#[derive(Default, Clone)]
struct Acc {
    // [band: 0 above raft, 1 raft][model: 0 outside, 1 inside][0 A only, 1 B only, 2 both]
    v: [[[f64; 3]; 2]; 2],
    a_label: Vec<f64>,
    miss_label: Vec<f64>,
    leaky: [u64; 3],
    rays: u64,
    diff: HashMap<(i32, i32, i32), [f32; 2]>,
}

impl Acc {
    fn merge(mut self, o: Acc) -> Acc {
        for b in 0..2 {
            for m in 0..2 {
                for k in 0..3 {
                    self.v[b][m][k] += o.v[b][m][k];
                }
            }
        }
        if self.a_label.len() < o.a_label.len() {
            self.a_label.resize(o.a_label.len(), 0.0);
            self.miss_label.resize(o.a_label.len(), 0.0);
        }
        for (i, x) in o.a_label.iter().enumerate() {
            self.a_label[i] += x;
            self.miss_label[i] += o.miss_label[i];
        }
        for k in 0..3 {
            self.leaky[k] += o.leaky[k];
        }
        self.rays += o.rays;
        for (k, v) in o.diff {
            let e = self.diff.entry(k).or_default();
            e[0] += v[0];
            e[1] += v[1];
        }
        self
    }
}

fn run_axis(
    ax: &str,
    pos: &[String],
    pitch: f64,
    raft: f64,
    lab: &[u8],
    nlab: usize,
    want_diff: bool,
    cell: f64,
) -> Acc {
    // Rays run along the third coordinate after this permutation; the raft band stays in real Z.
    let perm: [usize; 3] = match ax {
        "x" => [1, 2, 0],
        "y" => [2, 0, 1],
        _ => [0, 1, 2],
    };
    let zc = perm.iter().position(|&k| k == 2).unwrap(); // where real Z sits after the permutation
    let load = |p: &str| -> Vec<[P; 3]> {
        read_stl(p).into_iter().map(|t| t.map(|v| [v[perm[0]], v[perm[1]], v[perm[2]]])).collect()
    };
    let meshes: Vec<Mesh> = pos
        .iter()
        .take(3)
        .map(|p| {
            let tris = load(p);
            let tiles = tile_index(&tris, pitch);
            Mesh { tris, tiles }
        })
        .collect();
    let mut tiles: Vec<(i64, i64)> = meshes[0].tiles.keys().chain(meshes[1].tiles.keys()).copied().collect();
    tiles.sort();
    tiles.dedup();
    let col = pitch * pitch;
    let total: Acc = tiles
        .par_iter()
        .map(|&(tx, ty)| {
            let mut acc = Acc { a_label: vec![0.0; nlab], miss_label: vec![0.0; nlab], ..Default::default() };
            let mut ev: Vec<Ev> = Vec::new();
            for (s, m) in meshes.iter().enumerate().take(2) {
                if let Some(l) = m.tiles.get(&(tx, ty)) {
                    for &ti in l {
                        let label = if s == 0 { lab.get(ti as usize).copied().unwrap_or(0) } else { 0 };
                        raster(&m.tris[ti as usize], tx, ty, pitch, s as u8, label, None, &mut ev);
                    }
                }
            }
            if ev.is_empty() {
                return acc;
            }
            let mut mask = vec![false; (TILE * TILE) as usize];
            for e in &ev {
                mask[e.ray as usize] = true;
            }
            if meshes.len() > 2
                && let Some(l) = meshes[2].tiles.get(&(tx, ty))
            {
                for &ti in l {
                    raster(&meshes[2].tris[ti as usize], tx, ty, pitch, 2, 0, Some(&mask), &mut ev);
                }
            }
            ev.sort_by(|a, b| a.ray.cmp(&b.ray).then(a.z.total_cmp(&b.z)));
            let mut wl = vec![0i32; nlab];
            let mut k = 0;
            while k < ev.len() {
                let ray = ev[k].ray;
                let mut e = k;
                while e < ev.len() && ev[e].ray == ray {
                    e += 1;
                }
                // Leak check: a closed mesh's crossings sum to zero on every ray.
                let mut sum = [0i32; 3];
                for x in &ev[k..e] {
                    sum[x.src as usize] += x.d as i32;
                }
                acc.rays += 1;
                if sum.iter().any(|&s| s != 0) {
                    for s in 0..3 {
                        if sum[s] != 0 {
                            acc.leaky[s] += 1;
                        }
                    }
                    k = e;
                    continue;
                }
                let (gi, gj) = (ray as i64 % TILE + tx * TILE, ray as i64 / TILE + ty * TILE);
                let (jx, jy) = jit(gi, gj);
                let (px, py) = ((gi as f64 + jx) * pitch, (gj as f64 + jy) * pitch);
                let mut w = [0i32; 3];
                wl.iter_mut().for_each(|x| *x = 0);
                for q in k..e {
                    let x = ev[q];
                    w[x.src as usize] += x.d as i32;
                    if x.src == 0 {
                        wl[x.label as usize] += x.d as i32;
                    }
                    if q + 1 == e {
                        break;
                    }
                    let (z0, z1) = (x.z as f64, ev[q + 1].z as f64);
                    if z1 <= z0 || (w[0] == 0 && w[1] == 0) {
                        continue;
                    }
                    let cls = if w[0] != 0 && w[1] != 0 {
                        2
                    } else if w[0] != 0 {
                        0
                    } else {
                        1
                    };
                    let inm = (w[2] != 0) as usize;
                    // The real height of this stretch: along the ray when rays are Z, else one point.
                    let add = |za: f64, zb: f64, acc: &mut Acc| {
                        if zb <= za {
                            return;
                        }
                        let real_z = match zc {
                            2 => 0.5 * (za + zb),
                            0 => px,
                            _ => py,
                        };
                        let band = (real_z < raft) as usize;
                        let vol = (zb - za) * col;
                        acc.v[band][inm][cls] += vol;
                        if band == 0 && w[0] != 0 {
                            let n = wl.iter().filter(|&&c| c != 0).count().max(1) as f64;
                            for (li, &c) in wl.iter().enumerate() {
                                if c != 0 {
                                    acc.a_label[li] += vol / n;
                                    if cls == 0 {
                                        acc.miss_label[li] += vol / n;
                                    }
                                }
                            }
                        }
                        if want_diff && band == 0 && cls != 2 {
                            let mut z = za;
                            while z < zb {
                                let zn = ((z / cell).floor() + 1.0) * cell;
                                let ze = zn.min(zb);
                                let mid = 0.5 * (z + ze);
                                let p = [px, py, mid];
                                let r = [
                                    p[perm.iter().position(|&k| k == 0).unwrap()],
                                    p[perm.iter().position(|&k| k == 1).unwrap()],
                                    p[zc],
                                ];
                                let key = (
                                    (r[0] / cell).floor() as i32,
                                    (r[1] / cell).floor() as i32,
                                    (r[2] / cell).floor() as i32,
                                );
                                acc.diff.entry(key).or_default()[cls] += ((ze - z) * col) as f32;
                                z = ze;
                            }
                        }
                    };
                    if zc == 2 && raft > z0 && raft < z1 {
                        add(z0, raft, &mut acc);
                        add(raft, z1, &mut acc);
                    } else {
                        add(z0, z1, &mut acc);
                    }
                }
                k = e;
            }
            acc
        })
        .reduce(Acc::default, Acc::merge);
    total
}

/// The `overlap` subcommand: `args` are the arguments after the subcommand name.
pub fn run(args: &[String]) {
    let mut pos = Vec::new();
    let (mut pitch, mut raft, mut labels, mut names, mut diff, mut cell, mut axis) =
        (0.025f64, f64::MIN, None::<String>, None::<String>, None::<String>, 0.25f64, "all".to_string());
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pitch" => {
                pitch = args[i + 1].parse().unwrap();
                i += 1
            }
            "--raft" => {
                raft = args[i + 1].parse().unwrap();
                i += 1
            }
            "--labels" => {
                labels = Some(args[i + 1].clone());
                i += 1
            }
            "--names" => {
                names = Some(args[i + 1].clone());
                i += 1
            }
            "--diff" => {
                diff = Some(args[i + 1].clone());
                i += 1
            }
            "--cell" => {
                cell = args[i + 1].parse().unwrap();
                i += 1
            }
            "--axis" => {
                axis = args[i + 1].clone();
                i += 1
            }
            x => pos.push(x.to_string()),
        }
        i += 1;
    }
    let lab: Vec<u8> = labels.as_ref().map(|p| std::fs::read(p).unwrap()).unwrap_or_default();
    let nlab = lab.iter().map(|&x| x as usize + 1).max().unwrap_or(1);
    let axes: Vec<&str> = if axis == "all" { vec!["z", "x", "y"] } else { vec![axis.as_str()] };
    let mut per_axis = Vec::new();
    let mut total = Acc::default();
    for ax in &axes {
        let t = run_axis(ax, &pos, pitch, raft, &lab, nlab, diff.is_some(), cell);
        let h = [t.v[0][0][0] + t.v[0][1][0], t.v[0][0][1] + t.v[0][1][1], t.v[0][0][2] + t.v[0][1][2]];
        per_axis.push(json!({"axis": ax, "coverage": h[2] / (h[0] + h[2]).max(1e-30), "fit": h[2] / (h[1] + h[2]).max(1e-30), "iou": h[2] / (h[0] + h[1] + h[2]).max(1e-30)}));
        total = total.merge(t);
    }
    // Volumes are the mean over the axes used.
    let na = axes.len() as f64;
    for b in 0..2 {
        for m in 0..2 {
            for k in 0..3 {
                total.v[b][m][k] /= na;
            }
        }
    }
    total.a_label.iter_mut().for_each(|x| *x /= na);
    total.miss_label.iter_mut().for_each(|x| *x /= na);
    total.diff.values_mut().for_each(|x| {
        x[0] /= na as f32;
        x[1] /= na as f32;
    });
    let stat = |v: &[f64; 3]| {
        let (a, b, both) = (v[0] + v[2], v[1] + v[2], v[2]);
        let uni = v[0] + v[1] + v[2];
        json!({"a": a, "b": b, "both": both,
            "coverage": if a > 0.0 { Some(both / a) } else { None },
            "fit": if b > 0.0 { Some(both / b) } else { None },
            "iou": if uni > 0.0 { Some(both / uni) } else { None }})
    };
    let sum3 = |x: &[f64; 3], y: &[f64; 3]| [x[0] + y[0], x[1] + y[1], x[2] + y[2]];
    let v = &total.v;
    let names: Vec<String> = names.map(|n| n.split(',').map(|s| s.to_string()).collect()).unwrap_or_default();
    let by_label: Vec<_> = (0..total.a_label.len())
        .filter(|&i| total.a_label[i] > 0.0)
        .map(|i| json!({"label": names.get(i).cloned().unwrap_or(i.to_string()), "a": total.a_label[i], "missed": total.miss_label[i]}))
        .collect();
    let out = json!({
        "pitch": pitch, "raft": if raft == f64::MIN { None } else { Some(raft) }, "axis": axis, "per_axis": per_axis,
        "rays": total.rays, "leaky_rays": {"a": total.leaky[0], "b": total.leaky[1], "m": total.leaky[2]},
        "headline": stat(&sum3(&v[0][0], &v[0][1])),
        "outside_model": stat(&v[0][0]), "inside_model": stat(&v[0][1]),
        "raft_band": stat(&sum3(&v[1][0], &v[1][1])),
        "by_label": by_label,
    });
    println!("{out}");
    if let Some(p) = diff {
        let cells: Vec<_> = total
            .diff
            .iter()
            .filter(|(_, v)| v[0] + v[1] > (cell * cell * cell * 0.02) as f32)
            .map(|(k, v)| json!([k.0, k.1, k.2, v[0], v[1]]))
            .collect();
        std::fs::write(p, json!({"cell": cell, "cells": cells}).to_string()).unwrap();
    }
}
