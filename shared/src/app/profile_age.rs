//! A birthday-carrying profile's age and under-18 gate as of a given day, and
//! the calendar window a profile date is plausible in.

#[cfg(any(ffi_surface, test))]
use crate::envelope::{DraftFacts, ProfileDraft};
use crate::profile::Profile;
#[cfg(any(ffi_surface, test))]
use crate::schema::ParqQuestion;

/// Plausible calendar window for a wire epoch day on the profile (race / meet
/// / layoff / race-result dates): 1920-01-02 through 2120-01-02. A corrupt
/// value outside it would push week arithmetic toward overflow or make a
/// race "decades away"; it is dropped (`None` = unstated) instead.
pub(super) const PROFILE_EPOCH_DAY_RANGE: std::ops::RangeInclusive<i64> = -18_262..=54_787;

/// `p` as of local day `day`: a birthday-carrying profile's age and under-18
/// gate are those of `day`, not of today (SAFE-PEDS-001: a day the gate held
/// stays held after the 18th birthday). Borrowed unless the day's derived
/// values differ from the stored ones (only across a birthday).
pub(super) fn aged_on(p: &Profile, day: i64) -> std::borrow::Cow<'_, Profile> {
    let Some(birth) = p.birth_epoch_day else {
        return std::borrow::Cow::Borrowed(p);
    };
    let age = whole_years_between(birth, day) as f64;
    if p.age_years == Some(age) && p.health.youth == Profile::youth_age(age) {
        return std::borrow::Cow::Borrowed(p);
    }
    let mut q = p.clone();
    derive_birthday_age(&mut q, day);
    std::borrow::Cow::Owned(q)
}

/// With a birthday, `age_years` is the whole calendar years lived by `today`
/// and the youth flag is exactly "under 18 today": the birthday is the
/// authoritative age source, so the core owns both. A future birthday derives
/// age 0 (gate on, the conservative reading).
pub(super) fn derive_birthday_age(p: &mut Profile, today: i64) {
    if let Some(birth) = p.birth_epoch_day {
        let age = whole_years_between(birth, today) as f64;
        p.age_years = Some(age);
        p.health.youth = Profile::youth_age(age);
    }
}

/// A sent profile's age and pediatric gate: the birthday's age on `today`
/// when both are known, else the stated age; `health.youth` is exactly
/// "under 18" on that age (HARD RULE 3; safety-011 gives no cutoff, 18 y is
/// the core's). The core owns the flag: a `youth` the shell sends (an echo of
/// an earlier profile) never outlives the age it came from. No age = adult,
/// not gated (KB onboard-050). A NaN age arrives as 0, so it gates.
pub(super) fn derive_age_gate(p: &mut Profile, today: Option<i64>) {
    if let Some(today) = today {
        derive_birthday_age(p, today);
    }
    p.health.youth = p.age_years.is_some_and(Profile::youth_age);
}

/// The gates and form facts of a profile the shell is still editing, by the
/// same rules a sent `SetProfile` meets: the age from the birthday on `today`
/// (else the stated age), the pediatric gate on it, the health answers'
/// deferral gates and the PAR-Q+ answers after the user's latest one.
#[cfg(any(ffi_surface, test))]
pub(crate) fn draft_facts(d: ProfileDraft) -> DraftFacts {
    let health = match d.answer {
        Some(answer) => d.health.answered(answer),
        None => d.health,
    };
    let age_years = match (d.birth_epoch_day, d.today) {
        (Some(birth), Some(today)) => Some(whole_years_between(birth, today) as f64),
        _ => d.age_years,
    };
    DraftFacts {
        parq_yes: ParqQuestion::ALL
            .into_iter()
            .filter(|q| health.answered_yes(*q))
            .collect(),
        first_symptom: ParqQuestion::ALL
            .into_iter()
            .find(|q| q.is_symptom() && health.answered_yes(*q)),
        legacy_parq: health.legacy_parq(),
        health_defers: health.answers_defer(),
        age_years,
        youth: age_years.is_some_and(Profile::youth_age),
        can_hang: d
            .calis_equipment
            .map(|e| crate::calisthenics::Kit::from_equipment(e).hang()),
        health,
    }
}

/// Completed calendar years from `birth` to `today` (both local epoch days).
/// A 29 February birthday completes its year on 1 March in common years.
/// `today` is clamped to the profile date window so a corrupt clock can't
/// overflow the calendar math.
pub(super) fn whole_years_between(birth: i64, today: i64) -> i64 {
    let range = PROFILE_EPOCH_DAY_RANGE;
    let today = today.clamp(*range.start(), *range.end());
    if today <= birth {
        return 0;
    }
    let (by, bm, bd) = crate::day::civil_from_days(birth);
    let (ty, tm, td) = crate::day::civil_from_days(today);
    ty - by - i64::from((tm, td) < (bm, bd))
}
