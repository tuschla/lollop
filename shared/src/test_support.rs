//! Test support shared by every unit-test module of the crate.

use crate::gps::EARTH_RADIUS_M;

pub(crate) mod fit_writer;

/// Metres per degree of latitude, or of longitude on the equator, for the
/// core's haversine: `x` metres is `x / M_PER_DEG` degrees.
pub(crate) const M_PER_DEG: f64 = EARTH_RADIUS_M * std::f64::consts::PI / 180.0;

/// SplitMix64: a seedable stream; `SplitMix(x).next()` is one step from `x`.
pub(crate) struct SplitMix(pub(crate) u64);

impl SplitMix {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In `0..n`; `n` = 0 gives 0 without a draw.
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    /// `next() % n` as an index, in `0..n` (`n` = 0 reads as 1, one draw).
    pub(crate) fn index(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    /// In `lo..=hi`.
    pub(crate) fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    /// In [0, 1).
    pub(crate) fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// In `lo..hi`.
    pub(crate) fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    pub(crate) fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    pub(crate) fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize].clone()
    }
}
