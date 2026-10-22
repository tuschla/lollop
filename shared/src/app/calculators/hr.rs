use super::*;

/// Build a graded heart-rate-zone table from age: the Tanaka HRmax estimate plus
/// the five Daniels %HRmax training bands mapped to absolute bpm ranges
/// (`running::vdot_band_hr_pct`, running-007). The HRmax rows carry the Tanaka
/// formula's own claim RUN-HRMAX-001 (Moderate, ±10–11 bpm individual SEE), citing the VDOT
/// claim here would overstate the formula's evidence; the %HRmax band rows are
/// Daniels tables (RUN-VDOT-001, Moderate). No training numbers are invented
/// (HARD RULE 1/2). A non-positive or implausible age yields a single
/// explanatory row rather than a bogus HRmax. Alongside the rows it returns the
/// structured HRmax figure and one [`HrZoneBandView`] per band row, so a shell
/// draws the zone table from numbers.
pub(in crate::app) fn build_hr_zones(
    q: &HrZoneQuery,
    measured_hr_max: Option<f64>,
) -> (Vec<GuidanceView>, Option<HrMaxView>, Vec<HrZoneBandView>) {
    let mut rows = Vec::new();

    // Wire contract: a MEASURED HRmax (from the profile, logged off an all-out
    // effort) supersedes the age-based Tanaka estimate: it is the person's own
    // datum, not a population average. Only a physiologically plausible value is
    // trusted (`running::plausible_measured_hr_max`).
    let measured = running::plausible_measured_hr_max(measured_hr_max);

    // HRmax from age is only meaningful for a realistic adult/junior age; refuse
    // to emit a fabricated maximum outside that range. A measured max bypasses
    // the age requirement (it needs no age).
    if measured.is_none() && !running::tanaka_age_in_range(q.age_years) {
        let note = graded((), claims::RUN_HRMAX_001);
        push_guidance(
            &mut rows,
            "Heart-rate zones",
            "Enter an age between 5 and 100 to estimate HRmax and training zones.".to_string(),
            &note,
        );
        return (rows, None, Vec::new());
    }

    // One Tanaka estimator (`running::hr_max_tanaka`) for the zone table and the
    // running rules. A measured max replaces it.
    let hr_max = measured.unwrap_or_else(|| running::hr_max_tanaka(q.age_years));
    push_hr_max_row(&mut rows, q, measured, hr_max);
    let karvonen_rhr = push_hr_method_row(&mut rows, q.resting_hr_bpm);
    let bands = push_band_rows(&mut rows, hr_max, karvonen_rhr);
    push_maf_row(&mut rows, q.age_years);
    push_retest_rows(&mut rows, q);
    (rows, Some(hr_max_view(q, measured, hr_max)), bands)
}

/// The HRmax row: the measured maximum when trusted, else the Tanaka
/// estimate, each with a basis line saying which and why.
fn push_hr_max_row(
    rows: &mut Vec<GuidanceView>,
    q: &HrZoneQuery,
    measured: Option<f64>,
    hr_max: f64,
) {
    let hr_max_row = graded((), claims::RUN_HRMAX_001);
    let (hr_max_text, hr_max_basis) = if let Some(m) = measured {
        (
            format!("Measured HRmax: {m:.0} bpm (your logged maximum)"),
            format!(
                "Your measured HRmax ({m:.0} bpm) from an all-out effort replaces the age-based \
                 Tanaka estimate and sets the zones below."
            ),
        )
    } else {
        (
            format!("Estimated HRmax: {hr_max:.0} bpm"),
            format!(
                "Estimated from your age ({:.0}) with the Tanaka formula ({:.0} − {} × age = {hr_max:.0} bpm). \
                 It is a population average, not your measured maximum. Individuals vary by roughly ±10 bpm.",
                q.age_years,
                running::TANAKA_INTERCEPT_BPM,
                running::TANAKA_SLOPE_BPM_PER_YEAR
            ),
        )
    };
    push_guidance_basis(
        rows,
        "Heart-rate zones",
        hr_max_text,
        Some(hr_max_basis),
        &hr_max_row,
    );
}

/// running-005: HR-method preference by resting HR. Below RHR 55 the
/// %HRmax and Karvonen methods diverge → prefer Karvonen; ≥70 they
/// converge; 55–69 the KB states no rule (said honestly).
/// Returns the resting HR when Karvonen targets are selected.
fn push_hr_method_row(rows: &mut Vec<GuidanceView>, resting_hr_bpm: Option<f64>) -> Option<f64> {
    resting_hr_bpm.and_then(|rhr| {
        let pref = running::hr_method_preference(rhr);
        let (text, use_karvonen) = match pref.value {
            running::HrMethodPreference::PreferKarvonen => (
                format!(
                    "Resting HR {rhr:.0} is below 55, so each zone also shows a heart-rate-reserve range (Karvonen), which suits a low resting HR"
                ),
                true,
            ),
            running::HrMethodPreference::EitherConverged => (
                format!("Resting HR {rhr:.0} is 70 or more, so both ways of setting zones give about the same numbers"),
                false,
            ),
            running::HrMethodPreference::Unstated => (
                format!(
                    "Resting HR {rhr:.0} is between 55 and 69. The research gives no preference here, so zones use % of HRmax"
                ),
                false,
            ),
        };
        push_guidance(rows, "Heart-rate zones", text, &pref);
        // The boolean view of the same rule drives the band targets below;
        // the two stay consistent by construction.
        debug_assert_eq!(use_karvonen, running::prefer_karvonen(rhr).value);
        running::prefer_karvonen(rhr).value.then_some(rhr)
    })
}

/// Daniels VDOT bands, easy → hard. Each carries its own RUN-VDOT-001
/// grade; running-002 marks which bands are HR-anchored at all: the fast
/// bands are pace-governed and HR is only a coarse ceiling there. Returns the
/// same bands structured, each pointing at its row.
fn push_band_rows(
    rows: &mut Vec<GuidanceView>,
    hr_max: f64,
    karvonen_rhr: Option<f64>,
) -> Vec<HrZoneBandView> {
    let mut bands = Vec::new();
    for band in [
        VdotBand::Easy,
        VdotBand::Marathon,
        VdotBand::Threshold,
        VdotBand::Interval,
        VdotBand::Repetition,
    ] {
        let (lo_pct, hi_pct) = running::vdot_band_hr_pct(band);
        let (vo2_lo, vo2_hi) = running::vdot_band_vo2max_pct(band);
        let hr_anchored = running::vdot_band_uses_hr(band);
        let bpm_lo = hr_max * lo_pct / 100.0;
        let bpm_hi = hr_max * hi_pct / 100.0;
        let band_row = graded((), claims::RUN_VDOT_001);
        let range = if (bpm_hi - bpm_lo).abs() < 0.5 {
            format!("{bpm_lo:.0} bpm")
        } else {
            format!("{bpm_lo:.0}–{bpm_hi:.0} bpm")
        };
        // Karvonen (%HRR) targets per band when the preference selected them.
        // %HRR ≈ %VO2R ≈ %VO2max (Swain & Leutholtz 1997), NOT %HRmax — so the
        // reserve fraction is the band's %VO2max, never its %HRmax bound, or the
        // range reads ~7–10 bpm too hot for exactly the low-RHR cohort this
        // preference targets (running-005 formula: RHR + frac·(HRmax − RHR)).
        let karvonen = match (karvonen_rhr, vo2_hi) {
            (Some(rhr), Some(vo2_hi)) if hr_anchored => Some((
                load::karvonen_target_hr(rhr, hr_max, vo2_lo / 100.0),
                load::karvonen_target_hr(rhr, hr_max, vo2_hi / 100.0),
            )),
            _ => None,
        };
        let karvonen_seg = karvonen
            .map(|(k_lo, k_hi)| format!(" · Karvonen {k_lo:.0}–{k_hi:.0} bpm"))
            .unwrap_or_default();
        // An open-ended VO2max ceiling (Repetition is ">100 %VO2max") renders as
        // a floor. A pace-governed band (Interval/Repetition) drops the bpm
        // figure entirely — HR lags on short reps, so a bpm target there would
        // read as prescriptive when only pace governs. The %VO2max anchor rides
        // in the why? basis, off the face.
        let vo2_seg = match vo2_hi {
            Some(vo2_hi) => format!("{vo2_lo:.0}–{vo2_hi:.0}%"),
            None => format!("at least {vo2_lo:.0}%"),
        };
        let hr_seg = if hr_anchored {
            format!(" → {range}{karvonen_seg}")
        } else {
            " · go by pace, not heart rate".to_string()
        };
        let (lo_str, hi_str) = (format!("{lo_pct:.0}"), format!("{hi_pct:.0}"));
        let pct_range = if lo_str == hi_str {
            lo_str
        } else {
            format!("{lo_str}–{hi_str}")
        };
        bands.push(HrZoneBandView {
            kind: band,
            name: plan_views::vdot_band_name(band).to_string(),
            pct_low: lo_pct.round(),
            pct_high: hi_pct.round(),
            bpm_low: bpm_lo.round(),
            bpm_high: bpm_hi.round(),
            pace_governed: !hr_anchored,
            karvonen_low_bpm: karvonen.map_or(0.0, |k| k.0.round()),
            karvonen_high_bpm: karvonen.map_or(0.0, |k| k.1.round()),
            row: rows.len() as u32,
        });
        push_guidance_basis(
            rows,
            "Heart-rate zones",
            format!(
                "{}: {pct_range}% of HRmax{hr_seg}",
                plan_views::vdot_band_name(band)
            ),
            Some(format!(
                "This zone works at {vo2_seg} of VO2max (your maximum oxygen uptake), \
                 from the Daniels tables."
            )),
            &band_row,
        );
    }
    bands
}

/// running-036: Maffetone aerobic cap, a base-phase OPTION, never the
/// default (contested vs measured LT1; the claim carries that).
fn push_maf_row(rows: &mut Vec<GuidanceView>, age_years: f64) {
    let maf = running::maf_cap_bpm(age_years, running::MafAdjustment::None);
    push_guidance(
        rows,
        "Heart-rate zones",
        format!(
            "MAF aerobic cap (optional, for base building): 180 − age = {:.0} bpm. With a tested aerobic threshold (LT1), set the cap from that instead",
            maf.value
        ),
        &maf,
    );
}

/// Re-test reminders: zones (running-006) and training paces (running-041)
/// due on the 4–6-week cadence.
fn push_retest_rows(rows: &mut Vec<GuidanceView>, q: &HrZoneQuery) {
    // running-006: recompute zones off a measured max every 4–6 weeks.
    if let Some(weeks) = q.weeks_since_recalc {
        let due = running::hr_zone_recalc_due(weeks);
        if due.value {
            let noun = if weeks == 1 { "week" } else { "weeks" };
            push_guidance(
                rows,
                "Heart-rate zones",
                format!(
                    "Zones last recalculated {weeks} {noun} ago. Recompute from a measured HRmax (every 4–6 weeks)"
                ),
                &due,
            );
        }
    }

    // running-041: training paces re-test on the same 4–6-week cadence.
    if let Some(weeks) = q.weeks_since_pace_test {
        let due = running::pace_retest_due(weeks);
        if due.value {
            let noun = if weeks == 1 { "week" } else { "weeks" };
            push_guidance(
                rows,
                "Heart-rate zones",
                format!(
                    "Paces last tested {weeks} {noun} ago. Re-test to set paces from your current fitness"
                ),
                &due,
            );
        }
    }
}

/// The same resolved figure as the prose HRmax row above, structured so a
/// shell reads bpm / measured-vs-estimate / the Tanaka split from data.
/// The intercept / slope are `running::hr_max_tanaka`'s own constants;
/// they apply only to the age-based estimate, so they are 0 when measured.
fn hr_max_view(q: &HrZoneQuery, measured: Option<f64>, hr_max: f64) -> HrMaxView {
    if measured.is_some() {
        HrMaxView {
            bpm: hr_max.round(),
            measured: true,
            age_years: 0.0,
            tanaka_intercept: 0.0,
            tanaka_slope: 0.0,
        }
    } else {
        HrMaxView {
            bpm: hr_max.round(),
            measured: false,
            age_years: q.age_years,
            tanaka_intercept: running::TANAKA_INTERCEPT_BPM,
            tanaka_slope: running::TANAKA_SLOPE_BPM_PER_YEAR,
        }
    }
}
