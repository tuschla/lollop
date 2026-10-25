//! Differential / property tests: every calculator the app DISPLAYS, checked
//! against an INDEPENDENT oracle transcribed from the knowledge-base text (each
//! oracle cites its KB file:line), over wide seeded input grids including the
//! boundaries, plus monotonicity / consistency properties (faster race → faster
//! paces, higher HRmax → higher zones, unit round trips stable, prose == the
//! structured figure it stands for).
//!
//! The oracles deliberately do NOT call the engine's formula functions: they
//! are re-derived from the KB so a transcription slip on either side shows up
//! as a disagreement. Where the app documents a convention the KB leaves open
//! (e.g. the single-rep identity, the MP-relative easy pace), the oracle
//! follows the app's stated convention and says so.

use super::*;
use crate::app::run_targets::{HrAnchor, TargetBand, band_for, resolve_target};

// ─── Independent oracles (KB-cited) ─────────────────────────────────────────

mod oracle {
    /// KB extracted/07-data-harvesting.md:158 — Tanaka 2001: `208 − 0.7·age`.
    /// (04-running.md:65 restates it.)
    pub fn tanaka(age: f64) -> f64 {
        208.0 - 0.7 * age
    }

    /// 07-data-harvesting.md:163 and 04-running.md:54 — Karvonen:
    /// `target = HRrest + frac·(HRmax − HRrest)`.
    pub fn karvonen(rest: f64, max: f64, frac: f64) -> f64 {
        rest + (max - rest) * frac
    }

    /// 04-running.md:464-467 — Daniels band table (%VO2max, %HRmax).
    /// Returns (vo2_lo, vo2_hi, hr_lo, hr_hi); Repetition (04:468) is ">100%
    /// VO2max", "use pace, not HR".
    pub fn band_row(band: crate::schema::VdotBand) -> (f64, Option<f64>, Option<(f64, f64)>) {
        use crate::schema::VdotBand::*;
        match band {
            Easy => (59.0, Some(74.0), Some((65.0, 79.0))),
            Marathon => (80.0, Some(84.0), Some((80.0, 85.0))),
            Threshold => (83.0, Some(88.0), Some((88.0, 92.0))),
            Interval => (95.0, Some(100.0), Some((97.0, 100.0))),
            Repetition => (100.0, None, None),
        }
    }

    /// 07-data-harvesting.md:172 — `VO2 = −4.60 + 0.182258·v + 0.000104·v²`, v m/min.
    pub fn vo2(v: f64) -> f64 {
        0.000104 * v * v + 0.182258 * v - 4.60
    }

    /// 07-data-harvesting.md:173 — fraction of VO2max sustainable for t min.
    pub fn pct_max(t_min: f64) -> f64 {
        0.8 + 0.2989558 * (-0.1932605 * t_min).exp() + 0.1894393 * (-0.012778 * t_min).exp()
    }

    /// 07-data-harvesting.md:174 — `VDOT = VO2(v)/%max(t)`, v = d/t.
    pub fn vdot(d_m: f64, t_s: f64) -> f64 {
        let t_min = t_s / 60.0;
        vo2(d_m / t_min) / pct_max(t_min)
    }

    /// Finish time over `d_m` for a runner of `target` VDOT: the root of
    /// `vdot(d, t) = target`, found by bisection in LOG time over 1 s … 10
    /// days (no 12 h cap; a different bracket and variable from the engine).
    pub fn daniels_time(target: f64, d_m: f64) -> f64 {
        let (mut lo, mut hi) = (0.0_f64, (10.0 * 86_400.0_f64).ln());
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if vdot(d_m, mid.exp()) > target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (0.5 * (lo + hi)).exp()
    }

    /// Speed (m/min) whose VO2 cost (07:172) equals `vo2`: the positive root
    /// of the quadratic, solved with the numerically stable form.
    pub fn speed_for_vo2(target: f64) -> f64 {
        let (a, b, c) = (0.000104_f64, 0.182258_f64, -(4.60 + target));
        let q = -0.5 * (b + (b * b - 4.0 * a * c).sqrt());
        c / q
    }

    /// Pace (s/km) at `pct` % of VO2max for `vdot` (04:74-76 running-007:
    /// paces from the CURRENT VDOT's band %VO2max).
    pub fn pace_at(vdot: f64, pct: f64) -> f64 {
        1000.0 / speed_for_vo2(vdot * pct / 100.0) * 60.0
    }

    /// 07-data-harvesting.md:185 — Riegel exponent by weekly volume:
    /// 100+ km ≈ 1.04; 60–100 ≈ 1.06; 30–60 ≈ 1.09; <30 ≈ 1.12. (Grids avoid
    /// the exact shared boundaries 30/60/100, which the KB leaves ambiguous.)
    pub fn riegel_exponent(weekly_km: f64) -> f64 {
        match weekly_km {
            w if w >= 100.0 => 1.04,
            w if w >= 60.0 => 1.06,
            w if w >= 30.0 => 1.09,
            _ => 1.12,
        }
    }

    /// 07-data-harvesting.md:185 — `t2 = t1·(d2/d1)^k`.
    pub fn riegel(t1: f64, d1: f64, d2: f64, k: f64) -> f64 {
        (d2.ln() - d1.ln()).mul_add(k, t1.ln()).exp()
    }

    /// 04-running.md:427-428 (running-039) — midpoint when the two methods
    /// agree within ~2%, else the (low, high) span. Returns (low, high, agreed).
    pub fn equivalency(a: f64, b: f64) -> (f64, f64, bool) {
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        if hi / lo - 1.0 <= 0.02 {
            let mid = 0.5 * (a + b);
            (mid, mid, true)
        } else {
            (lo, hi, false)
        }
    }

    /// 02-strength-power.md:54 — Epley `load×(1+reps/30)`.
    pub fn epley(w: f64, r: f64) -> f64 {
        w + w * r / 30.0
    }
    /// 02-strength-power.md:54 — Brzycki `load/(1.0278−0.0278×reps)` (the KB's
    /// 4-decimal rounding of 37/36 and 1/36).
    pub fn brzycki_kb(w: f64, r: f64) -> f64 {
        w / (1.0278 - 0.0278 * r)
    }
    /// 02-strength-power.md:54 — Lombardi `load×reps^0.10`.
    pub fn lombardi(w: f64, r: f64) -> f64 {
        w * (0.10 * r.ln()).exp()
    }
    /// 02-strength-power.md:76 — RPE 6–10 ↔ RIR 4–0, RPE 10 = 0 RIR.
    pub fn rir_of_rpe(rpe: f64) -> f64 {
        4.0 - (rpe - 6.0)
    }

    /// 08-individualization-safety.md:165 (masters ≥1.2–1.6 g/kg/d) and
    /// 08:242-243 (deficit 1.8–2.7 g/kg/d).
    pub const MASTERS_G_PER_KG: (f64, f64) = (1.2, 1.6);
    pub const DEFICIT_G_PER_KG: (f64, f64) = (1.8, 2.7);

    /// 07-data-harvesting.md:176 — Cooper `(d − 504.9)/44.73`.
    pub fn cooper(d: f64) -> f64 {
        (d - 504.9) / 44.73
    }

    /// 07-data-harvesting.md:179-180 — CS from two efforts
    /// `CS = (D2 − D1)/(T2 − T1)`, `D′` the intercept of `D = CS·t + D′`.
    pub fn cs_two(d1: f64, t1: f64, d2: f64, t2: f64) -> (f64, f64) {
        let cs = (d2 - d1) / (t2 - t1);
        (cs, d2 - cs * t2)
    }

    /// 04-running.md:395 — MAF cap `180 − age` (no adjustment).
    pub fn maf(age: f64) -> f64 {
        180.0 - age
    }

    /// 04-running.md:32 (running-003) — Z1 <~82 %HRmax, Z2 ~82–88, Z3 >~88;
    /// 07-data-harvesting.md:191 Lucia TRIMP weights the zones 1/2/3.
    pub fn lucia_weight(pct_hr_max: f64) -> f64 {
        if pct_hr_max < 82.0 {
            1.0
        } else if pct_hr_max < 88.0 {
            2.0
        } else {
            3.0
        }
    }

    /// 07-data-harvesting.md:144 (`α = 1 − exp(−Δt/τ)`) with 07:201-204
    /// (CTL τ = 42 d, ATL τ = 7 d), Δt = 1 day.
    pub fn ewma_step(prev: f64, x: f64, tau: f64) -> f64 {
        let keep = (-1.0 / tau).exp();
        prev * keep + x * (1.0 - keep)
    }

    /// One international mile, metres (exact by the 1959 yard definition).
    pub const MILE_M: f64 = 1609.344;
    /// One avoirdupois pound, kilograms (exact by definition).
    pub const KG_PER_LB: f64 = 0.45359237;

    /// Round half AWAY from zero to a whole number, computed exactly (the
    /// fractional part of a finite double below 2^52 is exact).
    pub fn round_half_away(x: f64) -> f64 {
        let t = x.trunc();
        let f = (x - t).abs();
        if f >= 0.5 { t + x.signum() } else { t }
    }

    /// Clock oracle: whole seconds (half away), `h:mm:ss` from one hour, else
    /// `m:ss`; ":59.5" carries into the next minute / hour.
    pub fn clock(sec: f64) -> String {
        let s = if sec.is_finite() && sec > 0.0 {
            round_half_away(sec) as u64
        } else {
            0
        };
        let (h, m, ss) = (s / 3600, (s / 60) % 60, s % 60);
        if h > 0 {
            format!("{h}:{m:02}:{ss:02}")
        } else {
            format!("{}:{ss:02}", s / 60)
        }
    }

    pub fn mss(sec: f64) -> String {
        let s = if sec.is_finite() && sec > 0.0 {
            round_half_away(sec) as u64
        } else {
            0
        };
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Deterministic SplitMix64 for the seeded sweeps.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// `app == oracle.round()`, except an exact .5 tie in the oracle may land on
/// either neighbour (float noise decides which side the engine computed).
fn rounds_to(app: f64, exact: f64) -> bool {
    let f = exact - exact.floor();
    if (f - 0.5).abs() < 1e-9 {
        app == exact.floor() || app == exact.ceil()
    } else {
        app == oracle::round_half_away(exact)
    }
}

const BANDS: [VdotBand; 5] = [
    VdotBand::Easy,
    VdotBand::Marathon,
    VdotBand::Threshold,
    VdotBand::Interval,
    VdotBand::Repetition,
];

fn hr_query(age: f64, rhr: Option<f64>) -> HrZoneQuery {
    HrZoneQuery {
        age_years: age,
        resting_hr_bpm: rhr,
        weeks_since_recalc: None,
        weeks_since_pace_test: None,
    }
}

// ─── HRmax (Tanaka) + %HRmax / Karvonen zone table ─────────────────────────

#[test]
fn hr_zone_table_matches_tanaka_and_daniels_table_over_age_and_rhr_grid() {
    // Ages 5–100 in quarter years (the app's stated Tanaka window), resting
    // HR absent / Karvonen-selecting (<55) / %HRmax-only (55–69, ≥70).
    let rhrs = [
        None,
        Some(35.0),
        Some(48.0),
        Some(54.9),
        Some(60.0),
        Some(72.0),
    ];
    let mut checked = 0;
    for qa in 20..=400 {
        let age = f64::from(qa) * 0.25;
        for rhr in rhrs {
            let (_, hr_max, bands) = build_hr_zones(&hr_query(age, rhr), None);
            let hm = hr_max.expect("age 5–100 must yield an HRmax");
            let exact_max = oracle::tanaka(age);
            assert!(
                rounds_to(hm.bpm, exact_max),
                "age {age}: HRmax {} vs {exact_max}",
                hm.bpm
            );
            assert!(!hm.measured);
            assert_eq!((hm.tanaka_intercept, hm.tanaka_slope), (208.0, 0.7));
            assert_eq!(bands.len(), 5);
            for (b, band) in bands.iter().zip(BANDS) {
                assert_eq!(b.kind, band);
                let (vo2_lo, vo2_hi, hr) = oracle::band_row(band);
                // Interval/Repetition are pace-governed (04:467-468, "use pace").
                let pace_governed = matches!(band, VdotBand::Interval | VdotBand::Repetition);
                assert_eq!(b.pace_governed, pace_governed, "{band:?}");
                if let Some((lo, hi)) = hr {
                    assert_eq!((b.pct_low, b.pct_high), (lo, hi), "{band:?} %HRmax");
                    assert!(
                        rounds_to(b.bpm_low, exact_max * lo / 100.0),
                        "age {age} {band:?} low {}",
                        b.bpm_low
                    );
                    assert!(
                        rounds_to(b.bpm_high, exact_max * hi / 100.0),
                        "age {age} {band:?} high {}",
                        b.bpm_high
                    );
                }
                // Karvonen only when RHR < 55 (04:54) and only on HR-anchored bands;
                // the reserve fraction is the band's %VO2max (%HRR ≈ %VO2R).
                match (rhr, vo2_hi) {
                    (Some(r), Some(hi)) if r < 55.0 && !pace_governed => {
                        assert!(rounds_to(
                            b.karvonen_low_bpm,
                            oracle::karvonen(r, exact_max, vo2_lo / 100.0)
                        ));
                        assert!(rounds_to(
                            b.karvonen_high_bpm,
                            oracle::karvonen(r, exact_max, hi / 100.0)
                        ));
                    }
                    _ => assert_eq!(
                        (b.karvonen_low_bpm, b.karvonen_high_bpm),
                        (0.0, 0.0),
                        "age {age} rhr {rhr:?} {band:?}"
                    ),
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 10_000);
}

#[test]
fn hr_zone_table_uses_measured_max_and_refuses_out_of_window_ages() {
    // A plausible measured max (100–240) replaces Tanaka for every age, even
    // one outside the Tanaka window; implausible ones fall back.
    for m in 100..=240 {
        let m = f64::from(m);
        for age in [0.0, 13.0, 40.0, 100.0, 130.0] {
            let (_, hm, bands) = build_hr_zones(&hr_query(age, None), Some(m));
            let hm = hm.expect("measured max needs no age");
            assert!(hm.measured && hm.bpm == m);
            for b in bands.iter().filter(|b| !b.pace_governed) {
                assert!(
                    rounds_to(b.bpm_low, m * b.pct_low / 100.0)
                        && rounds_to(b.bpm_high, m * b.pct_high / 100.0)
                );
            }
        }
    }
    for age in [-5.0, 0.0, 4.99, 100.01, 120.0] {
        for measured in [None, Some(99.0), Some(241.0), Some(f64::NAN)] {
            let (rows, hm, bands) = build_hr_zones(&hr_query(age, None), measured);
            assert!(
                hm.is_none() && bands.is_empty(),
                "age {age} measured {measured:?}"
            );
            assert_eq!(rows.len(), 1, "one explanatory row");
        }
    }
}

#[test]
fn hr_zones_rise_with_hr_max_and_are_ordered_easy_to_interval() {
    // Higher HRmax (younger) → every band's bpm figure non-decreasing; bands
    // never invert (Easy ≤ Marathon ≤ Threshold ≤ Interval).
    let mut prev: Option<Vec<HrZoneBandView>> = None;
    for qa in (20..=400).rev() {
        let age = f64::from(qa) * 0.25;
        let (_, _, bands) = build_hr_zones(&hr_query(age, Some(45.0)), None);
        for w in bands.windows(2).take(3) {
            assert!(
                w[0].bpm_low <= w[1].bpm_low && w[0].bpm_high <= w[1].bpm_high,
                "age {age}"
            );
        }
        for b in &bands {
            assert!(b.bpm_low <= b.bpm_high && b.karvonen_low_bpm <= b.karvonen_high_bpm);
        }
        if let Some(p) = &prev {
            // `prev` is the OLDER age (lower HRmax): this one must not be lower.
            for (old, new) in p.iter().zip(&bands) {
                assert!(
                    new.bpm_low >= old.bpm_low && new.bpm_high >= old.bpm_high,
                    "age {age}"
                );
                assert!(
                    new.karvonen_low_bpm >= old.karvonen_low_bpm
                        && new.karvonen_high_bpm >= old.karvonen_high_bpm
                );
            }
        }
        prev = Some(bands);
    }
}

#[test]
fn hr_zones_reach_the_view_and_maf_matches_180_minus_age() {
    let app = Engine;
    for age in [13.0, 18.0, 30.0, 47.0, 64.0, 80.0, 100.0] {
        let mut model = Model::default();
        let _ = app.update(
            Event::ComputeHrZones {
                age_years: age,
                resting_hr_bpm: Some(50.0),
                weeks_since_recalc: None,
                weeks_since_pace_test: None,
            },
            &mut model,
        );
        let vm = app.view(&model);
        let (rows, hm, bands) = build_hr_zones(&hr_query(age, Some(50.0)), None);
        assert_eq!(vm.hr_max, hm);
        assert_eq!(vm.hr_zone_bands, bands);
        assert_eq!(vm.hr_zones.len(), rows.len());
        let maf = vm
            .hr_zones
            .iter()
            .find(|r| r.summary.starts_with("MAF aerobic cap"))
            .expect("MAF row");
        let want = format!("180 − age = {} bpm", oracle::maf(age));
        assert!(maf.summary.contains(&want), "{} lacks {want}", maf.summary);
    }
}

#[test]
fn hr_zone_prose_states_the_same_bpm_as_the_structured_figures() {
    // The structured figures are documented as the SAME figures the rows
    // show ("the same resolved figure as the prose HRmax row", "Index of this
    // band's evidence row"). Ages in quarter years and integer measured maxima.
    let mut mismatches = Vec::new();
    let mut check =
        |rows: &[GuidanceView], hm: &HrMaxView, bands: &[HrZoneBandView], what: String| {
            let head = if hm.measured {
                format!("Measured HRmax: {} bpm", hm.bpm)
            } else {
                format!("Estimated HRmax: {} bpm", hm.bpm)
            };
            if rows[0].summary != format!("{head} (your logged maximum)") && rows[0].summary != head
            {
                mismatches.push(format!("{what}: {:?} vs bpm {}", rows[0].summary, hm.bpm));
            }
            for b in bands.iter().filter(|b| !b.pace_governed) {
                let want = format!("→ {}–{} bpm", b.bpm_low, b.bpm_high);
                if !rows[b.row as usize].summary.contains(&want) {
                    mismatches.push(format!(
                        "{what}: {:?} lacks {want}",
                        rows[b.row as usize].summary
                    ));
                }
            }
        };
    for qa in 20..=400 {
        let age = f64::from(qa) * 0.25;
        let (rows, hm, bands) = build_hr_zones(&hr_query(age, None), None);
        check(&rows, &hm.unwrap(), &bands, format!("age {age}"));
    }
    for m in 100..=240 {
        let (rows, hm, bands) = build_hr_zones(&hr_query(30.0, None), Some(f64::from(m)));
        check(&rows, &hm.unwrap(), &bands, format!("measured {m}"));
    }
    assert!(
        mismatches.is_empty(),
        "{} prose/figure mismatches, e.g. {:#?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(6)]
    );
}

// ─── Race prediction: Riegel + Daniels VDOT, running-039 equivalency ───────

const GOALS_M: [f64; 6] = [1500.0, 1609.344, 5000.0, 10_000.0, 21_097.5, 42_195.0];
const WEEKLY_KM: [f64; 11] = [
    0.0, 10.0, 29.9, 30.5, 45.0, 59.9, 60.5, 80.0, 99.9, 100.5, 150.0,
];
/// The oracle's own Daniels search bracket (10 days, `oracle::daniels_time`);
/// the engine has no cap, so the two solve the same equation below it.
const ORACLE_BRACKET_S: f64 = 10.0 * 86_400.0;

fn race_q(d1: f64, t1: f64, d2: f64, weekly: f64) -> RaceQuery {
    RaceQuery {
        recent_distance_m: d1,
        recent_time_sec: t1,
        goal_distance_m: d2,
        weekly_km: weekly,
        weeks_since_race: None,
    }
}

/// The oracle band for one query, `None` when the Daniels root is beyond the
/// oracle's own bracket (never on these grids).
fn oracle_race(d1: f64, t1: f64, d2: f64, weekly: f64) -> Option<(f64, f64, bool)> {
    let riegel = oracle::riegel(t1, d1, d2, oracle::riegel_exponent(weekly));
    let daniels = oracle::daniels_time(oracle::vdot(d1, t1), d2);
    (daniels < ORACLE_BRACKET_S - 1.0).then(|| oracle::equivalency(riegel, daniels))
}

#[test]
fn race_prediction_matches_riegel_and_daniels_oracles_over_5k_grid() {
    // 5K 12:00 … 3:00:00 every 15 s, re-expressed over other recent distances
    // at the same Riegel-1.06 equivalent, × every goal × every volume band.
    let recents = [5000.0, 1609.344, 10_000.0, 21_097.5];
    let mut compared = 0;
    for step in 0..=((10_800 - 720) / 15) {
        let t5k = 720.0 + 15.0 * f64::from(step);
        for d1 in recents {
            let t1 = (t5k * (d1 / 5000.0f64).powf(1.06)).round();
            for d2 in GOALS_M {
                for weekly in WEEKLY_KM {
                    let Some((lo, hi, agreed)) = oracle_race(d1, t1, d2, weekly) else {
                        continue;
                    };
                    let v = to_race_view(&race_q(d1, t1, d2, weekly), None);
                    let ctx = || format!("{d1} m in {t1} s → {d2} m @ {weekly} km/wk");
                    // Near the 2% knife edge the two float paths may legitimately
                    // fall on different sides; the bounds then still bracket.
                    let rel = (v.high_sec / v.low_sec - 1.0 - 0.02).abs();
                    if (hi / lo - 1.0 - 0.02).abs() > 1e-6 && rel > 1e-6 {
                        assert_eq!(v.agreed, agreed, "{}", ctx());
                        assert!(
                            (v.low_sec - lo).abs() <= 1e-6 * lo,
                            "{} low {} vs {lo}",
                            ctx(),
                            v.low_sec
                        );
                        assert!(
                            (v.high_sec - hi).abs() <= 1e-6 * hi,
                            "{} high {} vs {hi}",
                            ctx(),
                            v.high_sec
                        );
                        let want = if agreed {
                            oracle::clock(lo)
                        } else {
                            format!("{}–{}", oracle::clock(lo), oracle::clock(hi))
                        };
                        // A tie at exactly .5 s between the two rounding paths is
                        // only possible if the engine and oracle differ by <1e-6 s
                        // across it; accept the engine's own low_sec clock then.
                        if v.predicted != want {
                            let alt = if agreed {
                                oracle::clock(v.low_sec)
                            } else {
                                format!(
                                    "{}–{}",
                                    oracle::clock(v.low_sec),
                                    oracle::clock(v.high_sec)
                                )
                            };
                            assert_eq!(v.predicted, alt, "{}", ctx());
                        }
                    }
                    compared += 1;
                }
            }
        }
    }
    assert!(compared > 100_000, "compared {compared}");
}

#[test]
fn race_prediction_identity_and_monotonicity() {
    // Same distance in → same time out (both methods), agreed.
    for t in (720..=7200).step_by(37) {
        let t = f64::from(t);
        let v = to_race_view(&race_q(5000.0, t, 5000.0, 40.0), None);
        assert!(v.agreed, "identity must agree: {t}");
        assert!((v.low_sec - t).abs() < 1e-6, "{t} → {}", v.low_sec);
        assert_eq!(v.predicted, oracle::clock(t));
    }
    // Faster recent race → faster (never slower) predictions. Each METHOD is
    // monotone, so the band centre (Riegel + Daniels)/2 — identical under both
    // running-039 shapes (midpoint or span) — is monotone; the individual
    // bounds are monotone while the agree/range shape is unchanged (switching
    // from a span to its midpoint legitimately lowers the upper bound).
    // Longer goal → slower bounds; more weekly volume → never slower.
    for d2 in GOALS_M {
        for weekly in WEEKLY_KM {
            let mut prev: Option<(f64, f64, bool)> = None;
            for t in (720..=4800).step_by(10) {
                let v = to_race_view(&race_q(5000.0, f64::from(t), d2, weekly), None);
                if let Some((lo, hi, agreed)) = prev {
                    let ctx = format!("{d2} m {weekly} km/wk at {t} s");
                    assert!(v.low_sec + v.high_sec >= lo + hi, "{ctx}");
                    if v.agreed == agreed {
                        assert!(v.low_sec >= lo && v.high_sec >= hi, "{ctx}");
                    }
                }
                prev = Some((v.low_sec, v.high_sec, v.agreed));
            }
        }
    }
    for t in [900.0, 1200.0, 1500.0, 2100.0] {
        let mut prev = (0.0, 0.0);
        for d2 in GOALS_M {
            let v = to_race_view(&race_q(5000.0, t, d2, 40.0), None);
            assert!(v.low_sec > prev.0 && v.high_sec > prev.1, "{t} s → {d2} m");
            prev = (v.low_sec, v.high_sec);
        }
        // More volume → a smaller Riegel exponent (07:185) → the Riegel time
        // and so the band centre never slower (Daniels ignores volume).
        let mut prev_mid = f64::INFINITY;
        for weekly in WEEKLY_KM {
            let v = to_race_view(&race_q(5000.0, t, 42_195.0, weekly), None);
            let mid = v.low_sec + v.high_sec;
            assert!(mid <= prev_mid + 1e-9, "{t} s marathon at {weekly} km/wk");
            prev_mid = mid;
        }
    }
}

#[test]
fn race_prediction_reaches_the_view_with_the_marathon_derate_only_slowing() {
    // End to end through PredictRace: no logged runs → the bare band; a
    // logged 12 km longest run (< 30 km, 04:437-439) → marathon band shifted
    // slower, never faster; a 32 km longest run → no derate.
    let app = Engine;
    let predict = |model: &mut Model, t: f64| {
        let _ = app.update(
            Event::PredictRace {
                recent_distance_m: 5000.0,
                recent_time_sec: t,
                goal_distance_m: 42_195.0,
                weekly_km: 40.0,
                weeks_since_race: None,
            },
            model,
        );
        app.view(model).race_prediction.expect("prediction")
    };
    for t in [1080.0, 1320.0, 1500.0, 1800.0, 2400.0] {
        let base = to_race_view(&race_q(5000.0, t, 42_195.0, 40.0), None);
        let mut bare = Model::default();
        let p = predict(&mut bare, t);
        assert_eq!(
            (p.low_sec, p.high_sec, p.agreed),
            (base.low_sec, base.high_sec, base.agreed)
        );

        for (longest, derated) in [(12.0, true), (32.0, false)] {
            let mut m = Model::default();
            let _ = app.update(
                Event::SetToday {
                    epoch_day: 20_000,
                    utc_offset_sec: 0,
                },
                &mut m,
            );
            let _ = app.update(
                log_run(
                    longest,
                    longest * 6.0,
                    70.0,
                    longest,
                    20_000 * DAY_SEC - 3600,
                    1,
                ),
                &mut m,
            );
            let p = predict(&mut m, t);
            if derated {
                assert!(
                    p.low_sec > base.low_sec && p.high_sec > base.high_sec,
                    "{t}: {p:?}"
                );
                // The derate is 2–3 VDOT points (04:87): never more than the
                // Daniels time at VDOT − 3 relative to VDOT.
                let v = oracle::vdot(5000.0, t);
                let worst =
                    oracle::daniels_time(v - 3.0, 42_195.0) / oracle::daniels_time(v, 42_195.0);
                assert!(
                    p.high_sec <= base.high_sec * worst * (1.0 + 1e-6),
                    "{t}: {} vs {}",
                    p.high_sec,
                    base.high_sec * worst
                );
            } else {
                assert_eq!(
                    (p.low_sec, p.high_sec),
                    (base.low_sec, base.high_sec),
                    "{t}"
                );
            }
        }
    }
}

#[test]
fn slow_recent_race_long_goal_band_still_brackets_the_daniels_time() {
    // running-039 (04:427): the band is the span of the two methods' own
    // predictions. For a recent 5K slower than ~1:20 the Daniels marathon (and
    // eventually half) root lies beyond 12 h; a capped bisection bracket would
    // return its edge instead of the root.
    let mut bad = Vec::new();
    for t in (4800..=10_800).step_by(300) {
        let t = f64::from(t);
        for d2 in [21_097.5, 42_195.0] {
            let daniels = oracle::daniels_time(oracle::vdot(5000.0, t), d2);
            let riegel = oracle::riegel(t, 5000.0, d2, oracle::riegel_exponent(10.0));
            let v = to_race_view(&race_q(5000.0, t, d2, 10.0), None);
            let lo = daniels.min(riegel);
            if v.low_sec < lo * (1.0 - 1e-6) {
                bad.push(format!(
                    "5K {} → {d2} m: shown low {} but Daniels {} / Riegel {}",
                    oracle::clock(t),
                    oracle::clock(v.low_sec),
                    oracle::clock(daniels),
                    oracle::clock(riegel)
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} cases, e.g. {:#?}",
        bad.len(),
        &bad[..bad.len().min(4)]
    );
}

// ─── Training paces from VDOT ──────────────────────────────────────────────

#[test]
fn band_paces_match_vo2_inverse_and_mp_relative_oracles() {
    // VDOT 20–85: the %VO2max bands (04:465-467) invert VO2(v) (07:172); Easy
    // is MP × 1.15–1.25 (04:163), long run MP × 1.10–1.20 (04:175), recovery
    // at least 20 % slower than MP (04:152).
    for k in 0..=650 {
        let vdot = 20.0 + 0.1 * f64::from(k);
        let mp = oracle::daniels_time(vdot, 42_195.0) / 42.195;
        for band in BANDS {
            let (fast, slow) = load::vdot_band_pace_sec_per_km(vdot, band).expect("positive VDOT");
            let (want_fast, want_slow) = match band {
                VdotBand::Easy => (mp * 1.15, mp * 1.25),
                b => {
                    let (lo, hi, _) = oracle::band_row(b);
                    let slow = oracle::pace_at(vdot, lo);
                    (hi.map_or(slow, |h| oracle::pace_at(vdot, h)), slow)
                }
            };
            assert!(
                (fast - want_fast).abs() < 1e-6 * want_fast,
                "VDOT {vdot} {band:?} fast {fast} vs {want_fast}"
            );
            assert!(
                (slow - want_slow).abs() < 1e-6 * want_slow,
                "VDOT {vdot} {band:?} slow {slow} vs {want_slow}"
            );
        }
        let lr = running::mp_relative_pace_sec_per_km(
            vdot,
            running::RunWorkout::Session(RunSessionType::LongRun),
        )
        .unwrap();
        assert!((lr.0 - mp * 1.10).abs() < 1e-6 * mp && (lr.1 - mp * 1.20).abs() < 1e-6 * mp);
        let rec = resolve_target(
            run_target_band(RunSessionType::Recovery),
            &RunIntensity::HrPercentMax {
                lo: None,
                hi: Some(76.0),
                hrr_hi: None,
            },
            Some(vdot),
            Default::default(),
        );
        assert!(
            f64::from(rec.pace_slow_sec_per_km) >= (mp * 1.20).floor(),
            "VDOT {vdot}: recovery floor {} vs MP {mp}",
            rec.pace_slow_sec_per_km
        );
        // Consistency: Daniels' own marathon pace sits inside the M band
        // (80–84 % VO2max) for VDOT 30–80.
        if (30.0..=80.0).contains(&vdot) {
            let (mf, ms) = load::vdot_band_pace_sec_per_km(vdot, VdotBand::Marathon).unwrap();
            assert!(
                mf <= mp && mp <= ms,
                "VDOT {vdot}: MP {mp} outside M {mf}–{ms}"
            );
        }
    }
}

fn run_target_band(kind: RunSessionType) -> TargetBand {
    band_for(
        Some(kind),
        claims::RUN_VDOT_001,
        &RunIntensity::HrPercentMax {
            lo: None,
            hi: Some(76.0),
            hrr_hi: None,
        },
    )
    .expect("recovery band")
}

#[test]
fn faster_race_gives_faster_paces_in_every_band_and_bands_stay_ordered() {
    // A faster 5K (higher VDOT) → strictly faster pace at both ends of every
    // band; and R ≤ I < T < M < E at each end (04:464-468 ordering).
    let mut prev: Option<Vec<(f64, f64)>> = None;
    for t in (720..=3600).rev().step_by(5) {
        let vdot = load::vdot(5000.0, f64::from(t));
        let paces: Vec<(f64, f64)> = BANDS
            .iter()
            .map(|&b| load::vdot_band_pace_sec_per_km(vdot, b).unwrap())
            .collect();
        for b in &paces {
            assert!(b.0 <= b.1);
        }
        let (e, m, th, i, r) = (paces[0], paces[1], paces[2], paces[3], paces[4]);
        assert!(
            r.0 <= i.0 && i.1 < th.0 && th.0 < m.0 && th.1 < m.1 && m.1 < e.0,
            "5K {t}: {paces:?}"
        );
        if let Some(p) = &prev {
            // `prev` is the SLOWER race (t + 5 s): every pace must be faster now.
            for (old, new) in p.iter().zip(&paces) {
                assert!(new.0 < old.0 && new.1 < old.1, "5K {t}: {old:?} → {new:?}");
            }
        }
        prev = Some(paces);
    }
}

#[test]
fn run_target_labels_match_the_clock_oracle_in_km_and_miles() {
    // The displayed pace strings are the unrounded band paces rendered once
    // (round half away) per km and per mile (1 mi = 1609.344 m exactly), and
    // the integer fields equal the rounded paces.
    let mut rng = Rng(0x5eed_0001);
    for _ in 0..4000 {
        let vdot = rng.range(18.0, 88.0);
        for band in [
            VdotBand::Easy,
            VdotBand::Marathon,
            VdotBand::Threshold,
            VdotBand::Interval,
        ] {
            let tb = band_for(None, claims::RUN_VDOT_001, &RunIntensity::Vdot(band)).unwrap();
            let t = resolve_target(
                tb,
                &RunIntensity::Vdot(band),
                Some(vdot),
                Default::default(),
            );
            let (fast, slow) = load::vdot_band_pace_sec_per_km(vdot, band).unwrap();
            assert_eq!(
                f64::from(t.pace_fast_sec_per_km),
                oracle::round_half_away(fast)
            );
            assert_eq!(
                f64::from(t.pace_slow_sec_per_km),
                oracle::round_half_away(slow)
            );
            assert_eq!(
                t.pace_label_km,
                format!("{}–{} /km", oracle::mss(fast), oracle::mss(slow))
            );
            let k = oracle::MILE_M / 1000.0;
            assert_eq!(
                t.pace_label_mi,
                format!("{}–{} /mi", oracle::mss(fast * k), oracle::mss(slow * k))
            );
        }
    }
}

#[test]
fn run_target_heart_rate_matches_pct_hrmax_oracle() {
    // E/M/T run targets: %HRmax band (04:464-466) × the resolved HRmax.
    for qa in 20..=400 {
        let age = f64::from(qa) * 0.25;
        let hr = HrAnchor {
            max_bpm: oracle::tanaka(age),
            measured: false,
            rest_bpm: 0.0,
        };
        for band in [VdotBand::Easy, VdotBand::Marathon, VdotBand::Threshold] {
            let tb = band_for(None, claims::RUN_VDOT_001, &RunIntensity::Vdot(band)).unwrap();
            let t = resolve_target(tb, &RunIntensity::Vdot(band), None, hr);
            let (lo, hi) = oracle::band_row(band).2.unwrap();
            assert!(
                rounds_to(f64::from(t.hr_low_bpm), hr.max_bpm * lo / 100.0),
                "age {age} {band:?}"
            );
            assert!(
                rounds_to(f64::from(t.hr_high_bpm), hr.max_bpm * hi / 100.0),
                "age {age} {band:?}"
            );
            assert_eq!(
                t.hr_label,
                format!("{}–{} bpm", t.hr_low_bpm, t.hr_high_bpm)
            );
        }
        for band in [VdotBand::Interval, VdotBand::Repetition] {
            let tb = band_for(None, claims::RUN_VDOT_001, &RunIntensity::Vdot(band)).unwrap();
            let t = resolve_target(tb, &RunIntensity::Vdot(band), None, hr);
            assert_eq!(
                (t.hr_low_bpm, t.hr_high_bpm),
                (0, 0),
                "{band:?} is pace-governed"
            );
        }
    }
}

// ─── e1RM, %1RM, RPE↔RIR (logged-set metrics) ─────────────────────────────

fn set(w: f64, reps: u32, rpe: f64) -> LoggedSet {
    LoggedSet {
        exercise: "Back squat".into(),
        weight_kg: w,
        reps,
        rpe,
        observed_at: 1,
        entry_id: 0,
        hold_sec: None,
    }
}

/// Effective reps the oracle credits: reps + RIR when the RPE is an exact
/// reading inside the KB's 6–10 ↔ 4–0 scale (02:76) and not the open-ended
/// "4+ left" floor (the app's stated convention), else the reps performed.
fn oracle_eff(reps: u32, rpe: f64) -> f64 {
    if rpe > 6.0 && rpe <= 10.0 {
        f64::from(reps) + oracle::rir_of_rpe(rpe)
    } else {
        f64::from(reps)
    }
}

#[test]
fn logged_set_e1rm_pct_and_rir_match_kb_formulas_over_weight_rep_rpe_grid() {
    let rpes = [0.0, 6.0, 6.5, 7.0, 7.5, 8.0, 8.5, 9.0, 9.5, 10.0];
    let mut n = 0;
    for wi in 0..=1120 {
        let w = 20.0 + 0.25 * f64::from(wi);
        for reps in 1..=15 {
            for rpe in rpes {
                let v = to_lift_view(&set(w, reps, rpe), None, false, false);
                let eff = oracle_eff(reps, rpe);
                // 02:54 Epley. Convention (app-stated, strength-005): one
                // effective rep IS the 1RM; between 1 and 2 effective reps the
                // estimate lies between the single and the double.
                let e1 = if eff <= 1.0 {
                    w
                } else if eff >= 2.0 {
                    oracle::epley(w, eff)
                } else {
                    let (a, b) = (w, oracle::epley(w, 2.0));
                    assert!(
                        v.e1rm_kg >= (a * 10.0).round() / 10.0
                            && v.e1rm_kg <= (b * 10.0).round() / 10.0
                    );
                    f64::NAN
                };
                if e1.is_finite() {
                    assert!(
                        (v.e1rm_kg - e1).abs() <= 0.05 + 1e-9,
                        "{w}×{reps}@{rpe}: {} vs {e1}",
                        v.e1rm_kg
                    );
                    let pct = 100.0 * w / e1;
                    assert!(
                        rounds_to(v.pct_1rm, pct),
                        "{w}×{reps}@{rpe}: %1RM {} vs {pct}",
                        v.pct_1rm
                    );
                }
                if (6.0..=10.0).contains(&rpe) {
                    assert_eq!(v.rir, oracle::rir_of_rpe(rpe), "RPE {rpe}");
                    assert!(v.rpe_recorded);
                } else {
                    assert!(!v.rpe_recorded);
                }
                assert!((v.tonnage_kg - w * f64::from(reps)).abs() < 1e-9);
                // 02:54 cross-check across Epley/Brzycki/Lombardi, valid window
                // 1–10 reps (strength-006 gate on whole effective reps).
                let in_window = eff.floor() <= 10.0;
                assert_eq!(v.cross_check.is_some(), in_window, "{w}×{reps}@{rpe}");
                if let Some(cc) = &v.cross_check {
                    let est = [
                        if eff >= 2.0 {
                            oracle::epley(w, eff)
                        } else {
                            to_lift_view(&set(w, reps, rpe), None, false, false).e1rm_kg
                        },
                        oracle::brzycki_kb(w, eff),
                        oracle::lombardi(w, eff),
                    ];
                    let lo = est.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = est.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    // 0.05 display rounding + the KB's 4-decimal Brzycki constants.
                    let tol = 0.05 + 4e-4 * hi;
                    assert!(
                        (cc.low_kg - lo).abs() <= tol && (cc.high_kg - hi).abs() <= tol,
                        "{w}×{reps}@{rpe}: {cc:?} vs {lo}–{hi}"
                    );
                    assert_eq!(cc.formulas, 3);
                    assert!(
                        cc.low_kg <= v.e1rm_kg + 1e-9 && v.e1rm_kg <= cc.high_kg + 1e-9,
                        "headline inside its own band"
                    );
                }
                n += 1;
            }
        }
    }
    assert!(n > 150_000);
}

#[test]
fn e1rm_is_monotone_in_weight_reps_and_reserve() {
    let rpes = [0.0, 6.5, 7.0, 8.0, 9.0, 9.5, 10.0];
    for reps in 1..=15 {
        for rpe in rpes {
            let mut prev = 0.0;
            for wi in 0..=560 {
                let w = 20.0 + 0.5 * f64::from(wi);
                let e = to_lift_view(&set(w, reps, rpe), None, false, false).e1rm_kg;
                assert!(e >= prev, "{w}×{reps}@{rpe}");
                assert!(
                    e >= w - 0.05,
                    "e1RM below the lifted load: {w}×{reps}@{rpe} → {e}"
                );
                prev = e;
            }
        }
    }
    for w in [20.0, 60.0, 102.5, 180.0, 300.0] {
        for rpe in rpes {
            let mut prev = 0.0;
            for reps in 1..=20 {
                let e = to_lift_view(&set(w, reps, rpe), None, false, false).e1rm_kg;
                assert!(e >= prev, "{w}×{reps}@{rpe}");
                prev = e;
            }
        }
        for reps in 1..=12 {
            // Exact RPE 6.5 → 10: less in reserve → a lower (never higher) e1RM.
            let mut prev = f64::INFINITY;
            for rpe in [6.5, 7.0, 7.5, 8.0, 8.5, 9.0, 9.5, 10.0] {
                let e = to_lift_view(&set(w, reps, rpe), None, false, false).e1rm_kg;
                assert!(e <= prev, "{w}×{reps}@{rpe}");
                prev = e;
            }
        }
    }
}

#[test]
fn logged_set_reaches_the_view_with_the_same_metrics_and_lb_display() {
    // Event path for a seeded sample, in kg and lb profiles; the lb summary
    // shows the e1RM in pounds rounded to the 0.5 lb display grid.
    let app = Engine;
    let mut rng = Rng(0x5eed_0002);
    for i in 0..300 {
        let w = (rng.range(20.0, 300.0) * 4.0).round() / 4.0;
        let reps = 1 + (rng.next() % 12) as u32;
        let rpe = [0.0, 7.0, 8.0, 8.5, 9.0, 10.0][(rng.next() % 6) as usize];
        let lb = i % 2 == 1;
        let mut model = Model::default();
        if lb {
            let _ = app.update(
                Event::SetProfile(Profile {
                    weight_lb: true,
                    ..base_profile()
                }),
                &mut model,
            );
        }
        let _ = app.update(log_set("Back squat", w, reps, rpe, 1_000_000), &mut model);
        let vm = app.view(&model);
        let got = vm.lifts.first().expect("one lift");
        let want = to_lift_view(&set(w, reps, rpe), None, false, lb);
        assert_eq!(
            (got.e1rm_kg, got.pct_1rm, got.rir),
            (want.e1rm_kg, want.pct_1rm, want.rir)
        );
        if lb {
            let exact_lb = e1rm_exact(w, reps, rpe) / oracle::KG_PER_LB;
            let shown: f64 = got
                .summary
                .split("e1RM ")
                .nth(1)
                .and_then(|s| s.trim_start_matches('≥').split("lb").next())
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| panic!("no lb e1RM in {:?}", got.summary));
            assert!(
                (shown * 2.0).fract() == 0.0,
                "lb e1RM on the 0.5 grid: {shown}"
            );
            // Stated chain: 0.1 kg (≤0.11 lb) then the 0.5 lb grid (≤0.25 lb).
            assert!(
                (shown - exact_lb).abs() <= 0.25 + 0.111,
                "{:?} vs {exact_lb}",
                got.summary
            );
        }
    }
}

fn e1rm_exact(w: f64, reps: u32, rpe: f64) -> f64 {
    let eff = oracle_eff(reps, rpe);
    if eff <= 1.0 {
        w
    } else if eff < 2.0 {
        w * (1.0 + (eff - 1.0) * 2.0 / 30.0)
    } else {
        oracle::epley(w, eff)
    }
}

#[test]
fn prescribed_pct_1rm_for_reps_in_reserve_inverts_epley() {
    // strength-005 (02:54) inverted: the %1RM at which `reps` leave `rir` in
    // reserve is 100/(1 + (reps+rir)/30); `max_pct_leaving_rir` floors it so
    // the reserve is never rounded away.
    for reps in 1..=20u8 {
        for rir in 0..=5u8 {
            let eff = f64::from(reps) + f64::from(rir);
            let want = if eff <= 1.0 {
                100.0
            } else {
                100.0 * 30.0 / (30.0 + eff)
            };
            let got = f64::from(hypertrophy::pct_1rm_for_reps_in_reserve(reps, rir));
            if eff >= 2.0 {
                assert!((got - want).abs() < 1e-4, "{reps}@{rir}: {got} vs {want}");
                let floor = strength::max_pct_leaving_rir(reps, rir);
                assert_eq!(f64::from(floor), want.floor(), "{reps}@{rir}");
                // A load at the floored % really leaves ≥ rir in reserve on Epley.
                let reps_possible = 30.0 * (100.0 / f64::from(floor) - 1.0);
                assert!(
                    reps_possible + 1e-9 >= eff,
                    "{reps}@{rir}: {floor}% allows {reps_possible}"
                );
            }
        }
    }
}

// ─── Plate grid and unit conversions ──────────────────────────────────────

#[test]
fn load_grid_floors_onto_the_display_unit_and_is_idempotent() {
    // 03:361 smallest increment ~2.5 kg / 5 lb; the app floors (never rounds
    // up past the prescribed %1RM) on the DISPLAY unit grid.
    let mut rng = Rng(0x5eed_0003);
    for _ in 0..200_000 {
        let kg = rng.range(0.0, 400.0);
        let g = round_load_grid(kg, false);
        assert!(g <= kg + 1e-9 && kg - g < 2.5 + 1e-9, "{kg} → {g}");
        assert!(
            (g / 2.5 - (g / 2.5).round()).abs() < 1e-9,
            "{g} on the 2.5 grid"
        );
        assert_eq!(round_load_grid(g, false), g, "idempotent at {g}");
        let gl = round_load_grid(kg, true) / oracle::KG_PER_LB;
        let lb = kg / oracle::KG_PER_LB;
        let step = if lb < 50.0 { 2.5 } else { 5.0 };
        assert!(
            gl <= lb + 1e-6 && lb - gl < step + 1e-6,
            "{kg} kg = {lb} lb → {gl}"
        );
        assert!(
            (gl / 2.5 - (gl / 2.5).round()).abs() < 1e-6,
            "{gl} lb on the plate grid"
        );
    }
}

#[test]
fn pound_display_of_shell_entered_loads_round_trips() {
    // The shell stores a pound entry as `lb / LB_PER_KG` (Units.kt:107); the
    // core shows it back on the 0.5 lb display grid as the same pounds.
    for half in 1..=2000 {
        let lb = 0.5 * f64::from(half);
        assert_eq!(
            fmt_weight_num(lb / LB_PER_KG, true),
            oracle_fmt_half(lb),
            "{lb} lb"
        );
        let kg = 0.1 * f64::from(half);
        let back: f64 = fmt_weight_num(kg, false).parse().unwrap();
        assert!((back - kg).abs() < 1e-9, "{kg} kg");
    }
    // The factor itself: LB_PER_KG × 0.45359237 = 1 to 1e-10.
    assert!((LB_PER_KG * oracle::KG_PER_LB - 1.0).abs() < 1e-10);
}

#[test]
fn pound_loads_already_on_the_plate_grid_stay_put() {
    // format.rs `round_load_grid`: round DOWN onto the display-unit plate grid
    // so the load never exceeds the prescribed %1RM. A load that IS on the grid
    // (a shell-entered 215 lb, or an e1RM × %1RM product that is exactly a
    // plate load) must stay; floor() of `lb/step` computed as 42.99999… would
    // drop it a whole 5 lb plate step. Callers pass their raw load
    // (flatten_lift.rs, first_set.rs, adjustments.rs, completion.rs).
    let mut bad = Vec::new();
    for half in 1..=2000 {
        let lb = 0.5 * f64::from(half);
        let on_grid = if lb < 50.0 {
            (lb / 2.5).fract() == 0.0
        } else {
            (lb / 5.0).fract() == 0.0
        };
        if on_grid {
            let back = round_load_grid(lb / LB_PER_KG, true) * LB_PER_KG;
            if (back - lb).abs() > 1e-6 {
                bad.push(format!("entered {lb} lb → {back:.1} lb"));
            }
        }
    }
    // flatten_lift.rs:158's expression, `round_load_grid(e * p / 100 * frac)`,
    // for a pound e1RM (a logged single) and a whole %1RM whose exact product
    // is a 5 lb plate load.
    for e_lb in (100..=600).step_by(5) {
        for p in 50..=100u32 {
            let exact = f64::from(e_lb) * f64::from(p) / 100.0;
            if exact >= 50.0 && (exact / 5.0).fract() == 0.0 {
                let e = f64::from(e_lb) / LB_PER_KG;
                let shown = round_load_grid(e * f64::from(p) / 100.0 * 1.0, true) * LB_PER_KG;
                if (shown - exact).abs() > 1e-6 {
                    bad.push(format!(
                        "e1RM {e_lb} lb × {p}% = {exact} lb → {shown:.1} lb"
                    ));
                }
            }
        }
    }
    // End to end through the plan-item flattener: a 300 lb Back Squat e1RM
    // and an 80 %1RM prescription is a 240 lb load.
    let rx = graded(
        Prescription::Lift(crate::schema::LiftPrescription {
            exercise: "Back Squat".into(),
            sets: 3,
            reps: 5,
            intensity: LiftIntensity::PercentOneRm(80.0),
            rest_sec: 180,
            velocity_loss_pct: None,
            reps_max: None,
            load_kg: None,
        }),
        claims::STR_INTENT_001,
    );
    let anchors = crate::plan::Anchors {
        lift_e1rm: vec![("Back Squat".into(), 300.0 / LB_PER_KG)],
        ..Default::default()
    };
    let item = flatten_prescription(&rx, &anchors, None, true);
    let shown = item.load_kg.expect("anchored load") * LB_PER_KG;
    if (shown - 240.0).abs() > 1e-6 {
        bad.push(format!(
            "plan item: 300 lb e1RM × 80% → {shown:.1} lb ({:?})",
            item.summary
        ));
    }
    assert!(
        bad.is_empty(),
        "{} cases, e.g. {:#?}",
        bad.len(),
        &bad[bad.len().saturating_sub(1)..]
    );
}

fn oracle_fmt_half(lb: f64) -> String {
    let h = oracle::round_half_away(lb * 2.0) / 2.0;
    if h.fract() == 0.0 {
        format!("{}", h as i64)
    } else {
        format!("{}.5", h.trunc() as i64)
    }
}

#[test]
fn kg_display_rounds_once_to_a_tenth() {
    let mut rng = Rng(0x5eed_0004);
    for _ in 0..100_000 {
        let kg = rng.range(0.0, 500.0);
        let s = fmt_weight_num(kg, false);
        let v: f64 = s.parse().unwrap();
        assert!((v - kg).abs() <= 0.05 + 1e-9, "{kg} → {s}");
        assert!(!s.ends_with(".0"), "{s}");
        let lbs = fmt_weight_num(kg, true);
        let l: f64 = lbs.parse().unwrap();
        assert!(
            (l - kg / oracle::KG_PER_LB).abs() <= 0.25 + 1e-6,
            "{kg} → {lbs}"
        );
    }
}

#[test]
fn distance_line_miles_twin_converts_the_km_actually_shown() {
    // `fmt_mi_of_km` is documented as "the km distance a line shows at one
    // decimal, restated in miles". The km line is `fmt_km_or_mi(km, false)`.
    // On an exact .x5 binary tie (quarter kilometres) the two round the km
    // differently, so the pair can describe two different distances.
    let mut bad = Vec::new();
    for q in 1..=4000 {
        let km = 0.25 * f64::from(q);
        let km_line = fmt_km_or_mi(km, false);
        let shown: f64 = km_line.trim_end_matches(" km").parse().unwrap();
        let mi = shown * 1000.0 / oracle::MILE_M;
        let tenths = mi * 10.0;
        let r = oracle::round_half_away(tenths) / 10.0;
        let mut want = format!("{r:.1}");
        if want.ends_with(".0") {
            want.truncate(want.len() - 2);
        }
        let mi_line = fmt_km_or_mi(km, true);
        if mi_line != format!("{want} mi") {
            bad.push(format!(
                "{km} km: km line {km_line:?}, mi line {mi_line:?}, miles of the shown km {want}"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} cases, e.g. {:#?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

// ─── Time formatting ───────────────────────────────────────────────────────

#[test]
fn clock_and_pace_formatting_match_the_oracle_including_half_second_carries() {
    for tenth in 0..=200_000 {
        let s = f64::from(tenth) / 10.0;
        assert_eq!(fmt_clock(s), oracle::clock(s), "{s}");
        assert_eq!(fmt_mss(s), oracle::mss(s), "{s}");
    }
    let mut rng = Rng(0x5eed_0005);
    for _ in 0..200_000 {
        let s = rng.range(0.0, 3.0 * 3600.0 + 10.0);
        assert_eq!(fmt_clock(s), oracle::clock(s), "{s}");
    }
    for (s, want) in [
        (59.5, "1:00"),
        (59.49999999, "0:59"),
        (3599.5, "1:00:00"),
        (3599.4999, "59:59"),
        (10_799.5, "3:00:00"),
        (720.0, "12:00"),
        (0.0, "0:00"),
        (-1.0, "0:00"),
    ] {
        assert_eq!(fmt_clock(s), want, "{s}");
        assert_eq!(
            fmt_race_clock(s.max(0.0)).as_str(),
            if s <= 0.0 { "-" } else { want }
        );
    }
}

// ─── Protein g/kg ───────────────────────────────────────────────────────────

#[test]
fn protein_figures_match_kb_g_per_kg_over_bodyweight_grid() {
    for tenth in 200..=3000 {
        let bw = f64::from(tenth) / 10.0;
        let q = ProteinQuery {
            bodyweight_kg: bw,
            masters: true,
            deficit: true,
        };
        let (rows, figs) = build_protein_targets(&q, false, false);
        assert_eq!(figs.len(), 2);
        for (f, (lo, hi)) in figs
            .iter()
            .zip([oracle::MASTERS_G_PER_KG, oracle::DEFICIT_G_PER_KG])
        {
            assert_eq!((f.low_g_per_kg, f.high_g_per_kg), (lo, hi));
            assert!(
                rounds_to(f.low_g_per_day, bw * lo) && rounds_to(f.high_g_per_day, bw * hi),
                "{bw}: {f:?}"
            );
            let want = format!("{}–{} g/day", f.low_g_per_day, f.high_g_per_day);
            assert!(
                rows[f.row as usize].summary.contains(&want),
                "{:?} lacks {want}",
                rows[f.row as usize].summary
            );
            assert!(!f.refused);
        }
        let (_, refused) = build_protein_targets(&q, true, false);
        assert!(
            refused[1].refused && refused[1].low_g_per_day == 0.0,
            "RED-S refuses the deficit target"
        );
    }
    // Monotone in bodyweight; nothing for a non-positive bodyweight.
    let mut prev = (0.0, 0.0);
    for tenth in 200..=3000 {
        let q = ProteinQuery {
            bodyweight_kg: f64::from(tenth) / 10.0,
            masters: true,
            deficit: false,
        };
        let f = &build_protein_targets(&q, false, false).1[0];
        assert!(f.low_g_per_day >= prev.0 && f.high_g_per_day >= prev.1);
        prev = (f.low_g_per_day, f.high_g_per_day);
    }
    for bw in [0.0, -10.0] {
        let q = ProteinQuery {
            bodyweight_kg: bw,
            masters: true,
            deficit: true,
        };
        assert!(build_protein_targets(&q, false, false).1.is_empty());
    }
}

#[test]
fn protein_reaches_the_view_with_the_pound_bodyweight_echo() {
    let app = Engine;
    for lb in [100.0, 155.0, 180.0, 242.5, 400.0] {
        let mut model = Model::default();
        let _ = app.update(
            Event::SetProfile(Profile {
                weight_lb: true,
                ..base_profile()
            }),
            &mut model,
        );
        let _ = app.update(
            Event::ComputeProtein {
                bodyweight_kg: lb * oracle::KG_PER_LB,
                masters: true,
                deficit: false,
            },
            &mut model,
        );
        let vm = app.view(&model);
        let f = &vm.protein_figures[0];
        let bw = lb * oracle::KG_PER_LB;
        assert!(rounds_to(f.low_g_per_day, bw * 1.2) && rounds_to(f.high_g_per_day, bw * 1.6));
        let row = &vm.protein_targets[f.row as usize].summary;
        // `{:.0}` echo of the pound bodyweight: within half a pound.
        let shown: f64 = row
            .split('(')
            .nth(1)
            .unwrap()
            .split(" lb")
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!((shown - lb).abs() <= 0.5 + 1e-9, "{row}");
    }
}

// ─── Hypertrophy volume landmarks and ramps ─────────────────────────────────

/// 03-hypertrophy.md:520-530 Table 1 (MV, MEV, MAV lo–hi, MRV "N+").
const TABLE1: [(&str, u8, u8, u8, u8, u8); 11] = [
    ("chest", 8, 10, 12, 20, 22),
    ("back", 8, 10, 14, 22, 25),
    ("quads", 6, 8, 12, 18, 20),
    ("hamstrings", 4, 6, 10, 16, 20),
    ("glutes", 0, 0, 4, 12, 16),
    ("side delts", 6, 8, 16, 22, 26),
    ("rear delts", 0, 6, 12, 18, 22),
    ("biceps", 4, 6, 14, 20, 26),
    ("triceps", 4, 6, 10, 14, 18),
    ("calves", 4, 6, 8, 16, 20),
    ("abs", 0, 0, 10, 16, 20),
];

/// The planner's figures and ramp for one muscle, optionally under a
/// hypertrophy profile stating `stated` weekly sets.
fn hyp_plan(
    muscle: &str,
    stated: Option<u8>,
    weeks: u8,
) -> (Option<HypertrophyFiguresView>, Vec<u8>) {
    let app = Engine;
    let mut model = Model::default();
    if let Some(s) = stated {
        let p = Profile {
            weekly_sets: s,
            lift_goal: LiftGoal::Hypertrophy,
            ..base_profile()
        };
        let _ = app.update(Event::SetProfile(p), &mut model);
    }
    let _ = app.update(
        Event::PlanHypertrophyMeso {
            muscle: muscle.to_uppercase(),
            weeks,
            not_growing: false,
            recovering_easily: false,
        },
        &mut model,
    );
    let vm = app.view(&model);
    (vm.hypertrophy_figures, vm.hypertrophy_ramp)
}

const STATED: [Option<u8>; 8] = [
    None,
    Some(0),
    Some(4),
    Some(8),
    Some(12),
    Some(16),
    Some(20),
    Some(30),
];

#[test]
fn hypertrophy_figures_match_table1_and_ramps_obey_the_kb_rules() {
    for (muscle, mv, mev, mav_lo, mav_hi, mrv) in TABLE1 {
        let lm = hypertrophy::landmarks_for(muscle).expect(muscle);
        assert_eq!(
            (lm.mv, lm.mev, lm.mav, lm.mrv),
            (mv, mev, (mav_lo, mav_hi), mrv),
            "{muscle}"
        );
        for stated in STATED {
            for weeks in 1..=8u8 {
                let (f, ramp) = hyp_plan(muscle, stated, weeks);
                let ctx = format!("{muscle} stated {stated:?} {weeks} wk");
                let f = f.unwrap_or_else(|| panic!("{ctx}: no figures"));
                assert_eq!(
                    (f.min_sets, f.productive_low, f.productive_high, f.max_sets),
                    (mev, mav_lo, mav_hi, mrv),
                    "{ctx}"
                );
                assert_eq!(ramp.len(), usize::from(weeks), "{ctx}");
                // 03:119-120 no abrupt jumps / 03:10 +2–4 sets per week at most;
                // the ramp only climbs (03:9, 03:350), with or without a profile.
                for w in ramp.windows(2) {
                    assert!(w[1] <= w[0] + 4, "{ctx}: {ramp:?}");
                    assert!(w[1] >= w[0], "{ctx}: {ramp:?}");
                }
                // Never above MRV (03:97); a standard ramp ends at most at MAV
                // high unless the stated target already sits above it (03:505).
                let top = ramp.iter().copied().max().unwrap();
                assert!(top <= mrv, "{ctx}: {ramp:?}");
                let stated_hi = stated.unwrap_or(0).clamp(mev, mrv);
                assert!(top <= mav_hi.max(stated_hi), "{ctx}: {ramp:?}");
                // Without a profile the ramp opens at MEV (03:9, 03:350). With
                // one, the 32-set whole-session cap (03:504-505) may scale
                // every standard muscle below MEV on a short lift week.
                if stated.is_none() && mev > 0 {
                    assert_eq!(ramp[0], mev, "{ctx}: {ramp:?}");
                }
                // 03:471: RIR descends ~1/week to 1 (novice floors at 2).
                let rir = &f.rir_by_week;
                assert_eq!(rir.len(), usize::from(weeks));
                for w in rir.windows(2) {
                    assert!(w[1] <= w[0], "{ctx}: {rir:?}");
                }
                assert!(rir.iter().all(|&r| (1..=4).contains(&r)), "{ctx}: {rir:?}");
                if weeks >= 3 {
                    assert!(*rir.last().unwrap() <= 2, "{ctx}: {rir:?}");
                }
            }
        }
    }
}

#[test]
fn hypertrophy_profile_ramp_never_drops_within_the_accumulation_block() {
    // 03-hypertrophy.md:9-10 "start … near MEV and add ~2–4 sets/week",
    // 03:350 "start at MEV and add sets weekly toward MRV (10→13→16→20)":
    // an accumulation ramp only climbs or holds. The whole-session cap
    // (03:504-505) may lower the LEVEL, but refitting each week separately
    // makes the shown weekly sets go down while the block is still building.
    let mut bad = Vec::new();
    for (muscle, ..) in TABLE1 {
        for stated in STATED.into_iter().flatten() {
            for weeks in 2..=8u8 {
                let (_, ramp) = hyp_plan(muscle, Some(stated), weeks);
                if ramp.windows(2).any(|w| w[1] < w[0]) {
                    bad.push(format!("{muscle} stated {stated} {weeks} wk: {ramp:?}"));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} ramps drop, e.g. {:#?}",
        bad.len(),
        &bad[..bad.len().min(6)]
    );
}

// ─── Cooper, Critical Speed ────────────────────────────────────────────────

#[test]
fn cooper_and_critical_speed_match_the_kb_formulas() {
    let mut prev = f64::NEG_INFINITY;
    for d in (510..=5000).step_by(5) {
        let d = f64::from(d);
        let row = &build_cooper(d)[0].summary;
        let shown: f64 = row
            .split("VO2max ")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            (shown - oracle::cooper(d)).abs() <= 0.05 + 1e-9,
            "{d}: {row}"
        );
        assert!(shown >= prev, "monotone at {d}");
        prev = shown;
    }
    for d in [0.0, 300.0, 504.9, f64::NAN] {
        assert!(build_cooper(d)[0].summary.contains("too short"), "{d}");
    }
    let mut rng = Rng(0x5eed_0006);
    for _ in 0..3000 {
        let cs = rng.range(2.0, 6.5);
        let dp = rng.range(50.0, 400.0);
        let t1 = rng.range(150.0, 480.0);
        let t2 = rng.range(720.0, 1500.0);
        let efforts = [
            CsEffortIn {
                distance_m: cs * t1 + dp,
                time_sec: t1,
            },
            CsEffortIn {
                distance_m: cs * t2 + dp,
                time_sec: t2,
            },
        ];
        let (ocs, odp) = oracle::cs_two(efforts[0].distance_m, t1, efforts[1].distance_m, t2);
        let row = &build_critical_speed(&efforts)[0].summary;
        let speed: f64 = row
            .split("Critical Speed ")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let dprime: f64 = row
            .split("D′ ")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!((speed - ocs).abs() <= 0.005 + 1e-9, "{row} vs {ocs}");
        assert!((dprime - odp).abs() <= 0.5 + 1e-6, "{row} vs {odp}");
        assert!(
            row.contains(&format!("({}/km)", oracle::mss(1000.0 / ocs))),
            "{row}"
        );
    }
}

// ─── Training load: Lucia TRIMP → CTL / ATL / TSB ───────────────────────────

/// A seeded history: `days` days ending on `last_day`, a run on ~60% of days.
fn seeded_runs(seed: u64, last_day: i64, days: i64) -> Vec<(i64, f64, f64)> {
    let mut rng = Rng(seed);
    let mut out = Vec::new();
    for d in (last_day - days + 1)..=last_day {
        if rng.unit() < 0.6 || d == last_day {
            let minutes = rng.range(15.0, 120.0).round();
            // Keep clear of the 82/88 boundaries (their inclusivity is
            // pinned elsewhere).
            let pct = loop {
                let p = rng.range(55.0, 98.0);
                if (p - 82.0).abs() > 0.2 && (p - 88.0).abs() > 0.2 {
                    break p;
                }
            };
            out.push((d, minutes, (pct * 10.0).round() / 10.0));
        }
    }
    out
}

fn oracle_load(runs: &[(i64, f64, f64)], through_day: i64) -> (f64, f64) {
    let first = runs.iter().map(|r| r.0).min().unwrap();
    let (mut ctl, mut atl) = (0.0, 0.0);
    for d in first..=through_day {
        let x: f64 = runs
            .iter()
            .filter(|r| r.0 == d)
            .map(|r| r.1 * oracle::lucia_weight(r.2))
            .sum();
        ctl = oracle::ewma_step(ctl, x, 42.0);
        atl = oracle::ewma_step(atl, x, 7.0);
    }
    (ctl, atl)
}

fn load_model(runs: &[(i64, f64, f64)], today: i64) -> (Engine, Model) {
    let app = Engine;
    let mut m = Model::default();
    let _ = app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut m,
    );
    for (i, &(d, minutes, pct)) in runs.iter().enumerate() {
        let km = minutes / 6.0;
        let _ = app.update(
            log_run(km, minutes, pct, km, d * DAY_SEC + 7 * 3600, i as u64 + 1),
            &mut m,
        );
    }
    (app, m)
}

#[test]
fn training_load_matches_lucia_trimp_and_ewma_oracle() {
    for seed in 0..12u64 {
        let last = 20_000 + seed as i64;
        let runs = seeded_runs(0x10ad + seed, last, 20 + 15 * seed as i64);
        let (app, m) = load_model(&runs, last);
        let tl = app.view(&m).training_load.expect("HR-carrying runs");
        let (ctl, atl) = oracle_load(&runs, last);
        assert!(
            (tl.ctl - ctl).abs() <= 0.05 + 1e-9,
            "seed {seed}: CTL {} vs {ctl}",
            tl.ctl
        );
        assert!(
            (tl.atl - atl).abs() <= 0.05 + 1e-9,
            "seed {seed}: ATL {} vs {atl}",
            tl.atl
        );
        assert!(
            (tl.tsb - (ctl - atl)).abs() <= 0.05 + 1e-9,
            "seed {seed}: TSB {} vs {}",
            tl.tsb,
            ctl - atl
        );
        assert_eq!(tl.sessions_counted as usize, runs.len());
        let want = format!(
            "Fitness (CTL) {:.1} · Fatigue (ATL) {:.1} · Form (TSB) {:+.1}",
            tl.ctl, tl.atl, tl.tsb
        );
        assert!(tl.summary.starts_with(&want), "{}", tl.summary);
    }
}

#[test]
fn training_load_decays_through_rest_days_up_to_today() {
    // 07:201-204: CTL/ATL are EWMAs of DAILY load — a rest day is a day of
    // zero load, so fatigue decays while you rest. The card's figures carry
    // no "as of" date, so they read as today's.
    let last = 20_100;
    let runs = seeded_runs(0x10ad_0bad, last, 42);
    let today = last + 21;
    let (app, m) = load_model(&runs, today);
    let tl = app.view(&m).training_load.expect("HR-carrying runs");
    let (ctl, atl) = oracle_load(&runs, today);
    assert!(
        (tl.atl - atl).abs() <= 0.05
            && (tl.ctl - ctl).abs() <= 0.05
            && (tl.tsb - (ctl - atl)).abs() <= 0.05,
        "today (+21 d rest): app CTL {} ATL {} TSB {} vs oracle CTL {ctl:.1} ATL {atl:.1} TSB {:.1}",
        tl.ctl,
        tl.atl,
        tl.tsb,
        ctl - atl
    );
}

// ─── ACWR stays hard-blocked (LOAD-ACWR-001, MarketingMyth) ─────────────────

#[test]
fn acwr_never_surfaces_in_a_loaded_view() {
    // 07 / evidence: the acute:chronic ratio is a hard-blocked myth. A view
    // over 10 weeks of HR runs (incl. a spike week) must cite nothing of it.
    let last = 20_300;
    let mut runs = seeded_runs(0xac_0001, last - 7, 63);
    for d in (last - 6)..=last {
        runs.push((d, 110.0, 90.0));
    }
    let (app, m) = load_model(&runs, last);
    let json = serde_json::to_string(&app.view(&m)).unwrap();
    for needle in [
        "LOAD-ACWR-001",
        "Gabbett 2016, BJSM",
        "sweet spot",
        "acute:chronic",
        "ACWR",
    ] {
        assert!(!json.contains(needle), "view mentions {needle:?}");
    }
}
