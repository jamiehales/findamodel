//! A small f64 vector type, with the evaluation orders numpy uses on this machine where they
//! differ from a plain left-to-right sum (BLAS matrix products use fused multiply-adds).

use std::ops::{Add, AddAssign, Div, Index, IndexMut, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct V3(pub [f64; 3]);

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3([x, y, z])
}

impl V3 {
    pub const ZERO: V3 = V3([0.0; 3]);
    pub fn x(self) -> f64 {
        self.0[0]
    }
    pub fn y(self) -> f64 {
        self.0[1]
    }
    pub fn z(self) -> f64 {
        self.0[2]
    }
    /// numpy `a @ b` for two 1-D vectors (and `np.dot`): a plain left-to-right sum.
    pub fn dot(self, o: V3) -> f64 {
        self.0[0] * o.0[0] + self.0[1] * o.0[1] + self.0[2] * o.0[2]
    }
    /// numpy `np.cross` (multiply, then subtract; no fused multiply-add).
    pub fn cross(self, o: V3) -> V3 {
        let (a, b) = (self.0, o.0);
        V3([a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]])
    }
    /// numpy `np.linalg.norm` of a 1-D vector, and of the rows of an (N, 3) array.
    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }
    /// geom.unit: v / max(|v|, 1e-12).
    pub fn unit(self) -> V3 {
        self / self.norm().max(1e-12)
    }
    pub fn min(self, o: V3) -> V3 {
        V3([self.0[0].min(o.0[0]), self.0[1].min(o.0[1]), self.0[2].min(o.0[2])])
    }
    pub fn max(self, o: V3) -> V3 {
        V3([self.0[0].max(o.0[0]), self.0[1].max(o.0[1]), self.0[2].max(o.0[2])])
    }
    pub fn map(self, f: impl Fn(f64) -> f64) -> V3 {
        V3(self.0.map(f))
    }
    pub fn to_f32(self) -> [f32; 3] {
        self.0.map(|x| x as f32)
    }
}

/// numpy `rows @ v` for an (N, 3) array with N >= 2 (OpenBLAS gemv): fma(p2, v2, fma(p0, v0, p1 * v1)).
/// For N == 1 numpy takes the plain dot instead (see `gemv_rows`).
pub fn gemv_row(p: V3, v: V3) -> f64 {
    p.0[2].mul_add(v.0[2], p.0[0].mul_add(v.0[0], p.0[1] * v.0[1]))
}

/// numpy `rows @ v` for an (N, 3) array, as numpy evaluates it for that N.
pub fn gemv_rows(rows: &[V3], v: V3) -> Vec<f64> {
    if rows.len() == 1 { vec![rows[0].dot(v)] } else { rows.iter().map(|&p| gemv_row(p, v)).collect() }
}

/// numpy `v @ M.T` and `M @ v` for a 1-D vector v and a 3x3 M (gemv): component k is
/// fma(v2, M[k][2], fma(v0, M[k][0], v1 * M[k][1])).
pub fn mat_vec(m: &[[f64; 3]; 3], v: V3) -> V3 {
    V3([0, 1, 2].map(|k| gemv_row(v, V3(m[k]))))
}

/// numpy `P @ M.T` for an (N, 3) (or stacked (N, 3, 3)) array P (gemm): component k of each row is
/// fma(p2, M[k][2], fma(p1, M[k][1], p0 * M[k][0])).
pub fn gemm_row(p: V3, m: &[[f64; 3]; 3]) -> V3 {
    V3([0, 1, 2].map(|k| p.0[2].mul_add(m[k][2], p.0[1].mul_add(m[k][1], p.0[0] * m[k][0]))))
}

/// numpy `P @ M` (M not transposed) for an (N, 3) array: column k of M.
pub fn gemm_row_cols(p: V3, m: &[[f64; 3]; 3]) -> V3 {
    V3([0, 1, 2].map(|k| p.0[2].mul_add(m[2][k], p.0[1].mul_add(m[1][k], p.0[0] * m[0][k]))))
}

/// numpy's pairwise summation (`np.sum` of a contiguous float64 array).
pub fn pairwise_sum(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut r = 0.0;
        for &x in a {
            r += x;
        }
        r
    } else if n <= 128 {
        let mut r = [0.0; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i < n - (n % 8) {
            for j in 0..8 {
                r[j] += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise_sum(&a[..n2]) + pairwise_sum(&a[n2..])
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        V3([self.0[0] + o.0[0], self.0[1] + o.0[1], self.0[2] + o.0[2]])
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        V3([self.0[0] - o.0[0], self.0[1] - o.0[1], self.0[2] - o.0[2]])
    }
}
impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}
impl SubAssign for V3 {
    fn sub_assign(&mut self, o: V3) {
        *self = *self - o;
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, s: f64) -> V3 {
        V3([self.0[0] * s, self.0[1] * s, self.0[2] * s])
    }
}
impl Mul<V3> for V3 {
    type Output = V3;
    fn mul(self, o: V3) -> V3 {
        V3([self.0[0] * o.0[0], self.0[1] * o.0[1], self.0[2] * o.0[2]])
    }
}
impl Div<f64> for V3 {
    type Output = V3;
    fn div(self, s: f64) -> V3 {
        V3([self.0[0] / s, self.0[1] / s, self.0[2] / s])
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        V3([-self.0[0], -self.0[1], -self.0[2]])
    }
}
impl Index<usize> for V3 {
    type Output = f64;
    fn index(&self, i: usize) -> &f64 {
        &self.0[i]
    }
}
impl IndexMut<usize> for V3 {
    fn index_mut(&mut self, i: usize) -> &mut f64 {
        &mut self.0[i]
    }
}
