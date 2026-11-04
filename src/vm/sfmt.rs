// SFMT (SIMD-oriented Fast Mersenne Twister) implementation
// Ported from Emuera's SFMT.cs (MT19937 variant)
// Original: Rei HOBARA, based on Saito & Matsumoto's SFMT

/// SFMT period: 2^19937-1
pub const MEXP: usize = 19937;
pub const N: usize = MEXP / 128 + 1; // 156
pub const N32: usize = N * 4; // 624

// MT19937-specific parameters
const POS1: usize = 122;
const SL1: usize = 18;
const SR1: usize = 11;
const MSK1: u32 = 0xdfffffef;
const MSK2: u32 = 0xddfecb7f;
const MSK3: u32 = 0xbffaffff;
const MSK4: u32 = 0xbffffff6;

const PARITY1: u32 = 0x00000001;
const PARITY2: u32 = 0x00000000;
const PARITY3: u32 = 0x00000000;
const PARITY4: u32 = 0x13c9e684;

/// SFMT random number generator matching Emuera's MTRandom exactly.
#[derive(Debug, Clone)]
pub struct Sfmt {
    sfmt: [u32; N32],
    idx: usize,
}

impl Sfmt {
    /// Create a new SFMT seeded with the current time (nanos since epoch).
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u32).unwrap_or(0x1234_5678);
        Self::with_seed(seed)
    }

    /// Create a new SFMT with the given 32-bit seed.
    pub fn with_seed(seed: u32) -> Self {
        let mut rng = Self { sfmt: [0; N32], idx: N32 };
        rng.init_gen_rand(seed);
        rng
    }

    /// Initialize the generator with a 32-bit seed.
    fn init_gen_rand(&mut self, seed: u32) {
        self.sfmt[0] = seed;
        for i in 1..N32 {
            self.sfmt[i] = 1812433253u32.wrapping_mul(self.sfmt[i - 1] ^ (self.sfmt[i - 1] >> 30)).wrapping_add(i as u32);
        }
        self.period_certification();
        self.idx = N32;
    }

    /// Verify and correct the period certification.
    fn period_certification(&mut self) {
        let parity = [PARITY1, PARITY2, PARITY3, PARITY4];
        let mut inner = 0u32;
        for i in 0..4 {
            inner ^= self.sfmt[i] & parity[i];
        }
        let mut i = 16;
        while i > 0 {
            inner ^= inner >> i;
            i >>= 1;
        }
        inner &= 1;
        if inner == 1 {
            return;
        }
        for i in 0..4 {
            let mut work = 1u32;
            for _ in 0..32 {
                if (work & parity[i]) != 0 {
                    self.sfmt[i] ^= work;
                    return;
                }
                work <<= 1;
            }
        }
    }

    /// Generate the next batch of random numbers.
    fn gen_rand_all(&mut self) {
        let p = &mut self.sfmt;
        let mut a = 0;
        let mut b = POS1 * 4;
        let mut c = (N - 2) * 4;
        let mut d = (N - 1) * 4;

        loop {
            p[a + 3] = p[a + 3] ^ p[a + 3] << 8 ^ p[a + 2] >> 24 ^ p[c + 3] >> 8 ^ (p[b + 3] >> SR1 & MSK4) ^ p[d + 3] << SL1;
            p[a + 2] = p[a + 2] ^ p[a + 2] << 8 ^ p[a + 1] >> 24 ^ p[c + 3] << 24 ^ p[c + 2] >> 8 ^ (p[b + 2] >> SR1 & MSK3) ^ p[d + 2] << SL1;
            p[a + 1] = p[a + 1] ^ p[a + 1] << 8 ^ p[a] >> 24 ^ p[c + 2] << 24 ^ p[c + 1] >> 8 ^ (p[b + 1] >> SR1 & MSK2) ^ p[d + 1] << SL1;
            p[a] = p[a] ^ p[a] << 8 ^ p[c + 1] << 24 ^ p[c] >> 8 ^ (p[b] >> SR1 & MSK1) ^ p[d] << SL1;

            c = d;
            d = a;
            a += 4;
            b += 4;
            if b >= N32 {
                b = 0;
            }
            if a >= N32 {
                break;
            }
        }
    }

    /// Get the next 32-bit random value.
    fn next_u32(&mut self) -> u32 {
        if self.idx >= N32 {
            self.gen_rand_all();
            self.idx = 0;
        }
        let val = self.sfmt[self.idx];
        self.idx += 1;
        val
    }

    /// Get the next 64-bit random value (two 32-bit values combined).
    pub fn next_u64(&mut self) -> u64 {
        let hi = self.next_u32() as u64;
        let lo = self.next_u32() as u64;
        (hi << 32) | lo
    }

    /// Get a random integer in [0, max). Matches Emuera's modulo bias behavior.
    pub fn next_int64(&mut self, max: i64) -> i64 {
        if max <= 0 {
            return 0;
        }
        (self.next_u64() % max as u64) as i64
    }

    /// Get a random f64 in [0, 1). Matches Emuera's NextDouble().
    #[allow(dead_code)]
    pub fn next_double(&mut self) -> f64 {
        self.next_u32() as f64 * (1.0 / 4294967296.0)
    }

    /// Serialize the full internal state (for DUMPRAND / save compatibility).
    /// Returns 625 u64 values: 624 state words + index.
    pub fn get_rand(&self) -> [u64; N32 + 1] {
        let mut out = [0u64; N32 + 1];
        for i in 0..N32 {
            out[i] = self.sfmt[i] as u64;
        }
        out[N32] = self.idx as u64;
        out
    }

    /// Deserialize the full internal state (for INITRAND / load compatibility).
    pub fn set_rand(&mut self, data: &[u64; N32 + 1]) {
        for i in 0..N32 {
            self.sfmt[i] = data[i] as u32;
        }
        self.idx = data[N32] as usize;
    }
}

impl Default for Sfmt {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sfmt_basic() {
        let mut rng = Sfmt::with_seed(12345);
        let v1 = rng.next_u32();
        let v2 = rng.next_u32();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_sfmt_deterministic() {
        let mut rng1 = Sfmt::with_seed(42);
        let mut rng2 = Sfmt::with_seed(42);
        for _ in 0..1000 {
            assert_eq!(rng1.next_u32(), rng2.next_u32());
        }
    }

    #[test]
    fn test_sfmt_next_int64() {
        let mut rng = Sfmt::with_seed(1);
        for _ in 0..100 {
            let v = rng.next_int64(100);
            assert!((0..100).contains(&v));
        }
    }

    #[test]
    fn test_sfmt_state_roundtrip() {
        let mut rng = Sfmt::with_seed(999);
        // Generate some values
        for _ in 0..50 {
            rng.next_u32();
        }
        let state = rng.get_rand();
        let mut rng2 = Sfmt::with_seed(0); // seed doesn't matter
        rng2.set_rand(&state);
        // Should produce identical subsequent values
        for _ in 0..50 {
            assert_eq!(rng.next_u32(), rng2.next_u32());
        }
    }
}
