//! Wire input sanitizing: every value an event carries is brought into the
//! domain the core computes on once, where the event enters the core
//! (`Event::normalized`), so no `update` arm or compaction rule re-clamps it.
//! Wire types `#[derive(Sanitize)]`; a field with a narrower domain names its
//! rule with `#[sanitize_with = "path"]`.

pub(crate) use shared_derive::Sanitize;

pub(crate) trait Sanitize {
    fn sanitize(&mut self);

    fn sanitized(mut self) -> Self
    where
        Self: Sized,
    {
        self.sanitize();
        self
    }
}

/// Bound of every wire float. `serde_json` already refuses NaN/Inf tokens,
/// but a huge finite magnitude (1e300) can multiply or accumulate to `inf` in
/// `view()`, which serializes to `null` and breaks the shell's non-null decode
/// on every launch (the event is replayed from the log). ±1e12 keeps any
/// downstream product inside f64 range and covers every real kg / km / min /
/// bpm value.
const FLOAT_BOUND: f64 = 1.0e12;

/// The one float rule: NaN reads as 0 (an absent reading), magnitudes clamp
/// to [`FLOAT_BOUND`], and `-0.0` becomes `+0.0` so no view prints "-0".
macro_rules! float_rule {
    ($($t:ty),*) => {$(
        impl Sanitize for $t {
            fn sanitize(&mut self) {
                const BOUND: $t = FLOAT_BOUND as $t;
                *self = if self.is_nan() {
                    0.0
                } else {
                    self.clamp(-BOUND, BOUND) + 0.0
                };
            }
        }
    )*};
}
float_rule!(f64, f32);

/// Every wire `u64` is an id the shell decodes as a Kotlin `Long` (`i64`): a
/// larger value would round-trip out of range.
impl Sanitize for u64 {
    fn sanitize(&mut self) {
        *self = (*self).min(i64::MAX as u64);
    }
}

impl<T: Sanitize> Sanitize for Option<T> {
    fn sanitize(&mut self) {
        if let Some(v) = self {
            v.sanitize();
        }
    }
}

impl<T: Sanitize> Sanitize for Vec<T> {
    fn sanitize(&mut self) {
        for v in self {
            v.sanitize();
        }
    }
}

/// Types whose every value is in domain. A fieldless wire enum derives
/// `Sanitize` where it is defined (serde rejects any other variant).
macro_rules! in_domain {
    ($($t:ty),*) => {$(
        impl Sanitize for $t {
            fn sanitize(&mut self) {}
        }
    )*};
}
in_domain!(bool, u8, u16, u32, i64, String);

/// A wire float that must not be negative (a distance, a duration, a weekly
/// volume).
pub(crate) fn non_negative(v: &mut f64) {
    v.sanitize();
    *v = v.max(0.0);
}

/// A rep count: `u32::MAX` reps would overflow an e1RM product to `inf`; no
/// real set exceeds a few thousand.
pub(crate) fn reps(r: &mut u32) {
    *r = (*r).min(10_000);
}

/// A whole-session CR-10 rating: 0–10.
pub(crate) fn cr10(rpe: &mut Option<u8>) {
    if let Some(r) = rpe {
        *r = (*r).min(10);
    }
}

/// `x`, with a negative zero turned into `+0.0`: a `-0.0` (an empty `f64`
/// sum is one, so is a tiny negative rounded to zero) prints as "-0" in a
/// view. The float rule above applies the same `+ 0.0` to every wire float.
pub(crate) fn positive_zero(x: f64) -> f64 {
    x + 0.0
}

/// A float sum that is never `-0.0` (`Iterator::sum` over `f64` starts from
/// `-0.0`, so an empty sum is one).
pub(crate) fn sum0(values: impl Iterator<Item = f64>) -> f64 {
    positive_zero(values.sum())
}
