//! Logged-lift views and the lift audit: per-set intensity, the memoized
//! lift result list, and the lift-audit guidance rows.

use super::*;

/// Estimated working %1RM for a logged set. With a trustworthy e1RM anchor for
/// the lift, intensity is the actual load as a share of that anchor; without one
/// it falls back to the rep/RPE estimate (File 02 strength-005). A set with
/// neither a loaded-anchor comparison nor a recorded RPE has no knowable
/// intensity.
pub(super) fn set_intensity_pct(set: &LoggedSet, anchor: Option<f64>) -> Option<f64> {
    match anchor {
        Some(a) if a > 0.0 && set.weight_kg > 0.0 => Some(set.weight_kg / a * 100.0),
        _ if set.rpe > 0.0 => Some(strength::est_pct_1rm(set.reps, set.rpe)),
        _ => None,
    }
}

/// Lowest mean working %1RM audited against Prilepin's table, a strength/power
/// construct (File 02 strength-011/013).
const PRILEPIN_MIN_PCT: f64 = 70.0;

/// Highest rep count per set audited against Prilepin's table (File 02
/// strength-011/013).
const PRILEPIN_MAX_REPS: u32 = 6;

/// How far back a back-squat set may lie and still anchor the depth-jump gate.
/// An engine window (the KB gives no recency parameter): an older e1RM does
/// not vouch for current strength.
const SQUAT_ANCHOR_RECENCY_DAYS: i64 = 90;

/// Lift-audit rows carry no section: the shell titles the list with its own
/// overline, so no prose label has to match between core and shell.
const AUDIT_SECTION: &str = "";

/// Lift-session audit for the most recent dated lifting day. Only rows that ask
/// for a change are emitted: a Prilepin total-rep check per exercise that falls
/// OUTSIDE the band (strength-013, descriptive %1RM from the rep count), and the
/// depth-jump readiness gate when it is NOT met (profile bodyweight + a squat
/// e1RM in the log). A passed check says nothing.
pub(super) fn build_lift_audit(cx: &ViewCtx) -> Vec<GuidanceView> {
    let model = cx.model;
    use std::collections::BTreeMap;
    let mut rows = Vec::new();
    // Calisthenics ladder work is governed by rep-max-at-tier, not %1RM/e1RM/
    // Prilepin (CAL-E1RM-001): for a Calisthenics profile a set named like a
    // leverage rung is excluded from every %1RM-derived audit row. A Weights
    // profile is unaffected — a loaded "Bulgarian split squat" still audits.
    let calis = model
        .profile
        .as_ref()
        .is_some_and(|p| p.resistance_modality == ResistanceModality::Calisthenics);
    // Exclude two kinds of set from every %1RM/e1RM/Prilepin audit row: a
    // Calisthenics profile's leverage-rung ladder work (rep-max-at-tier, not
    // %1RM — CAL-E1RM-001), and ANY timed hold (a hold has no e1RM regardless of
    // profile), so a weighted hold never fabricates a Prilepin/%1RM audit row.
    let include = |s: &LoggedSet| {
        s.hold_sec.is_none() && (!calis || crate::calisthenics::tier_by_name(&s.exercise).is_none())
    };
    let offset = model.today_utc_offset_sec;
    // Anchor on the most recent dated lifting day AT/BEFORE local today, so a
    // backdated future-timestamped set never pulls the audit onto a day that
    // has not happened (matching the sibling audits' `end_of_local_today_sec`).
    let today_end = end_of_local_today_sec(model);
    let within_today = |t: i64| today_end.is_none_or(|end| t <= end);

    if let Some(last_day) = model
        .sets
        .iter()
        .filter(|s| s.observed_at > 0 && within_today(s.observed_at))
        .map(|s| local_day(s.observed_at, offset))
        .max()
    {
        // Per-exercise day totals over WORKING sets: reps, the rep-weighted
        // working %1RM (actual load vs the lift's e1RM anchor where one exists,
        // else the rep/RPE estimate), the reps that carried a knowable
        // intensity, and the heaviest single set. Warm-up sets are not working
        // sets (STR-WARMUP-001: the ramp tops out near 80 % of the working
        // load), so a set lighter than the deepest back-off drop below the
        // day's heaviest same-exercise load (STR-BACKOFF-001: 10–15 %) is left
        // out; counting a warm-up pyramid would drag the mean under 70 % or
        // push the rep max past 6 and silence the check.
        let anchors = cx.lift_anchors_now();
        let day_sets: Vec<&LoggedSet> = model
            .sets
            .iter()
            .filter(|s| include(s) && local_day(s.observed_at, offset) == last_day)
            .collect();
        // Key on the trimmed lower-cased name (matching the e1RM chain's key)
        // so "back squat"/"Back squat" are one exercise.
        let key = |s: &LoggedSet| s.exercise.trim().to_lowercase();
        let mut heaviest: BTreeMap<String, f64> = BTreeMap::new();
        for s in &day_sets {
            let h = heaviest.entry(key(s)).or_insert(0.0);
            *h = h.max(s.weight_kg);
        }
        let working_floor = 1.0 - strength::rpe_anchored_back_off().value.drop_frac.1;
        let mut by_exercise: BTreeMap<String, (u32, f64, u32, u32, &str)> = BTreeMap::new();
        for s in day_sets {
            let k = key(s);
            if s.weight_kg + 1e-9 < working_floor * heaviest.get(&k).copied().unwrap_or(0.0) {
                continue;
            }
            // Display the first-seen original spelling.
            let e = by_exercise
                .entry(k)
                .or_insert((0, 0.0, 0, 0, s.exercise.as_str()));
            e.0 += s.reps;
            e.3 = e.3.max(s.reps);
            if let Some(pct) = set_intensity_pct(s, anchors.e1rm_for(&s.exercise)) {
                e.1 += f64::from(s.reps) * pct;
                e.2 += s.reps;
            }
        }
        for (_, (total_reps, pct_sum, intensity_reps, max_reps, exercise)) in by_exercise {
            if total_reps == 0 || intensity_reps == 0 {
                continue;
            }
            let mean_pct = pct_sum / f64::from(intensity_reps);
            // Prilepin's optimal-volume table is a strength/power construct: audit
            // against it only for non-isolation work loaded at ≥70 %1RM in sets of
            // ≤6 reps. A hypertrophy set as prescribed sits outside that table and
            // gets no row.
            let isolation = crate::plan::is_isolation_lift(exercise);
            if !isolation && mean_pct >= PRILEPIN_MIN_PCT && max_reps <= PRILEPIN_MAX_REPS {
                let ok = strength::prilepin_volume_ok(
                    mean_pct,
                    total_reps.min(u32::from(u16::MAX)) as u16,
                );
                if !ok.value
                    && let Some(row) = strength::prilepin_for(mean_pct)
                {
                    let (lo, hi) = row.total_range;
                    let side = if total_reps > u32::from(hi) {
                        "more"
                    } else {
                        "fewer"
                    };
                    push_guidance(
                        &mut rows,
                        AUDIT_SECTION,
                        format!(
                            "{exercise}: {total_reps} reps at ~{mean_pct:.0}% of your max is {side} than the usual {lo}–{hi} for that weight"
                        ),
                        &ok,
                    );
                }
            }

            // hypertrophy-012: below ~30 %1RM the stimulus underperforms even
            // near failure: flag the load floor per exercise-day.
            let floor =
                hypertrophy::load_below_effective_floor(mean_pct.round().clamp(0.0, 100.0) as u8);
            if floor.value {
                push_guidance(
                    &mut rows,
                    AUDIT_SECTION,
                    format!(
                        "{exercise}: ~{mean_pct:.0}% of your max is below the ~{}% that still builds muscle. Add weight",
                        hypertrophy::EFFECTIVE_LOAD_FLOOR_PCT
                    ),
                    &floor,
                );
            }
        }

        // hypertrophy-020: RIR estimates are only ~±1-rep accurate at 0–5 RIR;
        // beyond that the report is a guess: say so once for the day.
        if let Some(worst_rir) = model
            .sets
            .iter()
            .filter(|s| local_day(s.observed_at, offset) == last_day)
            .filter(|s| include(s))
            // An unrecorded RPE (0.0) carries no RIR: it must not fabricate a
            // "~10 RIR" reliability row.
            .filter(|s| s.rpe > 0.0)
            .map(|s| strength::rpe_to_rir(s.rpe).round().clamp(0.0, 255.0) as u8)
            .max()
        {
            let rel = hypertrophy::rir_reliability(worst_rir);
            if rel.value != hypertrophy::RirReliability::WithinOneRep {
                push_guidance(
                    &mut rows,
                    AUDIT_SECTION,
                    format!(
                        "A set was logged with about {worst_rir} reps left. Past {} reps left, that guess is off by 2 or more reps, so now and then take a set closer to failure to check",
                        hypertrophy::RIR_RELIABLE_MAX
                    ),
                    &rel,
                );
            }
        }

        // strength-006: when a set that day sits outside the reliable e1RM
        // window, point at the preferred 3–6-rep test set instead of trusting
        // formula output.
        if model
            .sets
            .iter()
            .filter(|s| local_day(s.observed_at, offset) == last_day)
            .filter(|s| include(s))
            .any(|s| s.reps > strength::E1RM_RELIABLE_REP_CAP)
        {
            let test = strength::e1rm_test_set_reps();
            push_guidance(
                &mut rows,
                AUDIT_SECTION,
                format!(
                    "Sets over {} reps give only a rough max. A {}–{}-rep set gauges strength better",
                    strength::E1RM_RELIABLE_REP_CAP,
                    test.value.0,
                    test.value.1
                ),
                &test,
            );
        }
    }

    // Depth-jump gate (strength plyometrics): squat ≥1.5× bodyweight.
    if let Some(bw) = model.profile.as_ref().and_then(|p| p.bodyweight_kg)
        && bw > 0.0
    {
        // Only a reliable, recent BARBELL BACK SQUAT anchors the 1.5×BW gate:
        // machine/unilateral squats ("leg press", "hack squat", split squats) and
        // high-rep (>10) sets do not estimate a trustworthy 1RM (strength-006/033).
        // A "smith"/"machine" token on the raw name is rejected before the
        // equipment-insensitive match, so a guided-bar Smith-machine back squat
        // never clears a free-barbell plyometric gate.
        let recent_cutoff =
            today_end.map(|end| end.saturating_sub(SQUAT_ANCHOR_RECENCY_DAYS * DAY_SEC));
        let squat_e1rm = model
            .sets
            .iter()
            .filter(|s| {
                include(s)
                    && crate::plan::movement_pattern(&s.exercise)
                        == Some(crate::plan::MovementPattern::Squat)
                    && !s
                        .exercise
                        .split(|c: char| !c.is_ascii_alphanumeric())
                        .any(|t| {
                            t.eq_ignore_ascii_case("smith") || t.eq_ignore_ascii_case("machine")
                        })
                    && crate::plan::same_movement("Barbell back squat", &s.exercise)
                    && crate::plan::e1rm_reliable_name(&s.exercise)
                    && strength::e1rm_reliable(strength::effective_reps(s.reps, s.rpe), false).value
                    && within_today(s.observed_at)
                    && recent_cutoff.is_none_or(|c| s.observed_at >= c)
            })
            .map(|s| strength::e1rm_epley_rpe(s.weight_kg, s.reps, s.rpe))
            .fold(0.0_f64, f64::max);
        if squat_e1rm > 0.0 {
            let ready = strength::depth_jump_ready(squat_e1rm, bw);
            // Only the unmet gate asks for anything; a cleared gate is silent.
            if !ready.value {
                let lb = model.profile.as_ref().is_some_and(|p| p.weight_lb);
                let unit = weight_suffix(lb);
                let ratio = strength::DEPTH_JUMP_SQUAT_BW_RATIO;
                let (e1rm_disp, need_disp) =
                    (weight_num(squat_e1rm, lb), weight_num(ratio * bw, lb));
                push_guidance(
                    &mut rows,
                    AUDIT_SECTION,
                    format!(
                        "Hold off on depth jumps: your squat max ({e1rm_disp:.0} {unit}) is under {ratio}× bodyweight ({need_disp:.0} {unit})"
                    ),
                    &ready,
                );
            }
        }
    }

    rows
}

/// Cap on the whole-list view memos ([`lift_views`] / [`run_views`]). Only a
/// couple of distinct set/run histories are live within a session (the current
/// one plus a just-superseded one across an edit), and `LIFT_VIEWS_CACHE` needs
/// the kg AND lb slots of an unchanged `model.sets` warm simultaneously (its key
/// folds `weight_lb`), so a small cap bounds growth without ever evicting the
/// hot keys. Each entry is a full-history list clone, so the cap directly bounds
/// worst-case accumulation.
pub(super) const VIEW_LIST_CACHE_CAP: usize = 8;

thread_local! {
    /// The finished lift list keyed by the sets' content fingerprint and the
    /// display unit: `model.sets` changes only on a log/amend/delete, so an
    /// unchanged model skips the sort and per-set e1RM math.
    static LIFT_VIEWS_CACHE: std::cell::RefCell<Memo<(u64, bool), Vec<LiftResultView>>> =
        std::cell::RefCell::new(Memo::new(VIEW_LIST_CACHE_CAP));
}

/// Cheap content fingerprint of the logged sets, every field a `LiftResultView`
/// depends on. Integer/string hashing versus the sort + per-set e1RM math +
/// string allocation the cached list avoids. Collision risk is negligible for a
/// personal history; the fingerprint also keys the plan snapshots.
pub(super) fn sets_fingerprint(sets: &[LoggedSet]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sets.len().hash(&mut h);
    for s in sets {
        s.entry_id.hash(&mut h);
        s.observed_at.hash(&mut h);
        s.exercise.hash(&mut h);
        s.weight_kg.to_bits().hash(&mut h);
        s.reps.hash(&mut h);
        s.rpe.to_bits().hash(&mut h);
        s.hold_sec.hash(&mut h);
    }
    h.finish()
}

/// Flatten every logged set, threading each exercise's previous e1RM through so
/// the view carries the per-lift trend (delta + direction) and the shell renders
/// it without arithmetic. "Previous" = the most recent earlier logged set of the
/// same exercise (exact name match), the core holds no session boundary for
/// sets, so set-over-set is the deterministic proxy for session-over-session.
/// Memoized on `sets_fp` ([`sets_fingerprint`] of `sets`) and the display unit,
/// so a kg↔lb toggle misses rather than serving a stale-unit summary.
pub(super) fn lift_views(sets: &[LoggedSet], sets_fp: u64, weight_lb: bool) -> Vec<LiftResultView> {
    memoized(&LIFT_VIEWS_CACHE, (sets_fp, weight_lb), || {
        lift_views_uncached(sets, weight_lb)
    })
}

/// The uncached lift-list derivation (see [`lift_views`]).
fn lift_views_uncached(sets: &[LoggedSet], weight_lb: bool) -> Vec<LiftResultView> {
    // Chronological (observed_at) order, tie-broken by `entry_id` (the shell's
    // monotonic epoch-MILLIS log id) so same-second sets keep a deterministic
    // order regardless of Vec position. `AmendSet` removes+pushes its row to the
    // Vec tail carrying the SAME `entry_id`; keying the sort on `entry_id` (not
    // insertion order) keeps the amended set in its original slot among ties, so
    // both History display and the per-exercise e1RM delta chain are stable
    // across amends. Legacy rows (`entry_id == 0`) fall back to the stable sort's
    // insertion order. A backdated set still slots into its true position.
    let mut ordered: Vec<&LoggedSet> = sets.iter().collect();
    ordered.sort_by(|a, b| {
        a.observed_at
            .cmp(&b.observed_at)
            .then(a.entry_id.cmp(&b.entry_id))
    });
    let mut last_e1rm: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    // Best prior e1RM per exercise, for the neutral PR marker.
    let mut best_e1rm: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    ordered
        .into_iter()
        .map(|s| {
            // Key the trend on the trimmed, lowercased name so "Squat" and
            // "squat" share one chain. This is NOT the plan anchors'
            // `movement_key`: "Smith machine back squat" and "Back squat" keep
            // separate chains here. A retired catalog label ("Barbell back
            // squat") was stored under its current name at ingest, so it
            // shares the current name's chain.
            let key = s.exercise.trim().to_lowercase();
            let prev = last_e1rm.get(&key).copied();
            let mut view = to_lift_view(s, prev, weight_lb);
            // A hold carries no e1RM (its field is zeroed): keep the previous
            // real e1RM as the trend anchor so an interleaved hold never breaks
            // the same-exercise delta chain.
            if s.hold_sec.is_none() {
                last_e1rm.insert(key.clone(), view.e1rm_kg);
                if view.e1rm_kg > 0.0 {
                    // A PR beats every PRIOR e1RM of the exercise (the first
                    // set of an exercise is not flagged: nothing to beat).
                    match best_e1rm.get(&key).copied() {
                        Some(best) if view.e1rm_kg > best + 0.05 => {
                            view.e1rm_pr = true;
                            best_e1rm.insert(key, view.e1rm_kg);
                        }
                        Some(_) => {}
                        None => {
                            best_e1rm.insert(key, view.e1rm_kg);
                        }
                    }
                }
            }
            view
        })
        .collect()
}

/// Derive strength metrics for one logged set (Epley e1RM, RIR from RPE).
/// `prev_e1rm_kg` is the same exercise's previous e1RM (already 0.1-rounded),
/// `None` for its first logged set.
pub(super) fn to_lift_view(
    s: &LoggedSet,
    prev_e1rm_kg: Option<f64>,
    weight_lb: bool,
) -> LiftResultView {
    // A timed hold is isometric time, not a rep-max, so it has NO e1RM / %1RM /
    // cross-check / trend (a hold has no e1RM; CAL-E1RM-001). Its numeric
    // strength fields stay zeroed and the delta/direction stay `None`, so a
    // weighted hold ("Weighted plank") never fabricates a %1RM or an e1RM trend
    // (the trend chain in `lift_views_uncached` skips holds too).
    let hold = s.hold_sec.is_some();
    let e1rm_kg = if hold {
        0.0
    } else {
        (strength::e1rm_epley_rpe(s.weight_kg, s.reps, s.rpe) * 10.0).round() / 10.0
    };
    let pct_1rm = if hold {
        0.0
    } else {
        strength::est_pct_1rm(s.reps, s.rpe).round()
    };
    let rir = strength::rpe_to_rir(s.rpe);
    let rpe_recorded = s.rpe > 0.0;

    // strength-005/006: cross-check the e1RM across Epley/Brzycki/Lombardi and
    // surface the spread. Isolation lifts (per the File 03 exercise catalog),
    // out-of-range rep counts, and timed holds yield `None`: the formulas are
    // unreliable / undefined there (the shell should suggest a 3–6-rep test set
    // instead). An exercise the catalog does not know is treated as
    // non-isolation: the exclusion is isolation-specific, not unknown-specific.
    let cross_check = if hold {
        None
    } else {
        let isolation = crate::plan::is_isolation_lift(&s.exercise);
        let check = strength::e1rm_cross_check(s.weight_kg, s.reps, s.rpe, isolation);
        check.value.map(|c| E1rmRangeView {
            low_kg: (c.low_kg * 10.0).round() / 10.0,
            high_kg: (c.high_kg * 10.0).round() / 10.0,
            formulas: c.formulas_used,
            evidence: EvidenceTagView::of(&check),
        })
    };
    // Delta of two 0.1-rounded values, re-rounded to kill float dust; direction
    // is factual (what changed), never an improving/declining judgment, that
    // phrasing belongs to the trend rules (see the field docs). A hold carries no
    // e1RM, so it has no delta/direction.
    let e1rm_delta_kg = if hold {
        None
    } else {
        prev_e1rm_kg.map(|p| ((e1rm_kg - p) * 10.0).round() / 10.0)
    };
    let e1rm_direction = e1rm_delta_kg.map(|d| {
        if d > 0.0 {
            TrendDirection::Up
        } else if d < 0.0 {
            TrendDirection::Down
        } else {
            TrendDirection::Flat
        }
    });
    LiftResultView {
        exercise: s.exercise.clone(),
        weight_kg: s.weight_kg,
        reps: s.reps,
        rpe: s.rpe,
        e1rm_kg,
        pct_1rm,
        rir,
        rpe_recorded,
        cross_check,
        e1rm_delta_kg,
        e1rm_direction,
        summary: if let Some(sec) = s.hold_sec {
            // A timed-hold set renders "<name> - <s> s hold", never reps×load→e1RM
            // (a hold has no e1RM; CAL-E1RM-001 / CAL-PROG-STATIC-001).
            format!("{} - {sec} s hold", s.exercise)
        } else {
            // `{}` (not `{:.0}`) on a kg weight so a fractional plate load (e.g.
            // 92.5 kg on 2.5 kg jumps, 101.25 kg with micro-plates) echoes what
            // the lifter actually did; integer loads still print clean ("100kg").
            // A lb weight and every e1RM go through the one display rounding
            // (`fmt_weight_num`: lb whole or .5, kg one decimal), so the e1RM
            // here reads the same as everywhere else. The unit suffix is GLUED.
            let unit = weight_suffix(weight_lb);
            let w = if weight_lb {
                fmt_weight_num(s.weight_kg, true)
            } else {
                format!("{}", s.weight_kg)
            };
            let e1 = fmt_weight_num(e1rm_kg, weight_lb);
            let pct_clause = if pct_1rm > 0.0 {
                format!("~{pct_1rm:.0}%1RM, ")
            } else {
                String::new() // 0-rep degenerate sentinel: no misleading %1RM
            };
            if rpe_recorded {
                // An open-ended rating ("4+ reps left") only bounds the max
                // from below (C11): say so instead of stating it as exact.
                let at_least = if strength::rpe_is_exact(s.rpe) {
                    ""
                } else {
                    "≥"
                };
                format!(
                    "{} {w}{unit} × {} @RPE{:.1} → e1RM {at_least}{e1}{unit} ({pct_clause}{:.1} RIR)",
                    s.exercise, s.reps, s.rpe, rir
                )
            } else {
                // No RPE was recorded: omit the fabricated "@RPE0.0" and the
                // "10.0 RIR" clause it would imply.
                format!(
                    "{} {w}{unit} × {} → e1RM {e1}{unit} ({pct_clause}RPE not recorded)",
                    s.exercise, s.reps,
                )
            }
        },
        observed_at: s.observed_at,
        entry_id: s.entry_id,
        hold_sec: s.hold_sec,
        // Set by `lift_views`, which sees the exercise's prior history.
        e1rm_pr: false,
        tonnage_kg: if hold {
            0.0
        } else {
            s.weight_kg * f64::from(s.reps)
        },
    }
}
