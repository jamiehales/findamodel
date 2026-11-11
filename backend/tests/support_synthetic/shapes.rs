//! gen.inside_shapes: whether points lie inside the model's analytic shapes (truth "shapes" JSON).

use crate::vec::{V3, gemm_row_cols};
use serde_json::Value;

fn v(x: &Value) -> V3 {
    V3([0, 1, 2].map(|k| x[k].as_f64().unwrap()))
}

fn m3(x: &Value) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|r| [0, 1, 2].map(|c| x[r][c].as_f64().unwrap()))
}

pub fn inside_shapes(shapes: &Value, pts: &[V3]) -> Vec<bool> {
    let mut inside = vec![false; pts.len()];
    for s in shapes.as_array().into_iter().flatten() {
        match s["kind"].as_str().unwrap() {
            "box" => {
                let (lo, hi) = (v(&s["lo"]), v(&s["hi"]));
                for (i, p) in pts.iter().enumerate() {
                    inside[i] |= (0..3).all(|k| p[k] >= lo[k] && p[k] <= hi[k]);
                }
            }
            "sphere" => {
                let (c, r) = (v(&s["c"]), s["r"].as_f64().unwrap());
                for (i, p) in pts.iter().enumerate() {
                    inside[i] |= (*p - c).norm() <= r;
                }
            }
            "obox" => {
                let (c, rm, half) = (v(&s["c"]), m3(&s["R"]), v(&s["half"]));
                for (i, p) in pts.iter().enumerate() {
                    let q = gemm_row_cols(*p - c, &rm);
                    inside[i] |= (0..3).all(|k| q[k].abs() <= half[k]);
                }
            }
            "hollow" => {
                let (lo, hi, ilo, ihi) = (v(&s["lo"]), v(&s["hi"]), v(&s["ilo"]), v(&s["ihi"]));
                for (i, p) in pts.iter().enumerate() {
                    let o = (0..3).all(|k| p[k] >= lo[k] && p[k] <= hi[k]);
                    let n = (0..3).all(|k| p[k] > ilo[k] && p[k] < ihi[k]);
                    inside[i] |= o && !n;
                }
            }
            _ => {}
        }
    }
    inside
}
