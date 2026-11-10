//! Lift-day regions: the LIFT-DAYS-001 day templates, the CAP-1 leg-day cap
//! and how often a week trains each region.

use crate::exercise::{MovementPattern, is_isolation_lift, movement_pattern};

/// Body region a lift day trains (KB Table 1.6 time-constrained templates,
/// LIFT-DAYS-001): full body, or one half of an upper/lower split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DayRegion {
    #[default]
    Full,
    Upper,
    Lower,
}

impl DayRegion {
    /// The day loads the legs (squat / hinge work).
    pub(crate) fn has_lower(self) -> bool {
        !matches!(self, DayRegion::Upper)
    }

    /// The day loads the upper body (push / pull work).
    pub(crate) fn has_upper(self) -> bool {
        !matches!(self, DayRegion::Lower)
    }

    /// Two days train at least one muscle group in common.
    pub(crate) fn overlaps(self, other: DayRegion) -> bool {
        (self.has_lower() && other.has_lower()) || (self.has_upper() && other.has_upper())
    }

    /// A lift of movement pattern `p` belongs on this day: push/pull on upper
    /// days, squat/hinge on lower days, an unclassified lift anywhere.
    pub(crate) fn fits(self, p: Option<MovementPattern>) -> bool {
        match (self, p) {
            (DayRegion::Full, _) | (_, None) => true,
            (DayRegion::Upper, Some(p)) => p.is_upper(),
            (DayRegion::Lower, Some(p)) => p.is_lower(),
        }
    }
}

/// The region of every lift day of one week, in weekday order (index = the
/// day's `LiftDay::slot`). `Copy`, so it rides along in `LiftDay`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WeekRegions {
    regions: [DayRegion; 7],
    len: usize,
}

impl WeekRegions {
    pub(crate) fn new(regions: &[DayRegion]) -> Self {
        let len = regions.len().min(7);
        let mut r = [DayRegion::Full; 7];
        r[..len].copy_from_slice(&regions[..len]);
        Self { regions: r, len }
    }

    /// `n` full-body days.
    pub(crate) fn full(n: usize) -> Self {
        Self {
            regions: [DayRegion::Full; 7],
            len: n.min(7),
        }
    }

    pub(crate) fn as_slice(&self) -> &[DayRegion] {
        &self.regions[..self.len]
    }
}

/// The lift-day template for `n` days a week (KB Table 1.6 via LIFT-DAYS-001):
/// 2 → full body ×2 (compounds), 3 → full body ×3, 4 → upper/lower ×2, 5 →
/// upper/lower ×2 plus a full-body day (every muscle 3×/week). Canonical order;
/// the scheduler tries its rotations.
pub(crate) fn lift_template(n: usize) -> Vec<DayRegion> {
    use DayRegion::{Full, Lower, Upper};
    match n {
        0 => Vec::new(),
        1..=3 => vec![Full; n],
        4 => vec![Upper, Lower, Upper, Lower],
        _ => vec![Upper, Lower, Full, Upper, Lower],
    }
}

/// CAP-1 (HYB-CAP-001): at most `max_lower` lift days carry lower-body work;
/// extra leg days become upper-body days, latest first.
pub(super) fn cap_lower_days(regions: &mut [DayRegion], max_lower: usize) {
    let mut lower = regions.iter().filter(|r| r.has_lower()).count();
    for r in regions.iter_mut().rev() {
        if lower <= max_lower {
            break;
        }
        if r.has_lower() {
            *r = DayRegion::Upper;
            lower -= 1;
        }
    }
}

/// How many days a week the muscles of a `region` day are trained, the
/// volume-split denominator handed to the lift builders.
pub(super) fn exposures(week: &[DayRegion], region: DayRegion) -> usize {
    let up = week.iter().filter(|r| r.has_upper()).count();
    let low = week.iter().filter(|r| r.has_lower()).count();
    match region {
        DayRegion::Upper => up,
        DayRegion::Lower => low,
        DayRegion::Full => up.max(low),
    }
    .max(1)
}

/// The day's exercises for its region: an upper day keeps push/pull lifts, a
/// lower day squat/hinge lifts, unclassified lifts go to both (and to full-body
/// days). `compounds_only` drops isolation work (Table 1.6: a 2-day week
/// preserves compounds and cuts accessories). Classified compounds lead, so
/// the first name stays a valid DUP primary.
pub(super) fn names_for_region(
    names: &[String],
    region: DayRegion,
    compounds_only: bool,
) -> Vec<String> {
    let fits =
        |n: &String| region.fits(movement_pattern(n)) && !(compounds_only && is_isolation_lift(n));
    let (mut out, rest): (Vec<String>, Vec<String>) = names
        .iter()
        .filter(|n| fits(n))
        .cloned()
        .partition(|n| movement_pattern(n).is_some());
    out.extend(rest);
    if out.is_empty() { names.to_vec() } else { out }
}

/// How many earlier lift days of the week train the muscles of day `k`: its
/// exposure index for the DUP rotation. DUP undulates each LIFT across its own
/// sessions (File 02 strength-023), so an upper/lower week rotates per region:
/// on U/L/U/L each region gets Heavy then HeavyVolume, and every main lift is
/// lifted at ≥80 % 1RM in ≥2 sessions (STR-HEAVYFREQ-001; File 13 strpow-add-001:
/// lighter days supplement, not replace, the heavy exposures). A full-body day
/// takes the further-along of its two regions; an all-full-body week reads `k`.
pub(super) fn region_exposure_index(regions: &[DayRegion], k: usize) -> usize {
    let before = &regions[..k];
    let upper = before.iter().filter(|r| r.has_upper()).count();
    let lower = before.iter().filter(|r| r.has_lower()).count();
    match regions[k] {
        DayRegion::Upper => upper,
        DayRegion::Lower => lower,
        DayRegion::Full => upper.max(lower),
    }
}
