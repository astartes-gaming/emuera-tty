//! A fast, non-cryptographic hasher for the interpreter's name tables.
//!
//! Every variable access, call and CSV-name lookup hashes a short string.
//! std's default SipHash guards against hash-flooding attacks, which don't
//! matter for a game's own scripts, and costs several times more on short
//! keys. This is the "Fx" hash used inside rustc: a multiply and rotate per
//! 8 bytes.

use std::hash::{BuildHasherDefault, Hasher};

/// std's HashMap with the Fx hasher; build with `HashMap::default()`.
pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;
/// std's HashSet with the Fx hasher.
pub type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<FxHasher>>;

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            self.add(u64::from_le_bytes(bytes[..8].try_into().unwrap()));
            bytes = &bytes[8..];
        }
        if bytes.len() >= 4 {
            self.add(u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64);
            bytes = &bytes[4..];
        }
        for &b in bytes {
            self.add(b as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_works() {
        let mut m: HashMap<String, i32> = HashMap::default();
        for i in 0..1000 {
            m.insert(format!("VAR{}", i), i);
        }
        assert_eq!(m.get("VAR500"), Some(&500));
        assert_eq!(m.len(), 1000);
    }
}
