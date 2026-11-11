//! numpy's default_rng (SeedSequence + PCG64) with the two calls the generator makes:
//! `random(n)` and `choice(len, n, p)`, giving the same numbers as numpy.

const INIT_A: u32 = 0x43b0d7e5;
const MULT_A: u32 = 0x931e8875;
const INIT_B: u32 = 0x8b51f9dd;
const MULT_B: u32 = 0x58f38ded;
const MIX_MULT_L: u32 = 0xca01f9dd;
const MIX_MULT_R: u32 = 0x4973f715;
const XSHIFT: u32 = 16;
const POOL: usize = 4;
const PCG_MULT: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

fn hashmix(value: u32, hash_const: &mut u32) -> u32 {
    let mut v = value ^ *hash_const;
    *hash_const = hash_const.wrapping_mul(MULT_A);
    v = v.wrapping_mul(*hash_const);
    v ^ (v >> XSHIFT)
}

fn mix(x: u32, y: u32) -> u32 {
    let r = MIX_MULT_L.wrapping_mul(x).wrapping_sub(MIX_MULT_R.wrapping_mul(y));
    r ^ (r >> XSHIFT)
}

/// SeedSequence(seed).generate_state(n_words, uint32) for a non-negative integer seed.
fn seed_state(seed: u64, n_words: usize) -> Vec<u32> {
    let mut entropy = vec![seed as u32];
    if seed >> 32 != 0 {
        entropy.push((seed >> 32) as u32);
    }
    let mut pool = [0u32; POOL];
    let mut hc = INIT_A;
    for (i, slot) in pool.iter_mut().enumerate() {
        *slot = hashmix(if i < entropy.len() { entropy[i] } else { 0 }, &mut hc);
    }
    for i_src in 0..POOL {
        for i_dst in 0..POOL {
            if i_src != i_dst {
                let h = hashmix(pool[i_src], &mut hc);
                pool[i_dst] = mix(pool[i_dst], h);
            }
        }
    }
    for &e in entropy.iter().skip(POOL) {
        for slot in pool.iter_mut() {
            let h = hashmix(e, &mut hc);
            *slot = mix(*slot, h);
        }
    }
    let mut hb = INIT_B;
    (0..n_words)
        .map(|i| {
            let mut d = pool[i % POOL] ^ hb;
            hb = hb.wrapping_mul(MULT_B);
            d = d.wrapping_mul(hb);
            d ^ (d >> XSHIFT)
        })
        .collect()
}

pub struct Rng {
    state: u128,
    inc: u128,
}

impl Rng {
    /// np.random.default_rng(seed)
    pub fn new(seed: u64) -> Rng {
        let w = seed_state(seed, 8);
        let word = |k: usize| u64::from(w[2 * k]) | (u64::from(w[2 * k + 1]) << 32);
        let initstate = (u128::from(word(0)) << 64) | u128::from(word(1));
        let initseq = (u128::from(word(2)) << 64) | u128::from(word(3));
        let mut r = Rng { state: 0, inc: (initseq << 1) | 1 };
        r.step();
        r.state = r.state.wrapping_add(initstate);
        r.step();
        r
    }
    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
    }
    pub fn next_u64(&mut self) -> u64 {
        self.step();
        let s = self.state;
        let xored = ((s >> 64) as u64) ^ (s as u64);
        xored.rotate_right((s >> 122) as u32)
    }
    pub fn random_one(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }
    /// rng.random(n)
    pub fn random(&mut self, n: usize) -> Vec<f64> {
        (0..n).map(|_| self.random_one()).collect()
    }
    /// rng.choice(len(p), n, p=p) (with replacement): cumsum, normalised by its last value, then
    /// searchsorted(side='right') of n uniforms.
    pub fn choice(&mut self, p: &[f64], n: usize) -> Vec<usize> {
        let mut cdf = Vec::with_capacity(p.len());
        let mut acc = 0.0;
        for &x in p {
            acc += x;
            cdf.push(acc);
        }
        let last = *cdf.last().unwrap();
        for c in cdf.iter_mut() {
            *c /= last;
        }
        self.random(n).into_iter().map(|u| cdf.partition_point(|&c| c <= u)).collect()
    }
}
