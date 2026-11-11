//! The day request every lift-day builder takes.

use super::regions::{DayRegion, WeekRegions};

/// One lift day to build: its weekday, its place among the week's lift days
/// and in the accumulation block, and the regions of the whole week
/// (LIFT-DAYS-001 templates).
#[derive(Debug, Clone, Copy)]
pub(super) struct LiftDay {
    /// Weekday offset, 0 = Monday.
    pub(super) day: u16,
    /// 0-based position among the week's lift days, in weekday order.
    pub(super) slot: usize,
    /// How many days a week this day's muscles are trained, the volume-split
    /// denominator: 2-day 2, 3-day 3, 4-day upper/lower 2, 5-day 3.
    pub(super) n_exposures: usize,
    /// 1-based week within the accumulation block.
    pub(super) week_in_block: u8,
    /// Loading weeks in the block.
    pub(super) block_weeks: u8,
    /// The region every lift day of the week trains.
    pub(super) regions: WeekRegions,
}

impl LiftDay {
    /// This day's region (`Full` past the end of the week's regions).
    pub(super) fn region(&self) -> DayRegion {
        self.regions
            .as_slice()
            .get(self.slot)
            .copied()
            .unwrap_or(DayRegion::Full)
    }

    pub(super) fn days_before(&self) -> &[DayRegion] {
        let regions = self.regions.as_slice();
        &regions[..self.slot.min(regions.len())]
    }
}
