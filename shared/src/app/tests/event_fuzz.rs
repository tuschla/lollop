//! Seeded random event-sequence fuzzing over the whole `Event` vocabulary.
//!
//! Each case draws a reproducible sequence (profile edits, set/run/GPS/
//! activity logs, amends, deletes, trims, readiness / check-in / review
//! submissions, pain and illness report + resolve + remove, imports, `SetToday`
//! jumps both ways, calculators, plan moves) from one 64-bit seed and checks
//! what a shell can observe:
//!
//! - `update` and `view` never panic;
//! - every view survives the JSON wire losslessly (a NaN/∞ serializes as
//!   `null`, which Kotlin's non-null decode rejects) and its numbers are sane;
//! - entry counts move only as the event says (an amend never duplicates);
//! - a train-blocking hold blanks the next session, and a fresh bare pain /
//!   below-neck illness / cardiac report blocks training at once;
//! - replaying the log from scratch on a cold thread (empty view caches)
//!   renders the identical view, as does the compacted log and a compacted
//!   prefix followed by the rest (the shell compacts at launch, then appends);
//! - every event survives the serde wire unchanged;
//! - delete + the shell's Undo event (and edit + Undo) restore the view.
//!
//! `FUZZ_ITERS=<n>` sets the case count of every fuzz loop (defaults keep the
//! file fast); `FUZZ_SEED=<seed>` (hex `0x…` or decimal) replays one case. A
//! failure names its seed.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;
use crate::compact_event_log;
use crate::schema::{LadderId, PainDetail, PainKind, PainTrend};

/// A Monday-ish epoch day in late 2025; every sequence starts near it.
const BASE_DAY: i64 = 20_400;

// ── Seeded PRNG (64-bit LCG, the `view_equivalence` pattern) ───────────────

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        let mut r = Rng(seed ^ 0x9e37_79b9_7f4a_7c15);
        r.u64();
        r
    }

    fn u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // The LCG's low bits are weak: mix the state before use.
        let x = self.0;
        (x ^ (x >> 31)).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ (x >> 29)
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.u64() % n }
    }

    fn unit(&mut self) -> f64 {
        (self.u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize].clone()
    }

    fn opt<T>(&mut self, p: f64, f: impl FnOnce(&mut Self) -> T) -> Option<T> {
        if self.chance(p) { Some(f(self)) } else { None }
    }
}

// ── Case driver ───────────────────────────────────────────────────────────

fn env_iters(default: usize) -> usize {
    std::env::var("FUZZ_ITERS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(default)
}

fn env_seed() -> Option<u64> {
    let s = std::env::var("FUZZ_SEED").ok()?;
    let s = s.trim();
    match s.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

/// Run `case` over `FUZZ_ITERS` (else `default`) seeds derived from `salt`,
/// or the one `FUZZ_SEED`; fail naming every failing seed (first 5).
fn run_cases(name: &str, salt: u64, default: usize, case: impl Fn(u64) -> Result<(), String>) {
    let seeds: Vec<u64> = match env_seed() {
        Some(s) => vec![s],
        None => (0..env_iters(default) as u64)
            .map(|i| SplitMix(salt ^ i.wrapping_mul(0x2545_f491_4f6c_dd1d)).next())
            .collect(),
    };
    let mut failures = Vec::new();
    for seed in seeds {
        match catch_unwind(AssertUnwindSafe(|| case(seed))) {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => failures.push(format!("seed {seed:#018x}: {msg}")),
            Err(p) => failures.push(format!("seed {seed:#018x}: PANIC {}", panic_text(&*p))),
        }
        if failures.len() >= 5 {
            break;
        }
    }
    assert!(
        failures.is_empty(),
        "{name}: {} failing case(s); reproduce one with \
         `FUZZ_SEED=<seed> cargo test -q -p shared {name}`\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn short(e: &Event) -> String {
    let mut s = serde_json::to_string(e).unwrap_or_else(|err| format!("<unserializable: {err}>"));
    if s.len() > 400 {
        let mut cut = 400;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}

// ── Generator ─────────────────────────────────────────────────────────────

/// How far values stray from what the Android shell sends.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    /// Shell-shaped values only (editor ranges, unique monotonic ids,
    /// `longest_recent_km = 0`): the Undo tests' ground.
    Shell,
    /// Plus boundary values a corrupt or hand-edited log can carry: 0,
    /// negative, huge-but-finite, empty / unicode / very long strings,
    /// duplicate, legacy-0 and maximal entry ids, far past/future stamps.
    Boundary,
    /// Plus non-finite floats and integer extremes (`i64::MIN/MAX`).
    Absurd,
}

#[derive(Clone, Copy)]
struct RunRef {
    id: u64,
    at: i64,
    /// First/last fix time of a GPS run, for trims.
    span: Option<(i64, i64)>,
}

struct Gen {
    rng: Rng,
    mode: Mode,
    today: i64,
    offset: i64,
    next_id: u64,
    sets: Vec<(u64, i64)>,
    runs: Vec<RunRef>,
    acts: Vec<(u64, i64)>,
}

const SIGNALS: [ReadinessSignal; 15] = [
    ReadinessSignal::Rpe,
    ReadinessSignal::EstimatedOneRm,
    ReadinessSignal::BarVelocity,
    ReadinessSignal::VelocityLoss,
    ReadinessSignal::WellnessZ,
    ReadinessSignal::HrvLnRmssd,
    ReadinessSignal::HrvCv,
    ReadinessSignal::AerobicDecoupling,
    ReadinessSignal::RestingHr,
    ReadinessSignal::Pain,
    ReadinessSignal::Soreness,
    ReadinessSignal::Illness,
    ReadinessSignal::RedS,
    ReadinessSignal::CardiacRedFlag,
    ReadinessSignal::BoneStress,
];

const LADDERS: [LadderId; 15] = [
    LadderId::HorizontalPush,
    LadderId::Dip,
    LadderId::VerticalPush,
    LadderId::HorizontalPull,
    LadderId::VerticalPull,
    LadderId::Squat,
    LadderId::Hinge,
    LadderId::Core,
    LadderId::Handstand,
    LadderId::FrontLever,
    LadderId::Planche,
    LadderId::BackLever,
    LadderId::MuscleUp,
    LadderId::Jump,
    LadderId::PlyoPush,
];

const WORKOUTS: [WorkoutType; 6] = [
    WorkoutType::Steady,
    WorkoutType::Interval,
    WorkoutType::Tempo,
    WorkoutType::LongRun,
    WorkoutType::Recovery,
    WorkoutType::Easy,
];

const ACTIVITY_KINDS: [ActivityKind; 7] = [
    ActivityKind::Sport,
    ActivityKind::Cycling,
    ActivityKind::Swimming,
    ActivityKind::Walking,
    ActivityKind::Dance,
    ActivityKind::Strength,
    ActivityKind::Other,
];

/// Names the shell's exercise picker offers (plus a retired catalog label).
const SHELL_EXERCISES: [&str; 12] = [
    "Back Squat",
    "Bench Press",
    "Deadlift",
    "Overhead Press",
    "Barbell Row",
    "Romanian Deadlift",
    "Pull-up",
    "Push-up",
    "Lat Pulldown",
    "Plank",
    "Dead Hang",
    "Barbell back squat",
];

/// Event kinds and their draw weights.
#[derive(Clone, Copy, Debug)]
enum K {
    LogSet,
    LogRun,
    LogRunTrack,
    Import,
    LogActivity,
    Delete,
    AmendSet,
    AmendRun,
    AmendRunMeta,
    Trim,
    Readiness,
    RemoveReadiness,
    ResolveHold,
    ClearReadiness,
    Checkin,
    ClearCheckins,
    RestartHrv,
    Review,
    Weekly,
    ClearReview,
    ClearWeekly,
    Profile,
    ClearProfile,
    Today,
    GeneratePlan,
    ClearPlan,
    MoveSession,
    Ended,
    Rung,
    Calculator,
    ClearCalculator,
    ClearSets,
    ClearRuns,
    ClearActivities,
}

const TABLE: [(u32, K); 34] = [
    (170, K::LogSet),
    (55, K::LogRun),
    (40, K::LogRunTrack),
    (10, K::Import),
    (30, K::LogActivity),
    (60, K::Delete),
    (50, K::AmendSet),
    (35, K::AmendRun),
    (25, K::AmendRunMeta),
    (30, K::Trim),
    (60, K::Readiness),
    (20, K::RemoveReadiness),
    (20, K::ResolveHold),
    (6, K::ClearReadiness),
    (60, K::Checkin),
    (2, K::ClearCheckins),
    (5, K::RestartHrv),
    (15, K::Review),
    (15, K::Weekly),
    (4, K::ClearReview),
    (4, K::ClearWeekly),
    (30, K::Profile),
    (3, K::ClearProfile),
    (95, K::Today),
    (15, K::GeneratePlan),
    (4, K::ClearPlan),
    (15, K::MoveSession),
    (15, K::Ended),
    (12, K::Rung),
    (40, K::Calculator),
    (12, K::ClearCalculator),
    (3, K::ClearSets),
    (3, K::ClearRuns),
    (3, K::ClearActivities),
];

impl Gen {
    fn new(seed: u64, mode: Mode) -> Self {
        let mut rng = Rng::new(seed);
        let today = BASE_DAY + rng.int(-200, 200);
        let offset = rng.pick(&[0, 0, 3600, 7200, -14_400, -25_200, 50_400, -43_200, 19_800]);
        Gen {
            rng,
            mode,
            today,
            offset,
            next_id: 1_760_000_000_000,
            sets: Vec::new(),
            runs: Vec::new(),
            acts: Vec::new(),
        }
    }

    fn wild(&self) -> bool {
        self.mode != Mode::Shell
    }

    /// A shell "now": a time of day inside the local `today`.
    fn now(&mut self) -> i64 {
        self.today * DAY_SEC - self.offset + self.rng.int(5 * 3600, 23 * 3600)
    }

    /// A log stamp: mostly now, sometimes backdated (the editor's date picker),
    /// and in the wilder modes legacy-undated, far-off or extreme.
    fn stamp(&mut self) -> i64 {
        let r = self.rng.unit();
        if r < 0.72 {
            self.now()
        } else if r < 0.90 {
            self.now() - self.rng.int(1, 30) * DAY_SEC
        } else if !self.wild() || r < 0.93 {
            self.now() - self.rng.int(31, 400) * DAY_SEC
        } else if r < 0.95 {
            0
        } else if r < 0.97 {
            self.now() + self.rng.int(1, 5) * DAY_SEC
        } else if self.mode == Mode::Absurd && r < 0.985 {
            self.rng
                .pick(&[i64::MIN, i64::MAX, -1, i64::MAX / 2, i64::MIN / 2])
        } else {
            self.rng
                .pick(&[1, -86_400 * 365, 4_102_444_800, 32_503_680_000, 315_532_800])
        }
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += self.rng.int(1, 90_000) as u64;
        self.next_id
    }

    /// The entry id of a new log: fresh, or (wild modes) a duplicate, legacy 0
    /// or maximal id.
    fn log_id(&mut self, existing: &[u64]) -> u64 {
        if !self.wild() {
            return self.fresh_id();
        }
        let r = self.rng.unit();
        if r < 0.82 || existing.is_empty() && r < 0.9 {
            self.fresh_id()
        } else if r < 0.9 {
            self.rng.pick(existing)
        } else if r < 0.96 {
            0
        } else {
            self.rng
                .pick(&[u64::MAX, i64::MAX as u64, i64::MAX as u64 + 1, 1])
        }
    }

    /// A float drawn from `[lo, hi)`, or (wild modes, probability `p`) a
    /// boundary value; `Absurd` adds the non-finite ones.
    fn f(&mut self, lo: f64, hi: f64, p: f64) -> f64 {
        if !self.wild() || !self.rng.chance(p) {
            return self.rng.range(lo, hi);
        }
        if self.mode == Mode::Absurd && self.rng.chance(0.3) {
            return self.rng.pick(&[f64::NAN, f64::INFINITY, f64::NEG_INFINITY]);
        }
        self.rng.pick(&[
            0.0,
            -0.0,
            -1.0,
            -1e6,
            1e6,
            1e12,
            1e300,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            5e-324,
            lo,
            hi,
            hi * 10.0,
        ])
    }

    fn opt_f(&mut self, p_some: f64, lo: f64, hi: f64) -> Option<f64> {
        if self.rng.chance(p_some) {
            Some(self.f(lo, hi, 0.15))
        } else {
            None
        }
    }

    fn day(&mut self) -> i64 {
        if self.mode == Mode::Absurd && self.rng.chance(0.02) {
            return self
                .rng
                .pick(&[i64::MIN, i64::MAX, -1_000_000_000, 1_000_000_000]);
        }
        if self.wild() && self.rng.chance(0.03) {
            return self.rng.pick(&[0, -1, -719_468, 2_932_896, 100_000]);
        }
        self.today + self.rng.int(-60, 200)
    }

    fn u8w(&mut self, lo: u8, hi: u8) -> u8 {
        if self.wild() && self.rng.chance(0.1) {
            self.rng
                .pick(&[0, 255, hi.saturating_add(1), lo.saturating_sub(1)])
        } else {
            self.rng.int(i64::from(lo), i64::from(hi)) as u8
        }
    }

    fn exercise(&mut self) -> String {
        if self.wild() && self.rng.chance(0.15) {
            return self.rng.pick(&[
                String::new(),
                "   ".into(),
                " back squat ".into(),
                "BACK SQUAT".into(),
                "Kniebeuge 🏋️‍♀️".into(),
                "ベンチプレス".into(),
                "a".repeat(400),
                "Tab\tName\nNewline".into(),
                "Quote \"x\" <b>&amp;".into(),
                "Front Lever".into(),
                "Smith machine back squat".into(),
                "Z\u{0}ero".into(),
            ]);
        }
        self.rng.pick(&SHELL_EXERCISES).into()
    }

    fn sequence(&mut self, len: usize) -> Vec<Event> {
        let mut out = Vec::with_capacity(len + 3);
        if !self.wild() || self.rng.chance(0.85) {
            out.push(Event::SetToday {
                epoch_day: self.today,
                utc_offset_sec: self.offset,
            });
        }
        if !self.wild() || self.rng.chance(0.8) {
            out.push(self.profile());
        }
        if self.rng.chance(0.6) {
            out.push(Event::GeneratePlan {
                start_epoch_day: self.today,
            });
        }
        while out.len() < len {
            let e = self.event();
            out.push(e);
        }
        out
    }

    fn event(&mut self) -> Event {
        let total: u32 = TABLE.iter().map(|(w, _)| w).sum();
        let mut r = self.rng.below(u64::from(total)) as u32;
        let mut kind = K::LogSet;
        for (w, k) in TABLE {
            if r < w {
                kind = k;
                break;
            }
            r -= w;
        }
        self.event_of(kind)
    }

    fn event_of(&mut self, kind: K) -> Event {
        match kind {
            K::LogSet => self.log_set(),
            K::LogRun => self.log_run(),
            K::LogRunTrack => self.log_run_track(),
            K::Import => self.import(),
            K::LogActivity => self.log_activity(),
            K::Delete => self.delete(),
            K::AmendSet => self.amend_set(),
            K::AmendRun => self.amend_run(),
            K::AmendRunMeta => self.amend_run_meta(),
            K::Trim => self.trim(),
            K::Readiness => Event::SubmitReadiness(self.readiness()),
            K::RemoveReadiness => Event::RemoveReadiness {
                signal: self.rng.pick(&SIGNALS),
                observed_at: if self.rng.chance(0.8) { self.now() } else { 0 },
            },
            K::ResolveHold => Event::ResolveHold {
                signal: self.rng.pick(&[
                    ReadinessSignal::Pain,
                    ReadinessSignal::Illness,
                    ReadinessSignal::Pain,
                    ReadinessSignal::Illness,
                    ReadinessSignal::Soreness,
                ]),
                observed_at: if !self.wild() || self.rng.chance(0.9) {
                    self.now()
                } else {
                    self.stamp()
                },
            },
            K::ClearReadiness => Event::ClearReadiness,
            K::Checkin => Event::SubmitCheckin(self.checkin()),
            K::ClearCheckins => Event::ClearCheckins,
            K::RestartHrv => Event::RestartHrvBaseline {
                observed_at: self.stamp(),
            },
            K::Review => Event::SubmitReview(self.review()),
            K::Weekly => Event::SubmitWeeklyCheckin(self.review()),
            K::ClearReview => Event::ClearReview,
            K::ClearWeekly => Event::ClearWeeklyCheckin,
            K::Profile => self.profile(),
            K::ClearProfile => Event::ClearProfile,
            K::Today => self.set_today(),
            K::GeneratePlan => Event::GeneratePlan {
                start_epoch_day: self.day(),
            },
            K::ClearPlan => Event::ClearPlan,
            K::MoveSession => {
                let from = self.today + self.rng.int(-3, 6);
                Event::MoveSession {
                    from_epoch_day: from,
                    to_epoch_day: if self.wild() && self.rng.chance(0.1) {
                        self.day()
                    } else {
                        from + self.rng.int(-3, 3)
                    },
                }
            }
            K::Ended => Event::SetExerciseEnded {
                exercise: self.exercise(),
                epoch_day: self.today + self.rng.int(-2, 1),
                ended: self.rng.chance(0.7),
                observed_at: self.now(),
            },
            K::Rung => Event::SetCalisthenicsRung {
                ladder: self.rng.pick(&LADDERS),
                rung: self.u8w(0, 12),
                observed_at: self.stamp(),
            },
            K::Calculator => self.calculator(),
            K::ClearCalculator => self.rng.pick(&[
                Event::ClearRacePrediction,
                Event::ClearHypertrophyPlan,
                Event::ClearProtein,
                Event::ClearHrZones,
            ]),
            K::ClearSets => Event::ClearSets,
            K::ClearRuns => Event::ClearRuns,
            K::ClearActivities => Event::ClearActivities,
        }
    }

    /// A `SetToday`. Extreme days/offsets are sent, but the generator's own
    /// clock only follows sane ones, so its stamp arithmetic cannot overflow.
    fn set_today(&mut self) -> Event {
        let r = self.rng.unit();
        let day = if r < 0.55 {
            self.today + 1
        } else if r < 0.7 {
            self.today
        } else if r < 0.82 {
            self.today + self.rng.int(2, 14)
        } else if r < 0.9 {
            self.today - self.rng.int(1, 10)
        } else if r < 0.95 {
            self.today + self.rng.int(30, 400)
        } else if !self.wild() {
            self.today - self.rng.int(11, 120)
        } else {
            self.day()
        };
        if (-10_000_000..=10_000_000).contains(&day) {
            self.today = day;
        }
        let mut offset = self.offset;
        if self.rng.chance(0.08) {
            offset = if self.mode == Mode::Absurd && self.rng.chance(0.2) {
                self.rng
                    .pick(&[i64::MIN, i64::MAX, 86_400 * 3, -86_400 * 3])
            } else {
                self.rng
                    .pick(&[0, 3600, 7200, -18_000, 50_400, -43_200, 20_700])
            };
            if offset.unsigned_abs() <= 3 * DAY_SEC as u64 {
                self.offset = offset;
            }
        }
        Event::SetToday {
            epoch_day: day,
            utc_offset_sec: offset,
        }
    }

    fn log_set(&mut self) -> Event {
        let hold = self.rng.chance(0.1);
        let exercise = if hold {
            self.rng
                .pick(&["Plank", "Dead Hang", "Front Lever", "L-sit"])
                .into()
        } else {
            self.exercise()
        };
        let weight_kg = if hold || self.rng.chance(0.08) {
            0.0
        } else if self.wild() {
            self.f(20.0, 220.0, 0.08)
        } else {
            (self.rng.range(20.0, 220.0) / 2.5).round() * 2.5
        };
        let reps = if self.wild() && self.rng.chance(0.06) {
            self.rng.pick(&[0, 10_000, 10_001, u32::MAX, 100])
        } else if hold {
            self.rng.int(0, 1) as u32
        } else {
            self.rng.int(1, 15) as u32
        };
        let rpe = if self.rng.chance(0.15) {
            0.0
        } else if self.wild() {
            self.f(6.0, 10.0, 0.08)
        } else {
            self.rng.int(12, 20) as f64 / 2.0
        };
        let hold_sec = if hold {
            Some(if self.wild() && self.rng.chance(0.1) {
                self.rng.pick(&[0, u16::MAX, 1])
            } else {
                self.rng.int(5, 120) as u16
            })
        } else {
            None
        };
        let ids: Vec<u64> = self.sets.iter().map(|s| s.0).collect();
        let entry_id = self.log_id(&ids);
        let observed_at = self.stamp();
        self.sets.push((entry_id, observed_at));
        Event::LogSet {
            exercise,
            weight_kg,
            reps,
            rpe,
            observed_at,
            entry_id,
            hold_sec,
        }
    }

    fn run_extras(&mut self) -> (Option<WorkoutType>, Option<u8>, Option<f64>) {
        let workout = self.rng.opt(0.5, |r| r.pick(&WORKOUTS));
        let rpe = if self.rng.chance(0.5) {
            Some(self.u8w(0, 10))
        } else {
            None
        };
        let avg_hr = if self.rng.chance(0.3) {
            Some(if self.wild() {
                self.f(90.0, 195.0, 0.15)
            } else {
                self.rng.int(90, 195) as f64
            })
        } else {
            None
        };
        (workout, rpe, avg_hr)
    }

    /// The shell never sends both figures: a run typed in bpm goes out with
    /// `hr_pct_max` 0 (`RunEditor.kt`'s bpm mode), which `restoreEventFor`
    /// mirrors. Only the wild modes pair a percentage with a bpm.
    fn shell_hr_pct(&self, hr_pct_max: f64, avg_hr_bpm: Option<f64>) -> f64 {
        if avg_hr_bpm.is_some() && !self.wild() {
            0.0
        } else {
            hr_pct_max
        }
    }

    fn log_run(&mut self) -> Event {
        let distance_km = if self.wild() {
            self.f(1.0, 30.0, 0.08)
        } else {
            (self.rng.range(1.0, 30.0) * 100.0).round() / 100.0
        };
        let pace = self.rng.range(4.0, 8.0);
        let duration_min = if self.wild() {
            self.f(
                distance_km.abs().min(100.0) * pace,
                distance_km.abs().min(100.0) * pace + 1.0,
                0.08,
            )
        } else {
            (distance_km * pace).round()
        };
        let hr_pct_max = if self.rng.chance(0.3) {
            0.0
        } else if self.wild() {
            self.f(55.0, 98.0, 0.08)
        } else {
            self.rng.int(55, 98) as f64
        };
        let (workout_type, session_rpe, avg_hr_bpm) = self.run_extras();
        let ids: Vec<u64> = self.runs.iter().map(|r| r.id).collect();
        let entry_id = self.log_id(&ids);
        let observed_at = self.stamp();
        self.runs.push(RunRef {
            id: entry_id,
            at: observed_at,
            span: None,
        });
        Event::LogRun {
            distance_km,
            duration_min,
            hr_pct_max: self.shell_hr_pct(hr_pct_max, avg_hr_bpm),
            longest_recent_km: if self.wild() {
                self.f(0.0, 25.0, 0.1)
            } else {
                0.0
            },
            observed_at,
            entry_id,
            workout_type,
            session_rpe,
            avg_hr_bpm,
            utc_offset_sec: None,
        }
    }

    /// A GPS track starting at `start`: steady movement with jitter, optional
    /// pauses (segment starts + relocation), and in the wild modes garbage
    /// fixes (bad accuracy, repeated / backwards time, teleports, null island).
    fn track(&mut self, start: i64) -> (Vec<GpsPoint>, Vec<u32>) {
        let n = if self.wild() && self.rng.chance(0.06) {
            self.rng.int(0, 2) as usize
        } else {
            self.rng.int(2, 500) as usize
        };
        let (mut lat, mut lon) = if self.wild() && self.rng.chance(0.08) {
            self.rng
                .pick(&[(0.0, 0.0), (89.99, 10.0), (-33.9, 179.999), (51.5, -0.0001)])
        } else {
            (
                48.1 + self.rng.range(-0.5, 0.5),
                11.5 + self.rng.range(-0.5, 0.5),
            )
        };
        let step = self.rng.int(1, 10);
        let speed = self.rng.range(2.0, 5.5);
        let mut heading = self.rng.range(0.0, std::f64::consts::TAU);
        let mut t = start;
        let mut pts = Vec::with_capacity(n);
        let mut segs = Vec::new();
        let pause_p = if self.rng.chance(0.35) { 0.01 } else { 0.0 };
        for i in 0..n {
            if i > 0 && self.rng.chance(pause_p) {
                segs.push(i as u32);
                t += self.rng.int(60, 3600);
                lat += self.rng.range(-0.02, 0.02);
                lon += self.rng.range(-0.02, 0.02);
            }
            heading += self.rng.range(-0.2, 0.2);
            let d = speed * step as f64 / 111_000.0;
            lat += d * heading.cos();
            lon += d * heading.sin() / lat.to_radians().cos().abs().max(0.01);
            let mut p = GpsPoint {
                lat,
                lon,
                observed_at: t,
                accuracy_m: self.rng.range(3.0, 15.0) as f32,
            };
            if self.rng.chance(0.01) {
                p.accuracy_m = 45.0; // a fix the core's QC should drop
            }
            if self.wild() && self.rng.chance(0.04) {
                match self.rng.below(7) {
                    0 => p.accuracy_m = self.rng.pick(&[0.0, -1.0, 1e6, f32::MAX]),
                    1 => p.observed_at = t - step, // repeated time
                    2 => p.observed_at = t - 300,  // backwards
                    3 => p.lat += 5.0,             // teleport
                    4 => {
                        p.lat = 0.0;
                        p.lon = 0.0;
                    }
                    5 => p.lat = self.rng.pick(&[95.0, -91.0, 1e9]),
                    _ => {
                        if self.mode == Mode::Absurd {
                            p.lat = f64::NAN;
                            p.accuracy_m = f32::NAN;
                        }
                    }
                }
            }
            pts.push(p);
            t += step;
        }
        if self.wild() && self.rng.chance(0.08) {
            let extra = self.rng.pick(&[0, n as u32, n as u32 + 7, u32::MAX, 1]);
            segs.push(extra);
            if self.rng.chance(0.5) {
                segs.reverse();
            }
        }
        (pts, segs)
    }

    fn log_run_track(&mut self) -> Event {
        // Fix times stay within ±100 000 years so the track/trim arithmetic
        // here cannot overflow; the run's own `observed_at` may still be
        // extreme (below).
        let start = self.stamp().clamp(-3_000_000_000_000, 3_000_000_000_000);
        let (points, segment_starts) = self.track(start);
        let span = points
            .first()
            .zip(points.last())
            .map(|(a, b)| (a.observed_at, b.observed_at));
        let ids: Vec<u64> = self.runs.iter().map(|r| r.id).collect();
        let entry_id = self.log_id(&ids);
        let observed_at = if self.rng.chance(0.85) {
            start
        } else {
            self.stamp()
        };
        self.runs.push(RunRef {
            id: entry_id,
            at: observed_at,
            span,
        });
        let (workout_type, session_rpe, avg_hr_bpm) = self.run_extras();
        let hr_pct_max = if self.rng.chance(0.5) {
            0.0
        } else {
            self.f(60.0, 95.0, 0.05)
        };
        Event::LogRunTrack {
            points,
            hr_pct_max: self.shell_hr_pct(hr_pct_max, avg_hr_bpm),
            longest_recent_km: if self.wild() {
                self.f(0.0, 25.0, 0.1)
            } else {
                0.0
            },
            segment_starts,
            observed_at,
            entry_id,
            workout_type,
            session_rpe,
            avg_hr_bpm,
            utc_offset_sec: None,
        }
    }

    /// A file import as `Gpx.kt::importedRunEvent` builds it: a fix with no
    /// accuracy takes the 30 m sentinel, segments become segment starts. The
    /// source is one of the FIT fixtures, optionally byte-damaged, so the
    /// parser's salvage output reaches the core too.
    fn import(&mut self) -> Event {
        let name = self.rng.pick(&["Activity.fit", "garmin-fenix-5-bike.fit"]);
        let mut bytes = fit_fixture(name);
        if self.rng.chance(0.5) {
            for _ in 0..self.rng.int(1, 8) {
                let i = self.rng.below(bytes.len() as u64) as usize;
                bytes[i] ^= 1 << self.rng.below(8);
            }
        }
        if self.rng.chance(0.2) {
            let keep = self.rng.below(bytes.len() as u64) as usize;
            bytes.truncate(keep);
        }
        let parsed = catch_unwind(|| crate::fit::parse_fit(&bytes))
            .ok()
            .and_then(Result::ok);
        // Unparseable, or refused by the import gate (too short, not a run):
        // the shell reports it and logs nothing.
        let Some(track) = parsed.filter(|t| crate::fit::import_gate(t).is_ok()) else {
            return self.log_run_track();
        };
        let mut points = Vec::new();
        let mut segment_starts = Vec::new();
        for seg in track.segments.iter().filter(|s| !s.is_empty()) {
            if !points.is_empty() {
                segment_starts.push(points.len() as u32);
            }
            points.extend(seg.iter().map(|f| GpsPoint {
                lat: f.lat,
                lon: f.lon,
                observed_at: f.time_sec,
                accuracy_m: f.accuracy_m.unwrap_or(30.0),
            }));
        }
        let span = points
            .first()
            .zip(points.last())
            .map(|(a, b)| (a.observed_at, b.observed_at));
        let observed_at = points.first().map_or(0, |p| p.observed_at);
        let entry_id = self.fresh_id();
        self.runs.push(RunRef {
            id: entry_id,
            at: observed_at,
            span,
        });
        Event::LogRunTrack {
            points,
            hr_pct_max: 0.0,
            longest_recent_km: 0.0,
            segment_starts,
            observed_at,
            entry_id,
            workout_type: None,
            session_rpe: None,
            avg_hr_bpm: None,
            utc_offset_sec: None,
        }
    }

    fn log_activity(&mut self) -> Event {
        let ids: Vec<u64> = self.acts.iter().map(|a| a.0).collect();
        let entry_id = self.log_id(&ids);
        let observed_at = self.stamp();
        self.acts.push((entry_id, observed_at));
        Event::LogActivity {
            kind: self.rng.pick(&ACTIVITY_KINDS),
            minutes: if self.wild() {
                self.f(5.0, 180.0, 0.15)
            } else {
                self.rng.int(5, 180) as f64
            },
            vigorous: self.rng.chance(0.4),
            strengthening: self.rng.chance(0.3),
            observed_at,
            entry_id,
        }
    }

    /// An (id, observed_at) target among the known entries, or (wild) a miss.
    fn target(&mut self, pool: &[(u64, i64)]) -> (u64, i64) {
        if pool.is_empty() || self.wild() && self.rng.chance(0.08) {
            return (self.rng.pick(&[0, 1, 42, u64::MAX]), self.stamp());
        }
        self.rng.pick(pool)
    }

    fn delete(&mut self) -> Event {
        let kind = self.rng.pick(&[
            EntryKind::Set,
            EntryKind::Set,
            EntryKind::Run,
            EntryKind::Activity,
        ]);
        let pool: Vec<(u64, i64)> = match kind {
            EntryKind::Set => self.sets.clone(),
            EntryKind::Run => self.runs.iter().map(|r| (r.id, r.at)).collect(),
            EntryKind::Activity => self.acts.clone(),
        };
        let (entry_id, observed_at_fallback) = self.target(&pool);
        Event::DeleteEntry {
            kind,
            entry_id,
            observed_at_fallback,
        }
    }

    /// An amend's (new date, old date). A legacy (`entry_id` 0) row keeps its
    /// date: the shell pins `observed_at == observed_at_fallback` for those
    /// (`SetEditorState.kt` / `RunEditor.kt`), and compaction relies on it
    /// (`log::tests::rule3_walk_assumes_legacy_rows_are_never_redated`).
    fn amend_stamps(&mut self, entry_id: u64, old_at: i64) -> (i64, i64) {
        if entry_id == 0 {
            return (old_at, old_at);
        }
        let at = if self.rng.chance(0.7) {
            old_at
        } else {
            self.stamp()
        };
        (at, if self.rng.chance(0.8) { old_at } else { 0 })
    }

    fn amend_set(&mut self) -> Event {
        let pool = self.sets.clone();
        let (entry_id, old_at) = self.target(&pool);
        let Event::LogSet {
            exercise,
            weight_kg,
            reps,
            rpe,
            hold_sec,
            ..
        } = self.log_set()
        else {
            unreachable!()
        };
        self.sets.pop(); // the template's bookkeeping row is not a new entry
        let (observed_at, observed_at_fallback) = self.amend_stamps(entry_id, old_at);
        Event::AmendSet {
            entry_id,
            exercise,
            weight_kg,
            reps,
            rpe,
            observed_at,
            observed_at_fallback,
            hold_sec,
        }
    }

    fn amend_run(&mut self) -> Event {
        let pool: Vec<(u64, i64)> = self.runs.iter().map(|r| (r.id, r.at)).collect();
        let (entry_id, old_at) = self.target(&pool);
        let Event::LogRun {
            distance_km,
            duration_min,
            hr_pct_max,
            longest_recent_km,
            workout_type,
            session_rpe,
            avg_hr_bpm,
            ..
        } = self.log_run()
        else {
            unreachable!()
        };
        self.runs.pop();
        let (observed_at, observed_at_fallback) = self.amend_stamps(entry_id, old_at);
        Event::AmendRun {
            entry_id,
            distance_km,
            duration_min,
            hr_pct_max,
            longest_recent_km,
            observed_at,
            observed_at_fallback,
            workout_type,
            session_rpe,
            avg_hr_bpm,
        }
    }

    fn amend_run_meta(&mut self) -> Event {
        let pool: Vec<(u64, i64)> = self.runs.iter().map(|r| (r.id, r.at)).collect();
        let (entry_id, observed_at_fallback) = self.target(&pool);
        let (workout_type, session_rpe, _) = self.run_extras();
        Event::AmendRunMeta {
            entry_id,
            observed_at_fallback,
            session_rpe,
            workout_type,
        }
    }

    fn trim(&mut self) -> Event {
        let gps: Vec<RunRef> = self
            .runs
            .iter()
            .copied()
            .filter(|r| r.span.is_some())
            .collect();
        if gps.is_empty() {
            return self.amend_run_meta();
        }
        let run = self.rng.pick(&gps);
        let (a, b) = run.span.expect("filtered to GPS runs");
        let len = (b - a).max(1);
        let (from, until) = match self.rng.below(6) {
            0 => (a + len / 4, b),
            1 => (a, b - len / 4),
            2 => (a + self.rng.int(0, len / 2), b - self.rng.int(0, len / 2)),
            3 if self.wild() => (b, a), // inverted
            4 if self.wild() => (a - 10 * DAY_SEC, b + 10 * DAY_SEC),
            _ => (a + len / 3, a + len / 3), // a single instant
        };
        Event::TrimRunTrack {
            entry_id: run.id,
            observed_at_fallback: run.at,
            keep_from_sec: from,
            keep_until_sec: until,
        }
    }

    fn readiness(&mut self) -> ReadinessInput {
        let signal = self.rng.pick(&SIGNALS);
        let value = match signal {
            ReadinessSignal::Pain => {
                if self.rng.chance(0.15) {
                    0.0
                } else {
                    self.rng.int(1, 10) as f64
                }
            }
            ReadinessSignal::Illness => self.rng.pick(&[0.0, 1.0, 2.0, 3.0]),
            ReadinessSignal::Soreness => self.rng.int(1, 7) as f64,
            ReadinessSignal::WellnessZ | ReadinessSignal::HrvLnRmssd => self.f(-3.0, 3.0, 0.1),
            ReadinessSignal::RestingHr => self.f(-5.0, 15.0, 0.1),
            ReadinessSignal::RedS
            | ReadinessSignal::CardiacRedFlag
            | ReadinessSignal::BoneStress => self.rng.pick(&[0.0, 1.0, 1.0]),
            _ => self.f(-40.0, 40.0, 0.1),
        };
        let pain = if signal == ReadinessSignal::Pain && self.rng.chance(0.6) {
            Some(PainDetail {
                kind: self.rng.pick(&[
                    PainKind::SharpJoint,
                    PainKind::TendonLoadRelated,
                    PainKind::Doms,
                    PainKind::Other,
                ]),
                severity: self.rng.opt(0.8, |r| r.int(0, 10) as u8),
                trend: self.rng.pick(&[
                    PainTrend::Stable,
                    PainTrend::Rising,
                    PainTrend::WorseningAfter24h,
                ]),
                persists: self.rng.chance(0.2),
                location: self.rng.opt(0.6, |r| {
                    r.pick(&[
                        "Left knee",
                        "lower back",
                        "Shoulder",
                        "Achilles",
                        "Hüfte",
                        "",
                        "Schulter 💥",
                    ])
                    .to_owned()
                }),
            })
        } else {
            None
        };
        ReadinessInput {
            signal,
            value,
            observed_at: if self.rng.chance(0.85) {
                self.now()
            } else {
                self.stamp()
            },
            streak: self.u8w(0, 5),
            pain,
            effort_min: self.opt_f(0.3, 5.0, 120.0),
        }
    }

    fn checkin(&mut self) -> CheckinInput {
        CheckinInput {
            observed_at: if self.rng.chance(0.85) {
                self.now()
            } else {
                self.stamp()
            },
            sleep_quality: self.rng.opt(0.8, |_| 0).map(|_| self.u8w(1, 5)),
            soreness: self.rng.opt(0.8, |_| 0).map(|_| self.u8w(1, 5)),
            mood: self.rng.opt(0.7, |_| 0).map(|_| self.u8w(1, 5)),
            fatigue: self.rng.opt(0.7, |_| 0).map(|_| self.u8w(1, 5)),
            stress: self.rng.opt(0.7, |_| 0).map(|_| self.u8w(1, 5)),
            resting_hr_bpm: self.opt_f(0.6, 40.0, 80.0),
            hrv_rmssd_ms: self.opt_f(0.5, 20.0, 130.0),
        }
    }

    fn review(&mut self) -> SessionReview {
        let p = 0.25;
        let b = |g: &mut Gen| g.rng.chance(p * 0.6);
        let ou8 = |g: &mut Gen, hi: u8| {
            if g.rng.chance(p) {
                Some(g.u8w(0, hi))
            } else {
                None
            }
        };
        SessionReview {
            bone_pain_red_flag: self.rng.chance(0.05),
            compulsive_flag: self.rng.chance(0.03),
            reds_signal_reported: self.rng.chance(0.03),
            overtraining_signal_count: self.u8w(0, 4),
            overtraining_signal_weeks: self.opt_f(p, 0.0, 6.0),
            overtraining_signs: self.rng.opt(p, |r| OvertrainingSigns {
                worse_performance: r.chance(0.5),
                low_mood: r.chance(0.5),
                higher_resting_hr: r.chance(0.5),
                worse_sleep: r.chance(0.5),
            }),
            single_session_spike_frac: self.opt_f(p, 0.0, 1.0),
            lift: self.rng.opt(p, |r| LiftExec {
                reps_met: r.chance(0.6),
                rir_actual: r.int(0, 5) as u8,
                rir_target: r.int(0, 4) as u8,
            }),
            interval: self.rng.opt(p, |r| IntervalExec {
                target_paces_met: r.chance(0.5),
                rpe_at_or_below_target: r.chance(0.5),
            }),
            decoupling: if self.rng.chance(p) {
                Some(Decouple {
                    drift_pct: self.f(-5.0, 15.0, 0.1),
                    cool_steady_context: self.rng.chance(0.5),
                })
            } else {
                None
            },
            easy_frac_time_above_vt1: self.opt_f(p, 0.0, 1.0),
            positive_split_pct: self.opt_f(p, -10.0, 15.0),
            rpe_load_gap_sessions: ou8(self, 4),
            weekly_velocity_drop_m_s: self.opt_f(p, 0.0, 0.2),
            failed_key_sessions: ou8(self, 4),
            mrv_sign_cluster: b(self),
            decline_weeks: ou8(self, 6),
            suppressed_wellness_domains: ou8(self, 5),
            despite_deload: b(self),
            pace_at_hr_improved_weeks: ou8(self, 6),
            hypertrophy_deload_triggers: ou8(self, 6),
            joint_ache: b(self),
            performance_down: b(self),
            low_recovery: b(self),
            rep_drop_frac: self.opt_f(p, 0.0, 0.5),
            mcv_delta_m_s: self.opt_f(p, -0.2, 0.2),
            first_set_reps_met: self.rng.opt(p, |r| r.chance(0.5)),
            first_set_rpe_delta: self.opt_f(p, -2.0, 2.0),
            cut_last_two_sessions: b(self),
            rpe_creep_plus_one: b(self),
            wellness_z_low_days: ou8(self, 7),
            interval_reps_over_target: ou8(self, 6),
            can_hold_easy_pace_under_hr_cap: self.rng.opt(p, |r| r.chance(0.5)),
            hrv_unreliable_last_three: ou8(self, 3),
            hrv_suppressed_days: ou8(self, 7),
            wellness_suppressed_days: ou8(self, 7),
            rhr_rising: b(self),
            hrv_single_day_z: self.opt_f(p, -3.0, 3.0),
            hrv_downtrend_days: ou8(self, 7),
            interference_symptoms: b(self),
            stall_failed_sessions: ou8(self, 5),
            stall_adequate_recovery: b(self),
            stalled_again_after_reramp: b(self),
            planned_hard: self.rng.opt(p, |r| r.chance(0.5)),
            trend_direction: self.rng.opt(p, |r| {
                r.pick(&[
                    TrendDirection::Up,
                    TrendDirection::Down,
                    TrendDirection::Flat,
                ])
            }),
            weeks_flat: ou8(self, 8),
            bad_day: b(self),
            observed_at: if self.rng.chance(0.9) {
                self.now()
            } else {
                self.stamp()
            },
        }
    }

    fn profile(&mut self) -> Event {
        let rng = &mut self.rng;
        let running = rng.chance(0.6);
        let health = if rng.chance(0.8) {
            HealthScreen::default()
        } else {
            HealthScreen {
                youth: rng.chance(0.2),
                parq_positive: rng.chance(0.3),
                medically_cleared: rng.chance(0.5),
                pregnant: rng.chance(0.1),
                pregnancy_warning_sign: rng.chance(0.05),
                injury_or_rehab: rng.chance(0.2),
                reds_signal: rng.chance(0.1),
                symptomatic: rng.chance(0.1),
                acute_illness: rng.chance(0.1),
                parq_yes: rng.below(128) as u8,
                cleared_parq_mask: rng.below(128) as u8,
            }
        };
        let modality = if rng.chance(0.2) {
            ResistanceModality::Calisthenics
        } else {
            ResistanceModality::Weights
        };
        let today = self.today;
        let birth = self
            .rng
            .opt(0.4, |r| today - r.int(6, 85) * 365 - r.int(0, 364));
        let age_years = if birth.is_none() {
            self.opt_f(0.3, 12.0, 80.0)
        } else {
            None
        };
        let km = if running { self.f(5.0, 80.0, 0.1) } else { 0.0 };
        let p = Profile {
            resistance_modality: modality,
            progression_cadence: self.rng.pick(&[
                ProgressionCadence::EverySession,
                ProgressionCadence::WeekToWeek,
                ProgressionCadence::MonthToMonth,
            ]),
            lift_goal: self.rng.pick(&[
                LiftGoal::MaxStrength,
                LiftGoal::Power,
                LiftGoal::Hypertrophy,
            ]),
            goal_distance: self.rng.pick(&[
                GoalDistance::General,
                GoalDistance::C25k,
                GoalDistance::FiveK,
                GoalDistance::TenK,
                GoalDistance::HalfMarathon,
                GoalDistance::Marathon,
            ]),
            concurrent_goal: self.rng.pick(&[
                ConcurrentGoal::Strength,
                ConcurrentGoal::Power,
                ConcurrentGoal::Hypertrophy,
                ConcurrentGoal::EndurancePriority,
            ]),
            weekly_sets: self.u8w(6, 24),
            running_days_per_week: if running { self.u8w(1, 6) } else { 0 },
            running_km_per_week: km,
            running_km_per_week_set: !running || self.rng.chance(0.8),
            advanced: self.rng.chance(0.3),
            female: self.rng.opt(0.8, |r| r.chance(0.5)),
            high_load_block: self.rng.chance(0.1),
            health,
            environment: self.rng.opt(0.2, |r| {
                r.pick(&[
                    Environment::Heat,
                    Environment::Altitude,
                    Environment::Cold,
                    Environment::Neutral,
                ])
            }),
            env_temp_c: self.opt_f(0.15, -10.0, 40.0),
            env_altitude_m: self.opt_f(0.1, 0.0, 3500.0),
            weeks_off: self.opt_f(0.1, 1.0, 30.0),
            bodyweight_kg: self.opt_f(0.6, 40.0, 140.0),
            age_years,
            birth_epoch_day: birth,
            resting_hr_bpm: self.opt_f(0.4, 40.0, 80.0),
            measured_hr_max: self.opt_f(0.3, 160.0, 210.0),
            weight_lb: self.rng.chance(0.2),
            // The Android shell always dates an edit with the core's today
            // (`ProfileDraft.toEvent(model.today())`); an undated edit is a
            // legacy or hand-edited log.
            effective_from_epoch_day: if self.rng.chance(0.7) {
                Some(self.today + self.rng.int(-1, 0))
            } else if self.wild() && self.rng.chance(0.5) {
                Some(self.day())
            } else if self.wild() {
                None
            } else {
                Some(self.today)
            },
            edited_at: None,
            lift_days_per_week: self.rng.opt(0.5, |_| 0).map(|_| self.u8w(2, 5)),
            training_weekdays: self.rng.opt(0.3, |r| r.below(128) as u8),
            goal_race_epoch_day: self.rng.opt(0.25, |_| 0).map(|_| self.day()),
            recent_race: if self.rng.chance(0.2) {
                Some(RecentRace {
                    distance_km: self.f(3.0, 42.2, 0.1),
                    time_sec: self.rng.int(900, 16_000) as u32,
                    epoch_day: self.rng.opt(0.6, |r| today - r.int(0, 200)),
                })
            } else {
                None
            },
            stated_longest_recent_km: self.opt_f(0.2, 2.0, 30.0),
            layoff_returned_epoch_day: self.rng.opt(0.05, |r| today - r.int(0, 60)),
            meet_epoch_day: self.rng.opt(0.1, |_| 0).map(|_| self.day()),
            calis_equipment: self.rng.opt(0.3, |r| CalisEquipment {
                pull_up_bar: r.chance(0.7),
                dip_bars: r.chance(0.5),
                rings: r.chance(0.3),
            }),
            specialization_muscle: self.rng.opt(0.15, |r| {
                r.pick(&[
                    "chest",
                    "glutes",
                    "side_delts",
                    "biceps",
                    "",
                    "unknown muscle",
                ])
                .to_owned()
            }),
            runner_strength: self.rng.chance(0.2),
        };
        Event::SetProfile(p)
    }

    fn calculator(&mut self) -> Event {
        match self.rng.below(4) {
            0 => Event::PredictRace {
                recent_distance_m: self.f(1500.0, 42_195.0, 0.15),
                recent_time_sec: self.f(300.0, 15_000.0, 0.15),
                goal_distance_m: self.f(1500.0, 42_195.0, 0.15),
                weekly_km: self.f(0.0, 120.0, 0.15),
                weeks_since_race: self.rng.opt(0.5, |r| r.int(0, 20) as u8),
            },
            1 => Event::PlanHypertrophyMeso {
                muscle: self
                    .rng
                    .pick(&["chest", "quads", "back", "side_delts", "", "Brust 💪"])
                    .into(),
                weeks: self.u8w(1, 8),
                not_growing: self.rng.chance(0.3),
                recovering_easily: self.rng.chance(0.3),
            },
            2 => Event::ComputeProtein {
                bodyweight_kg: self.f(40.0, 140.0, 0.15),
                masters: self.rng.chance(0.5),
                deficit: self.rng.chance(0.5),
            },
            _ => Event::ComputeHrZones {
                age_years: self.f(12.0, 85.0, 0.15),
                resting_hr_bpm: self.opt_f(0.5, 40.0, 80.0),
            },
        }
    }
}

fn fit_fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

// ── Invariants ────────────────────────────────────────────────────────────

/// Every float the view carries, with its field path, through a serde
/// `Serializer` that discards everything else: an `Option::None` never
/// reaches `serialize_f64`, so a non-finite value is told apart from an
/// honest absence (both are `null` in the JSON).
#[derive(Default)]
struct FloatWalk {
    path: Vec<&'static str>,
    bad: Vec<String>,
}

impl FloatWalk {
    fn float(&mut self, v: f64) -> Result<(), std::fmt::Error> {
        if !v.is_finite() && self.bad.len() < 8 {
            self.bad.push(format!("{} = {v}", self.path.join(".")));
        }
        Ok(())
    }

    fn nested<T: ?Sized + serde::Serialize>(
        &mut self,
        seg: &'static str,
        v: &T,
    ) -> Result<(), std::fmt::Error> {
        self.path.push(seg);
        let out = v.serialize(&mut *self);
        self.path.pop();
        out
    }
}

macro_rules! walk_noop {
    ($($m:ident: $t:ty),* $(,)?) => {
        $(fn $m(self, _: $t) -> Result<(), std::fmt::Error> { Ok(()) })*
    };
}

macro_rules! walk_compound {
    ($($tr:ident :: $m:ident),* $(,)?) => {
        $(impl serde::ser::$tr for &mut FloatWalk {
            type Ok = ();
            type Error = std::fmt::Error;
            fn $m<T: ?Sized + serde::Serialize>(&mut self, v: &T) -> Result<(), std::fmt::Error> {
                self.nested("[]", v)
            }
            fn end(self) -> Result<(), std::fmt::Error> {
                Ok(())
            }
        })*
    };
}

walk_compound!(
    SerializeSeq::serialize_element,
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
    SerializeTupleVariant::serialize_field,
);

impl serde::ser::SerializeMap for &mut FloatWalk {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_key<T: ?Sized + serde::Serialize>(
        &mut self,
        k: &T,
    ) -> Result<(), std::fmt::Error> {
        self.nested("{key}", k)
    }
    fn serialize_value<T: ?Sized + serde::Serialize>(
        &mut self,
        v: &T,
    ) -> Result<(), std::fmt::Error> {
        self.nested("{}", v)
    }
    fn end(self) -> Result<(), std::fmt::Error> {
        Ok(())
    }
}

impl serde::ser::SerializeStruct for &mut FloatWalk {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_field<T: ?Sized + serde::Serialize>(
        &mut self,
        key: &'static str,
        v: &T,
    ) -> Result<(), std::fmt::Error> {
        self.nested(key, v)
    }
    fn end(self) -> Result<(), std::fmt::Error> {
        Ok(())
    }
}

impl serde::ser::SerializeStructVariant for &mut FloatWalk {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_field<T: ?Sized + serde::Serialize>(
        &mut self,
        key: &'static str,
        v: &T,
    ) -> Result<(), std::fmt::Error> {
        self.nested(key, v)
    }
    fn end(self) -> Result<(), std::fmt::Error> {
        Ok(())
    }
}

impl serde::Serializer for &mut FloatWalk {
    type Ok = ();
    type Error = std::fmt::Error;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    walk_noop!(
        serialize_bool: bool,
        serialize_i8: i8,
        serialize_i16: i16,
        serialize_i32: i32,
        serialize_i64: i64,
        serialize_u8: u8,
        serialize_u16: u16,
        serialize_u32: u32,
        serialize_u64: u64,
        serialize_char: char,
        serialize_str: &str,
        serialize_bytes: &[u8],
        serialize_unit_struct: &'static str,
    );

    fn serialize_f32(self, v: f32) -> Result<(), std::fmt::Error> {
        self.float(f64::from(v))
    }
    fn serialize_f64(self, v: f64) -> Result<(), std::fmt::Error> {
        self.float(v)
    }
    fn serialize_none(self) -> Result<(), std::fmt::Error> {
        Ok(())
    }
    fn serialize_some<T: ?Sized + serde::Serialize>(self, v: &T) -> Result<(), std::fmt::Error> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), std::fmt::Error> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), std::fmt::Error> {
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(
        self,
        _: &'static str,
        v: &T,
    ) -> Result<(), std::fmt::Error> {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + serde::Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        v: &T,
    ) -> Result<(), std::fmt::Error> {
        self.nested(variant, v)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
}

/// The view crosses the JSON wire intact: no float is NaN/±∞ (serde_json
/// writes those as `null`, which the shell's non-null decode rejects or, in
/// an optional field, silently shows as absent), and the JSON decodes back
/// into a `ViewModel` the way the shell decodes it.
fn check_wire(vm: &ViewModel, json: &str) -> Result<(), String> {
    let mut walk = FloatWalk::default();
    serde::Serialize::serialize(vm, &mut walk).map_err(|_| "float walk failed".to_owned())?;
    if !walk.bad.is_empty() {
        return Err(format!("non-finite floats in the view: {:?}", walk.bad));
    }
    serde_json::from_str::<ViewModel>(json)
        .map_err(|e| format!("view JSON does not decode: {e}"))?;
    Ok(())
}

/// A display string that leaked a non-finite float ("NaN km", "inf", "∞").
fn leaked_non_finite(json: &str) -> Option<String> {
    let bytes = json.as_bytes();
    let hits = json
        .match_indices("NaN")
        .chain(json.match_indices("inf"))
        .chain(json.match_indices('∞'));
    for (i, w) in hits {
        let before = i.checked_sub(1).map(|j| bytes[j]);
        let after = bytes.get(i + w.len()).copied();
        let word = |b: Option<u8>| b.is_some_and(|c| c.is_ascii_alphabetic());
        if w == "∞" || !word(before) && !word(after) {
            let lo = char_floor(json, i.saturating_sub(60));
            let hi = char_floor(json, i + 60);
            return Some(json[lo..hi].to_owned());
        }
    }
    None
}

/// The wire and the display text of one view: the costly half of the view
/// checks (a JSON encode + decode), so the fuzz loop samples it.
fn check_wire_text(vm: &ViewModel, json: &str) -> Result<(), String> {
    check_wire(vm, json)?;
    if let Some(s) = leaked_non_finite(json) {
        return Err(format!("non-finite number leaked into display text: …{s}…"));
    }
    Ok(())
}

/// Number sanity and HARD RULE 3 on one rendered view.
fn check_numbers(vm: &ViewModel) -> Result<(), String> {
    for r in &vm.runs {
        if !(r.distance_km >= 0.0 && r.duration_min >= 0.0) {
            return Err(format!(
                "run {} shows distance {} km / duration {} min",
                r.entry_id, r.distance_km, r.duration_min
            ));
        }
        if !(0.0..=100.0).contains(&r.hr_pct_max) {
            return Err(format!("run {} shows {}% HRmax", r.entry_id, r.hr_pct_max));
        }
    }
    for l in &vm.lifts {
        if !(l.weight_kg >= 0.0 && l.e1rm_kg >= 0.0 && l.tonnage_kg >= 0.0) {
            return Err(format!(
                "set {} shows weight {} / e1RM {} / tonnage {}",
                l.entry_id, l.weight_kg, l.e1rm_kg, l.tonnage_kg
            ));
        }
        if l.reps == 0 && l.hold_sec.is_none() {
            return Err(format!("set {} has 0 reps and no hold", l.entry_id));
        }
    }
    for a in &vm.activities {
        if !(a.minutes > 0.0 && a.minutes <= 600.0) {
            return Err(format!("activity {} shows {} min", a.entry_id, a.minutes));
        }
    }
    let sessions = vm.next_session.iter().chain(&vm.week_plan);
    for s in sessions {
        for it in &s.items {
            if let Some(kg) = it.load_kg
                && kg < 0.0
            {
                return Err(format!(
                    "{} prescribes {kg} kg on day {}",
                    it.exercise, s.epoch_day
                ));
            }
            if it.plates_per_side.iter().any(|p| *p <= 0.0) {
                return Err(format!("{} plates {:?}", it.exercise, it.plates_per_side));
            }
        }
    }
    if vm.hold_reason.is_some()
        && let Some(ns) = &vm.next_session
        && (ns.status != SessionStatus::Blocked || !ns.items.is_empty())
    {
        return Err(format!(
            "training held but the next session is {:?} with {} item(s): {:?}",
            ns.status,
            ns.items.len(),
            ns.items.iter().map(|i| &i.summary).collect::<Vec<_>>()
        ));
    }
    Ok(())
}

/// What one event may do to the History lists, and the immediate safety
/// response to a fresh red-flag report.
fn check_step(prev: &ViewModel, e: &Event, next: &ViewModel, model: &Model) -> Result<(), String> {
    let d = |a: usize, b: usize| b as i64 - a as i64;
    let (dl, dr, da) = (
        d(prev.lifts.len(), next.lifts.len()),
        d(prev.runs.len(), next.runs.len()),
        d(prev.activities.len(), next.activities.len()),
    );
    let ok = match e {
        Event::LogSet { .. } => (0..=1).contains(&dl) && dr == 0 && da == 0,
        Event::LogRun { .. } | Event::LogRunTrack { .. } => dr == 1 && dl == 0 && da == 0,
        Event::LogActivity { minutes, .. } => {
            let valid = minutes.is_finite() && *minutes > 0.0 && *minutes <= 600.0;
            da == i64::from(valid) && dl == 0 && dr == 0
        }
        Event::DeleteEntry { kind, .. } => {
            let (mine, others) = match kind {
                EntryKind::Set => (dl, dr.abs() + da.abs()),
                EntryKind::Run => (dr, dl.abs() + da.abs()),
                EntryKind::Activity => (da, dl.abs() + dr.abs()),
            };
            (-1..=0).contains(&mine) && others == 0
        }
        Event::ClearSets => next.lifts.is_empty() && dr == 0 && da == 0,
        Event::ClearRuns => next.runs.is_empty() && dl == 0 && da == 0,
        Event::ClearActivities => next.activities.is_empty() && dl == 0 && dr == 0,
        // Amends and trims edit in place: never a new row, never a lost one.
        _ => dl == 0 && dr == 0 && da == 0,
    };
    if !ok {
        return Err(format!(
            "History counts moved by sets {dl:+} / runs {dr:+} / activities {da:+}"
        ));
    }
    // An amend replaces its row: the targeted id never appears more often.
    if let Event::AmendSet { entry_id, .. } = e {
        let n = |v: &ViewModel| v.lifts.iter().filter(|l| l.entry_id == *entry_id).count();
        if *entry_id != 0 && n(next) > n(prev) {
            return Err(format!(
                "AmendSet duplicated entry {entry_id}: {} → {}",
                n(prev),
                n(next)
            ));
        }
    }
    if let Event::AmendRun { entry_id, .. } = e {
        let n = |v: &ViewModel| v.runs.iter().filter(|r| r.entry_id == *entry_id).count();
        if *entry_id != 0 && n(next) > n(prev) {
            return Err(format!(
                "AmendRun duplicated entry {entry_id}: {} → {}",
                n(prev),
                n(next)
            ));
        }
    }
    // HARD RULE 3: a bare pain report, a below-neck illness or a cardiac red
    // flag logged today stops training at once, unless a later-stamped
    // reading of the same signal (an "all clear" stamped after it) is in force.
    if let Event::SubmitReadiness(input) = e
        && let Some(today) = model.today_epoch_day
        && local_day(input.observed_at, model.today_utc_offset_sec) == today
        && !model
            .inputs
            .iter()
            .any(|i| i.signal == input.signal && i.observed_at > input.observed_at)
    {
        let red = match input.signal {
            ReadinessSignal::Pain => input.pain.is_none() && input.value > 0.0,
            ReadinessSignal::Illness => input.value >= 2.0,
            ReadinessSignal::CardiacRedFlag => input.value > 0.0,
            _ => false,
        };
        if red && next.hold_reason.is_none() {
            return Err(format!(
                "{:?} {} reported today but training is not blocked (tier {:?})",
                input.signal, input.value, next.safety_tier
            ));
        }
    }
    Ok(())
}

/// One fuzz case: replay the sequence step by step with every invariant, then
/// the whole-log equivalences.
fn fuzz_case(seed: u64, mode: Mode) -> Result<(), String> {
    let mut g = Gen::new(seed, mode);
    let len = g.rng.int(8, 45) as usize;
    let events: Vec<Event> = g.sequence(len);
    let app = Engine;
    let mut model = Model::default();
    let mut prev = app.view(&model);
    for (i, e) in events.iter().enumerate() {
        let step = catch_unwind(AssertUnwindSafe(|| {
            let _ = app.update(e.clone(), &mut model);
            app.view(&model)
        }));
        let next =
            step.map_err(|p| format!("event #{i} PANIC {}: {}", panic_text(&*p), short(e)))?;
        check_numbers(&next).map_err(|m| format!("after event #{i} {}: {m}", short(e)))?;
        // The JSON encode + decode costs as much as the view: sample it.
        if i % 4 == 3 || i + 1 == events.len() {
            check_wire_text(&next, &vm_json(&next))
                .map_err(|m| format!("after event #{i} {}: {m}", short(e)))?;
        }
        check_step(&prev, e, &next, &model).map_err(|m| format!("event #{i} {}: {m}", short(e)))?;
        prev = next;
    }
    let final_json = vm_json(&prev);

    // Determinism: a cold-cache replay of the same log renders the same view.
    let cold = replay_cold(events.clone())?;
    if cold != final_json {
        return Err(format!(
            "cold replay of the same {} events differs from the live view: {}",
            events.len(),
            json_diff(&final_json, &cold)
        ));
    }

    if mode != Mode::Absurd {
        // Wire: every event survives serde. serde_json's default float parser
        // is best-effort (within an ULP), so numbers compare to 1e-15; every
        // other field (ids, stamps, strings, enums, optionals) exactly.
        for (i, e) in events.iter().enumerate() {
            let s = serde_json::to_string(e)
                .map_err(|err| format!("event #{i} won't serialize: {err}"))?;
            let back: Event = serde_json::from_str(&s).map_err(|err| {
                format!("event #{i} wire form does not decode ({err}): {}", short(e))
            })?;
            let (va, vb) = (
                serde_json::to_value(e).expect("event to value"),
                serde_json::to_value(&back).expect("event to value"),
            );
            if let Some(d) = value_diff(&va, &vb, 1e-15) {
                return Err(format!(
                    "event #{i} wire round-trip changed {d}: {}",
                    short(e)
                ));
            }
        }
    }

    // Compaction is replay-equivalent, whole and as a launch-time prefix.
    let compacted = compact_event_log(events.clone());
    if compacted.len() > events.len() {
        return Err(format!(
            "compaction grew the log {} → {}",
            events.len(),
            compacted.len()
        ));
    }
    let replayed = replay_cold(compacted.clone())?;
    if replayed != final_json {
        return Err(format!(
            "compacted log ({} of {} events) renders a different view: {}",
            compacted.len(),
            events.len(),
            json_diff(&final_json, &replayed)
        ));
    }
    let k = g.rng.below(events.len() as u64 + 1) as usize;
    let mut resumed = compact_event_log(events[..k].to_vec());
    resumed.extend_from_slice(&events[k..]);
    let replayed = replay_cold(resumed)?;
    if replayed != final_json {
        return Err(format!(
            "compacting the first {k} events, then appending the rest, renders a different view: {}",
            json_diff(&final_json, &replayed)
        ));
    }
    Ok(())
}

#[test]
fn fuzz_shell_shaped_sequences_keep_every_invariant() {
    run_cases(
        "fuzz_shell_shaped_sequences_keep_every_invariant",
        0x5348_454c_4c00_0001,
        25,
        |seed| fuzz_case(seed, Mode::Shell),
    );
}

#[test]
fn fuzz_boundary_sequences_keep_every_invariant() {
    run_cases(
        "fuzz_boundary_sequences_keep_every_invariant",
        0x424f_554e_4400_0002,
        20,
        |seed| fuzz_case(seed, Mode::Boundary),
    );
}

#[test]
fn fuzz_absurd_values_never_break_the_view() {
    run_cases(
        "fuzz_absurd_values_never_break_the_view",
        0x4142_5355_5244_0003,
        20,
        |seed| fuzz_case(seed, Mode::Absurd),
    );
}

// ── Delete → Undo and Edit → Undo, with the shell's own restore events ─────

/// `HistoryDetail.kt::deleteEventFor` (and the activity list's delete).
fn delete_of(kind: EntryKind, entry_id: u64, observed_at: i64) -> Event {
    Event::DeleteEntry {
        kind,
        entry_id,
        observed_at_fallback: observed_at,
    }
}

/// `HistoryDetail.kt::restoreEventFor` for a set.
fn restore_lift(v: &LiftResultView) -> Event {
    Event::LogSet {
        exercise: v.exercise.clone(),
        weight_kg: v.weight_kg,
        reps: v.reps,
        rpe: v.rpe,
        observed_at: v.observed_at,
        entry_id: v.entry_id,
        hold_sec: v.hold_sec,
    }
}

/// `HistoryDetail.kt::loggedHrPctMax`: a bpm entry sends 0 and lets the core
/// derive the percentage again.
fn logged_hr_pct(v: &RunResultView) -> f64 {
    if v.avg_hr_bpm.is_some() {
        0.0
    } else {
        v.hr_pct_max
    }
}

/// `restoreEventFor` for a run without a route.
fn restore_manual_run(v: &RunResultView) -> Event {
    Event::LogRun {
        distance_km: v.distance_km,
        duration_min: v.duration_min,
        hr_pct_max: logged_hr_pct(v),
        longest_recent_km: 0.0,
        observed_at: v.observed_at,
        entry_id: v.entry_id,
        workout_type: v.workout_type,
        session_rpe: v.session_rpe,
        avg_hr_bpm: v.avg_hr_bpm,
        utc_offset_sec: v.utc_offset_sec,
    }
}

/// `YouthActivity.kt::restoreActivityEvent`.
fn restore_activity(a: &ActivityResultView) -> Event {
    Event::LogActivity {
        kind: a.kind,
        minutes: a.minutes,
        vigorous: a.vigorous,
        strengthening: a.strengthening,
        observed_at: a.observed_at,
        entry_id: a.entry_id,
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` → unix seconds (Hinnant's days-from-civil).
fn rfc3339_to_unix(s: &str) -> i64 {
    let n = |a: usize, b: usize| s[a..b].parse::<i64>().expect("GPX time digits");
    let (y, m, d) = (n(0, 4), n(5, 7), n(8, 10));
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * DAY_SEC + n(11, 13) * 3600 + n(14, 16) * 60 + n(17, 19)
}

/// The core's GPX document parsed the way `Gpx.kt` reads it: one fix list per
/// `<trkseg>`, accuracy = `<hdop>` × 5 m, else the 30 m import sentinel.
fn parse_gpx(doc: &str) -> Vec<Vec<GpsPoint>> {
    doc.split("<trkseg>")
        .skip(1)
        .map(|seg| {
            seg.split("<trkpt ")
                .skip(1)
                .map(|pt| {
                    let attr = |name: &str| {
                        let key = format!("{name}=\"");
                        let s = pt.find(&key).expect("trkpt attribute") + key.len();
                        let e = s + pt[s..].find('"').expect("closing quote");
                        pt[s..e].parse::<f64>().expect("numeric attribute")
                    };
                    let tag = |name: &str| {
                        let open = format!("<{name}>");
                        let s = pt.find(&open)? + open.len();
                        let e = s + pt[s..].find('<')?;
                        Some(&pt[s..e])
                    };
                    GpsPoint {
                        lat: attr("lat"),
                        lon: attr("lon"),
                        observed_at: rfc3339_to_unix(tag("time").expect("trkpt time")),
                        accuracy_m: tag("hdop")
                            .and_then(|h| h.trim().parse::<f64>().ok())
                            .filter(|h| h.is_finite() && *h > 0.0)
                            .map_or(30.0, |h| (h * 5.0) as f32),
                    }
                })
                .collect()
        })
        .collect()
}

/// `restoreEventFor` for a GPS run (a route, or a track QC left unmeasurable):
/// the fixes rebuilt from the core's own GPX export (the registry the last
/// `view()` on this thread refreshed); `None` when fewer than two fixes come
/// back, where the shell offers no Undo.
fn restore_gps_run(v: &RunResultView) -> Option<Event> {
    let mut points = Vec::new();
    let mut segment_starts = Vec::new();
    for seg in parse_gpx(&export_run_gpx(v.entry_id, v.observed_at)) {
        if seg.is_empty() {
            continue;
        }
        if !points.is_empty() {
            segment_starts.push(points.len() as u32);
        }
        points.extend(seg);
    }
    (points.len() >= 2).then(|| Event::LogRunTrack {
        points,
        hr_pct_max: logged_hr_pct(v),
        longest_recent_km: 0.0,
        segment_starts,
        observed_at: v.observed_at,
        entry_id: v.entry_id,
        workout_type: v.workout_type,
        session_rpe: v.session_rpe,
        avg_hr_bpm: v.avg_hr_bpm,
        utc_offset_sec: v.utc_offset_sec,
    })
}

/// A shell-shaped history replayed into a live model on this thread.
fn shell_history(seed: u64) -> (Gen, Engine, Model) {
    let mut g = Gen::new(seed, Mode::Shell);
    let len = g.rng.int(12, 40) as usize;
    let events = g.sequence(len);
    let app = Engine;
    let mut model = Model::default();
    for e in events {
        let _ = app.update(e, &mut model);
    }
    (g, app, model)
}

/// One History row a delete or edit can target.
enum Row<'a> {
    Lift(&'a LiftResultView),
    Run(&'a RunResultView),
    Activity(&'a ActivityResultView),
}

impl Row<'_> {
    fn kind(&self) -> EntryKind {
        match self {
            Row::Lift(_) => EntryKind::Set,
            Row::Run(_) => EntryKind::Run,
            Row::Activity(_) => EntryKind::Activity,
        }
    }

    fn id(&self) -> (u64, i64) {
        match self {
            Row::Lift(v) => (v.entry_id, v.observed_at),
            Row::Run(v) => (v.entry_id, v.observed_at),
            Row::Activity(v) => (v.entry_id, v.observed_at),
        }
    }

    fn count(kind: EntryKind, vm: &ViewModel) -> usize {
        match kind {
            EntryKind::Set => vm.lifts.len(),
            EntryKind::Run => vm.runs.len(),
            EntryKind::Activity => vm.activities.len(),
        }
    }
}

fn json_value(json: &str) -> serde_json::Value {
    serde_json::from_str(json).expect("view parses")
}

/// Up to three delete → Undo rounds on random History rows (GPS runs only
/// when `gps`, otherwise sets, hand-entered runs and activities): each must
/// remove exactly the row, and its Undo must render the view from before.
/// GPS routes come back through the core's GPX at full (shortest round-trip)
/// precision, so the live tracker's raw doubles survive the Undo. Every float
/// sum and tie the view shows is taken in log order (time, then `entry_id`),
/// never storage order, so an Undo (the row re-pushed at the tail) must render
/// the view EXACTLY: no float or text tolerance.
fn undo_delete_case(seed: u64, gps: bool) -> Result<(), String> {
    let (mut g, app, mut model) = shell_history(seed);
    let mut before = app.view(&model);
    for round in 0..3 {
        let mut rows: Vec<Row> = Vec::new();
        let runs = before.runs.iter();
        if gps {
            rows.extend(runs.filter(|r| r.has_gpx).map(Row::Run));
        } else {
            rows.extend(before.lifts.iter().map(Row::Lift));
            rows.extend(runs.filter(|r| !r.has_gpx).map(Row::Run));
            rows.extend(before.activities.iter().map(Row::Activity));
        }
        if rows.is_empty() {
            break;
        }
        let row = rows.swap_remove(g.rng.below(rows.len() as u64) as usize);
        let restore = match &row {
            Row::Lift(v) => Some(restore_lift(v)),
            Row::Run(v) if gps || v.gps_unmeasurable => restore_gps_run(v),
            Row::Run(v) => Some(restore_manual_run(v)),
            Row::Activity(a) => Some(restore_activity(a)),
        };
        let Some(restore) = restore else { continue };
        let (id, at) = row.id();
        let kind = row.kind();
        let before_json = vm_json(&before);
        let _ = app.update(delete_of(kind, id, at), &mut model);
        let mid = app.view(&model);
        if Row::count(kind, &mid) + 1 != Row::count(kind, &before) {
            return Err(format!(
                "round {round}: deleting {kind:?} {id}@{at} did not remove exactly one row"
            ));
        }
        let _ = app.update(restore.clone(), &mut model);
        let after = app.view(&model);
        let after_json = vm_json(&after);
        let diff = value_diff(&json_value(&before_json), &json_value(&after_json), 0.0);
        if let Some(d) = diff {
            return Err(format!(
                "round {round}: delete + Undo of {kind:?} {id}@{at} changed the view: {d} (undo event {})",
                short(&restore)
            ));
        }
        before = after;
    }
    Ok(())
}

/// Edit a set or hand-entered run from History, then Undo it with the
/// shell's `undoEditFor` amend: the edit must show, and the Undo must render
/// the view from before.
fn undo_edit_case(seed: u64) -> Result<(), String> {
    let (mut g, app, mut model) = shell_history(seed);
    let mut before = app.view(&model);
    for round in 0..3 {
        let mut rows: Vec<Row> = before.lifts.iter().map(Row::Lift).collect();
        // The shell edits only hand-entered runs (`RunLogSection` editable:
        // no route and a distance), never a GPS run QC left unmeasurable.
        rows.extend(
            before
                .runs
                .iter()
                .filter(|r| !r.has_gpx && !r.gps_unmeasurable)
                .map(Row::Run),
        );
        if rows.is_empty() {
            break;
        }
        let row = rows.swap_remove(g.rng.below(rows.len() as u64) as usize);
        let new_at = |g: &mut Gen, at: i64| {
            if g.rng.chance(0.3) {
                at - g.rng.int(1, 5) * DAY_SEC
            } else {
                at
            }
        };
        let (edit, undo) = match &row {
            Row::Lift(v) => {
                let Event::LogSet {
                    exercise,
                    weight_kg,
                    reps,
                    rpe,
                    hold_sec,
                    ..
                } = g.log_set()
                else {
                    unreachable!()
                };
                // The edit sheet keeps a hold a hold.
                let (reps, hold_sec) = match (v.hold_sec, hold_sec) {
                    (Some(_), _) => (reps.min(1), Some(g.rng.int(5, 120) as u16)),
                    (None, _) => (reps.max(1), None),
                };
                let observed_at = new_at(&mut g, v.observed_at);
                let edit = Event::AmendSet {
                    entry_id: v.entry_id,
                    exercise,
                    weight_kg,
                    reps,
                    rpe,
                    observed_at,
                    observed_at_fallback: v.observed_at,
                    hold_sec,
                };
                let undo = Event::AmendSet {
                    entry_id: v.entry_id,
                    exercise: v.exercise.clone(),
                    weight_kg: v.weight_kg,
                    reps: v.reps,
                    rpe: v.rpe,
                    observed_at: v.observed_at,
                    observed_at_fallback: observed_at,
                    hold_sec: v.hold_sec,
                };
                (edit, undo)
            }
            Row::Run(v) => {
                let Event::LogRun {
                    distance_km,
                    duration_min,
                    hr_pct_max,
                    workout_type,
                    session_rpe,
                    avg_hr_bpm,
                    ..
                } = g.log_run()
                else {
                    unreachable!()
                };
                let observed_at = new_at(&mut g, v.observed_at);
                let edit = Event::AmendRun {
                    entry_id: v.entry_id,
                    distance_km,
                    duration_min,
                    hr_pct_max,
                    longest_recent_km: 0.0,
                    observed_at,
                    observed_at_fallback: v.observed_at,
                    workout_type,
                    session_rpe,
                    avg_hr_bpm,
                };
                let undo = Event::AmendRun {
                    entry_id: v.entry_id,
                    distance_km: v.distance_km,
                    duration_min: v.duration_min,
                    hr_pct_max: logged_hr_pct(v),
                    longest_recent_km: 0.0,
                    observed_at: v.observed_at,
                    observed_at_fallback: observed_at,
                    workout_type: v.workout_type,
                    session_rpe: v.session_rpe,
                    avg_hr_bpm: v.avg_hr_bpm,
                };
                (edit, undo)
            }
            Row::Activity(_) => unreachable!("activities have no edit"),
        };
        let kind = row.kind();
        let (id, _) = row.id();
        let before_json = vm_json(&before);
        let _ = app.update(edit.clone(), &mut model);
        let mid = app.view(&model);
        if Row::count(kind, &mid) != Row::count(kind, &before) {
            return Err(format!(
                "round {round}: the edit changed the row count: {}",
                short(&edit)
            ));
        }
        let shows_edit = match &edit {
            Event::AmendSet { weight_kg, .. } => mid
                .lifts
                .iter()
                .any(|l| l.entry_id == id && (l.weight_kg - weight_kg).abs() < 1e-9),
            Event::AmendRun { distance_km, .. } => mid
                .runs
                .iter()
                .any(|r| r.entry_id == id && (r.distance_km - distance_km).abs() < 0.01),
            _ => true,
        };
        if !shows_edit {
            return Err(format!(
                "round {round}: the edit does not show in History: {}",
                short(&edit)
            ));
        }
        let _ = app.update(undo.clone(), &mut model);
        let after = app.view(&model);
        let after_json = vm_json(&after);
        let diff = value_diff(&json_value(&before_json), &json_value(&after_json), 0.0);
        if let Some(d) = diff {
            return Err(format!(
                "round {round}: edit + Undo changed the view: {d} (edit {}, undo {})",
                short(&edit),
                short(&undo)
            ));
        }
        before = after;
    }
    Ok(())
}

#[test]
fn delete_then_undo_restores_the_view() {
    run_cases(
        "delete_then_undo_restores_the_view",
        0x554e_444f_0000_0004,
        40,
        |seed| undo_delete_case(seed, false),
    );
}

#[test]
fn gps_run_delete_then_undo_via_gpx_restores_the_view() {
    run_cases(
        "gps_run_delete_then_undo_via_gpx_restores_the_view",
        0x4750_5355_4e44_0005,
        40,
        |seed| undo_delete_case(seed, true),
    );
}

#[test]
fn edit_then_undo_restores_the_view() {
    run_cases(
        "edit_then_undo_restores_the_view",
        0x4544_4954_0000_0006,
        40,
        undo_edit_case,
    );
}

// ── Mutated wire JSON ("NaN-like" values the JSON grammar can carry) ───────

/// Every JSON-pointer path to a leaf (or empty container) of `v`.
fn leaves(v: &serde_json::Value, path: &mut String, out: &mut Vec<String>) {
    use serde_json::Value as V;
    match v {
        V::Object(m) if !m.is_empty() => {
            for (k, c) in m {
                let len = path.len();
                path.push('/');
                path.push_str(&k.replace('~', "~0").replace('/', "~1"));
                leaves(c, path, out);
                path.truncate(len);
            }
        }
        V::Array(a) if !a.is_empty() => {
            // A long GPS track: a handful of fixes is enough.
            for (i, c) in a.iter().enumerate().take(6) {
                let len = path.len();
                path.push_str(&format!("/{i}"));
                leaves(c, path, out);
                path.truncate(len);
            }
        }
        _ => out.push(path.clone()),
    }
}

/// One random structural or value mutation of an event's JSON.
fn mutate(rng: &mut Rng, v: &mut serde_json::Value) {
    use serde_json::json;
    let mut paths = Vec::new();
    leaves(v, &mut String::new(), &mut paths);
    let path = rng.pick(&paths);
    let r = rng.unit();
    if r < 0.12 {
        // Drop the field: required fields must fail to decode, optional ones
        // must default.
        if let Some((parent, key)) = path.rsplit_once('/')
            && let Some(serde_json::Value::Object(m)) = v.pointer_mut(parent)
        {
            m.remove(key);
        }
        return;
    }
    if r < 0.2 {
        if let Some(serde_json::Value::Object(m)) =
            v.as_object_mut().and_then(|m| m.values_mut().next())
        {
            m.insert("zz_unknown_field".into(), json!(1));
        }
        return;
    }
    let replacement = rng.pick(&[
        serde_json::Value::Null,
        json!(1e308),
        json!(-1e308),
        json!(1.7976931348623157e308),
        json!(1e20),
        json!(-1e20),
        json!(i64::MAX),
        json!(i64::MIN),
        json!(u64::MAX),
        json!(4_294_967_296_u64),
        json!(65_536),
        json!(256),
        json!(-1),
        json!(0),
        json!(-0.0),
        json!(0.5),
        json!(5e-324),
        json!("NaN"),
        json!("Infinity"),
        json!(""),
        json!("🏃‍♀️"),
        json!(true),
        json!([]),
        json!({}),
    ]);
    if let Some(slot) = v.pointer_mut(&path) {
        *slot = replacement;
    }
}

/// Text-level damage serde_json must reject without panicking: non-JSON
/// number spellings and truncation.
fn mutate_text(rng: &mut Rng, s: &str) -> String {
    match rng.below(3) {
        0 => {
            // Replace the first number with a spelling JSON does not allow
            // (or one that overflows f64).
            let start = s.find(|c: char| c.is_ascii_digit()).unwrap_or(0);
            let end = start
                + s[start..]
                    .find(|c: char| !c.is_ascii_digit() && c != '.')
                    .unwrap_or(0);
            let bad = rng.pick(&[
                "1e999",
                "-1e999",
                "NaN",
                "Infinity",
                "-Infinity",
                "0x10",
                "1e-999",
                "01",
            ]);
            format!("{}{bad}{}", &s[..start], &s[end..])
        }
        1 => s[..char_floor(s, rng.below(s.len() as u64) as usize)].to_owned(),
        _ => s.replacen(':', ":-", 1),
    }
}

fn mutation_case(seed: u64) -> Result<(), String> {
    let mut g = Gen::new(seed, Mode::Shell);
    let len = g.rng.int(6, 20) as usize;
    let app = Engine;
    let mut model = Model::default();
    for e in g.sequence(len) {
        let _ = app.update(e, &mut model);
    }
    // Once a mutation put the literal string "NaN"/"Infinity" into a user
    // field (an exercise name), the view echoes it: only the wire is checked.
    let mut nan_user_text = false;
    for j in 0..14 {
        let e = g.event();
        let mut v = serde_json::to_value(&e).expect("event to value");
        for _ in 0..g.rng.int(1, 3) {
            mutate(&mut g.rng, &mut v);
        }
        let text = if g.rng.chance(0.15) {
            mutate_text(&mut g.rng, &v.to_string())
        } else {
            v.to_string()
        };
        let decoded = catch_unwind(|| serde_json::from_str::<Event>(&text))
            .map_err(|p| format!("decoding #{j} PANIC {}: {text}", panic_text(&*p)))?;
        let Ok(ev) = decoded else { continue };
        let vm = catch_unwind(AssertUnwindSafe(|| {
            let _ = app.update(ev, &mut model);
            app.view(&model)
        }))
        .map_err(|p| format!("mutated event #{j} PANIC {}: {text}", panic_text(&*p)))?;
        check_numbers(&vm).map_err(|m| format!("after mutated event #{j} {text}: {m}"))?;
        let json = vm_json(&vm);
        nan_user_text |= text.contains("NaN") || text.contains("Infinity");
        let check = if nan_user_text {
            check_wire(&vm, &json)
        } else {
            check_wire_text(&vm, &json)
        };
        check.map_err(|m| format!("after mutated event #{j} {text}: {m}"))?;
    }
    Ok(())
}

#[test]
fn mutated_wire_json_never_breaks_the_core() {
    run_cases(
        "mutated_wire_json_never_breaks_the_core",
        0x4a53_4f4e_0000_0007,
        40,
        mutation_case,
    );
}

// ── Focused regressions for the bugs the fuzzers found ─────────────────────

/// A hand-entered run with a typed average HR, logged under a profile that
/// knows the athlete's HRmax.
fn bpm_run(at: i64) -> Event {
    Event::LogRun {
        distance_km: 8.0,
        duration_min: 45.0,
        hr_pct_max: 0.0,
        longest_recent_km: 0.0,
        observed_at: at,
        entry_id: 1_760_000_000_001,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: Some(150.0),
        utc_offset_sec: None,
    }
}

#[test]
fn compaction_keeps_the_profile_a_runs_bpm_was_converted_against() {
    let at = BASE_DAY * DAY_SEC + 8 * 3600;
    // A later profile edit (here: the measured HRmax removed) supersedes the
    // one the run was converted against.
    let profile_edit = vec![
        set_today_ev(BASE_DAY),
        Event::SetProfile(Profile {
            measured_hr_max: Some(190.0),
            ..base_profile()
        }),
        bpm_run(at),
        Event::SetProfile(base_profile()),
    ];
    // The birthday-derived age (Tanaka HRmax) at log time, superseded by a
    // later SetToday: compaction keeps only the last day line.
    let birthday = vec![
        set_today_ev(BASE_DAY),
        Event::SetProfile(Profile {
            birth_epoch_day: Some(BASE_DAY - 40 * 365),
            ..base_profile()
        }),
        bpm_run(at),
        set_today_ev(BASE_DAY + 1),
    ];
    for (name, events) in [("profile edit", profile_edit), ("later SetToday", birthday)] {
        assert_compaction_equivalent(name, events);
    }
}

#[test]
fn a_negative_hr_percentage_never_reaches_the_run_card() {
    let at = BASE_DAY * DAY_SEC + 8 * 3600;
    let app = Engine;
    let mut model = Model::default();
    let _ = app.update(log_run(8.0, 45.0, -5.0, 0.0, at, 7), &mut model);
    let _ = app.update(
        Event::AmendRun {
            entry_id: 7,
            distance_km: 8.0,
            duration_min: 45.0,
            hr_pct_max: -1e300,
            longest_recent_km: 0.0,
            observed_at: at,
            observed_at_fallback: at,
            workout_type: None,
            session_rpe: None,
            avg_hr_bpm: None,
        },
        &mut model,
    );
    let _ = app.update(log_run(5.0, 30.0, -5.0, 0.0, at + 3600, 8), &mut model);
    let vm = app.view(&model);
    for r in &vm.runs {
        assert!(
            (0.0..=100.0).contains(&r.hr_pct_max),
            "run {} shows {}% HRmax ({:?})",
            r.entry_id,
            r.hr_pct_max,
            r.hr_label
        );
    }
}

/// Delete a run row from History and send the shell's Undo; the row as
/// shown (before, after).
fn delete_and_undo_run(app: &Engine, model: &mut Model, id: u64) -> (RunResultView, RunResultView) {
    let row = |vm: ViewModel| {
        vm.runs
            .into_iter()
            .find(|r| r.entry_id == id)
            .expect("run row")
    };
    let before = row(app.view(model));
    let _ = app.update(delete_of(EntryKind::Run, id, before.observed_at), model);
    let _ = app.update(restore_manual_run(&before), model);
    (before, row(app.view(model)))
}

#[test]
fn undo_of_a_deleted_bpm_run_keeps_its_hr_percentage() {
    let app = Engine;
    let mut model = Model::default();
    // The shell dates every edit with the core's today (an undated legacy
    // edit pins the runs logged before it instead, so the Undo, a later log
    // line, would read it).
    let measured = |hr_max: f64, eff: i64| {
        Event::SetProfile(Profile {
            measured_hr_max: Some(hr_max),
            effective_from_epoch_day: Some(eff),
            ..base_profile()
        })
    };
    set_today(&app, &mut model, BASE_DAY);
    let _ = app.update(measured(190.0, BASE_DAY), &mut model);
    let _ = app.update(bpm_run(BASE_DAY * DAY_SEC + 8 * 3600), &mut model);
    // A later lab test lowers the measured HRmax.
    set_today(&app, &mut model, BASE_DAY + 1);
    let _ = app.update(measured(168.0, BASE_DAY + 1), &mut model);
    let (before, after) = delete_and_undo_run(&app, &mut model, 1_760_000_000_001);
    assert_eq!(
        (before.hr_pct_max, &before.hr_label, before.zone),
        (after.hr_pct_max, &after.hr_label, after.zone),
        "delete + Undo changed the run's heart-rate figures"
    );
}

#[test]
fn undo_of_a_deleted_run_keeps_its_spike_chip() {
    let app = Engine;
    let mut model = Model::default();
    let at = BASE_DAY * DAY_SEC + 8 * 3600;
    set_today(&app, &mut model, BASE_DAY);
    // Today's 20 km run first, then a 5 km run from two days ago logged late.
    let _ = app.update(log_run(20.0, 110.0, 70.0, 0.0, at, 11), &mut model);
    let _ = app.update(
        log_run(5.0, 30.0, 70.0, 0.0, at - 2 * DAY_SEC, 12),
        &mut model,
    );
    let (before, after) = delete_and_undo_run(&app, &mut model, 11);
    assert_eq!(
        (
            before.spike_flag,
            before.spike_has_baseline,
            &before.spike_note
        ),
        (
            after.spike_flag,
            after.spike_has_baseline,
            &after.spike_note
        ),
        "delete + Undo changed the run card's spike chip"
    );
}

#[test]
fn a_run_trimmed_to_its_standing_start_never_shows_negative_zero() {
    let start = BASE_DAY * DAY_SEC + 8 * 3600;
    // 20 fixes milling about at the start (0.2 m/s, under the 0.5 m/s
    // auto-pause floor), then 3 m/s for 8 min.
    let mut points = Vec::new();
    for i in 0..120i64 {
        let metres = if i < 20 {
            i as f64
        } else {
            20.0 + (i - 20) as f64 * 15.0
        };
        points.push(GpsPoint {
            lat: 48.1 + metres / 111_000.0,
            lon: 11.5,
            observed_at: start + i * 5,
            accuracy_m: 5.0,
        });
    }
    let app = Engine;
    let mut model = Model::default();
    let _ = app.update(log_run_track(points, 0.0, 0.0, start, 21), &mut model);
    let moved = app.view(&model);
    assert!(
        moved.runs[0].distance_km > 1.0,
        "the untrimmed run moved: {}",
        moved.runs[0].summary
    );
    let _ = app.update(trim(21, start, start, start + 19 * 5), &mut model);
    let r = &app.view(&model).runs[0];
    assert!(
        !r.distance_km.is_sign_negative() && !r.summary.contains("-0"),
        "distance {:?}, summary {:?}, split_pct {:?}",
        r.distance_km,
        r.summary,
        r.split_pct
    );
}

#[test]
fn compaction_keeps_the_date_of_an_undated_race() {
    // A legacy (undated) profile edit stating a race: the race has no day to
    // be dated with, whatever SetToday preceded it (compaction keeps only the
    // last one), so it stays undated in both replays.
    assert_compaction_equivalent(
        "legacy undated race",
        vec![
            set_today_ev(BASE_DAY),
            Event::SetProfile(Profile {
                recent_race: Some(RecentRace {
                    distance_km: 10.0,
                    time_sec: 2700,
                    epoch_day: None,
                }),
                ..base_profile()
            }),
            set_today_ev(BASE_DAY + 6),
        ],
    );
    // The shell's path: the race is dated with the edit's effective day, and
    // an unchanged undated re-send keeps that date, in both replays.
    let race = |eff: i64| {
        Event::SetProfile(Profile {
            recent_race: Some(RecentRace {
                distance_km: 10.0,
                time_sec: 2700,
                epoch_day: None,
            }),
            effective_from_epoch_day: Some(eff),
            ..base_profile()
        })
    };
    let events = vec![
        set_today_ev(BASE_DAY),
        race(BASE_DAY),
        set_today_ev(BASE_DAY + 6),
        race(BASE_DAY + 6),
        set_today_ev(BASE_DAY + 7),
    ];
    for log in [events.clone(), compact_event_log(events.clone())] {
        let app = Engine;
        let mut model = Model::default();
        for e in log {
            let _ = app.update(e, &mut model);
        }
        let day = model
            .profile()
            .and_then(|p| p.recent_race.as_ref())
            .and_then(|r| r.epoch_day);
        assert_eq!(day, Some(BASE_DAY), "the re-sent race keeps its first day");
    }
    assert_compaction_equivalent("re-sent race", events);
}

#[test]
fn compaction_keeps_in_window_checkins_when_one_is_dated_after_today() {
    // Minimized from fuzz seed 0xa86ae7dfa6e1e015 (shell mode): a check-in
    // stamped 84 days ahead (a wrong device clock, later corrected), then
    // yesterday's check-in, then today.
    assert_compaction_equivalent(
        "check-in dated after today",
        vec![
            Event::SubmitCheckin(CheckinInput {
                observed_at: 1_770_829_517,
                mood: Some(1),
                resting_hr_bpm: Some(55.6),
                ..CheckinInput::default()
            }),
            Event::SubmitCheckin(CheckinInput {
                observed_at: 1_763_517_034,
                sleep_quality: Some(3),
                soreness: Some(5),
                mood: Some(4),
                fatigue: Some(5),
                stress: Some(3),
                ..CheckinInput::default()
            }),
            Event::SetToday {
                epoch_day: 20_412,
                utc_offset_sec: 19_800,
            },
        ],
    );
}

/// A corrupt log's extreme `SetToday` never panics `view()`: for a held youth
/// (the WHO card's Monday and minutes) and for a planned adult with a meet, a
/// race, a run and a set, whose plan synthesis takes the meet offset and the
/// race countdown (`saturating_sub` in `plan::apply_lift_week_modifiers` and
/// `plan::run::cycle_pos`).
#[test]
fn an_extreme_set_today_never_panics_the_view() {
    let youth = Profile {
        health: HealthScreen {
            youth: true,
            ..HealthScreen::default()
        },
        ..base_profile()
    };
    let adult = Profile {
        goal_distance: GoalDistance::TenK,
        goal_race_epoch_day: Some(BASE_DAY + 60),
        meet_epoch_day: Some(BASE_DAY + 40),
        running_days_per_week: 3,
        running_km_per_week: 30.0,
        weekly_sets: 12,
        lift_days_per_week: Some(3),
        ..base_profile()
    };
    for (who, profile) in [("youth", youth), ("planned adult", adult)] {
        for day in [i64::MIN, i64::MIN / 2, i64::MAX] {
            let app = Engine;
            let mut model = Model::default();
            let at = BASE_DAY * DAY_SEC + 7 * 3600;
            for e in [
                set_today_ev(BASE_DAY),
                Event::SetProfile(profile.clone()),
                Event::GeneratePlan {
                    start_epoch_day: BASE_DAY,
                },
                log_run(6.0, 36.0, 70.0, 0.0, at, 1),
                log_set_id("Back squat", 100.0, 2, at),
                set_today_ev(day),
            ] {
                let _ = app.update(e, &mut model);
            }
            let vm = catch_unwind(AssertUnwindSafe(|| app.view(&model)));
            assert!(
                vm.is_ok(),
                "view() panicked for a {who} after SetToday {day}"
            );
        }
    }
}

#[test]
fn undo_of_a_deleted_set_keeps_the_lift_audit_name() {
    let day = BASE_DAY * DAY_SEC + 17 * 3600;
    let app = Engine;
    let mut model = Model::default();
    for e in [
        set_today_ev(BASE_DAY),
        // Yesterday's heavy triple anchors the e1RM.
        log_set_id("Back Squat", 120.0, 1, day - DAY_SEC),
        // Today: two singles at ~86 %, one under the retired catalog label
        // (stored as "Back squat").
        with_entry_id(log_set("Back Squat", 115.0, 1, 9.0, day), 2),
        with_entry_id(log_set("Barbell back squat", 115.0, 1, 9.0, day + 300), 3),
    ] {
        let _ = app.update(e, &mut model);
    }
    let audit = |vm: &ViewModel| {
        vm.lift_audit
            .iter()
            .map(|g| g.summary.clone())
            .collect::<Vec<_>>()
    };
    let before = app.view(&model);
    assert!(
        audit(&before).iter().any(|s| s.contains("quat")),
        "the fixture raises a squat audit row: {:?}",
        audit(&before)
    );
    let row = before
        .lifts
        .iter()
        .find(|l| l.entry_id == 2)
        .expect("set 2")
        .clone();
    let _ = app.update(delete_of(EntryKind::Set, 2, row.observed_at), &mut model);
    let _ = app.update(restore_lift(&row), &mut model);
    assert_eq!(
        audit(&before),
        audit(&app.view(&model)),
        "delete + Undo renamed the audit row"
    );
}

#[test]
fn gps_undo_of_a_live_recorded_run_keeps_its_pace() {
    // Fuzz seed whose live-precision track lands on a pace rounding edge:
    // "4:07/km" before the delete, "4:06/km" after the Undo.
    undo_delete_case(0x8983_8f5b_2a87_82d5, true).unwrap_or_else(|m| panic!("{m}"));
}

#[test]
fn undo_of_a_deleted_unmeasurable_gps_run_restores_it_as_recorded() {
    // Fuzz seed 0x1b7c7ac2de61f20e: a GPS run whose fixes all fail QC ("GPS
    // signal too poor") came back from Undo as a 0 km manual run, with another
    // card. The core now exports its recorded fixes for Undo, so the shell
    // re-logs it as the same tracked run.
    undo_delete_case(0x1b7c_7ac2_de61_f20e, false).unwrap_or_else(|m| panic!("{m}"));
    let at = BASE_DAY * DAY_SEC + 8 * 3600;
    let still = |i: i64| GpsPoint {
        lat: 48.1,
        lon: 11.5,
        observed_at: at - 60 + i * 30,
        accuracy_m: 5.0,
    };
    let app = Engine;
    let mut model = Model::default();
    let _ = app.update(
        log_run_track(vec![still(0), still(1)], 0.0, 0.0, at, 31),
        &mut model,
    );
    let before = app.view(&model);
    let row = before.runs[0].clone();
    assert!(row.gps_unmeasurable && !row.has_gpx, "{}", row.summary);
    let restore = restore_gps_run(&row).expect("the recorded fixes export for Undo");
    let _ = app.update(delete_of(EntryKind::Run, 31, at), &mut model);
    let _ = app.update(restore, &mut model);
    assert_eq!(vm_json(&before), vm_json(&app.view(&model)));
}

#[test]
fn undo_of_a_run_keeps_the_weekly_km() {
    let at = BASE_DAY * DAY_SEC + 8 * 3600;
    let app = Engine;
    let mut model = Model::default();
    set_today(&app, &mut model, BASE_DAY);
    // 13.69 + 29.27 + 6.89 = 49.85 exactly; the float sum lands either side
    // of the rounding edge depending on the order.
    for (i, km) in [13.69, 29.27, 6.89].into_iter().enumerate() {
        let _ = app.update(
            log_run(km, 60.0, 70.0, 0.0, at - i as i64 * 3600, 100 + i as u64),
            &mut model,
        );
    }
    let week_km = |vm: &ViewModel| {
        vm.run_progress
            .as_ref()
            .and_then(|p| p.weekly.last())
            .map(|w| w.km)
    };
    delete_and_undo_run(&app, &mut model, 100);
    // The same three runs logged fresh, in their original order.
    let mut fresh = Model::default();
    set_today(&app, &mut fresh, BASE_DAY);
    for (i, km) in [13.69, 29.27, 6.89].into_iter().enumerate() {
        let _ = app.update(
            log_run(km, 60.0, 70.0, 0.0, at - i as i64 * 3600, 100 + i as u64),
            &mut fresh,
        );
    }
    assert_eq!(
        week_km(&app.view(&fresh)),
        week_km(&app.view(&model)),
        "the same three runs show another weekly total after delete + Undo"
    );
}

#[test]
fn a_huge_run_duration_never_panics_the_youth_card() {
    let at = BASE_DAY * DAY_SEC + 8 * 3600;
    let app = Engine;
    let mut model = Model::default();
    for e in [
        set_today_ev(BASE_DAY),
        Event::SetProfile(Profile {
            health: HealthScreen {
                youth: true,
                ..HealthScreen::default()
            },
            ..base_profile()
        }),
        // Started today at 08:00 (the day the card buckets it), so its
        // 1e10 minutes reach the weekly total.
        log_run(5.0, 1e10, 70.0, 0.0, at + 600_000_000_000, 1),
        Event::LogActivity {
            kind: ActivityKind::Sport,
            minutes: 30.0,
            vigorous: false,
            strengthening: false,
            observed_at: at,
            entry_id: 2,
        },
    ] {
        let _ = app.update(e, &mut model);
    }
    let vm = catch_unwind(AssertUnwindSafe(|| app.view(&model))).expect("view() must not panic");
    let card = vm.youth_activity.expect("the under-18 card shows");
    assert!(
        card.minutes >= card.run_minutes,
        "weekly minutes {} below the run minutes {}",
        card.minutes,
        card.run_minutes
    );
}

#[test]
fn a_subnormal_run_distance_never_shows_infinite_percent() {
    let at = BASE_DAY * DAY_SEC + 8 * 3600;
    let app = Engine;
    let mut model = Model::default();
    for e in [
        set_today_ev(BASE_DAY),
        Event::SetProfile(sample_profile()),
        log_run(5e-324, 30.0, 70.0, 0.0, at - 8 * DAY_SEC, 1),
        log_run(5e-324, 30.0, 70.0, 0.0, at - DAY_SEC, 2),
        log_run(10.0, 60.0, 70.0, 0.0, at, 3),
    ] {
        let _ = app.update(e, &mut model);
    }
    let json = vm_json(&app.view(&model));
    assert_eq!(
        leaked_non_finite(&json),
        None,
        "a non-finite percentage reached display text"
    );
}

/// Every view cache is `thread_local`, and the shell's JNI calls arrive on
/// whichever JVM thread holds the `Core` monitor, so one model is viewed by
/// threads whose memos hold states the others moved past. Each event of a
/// case is applied and viewed on a random one of four long-lived workers
/// (warm with every earlier case's caches, so a memo key that misses a field
/// meets an old entry with the same key); a fresh thread then views the same
/// model with cold caches, and the two must match byte for byte.
#[test]
fn views_from_many_warm_threads_equal_a_cold_thread_every_step() {
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;
    type Job = Box<dyn FnOnce() + Send>;
    let workers: Vec<mpsc::Sender<Job>> = (0..4)
        .map(|w| {
            let (tx, rx) = mpsc::channel::<Job>();
            std::thread::Builder::new()
                .name(format!("jni-{w}"))
                .stack_size(32 << 20)
                .spawn(move || {
                    for job in rx {
                        job();
                    }
                })
                .expect("spawn worker");
            tx
        })
        .collect();
    // `None` views on a fresh thread, with every cache cold.
    let on =
        |w: Option<usize>, job: Box<dyn FnOnce() -> String + Send>| -> Result<String, String> {
            let (tx, rx) = mpsc::channel();
            let run: Job = Box::new(move || {
                let _ = tx.send(catch_unwind(AssertUnwindSafe(job)).map_err(|p| panic_text(&*p)));
            });
            match w {
                Some(w) => workers[w].send(run).map_err(|e| e.to_string())?,
                None => drop(
                    std::thread::Builder::new()
                        .stack_size(32 << 20)
                        .spawn(run)
                        .map_err(|e| e.to_string())?,
                ),
            }
            rx.recv_timeout(Duration::from_secs(120))
                .map_err(|_| format!("thread {w:?} gave no view in 120 s"))?
                .map_err(|p| format!("PANIC on thread {w:?}: {p}"))
        };
    for mode in [Mode::Shell, Mode::Boundary, Mode::Absurd] {
        run_cases(
            "views_from_many_warm_threads_equal_a_cold_thread_every_step",
            0x5448_5245_4144_0008 ^ mode as u64,
            4,
            |seed| {
                let mut g = Gen::new(seed, mode);
                let len = g.rng.int(8, 45) as usize;
                let events = g.sequence(len);
                let model = Arc::new(Mutex::new(Model::default()));
                for (i, e) in events.iter().enumerate() {
                    let w = g.rng.below(4) as usize;
                    let (m, ev) = (model.clone(), e.clone());
                    let warm = on(
                        Some(w),
                        Box::new(move || {
                            let mut m = m.lock().unwrap_or_else(|p| p.into_inner());
                            let _ = Engine.update(ev, &mut m);
                            vm_json(&Engine.view(&m))
                        }),
                    )
                    .map_err(|m| format!("event #{i} {}: {m}", short(e)))?;
                    let m = model.clone();
                    let cold = on(
                        None,
                        Box::new(move || {
                            vm_json(&Engine.view(&m.lock().unwrap_or_else(|p| p.into_inner())))
                        }),
                    )
                    .map_err(|m| format!("cold view after event #{i}: {m}"))?;
                    if warm != cold {
                        return Err(format!(
                            "after event #{i} {} on worker {w}: warm view != cold view: {}",
                            short(e),
                            json_diff(&warm, &cold)
                        ));
                    }
                }
                Ok(())
            },
        );
    }
}
