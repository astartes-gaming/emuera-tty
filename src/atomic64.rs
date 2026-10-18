//! `AtomicU64` where the CPU has 64-bit atomics; on the others (32-bit
//! PowerPC, MIPS, SPARC, m68k, ARMv5, RISC-V 32…) a stand-in made of two
//! 32-bit halves.
//!
//! The stand-in is atomic per half only, which is enough for what uses it:
//! caches and counters that only the thread running the program changes
//! (two threads writing one at once, which would tear it, never happens).

#[cfg(target_has_atomic = "64")]
pub use std::sync::atomic::AtomicU64;

#[cfg(not(target_has_atomic = "64"))]
pub use split::AtomicU64;

#[cfg_attr(target_has_atomic = "64", allow(dead_code))]
mod split {
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Default, Debug)]
    pub struct AtomicU64 {
        hi: AtomicU32,
        lo: AtomicU32,
    }

    impl AtomicU64 {
        pub const fn new(v: u64) -> AtomicU64 {
            AtomicU64 { hi: AtomicU32::new((v >> 32) as u32), lo: AtomicU32::new(v as u32) }
        }

        pub fn load(&self, o: Ordering) -> u64 {
            let hi = self.hi.load(o);
            ((hi as u64) << 32) | self.lo.load(o) as u64
        }

        pub fn store(&self, v: u64, o: Ordering) {
            self.hi.store((v >> 32) as u32, o);
            self.lo.store(v as u32, o);
        }

        pub fn swap(&self, v: u64, o: Ordering) -> u64 {
            let old = self.load(o);
            self.store(v, o);
            old
        }

        pub fn fetch_add(&self, d: u64, o: Ordering) -> u64 {
            let old = self.load(o);
            self.store(old.wrapping_add(d), o);
            old
        }
    }

    #[cfg(test)]
    mod tests {
        use super::AtomicU64;
        use std::sync::atomic::Ordering::Relaxed;

        #[test]
        fn split_halves() {
            let a = AtomicU64::new(0xFFFF_FFFF);
            assert_eq!(a.fetch_add(1, Relaxed), 0xFFFF_FFFF);
            assert_eq!(a.load(Relaxed), 1 << 32);
            assert_eq!(a.swap(u64::MAX, Relaxed), 1 << 32);
            assert_eq!(a.load(Relaxed), u64::MAX);
        }
    }
}
