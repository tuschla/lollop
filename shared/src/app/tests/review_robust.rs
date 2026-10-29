//! Differential guards for the `SetIndex` memo, the per-thread view caches,
//! the storage-order tie rules and the ratio-base thresholds. Knobs:
//! `REVIEW_SEQS`, `REVIEW_STEPS`, `REVIEW_SEED`.

use super::*;

fn env_or(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Names that collide on movement key, case, whitespace, equipment words,
/// retired catalog labels, isolation keywords and calisthenics rungs.
const POOL: &[&str] = &[
    "Back squat",
    "back squat",
    " Back squat ",
    "Barbell back squat",
    "Smith machine back squat",
    "Front squat",
    "Deadlift",
    "Romanian deadlift",
    "Bench press",
    "Barbell bench press",
    "Dumbbell bench press",
    "Overhead press",
    "Row",
    "Cable row",
    "Row (barbell/cable/machine)",
    "Pull-up",
    "Pull up",
    "Push-up",
    "Decline push-up",
    "Feet-elevated / decline push-up",
    "Biceps curl",
    "Lateral raise",
    "Leg extension",
    "Back extension",
    "Hack squat / leg press",
    "Hack squat",
    "Chest fly",
    "Cable fly / pec deck",
    "Plank",
    "Dead hang",
    "Dip",
    "Straight-bar dip",
    "Kniebeuge",
];

#[derive(Clone)]
struct SetRow {
    id: u64,
    at: i64,
    exercise: String,
    weight_kg: f64,
    reps: u32,
    rpe: f64,
    hold_sec: Option<u16>,
}

impl SetRow {
    fn log(&self) -> Event {
        Event::LogSet {
            exercise: self.exercise.clone(),
            weight_kg: self.weight_kg,
            reps: self.reps,
            rpe: self.rpe,
            observed_at: self.at,
            entry_id: self.id,
            hold_sec: self.hold_sec,
        }
    }
    fn amend(&self, old_at: i64) -> Event {
        Event::AmendSet {
            entry_id: self.id,
            exercise: self.exercise.clone(),
            weight_kg: self.weight_kg,
            reps: self.reps,
            rpe: self.rpe,
            observed_at: self.at,
            observed_at_fallback: old_at,
            hold_sec: self.hold_sec,
        }
    }
}

#[derive(Clone)]
struct RunRow {
    id: u64,
    at: i64,
    km: f64,
    min: f64,
    hr: f64,
}

impl RunRow {
    fn log(&self) -> Event {
        log_run(self.km, self.min, self.hr, 0.0, self.at, self.id)
    }
}

const D0: i64 = 20_300;

/// A shell-shaped random life: sets (logged / amended / deleted / undone,
/// renamed to aliases), runs, clock moves, profile flips (unit, modality,
/// goal), plan (re)generation, ended exercises and moved sessions.
struct Gen {
    rng: XorShift,
    today: i64,
    offset: i64,
    next_id: u64,
    sets: Vec<SetRow>,
    runs: Vec<RunRow>,
    deleted_sets: Vec<SetRow>,
    deleted_runs: Vec<RunRow>,
    profile: Profile,
}

impl Gen {
    fn new(seed: u64) -> Self {
        Self {
            rng: XorShift(seed | 1),
            today: D0,
            offset: 0,
            next_id: 1_700_000_000_000,
            sets: Vec::new(),
            runs: Vec::new(),
            deleted_sets: Vec::new(),
            deleted_runs: Vec::new(),
            profile: Profile {
                running_days_per_week: 2,
                running_km_per_week: 15.0,
                ..base_profile()
            },
        }
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1 + self.rng.below(5_000);
        self.next_id
    }

    /// A stamp in the last ~120 days, sometimes the exact second of another set
    /// (a same-stamp tie).
    fn stamp(&mut self) -> i64 {
        if !self.sets.is_empty() && self.rng.chance(0.15) {
            return self.rng.pick(&self.sets).at;
        }
        let back = if self.rng.chance(0.6) {
            self.rng.below(8) as i64
        } else {
            self.rng.below(120) as i64
        };
        (self.today - back) * DAY_SEC - self.offset + 6 * 3600 + self.rng.below(14 * 3600) as i64
    }

    fn set_fields(&mut self, row: &mut SetRow) {
        row.exercise = self.rng.pick(POOL).to_string();
        let hold = matches!(row.exercise.as_str(), "Plank" | "Dead hang");
        row.hold_sec = hold.then(|| 10 + self.rng.below(80) as u16);
        // Float-dust loads too: lb plates stored in kg, 0.25 kg microplates.
        row.weight_kg = match self.rng.below(8) {
            0 => 0.0,
            1 | 2 => (45 + 5 * self.rng.below(60)) as f64 * 0.453_592_37,
            3 => 20.0 + 0.25 * self.rng.below(400) as f64,
            _ => 20.0 + 2.5 * self.rng.below(60) as f64,
        };
        row.reps = if hold {
            1
        } else {
            1 + self.rng.below(14) as u32
        };
        row.rpe = self.rng.pick(&[0.0, 6.0, 7.0, 7.5, 8.0, 9.0, 10.0]);
    }

    fn init(&mut self) -> Vec<Event> {
        vec![
            Event::SetToday {
                epoch_day: self.today,
                utc_offset_sec: self.offset,
            },
            Event::SetProfile(self.profile.clone()),
            Event::GeneratePlan {
                start_epoch_day: self.today - 14,
            },
        ]
    }

    fn step(&mut self) -> Event {
        let r = self.rng.below(100);
        match r {
            0..=37 => {
                let mut row = SetRow {
                    id: self.id(),
                    at: self.stamp(),
                    exercise: String::new(),
                    weight_kg: 0.0,
                    reps: 0,
                    rpe: 0.0,
                    hold_sec: None,
                };
                self.set_fields(&mut row);
                // A same-stamp twin of a logged set (same name, load, reps),
                // rated differently: a full tie for every "top set" rule.
                if !self.sets.is_empty() && self.rng.chance(0.1) {
                    let twin = self.rng.pick(&self.sets);
                    row.at = twin.at;
                    row.exercise = twin.exercise;
                    row.weight_kg = twin.weight_kg;
                    row.reps = twin.reps;
                    row.hold_sec = twin.hold_sec;
                }
                self.sets.push(row.clone());
                row.log()
            }
            38..=52 if !self.sets.is_empty() => {
                let i = self.rng.below(self.sets.len() as u64) as usize;
                let old_at = self.sets[i].at;
                let mut row = self.sets[i].clone();
                match self.rng.below(5) {
                    0 => row.exercise = self.rng.pick(POOL).to_string(),
                    1 => row.weight_kg += 2.5,
                    2 => row.reps = (row.reps + 1).min(30),
                    3 => row.at = self.stamp(),
                    _ => {} // an identical amend: moves the row to the tail
                }
                if row.hold_sec.is_some() && !matches!(row.exercise.as_str(), "Plank" | "Dead hang")
                {
                    row.hold_sec = None;
                    row.reps = row.reps.max(1);
                }
                self.sets.remove(i);
                self.sets.push(row.clone());
                row.amend(old_at)
            }
            53..=60 if !self.sets.is_empty() => {
                let i = self.rng.below(self.sets.len() as u64) as usize;
                let row = self.sets.remove(i);
                let ev = Event::DeleteEntry {
                    kind: EntryKind::Set,
                    entry_id: row.id,
                    observed_at_fallback: row.at,
                };
                self.deleted_sets.push(row);
                ev
            }
            61..=67 if !self.deleted_sets.is_empty() => {
                let row = self.deleted_sets.pop().unwrap();
                self.sets.push(row.clone());
                row.log()
            }
            68..=74 => {
                self.today += self.rng.below(4) as i64;
                if self.rng.chance(0.2) {
                    self.offset = self.rng.pick(&[0, 3600, 7200, -18_000]);
                }
                Event::SetToday {
                    epoch_day: self.today,
                    utc_offset_sec: self.offset,
                }
            }
            75..=79 => {
                match self.rng.below(4) {
                    0 => self.profile.weight_lb = !self.profile.weight_lb,
                    1 => {
                        self.profile.resistance_modality = match self.profile.resistance_modality {
                            ResistanceModality::Weights => ResistanceModality::Calisthenics,
                            ResistanceModality::Calisthenics => ResistanceModality::Weights,
                        }
                    }
                    2 => {
                        self.profile.lift_goal = self.rng.pick(&[
                            LiftGoal::MaxStrength,
                            LiftGoal::Hypertrophy,
                            LiftGoal::Power,
                        ])
                    }
                    _ => self.profile.advanced = !self.profile.advanced,
                }
                self.profile.effective_from_epoch_day = Some(self.today);
                Event::SetProfile(self.profile.clone())
            }
            80..=81 => Event::GeneratePlan {
                start_epoch_day: self.today - self.rng.below(20) as i64,
            },
            82..=90 => {
                let row = RunRow {
                    id: self.id(),
                    at: self.stamp(),
                    km: 0.1 * (20 + self.rng.below(150)) as f64 + 0.01 * self.rng.below(10) as f64,
                    min: 10.0 + self.rng.below(90) as f64,
                    hr: 60.0 + self.rng.below(35) as f64,
                };
                self.runs.push(row.clone());
                row.log()
            }
            91..=93 if !self.runs.is_empty() => {
                let i = self.rng.below(self.runs.len() as u64) as usize;
                let row = self.runs.remove(i);
                let ev = Event::DeleteEntry {
                    kind: EntryKind::Run,
                    entry_id: row.id,
                    observed_at_fallback: row.at,
                };
                self.deleted_runs.push(row);
                ev
            }
            94..=95 if !self.deleted_runs.is_empty() => {
                let row = self.deleted_runs.pop().unwrap();
                self.runs.push(row.clone());
                row.log()
            }
            96..=97 => Event::SetExerciseEnded {
                exercise: self.rng.pick(POOL).to_string(),
                epoch_day: self.today - self.rng.below(3) as i64,
                ended: self.rng.chance(0.7),
                observed_at: self.today * DAY_SEC,
            },
            _ => {
                let from = self.today - self.rng.below(3) as i64;
                Event::MoveSession {
                    from_epoch_day: from,
                    to_epoch_day: from + 1 + self.rng.below(2) as i64,
                }
            }
        }
    }
}

/// Memo staleness, differential: after every 5th event (and the last) of many
/// random lives, the view rendered on this (warm-cache, shared across lives)
/// thread equals the view a cold thread renders by replaying the same prefix.
/// Every warm view also prints no negative zero ("-0.0", "-0 ", "-0%", …) in
/// any string; the failure lists each distinct offending string shape.
#[test]
fn review_incremental_view_equals_cold_replay_every_step() {
    let seqs = env_or("REVIEW_SEQS", 12);
    let steps = env_or("REVIEW_STEPS", 45);
    let seed = env_or("REVIEW_SEED", 0xC0FFEE);
    let app = Engine;
    let mut negative_zero: std::collections::BTreeMap<String, usize> = Default::default();
    for s in 0..seqs {
        let mut g = Gen::new(seed.wrapping_add(s.wrapping_mul(0x9E37_79B9)));
        let mut events = g.init();
        let mut model = Model::default();
        for e in &events {
            let _ = app.update(e.clone(), &mut model);
        }
        for step in 0..steps {
            let e = g.step();
            events.push(e.clone());
            let _ = app.update(e.clone(), &mut model);
            let warm = vm_json(&app.view(&model));
            for piece in warm.split('"') {
                let neg = ["-0.0", "-0 ", "-0%", "-0,", "-0)", "−0.0", "−0 "]
                    .iter()
                    .any(|p| piece.contains(p));
                if neg {
                    let shape: String = piece
                        .chars()
                        .map(|c| if c.is_ascii_digit() { '9' } else { c })
                        .collect();
                    *negative_zero.entry(shape).or_default() += 1;
                }
            }
            if step % 5 != 4 && step + 1 != steps {
                continue;
            }
            let cold = replay_cold(events.clone()).expect("cold replay");
            assert!(
                warm == cold,
                "seq {s} step {step}: warm view != cold replay after {e:?}\n  first diff: {}",
                json_diff(&warm, &cold)
            );
        }
    }
    assert!(
        negative_zero.is_empty(),
        "negative zero in view strings: {negative_zero:#?}"
    );
}

/// The storage-order claim ("Undo or amend moves a row to the tail and no
/// shown figure changes"), over the WHOLE view: for random lives, deleting an
/// entry and re-logging it exactly as the shell's Undo does, or amending it to
/// identical values, renders the identical view.
#[test]
fn review_undo_and_identical_amend_never_change_the_view() {
    let mut seen: std::collections::BTreeMap<String, (usize, String)> = Default::default();
    let seqs = env_or("REVIEW_SEQS", 12);
    let steps = env_or("REVIEW_STEPS", 45);
    let seed = env_or("REVIEW_SEED", 0xC0FFEE);
    let app = Engine;
    for s in 0..seqs {
        let mut g = Gen::new(seed.wrapping_add(s.wrapping_mul(0x9E37_79B9)) ^ 0x55);
        let mut model = Model::default();
        for e in g.init() {
            let _ = app.update(e, &mut model);
        }
        for _ in 0..steps {
            let e = g.step();
            let _ = app.update(e, &mut model);
        }
        for probe in 0..8 {
            let before = vm_json(&app.view(&model));
            let (label, evs): (String, Vec<Event>) = match probe % 3 {
                0 if !g.sets.is_empty() => {
                    let row = g.rng.pick(&g.sets);
                    g.sets.retain(|r| r.id != row.id);
                    g.sets.push(row.clone());
                    (
                        format!("delete+undo set {} {:?} @{}", row.id, row.exercise, row.at),
                        vec![
                            Event::DeleteEntry {
                                kind: EntryKind::Set,
                                entry_id: row.id,
                                observed_at_fallback: row.at,
                            },
                            row.log(),
                        ],
                    )
                }
                1 if !g.sets.is_empty() => {
                    let row = g.rng.pick(&g.sets);
                    g.sets.retain(|r| r.id != row.id);
                    g.sets.push(row.clone());
                    (
                        format!(
                            "identical amend set {} {:?} @{}",
                            row.id, row.exercise, row.at
                        ),
                        vec![row.amend(row.at)],
                    )
                }
                _ if !g.runs.is_empty() => {
                    let row = g.rng.pick(&g.runs);
                    g.runs.retain(|r| r.id != row.id);
                    g.runs.push(row.clone());
                    (
                        format!("delete+undo run {} {} km @{}", row.id, row.km, row.at),
                        vec![
                            Event::DeleteEntry {
                                kind: EntryKind::Run,
                                entry_id: row.id,
                                observed_at_fallback: row.at,
                            },
                            row.log(),
                        ],
                    )
                }
                _ => continue,
            };
            for e in evs {
                let _ = app.update(e, &mut model);
            }
            let after = vm_json(&app.view(&model));
            if before != after {
                for d in all_diffs(&before, &after) {
                    let key: String = d
                        .split(':')
                        .next()
                        .unwrap_or("")
                        .split('[')
                        .map(|p| p.split_once(']').map_or(p, |(_, rest)| rest))
                        .collect::<Vec<_>>()
                        .join("[]");
                    let e = seen
                        .entry(key)
                        .or_insert((0, format!("seq {s}: {label}: {d}")));
                    e.0 += 1;
                }
            }
        }
    }
    assert!(
        seen.is_empty(),
        "view fields that changed on Undo / identical amend (count, first example):\n{}",
        seen.iter()
            .map(|(k, (n, ex))| format!("  {k}  x{n}  e.g. {ex}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Every JSON path where two view documents differ (leaf values).
fn all_diffs(a: &str, b: &str) -> Vec<String> {
    fn walk(a: &serde_json::Value, b: &serde_json::Value, path: String, out: &mut Vec<String>) {
        use serde_json::Value as V;
        match (a, b) {
            (V::Object(x), V::Object(y)) => {
                for (k, va) in x {
                    match y.get(k) {
                        Some(vb) => walk(va, vb, format!("{path}.{k}"), out),
                        None => out.push(format!("{path}.{k}: missing on right")),
                    }
                }
            }
            (V::Array(x), V::Array(y)) => {
                for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                    walk(va, vb, format!("{path}[{i}]"), out);
                }
                if x.len() != y.len() {
                    out.push(format!("{path}: len {} vs {}", x.len(), y.len()));
                }
            }
            _ => {
                if a != b {
                    out.push(format!("{path}: {a} vs {b}"));
                }
            }
        }
    }
    let va: serde_json::Value = serde_json::from_str(a).unwrap();
    let vb: serde_json::Value = serde_json::from_str(b).unwrap();
    let mut out = Vec::new();
    walk(&va, &vb, String::new(), &mut out);
    out
}

/// Every catalog name (hypertrophy table, retired labels, every calisthenics
/// rung, retired rung labels) and spelling variants of each.
fn name_universe() -> Vec<String> {
    let mut base: Vec<String> = crate::hypertrophy::EXERCISES
        .iter()
        .map(|e| e.name.to_string())
        .collect();
    base.extend(
        crate::hypertrophy::LEGACY_NAMES
            .iter()
            .chain(crate::calisthenics::LEGACY_NAMES)
            .flat_map(|&(a, b)| [a.to_string(), b.to_string()]),
    );
    for l in crate::calisthenics::LADDERS {
        base.extend(l.tiers.iter().map(|t| t.name.to_string()));
    }
    base.extend(POOL.iter().map(|s| s.to_string()));
    base.extend(
        [
            "",
            " ",
            "-",
            "Squat",
            "SQUAT",
            "squats",
            "Barbell",
            "Cable",
            "Smith squat",
            "Kniebeuge",
            "Übung",
            "übung",
            "ÜBUNG",
            "Squat\u{a0}",
            "Ｓｑｕａｔ",
            "Dumbbells row",
            "Kettlebell swing",
            "Machine chest press",
        ]
        .map(String::from),
    );
    let mut all = Vec::new();
    for n in &base {
        all.push(n.clone());
        all.push(n.to_lowercase());
        all.push(n.to_uppercase());
        all.push(format!("  {n}  "));
        all.push(format!("Barbell {n}"));
        all.push(format!("{n} (cable)"));
        all.push(n.replace(' ', "-"));
    }
    all.sort();
    all.dedup();
    all
}

/// `SetIndex` classification equals the per-call functions it replaced, for
/// every catalog name and spelling variant: movement ids partition exactly
/// like `same_movement`, isolation/rung facts match, and
/// `same_movement_sets(q)` lists exactly the sets `same_movement(q, s)` keeps.
#[test]
fn review_set_index_matches_per_call_classifiers() {
    let names = name_universe();
    let sets: Vec<LoggedSet> = names
        .iter()
        .enumerate()
        .map(|(i, n)| LoggedSet {
            exercise: n.clone(),
            weight_kg: 50.0,
            reps: 5,
            rpe: 8.0,
            observed_at: 1_700_000_000 + i as i64,
            entry_id: i as u64 + 1,
            hold_sec: None,
        })
        .collect();
    let idx = SetIndex::new(&sets);
    let mut mismatches = Vec::new();
    for i in 0..sets.len() {
        let f = idx.facts(i);
        let n = &sets[i].exercise;
        if f.isolation != crate::plan::is_isolation_lift(n) {
            mismatches.push(format!("isolation {n:?}"));
        }
        if f.calis_tier != crate::calisthenics::tier_by_name(n) {
            mismatches.push(format!("calis_tier {n:?}"));
        }
        for (j, other) in sets.iter().enumerate() {
            let same_id = f.movement == idx.facts(j).movement;
            if same_id != crate::plan::same_movement(n, &other.exercise) {
                mismatches.push(format!("movement {n:?} vs {:?}", other.exercise));
            }
        }
    }
    for q in &names {
        let want: Vec<usize> = (0..sets.len())
            .filter(|&i| crate::plan::same_movement(q, &sets[i].exercise))
            .collect();
        if idx.same_movement_sets(q) != want.as_slice() {
            mismatches.push(format!("same_movement_sets {q:?}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} names disagree: {:?}",
        mismatches.len(),
        names.len(),
        &mismatches[..mismatches.len().min(20)]
    );
}

/// Two sets of one movement on one day, same load, reps AND stamp (a backdated
/// "Add set" at a picked minute twice), differing in rated effort. The plan
/// item's "Last:" line breaks the full tie in log order (`last_top_set` via
/// `LoggedSet::chrono_cmp`), never by storage position, so the shell's Undo of
/// a delete (which re-logs the row at the tail with the same id and stamp)
/// keeps the shown set.
#[test]
fn review_last_set_full_tie_survives_delete_undo() {
    let (app, mut model) = planned_strength_model_on(MON, false);
    let at = (MON - 1) * DAY_SEC + 18 * 3600;
    let first = with_entry_id(log_set("Back Squat", 100.0, 5, 7.0, at), 11);
    let _ = app.update(first.clone(), &mut model);
    let _ = app.update(
        with_entry_id(log_set("Back Squat", 100.0, 5, 9.0, at), 12),
        &mut model,
    );
    let last_set = |m: &Model| -> Vec<(String, Option<(u64, f64)>)> {
        app.view(m)
            .week_plan
            .iter()
            .flat_map(|s| s.items.iter())
            .filter(|it| it.exercise.to_lowercase().contains("squat"))
            .map(|it| {
                (
                    it.exercise.clone(),
                    it.last_set.as_ref().map(|l| (l.entry_id, l.rpe)),
                )
            })
            .collect()
    };
    let before = last_set(&model);
    assert!(
        before.iter().any(|(_, l)| l.is_some()),
        "fixture: a squat item carries a Last: set, got {before:?}"
    );
    let _ = app.update(
        Event::DeleteEntry {
            kind: EntryKind::Set,
            entry_id: 11,
            observed_at_fallback: at,
        },
        &mut model,
    );
    let _ = app.update(first, &mut model);
    assert_eq!(
        before,
        last_set(&model),
        "delete + Undo of an identical set changed the plan's Last: set"
    );
}

/// The content leg of `LoggedSet::chrono_cmp`: two legacy (`entry_id == 0`)
/// sets of one lift on the same second, same load and reps, differing in
/// spelling and rated effort, render the identical PLAN card whichever was
/// stored first (the item's name, its "Last:" set, its `logged_sets` order).
/// History (`lifts`) keeps a legacy pair in its logged (insertion) order by
/// design, so only the plan surfaces are compared.
#[test]
fn legacy_same_second_sets_render_alike_in_either_storage_order() {
    // Today, the plan's first (squat) session: the sets are its logged sets
    // and the later squat days' "Last:" basis.
    let at = MON * DAY_SEC + 7 * 3600;
    let a = log_set("Back Squat", 100.0, 5, 7.0, at);
    let b = log_set("back squat", 100.0, 5, 9.0, at);
    let render = |first: &Event, second: &Event| {
        let (app, mut model) = planned_strength_model_on(MON, false);
        let _ = app.update(first.clone(), &mut model);
        let _ = app.update(second.clone(), &mut model);
        let vm = app.view(&model);
        serde_json::to_string(&(&vm.week_plan, &vm.next_session)).expect("plan serializes")
    };
    let (ab, ba) = (render(&a, &b), render(&b, &a));
    assert!(
        ab.contains("\"entry_id\":0"),
        "fixture: the tied sets are on the plan card"
    );
    assert_eq!(
        ab,
        ba,
        "storage order changed the plan card: {}",
        json_diff(&ab, &ba)
    );
}

/// A 40 m logged run (an accidental tracker start) is the only run in the
/// 30-day window, then an 8 km run. The run CARD (`spike_has_baseline`, the
/// shell's red SPIKE alarm, "Longer than your recent longest run") and the
/// feedback safety gate share one baseline rule (`is_ratio_base_km`), so a
/// base under `MIN_RATIO_BASE_KM` is "no earlier run" for both and the two
/// spike verdicts on one screen agree. A 3 km base is the control: both warn.
#[test]
fn review_spike_card_and_gate_agree_on_a_sub_50m_baseline() {
    for (base_km, warns) in [(0.04, false), (3.0, true)] {
        let app = Engine;
        let mut model = Model::default();
        let today = D0;
        set_today(&app, &mut model, today);
        let _ = app.update(Event::SetProfile(running_profile()), &mut model);
        let _ = app.update(
            log_run(base_km, 1.0, 60.0, 0.0, (today - 2) * DAY_SEC + 8 * 3600, 1),
            &mut model,
        );
        let _ = app.update(
            log_run(8.0, 48.0, 70.0, 0.0, today * DAY_SEC + 8 * 3600, 2),
            &mut model,
        );
        let vm = app.view(&model);
        let card = vm
            .runs
            .iter()
            .find(|r| r.entry_id == 2)
            .expect("the 8 km card");
        let gate = vm
            .feedback
            .as_ref()
            .is_some_and(|f| f.message.contains("longer than your longest"));
        let card_alarm = card.spike_flag && card.spike_has_baseline;
        assert_eq!(
            (card_alarm, gate),
            (warns, warns),
            "{base_km} km base: card spike_flag {} has_baseline {} note {:?}; feedback: {:?}",
            card.spike_flag,
            card.spike_has_baseline,
            card.spike_note,
            vm.feedback.as_ref().map(|f| &f.message)
        );
    }
}

/// The weekly report's hard-run sums (`hard_km` in `push_volume_cap_row`) are
/// an empty `f64` sum when the week has no threshold/interval run, and an
/// empty float sum is -0.0 (rustc 1.96), so the "within weekly limits" row
/// could print "threshold -0.0, intervals -0.0". Every report sum goes through
/// `weekly_report::sum0` (`+ 0.0`).
#[test]
fn review_weekly_report_never_prints_negative_zero() {
    let app = Engine;
    let mut model = Model::default();
    let today = D0;
    set_today(&app, &mut model, today);
    let _ = app.update(Event::SetProfile(running_profile()), &mut model);
    // Five easy 5 km runs this week (D0 is a Thursday): the long run is 25 %
    // of the week, so the row is "within weekly limits" and prints all three.
    for (k, back) in [0, 1, 2, 3, 3].into_iter().enumerate() {
        let at = (today - back) * DAY_SEC + (8 + 2 * k as i64) * 3600;
        let _ = app.update(log_run(5.0, 32.0, 65.0, 0.0, at, 10 + k as u64), &mut model);
    }
    let view = app.view(&model);
    let report: Vec<&str> = view
        .weekly_report
        .iter()
        .map(|r| r.summary.as_str())
        .collect();
    assert!(
        report.iter().any(|s| s.contains("within weekly limits")),
        "fixture: the limits row renders, got {report:?}"
    );
    let vm = vm_json(&view);
    let hits: Vec<&str> = vm
        .split('"')
        .filter(|s| s.contains("-0.0") || s.contains("-0 ") || s.contains("-0,"))
        .collect();
    assert!(hits.is_empty(), "negative zero in view copy: {hits:?}");
}

/// Two weighted "Full pull-up" sets with the same stamp (+10 kg and +12.5 kg).
/// `last_added_kg` is the day's LAST set, so `build_calisthenics_anchors`
/// orders a day's sets in log order (`LoggedSet::chrono_cmp`), never storage
/// order: an identical amend (the row moves to the tail) must not change the
/// PRESCRIBED added load.
#[test]
fn review_calis_added_load_survives_identical_amend() {
    let (app, mut model) = planned_calisthenics();
    let _ = app.update(
        Event::SetProfile(Profile {
            bodyweight_kg: Some(80.0),
            ..calisthenics_profile()
        }),
        &mut model,
    );
    let at = (MON - 1) * DAY_SEC + 18 * 3600;
    let first = SetRow {
        id: 21,
        at,
        exercise: "Full pull-up".into(),
        weight_kg: 12.5,
        reps: 5,
        rpe: 8.0,
        hold_sec: None,
    };
    let _ = app.update(first.log(), &mut model);
    let _ = app.update(
        with_entry_id(log_set("Full pull-up", 10.0, 5, 8.0, at), 22),
        &mut model,
    );
    let added = |m: &Model| cal_item(&app.view(m), Lad::VerticalPull).and_then(|it| it.added_kg);
    let before = added(&model);
    assert!(before.is_some(), "fixture: a loaded pull-up item");
    let _ = app.update(first.amend(at), &mut model);
    assert_eq!(
        before,
        added(&model),
        "an identical amend changed the prescribed added load"
    );
}

/// One hard run this week (88 % HRmax, no easy run): `push_easy_share_row`
/// sums zero easy minutes over an empty iterator (-0.0), and prints
/// "Easy (Z1) share -0% of run time".
#[test]
fn review_easy_share_never_prints_negative_zero() {
    let app = Engine;
    let mut model = Model::default();
    set_today(&app, &mut model, D0);
    let _ = app.update(Event::SetProfile(running_profile()), &mut model);
    let _ = app.update(
        log_run(8.0, 40.0, 88.0, 0.0, D0 * DAY_SEC + 8 * 3600, 1),
        &mut model,
    );
    let view = app.view(&model);
    let easy: Vec<&str> = view
        .weekly_report
        .iter()
        .map(|r| r.summary.as_str())
        .filter(|s| s.starts_with("Easy (Z1) share"))
        .collect();
    assert!(!easy.is_empty(), "fixture: the easy-share row renders");
    assert!(
        easy.iter().all(|s| !s.contains("-0%")),
        "negative zero: {easy:?}"
    );
}

/// Saturating-math probe: a corrupt log's `SetToday` / plan start / stamps at
/// the i64 extremes, with a plan, runs and sets, never panics in `view()` (a
/// panic bricks every later view through the FFI firewall).
#[test]
fn review_extreme_days_never_panic_the_view() {
    let mut panics = Vec::new();
    let extremes = [
        i64::MIN,
        i64::MIN + 1,
        i64::MIN / DAY_SEC,
        i64::MAX / DAY_SEC,
        i64::MAX - 1,
        i64::MAX,
    ];
    for &today in &extremes {
        for &start in &extremes {
            for &at in &[1_700_000_000, i64::MIN, i64::MAX, -1] {
                let r = std::panic::catch_unwind(|| {
                    let app = Engine;
                    let mut model = Model::default();
                    for e in [
                        Event::SetProfile(Profile {
                            goal_race_epoch_day: Some(today),
                            meet_epoch_day: Some(start),
                            ..sample_profile()
                        }),
                        Event::GeneratePlan {
                            start_epoch_day: start,
                        },
                        Event::SetToday {
                            epoch_day: today,
                            utc_offset_sec: 3600,
                        },
                        log_run(8.0, 45.0, 80.0, 0.0, at, 1),
                        log_run(5.0, 30.0, 65.0, 0.0, 1_700_000_000, 2),
                        with_entry_id(log_set("Back squat", 100.0, 5, 8.0, at), 3),
                        with_entry_id(log_set("Back squat", 90.0, 5, 8.0, 1_700_000_000), 4),
                    ] {
                        let _ = app.update(e, &mut model);
                    }
                    let _ = app.view(&model);
                });
                if let Err(p) = r {
                    let msg = panic_text(&*p);
                    panics.push(format!("today {today} start {start} at {at}: {msg}"));
                }
            }
        }
    }
    assert!(
        panics.is_empty(),
        "{} panics, e.g. {:#?}",
        panics.len(),
        &panics[..panics.len().min(8)]
    );
}

// ── Standing guards of the fixes a fuzz seed alone used to catch ────────────

/// Log `entries` (kind, entry id, stamp, its log event) after `setup`, then
/// delete each entry and log it again (the shell's Undo, which re-pushes the
/// row at the tail): every Undo must render the view byte for byte as
/// before. Returns each Undo's first differing path.
fn undo_each_entry_diffs(
    setup: Vec<Event>,
    entries: Vec<(EntryKind, u64, i64, Event)>,
) -> Vec<String> {
    let app = Engine;
    let mut model = Model::default();
    for e in setup
        .into_iter()
        .chain(entries.iter().map(|(.., e)| e.clone()))
    {
        let _ = app.update(e, &mut model);
    }
    let mut diffs = Vec::new();
    for (kind, entry_id, at, e) in entries {
        let before = vm_json(&app.view(&model));
        let _ = app.update(
            Event::DeleteEntry {
                kind,
                entry_id,
                observed_at_fallback: at,
            },
            &mut model,
        );
        let _ = app.update(e, &mut model);
        let after = vm_json(&app.view(&model));
        if before != after {
            diffs.push(format!(
                "Undo of {kind:?} {entry_id}: {}",
                json_diff(&before, &after)
            ));
        }
    }
    diffs
}

/// The weekly report's easy share sums run minutes chronologically
/// (`RunFacts::chrono`), never in storage order: these minutes put the share
/// at exactly 87.5 %, where the float sum order decides the printed digit
/// (storage order after an Undo of the first run sums to 87.49999999999999,
/// "87%", vs "88%"). Replaces the `FUZZ_SEED=0x33fb24393cdfcb91` replay.
#[test]
fn undo_of_a_run_keeps_the_weekly_easy_share() {
    let setup = vec![set_today_ev(D0), Event::SetProfile(running_profile())];
    // Mon–Thu of D0's week (D0 is a Thursday): three easy runs, one hard.
    let runs: Vec<(EntryKind, u64, i64, Event)> =
        [(20.1, 65.0), (21.4, 65.0), (37.6, 65.0), (11.3, 88.0)]
            .into_iter()
            .enumerate()
            .map(|(k, (min, hr))| {
                let at = (D0 - 3 + k as i64) * DAY_SEC + 8 * 3600;
                let id = 30 + k as u64;
                (
                    EntryKind::Run,
                    id,
                    at,
                    log_run(min / 6.0, min, hr, 0.0, at, id),
                )
            })
            .collect();
    let app = Engine;
    let mut model = Model::default();
    for e in setup
        .iter()
        .cloned()
        .chain(runs.iter().map(|(.., e)| e.clone()))
    {
        let _ = app.update(e, &mut model);
    }
    let vm = app.view(&model);
    assert!(
        vm.weekly_report
            .iter()
            .any(|r| r.summary.starts_with("Easy (Z1) share 88%")),
        "fixture: the 87.5 % easy share prints, got {:?}",
        vm.weekly_report
            .iter()
            .map(|r| &r.summary)
            .collect::<Vec<_>>()
    );
    let diffs = undo_each_entry_diffs(setup, runs);
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

/// Mastery's longest-run tie keeps the EARLIER run (`observed_at`, then
/// `entry_id`), never the later-stored one: two 12 km runs today after a
/// 10 km run seven weeks ago. The earlier 12 km has no 30-day baseline (no
/// praise); the later one would (the earlier is its baseline), so a storage-
/// order tie flips the feedback card on an Undo. Replaces the
/// `FUZZ_SEED=0xbcf5ea6d20e1a264` replay.
#[test]
fn undo_of_a_tied_longest_run_keeps_the_feedback() {
    let setup = vec![set_today_ev(D0), Event::SetProfile(running_profile())];
    let at = |d: i64, h: i64| d * DAY_SEC + h * 3600;
    let runs = vec![
        (
            EntryKind::Run,
            41,
            at(D0 - 50, 8),
            log_run(10.0, 60.0, 70.0, 0.0, at(D0 - 50, 8), 41),
        ),
        (
            EntryKind::Run,
            42,
            at(D0, 7),
            log_run(12.0, 72.0, 70.0, 0.0, at(D0, 7), 42),
        ),
        (
            EntryKind::Run,
            43,
            at(D0, 18),
            log_run(12.0, 72.0, 70.0, 0.0, at(D0, 18), 43),
        ),
    ];
    let diffs = undo_each_entry_diffs(setup, runs);
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

/// The lift trend takes no ratio over a prior 4-week best under
/// `MIN_RATIO_BASE_KG` (a corrupt-log subnormal or near-zero weight), which
/// would print "up inf%" or "up 856000%". Replaces the
/// `FUZZ_SEED=0x9c8af499c8f7b82f` replay.
#[test]
fn a_near_zero_prior_best_never_reads_as_a_trend() {
    // 80 kg is the control: a real prior best does read as a trend here.
    for base_kg in [80.0, 5e-324, 0.01, 0.04] {
        let app = Engine;
        let mut model = Model::default();
        for e in [
            set_today_ev(D0),
            Event::SetProfile(strength_profile()),
            with_entry_id(
                log_set(
                    "Back squat",
                    base_kg,
                    5,
                    8.0,
                    (D0 - 35) * DAY_SEC + 18 * 3600,
                ),
                1,
            ),
            with_entry_id(
                log_set("Back squat", 100.0, 5, 8.0, (D0 - 1) * DAY_SEC + 18 * 3600),
                2,
            ),
        ] {
            let _ = app.update(e, &mut model);
        }
        let vm = app.view(&model);
        let trend = vm.trend.as_ref().map(|t| t.summary.as_str()).unwrap_or("");
        assert_eq!(
            trend.contains("on the 4 weeks before"),
            base_kg == 80.0,
            "prior best {base_kg:e} kg, trend {trend:?}"
        );
    }
}
