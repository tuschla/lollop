//! Red-team of HARD RULE 3 (safety overrides everything, including goals):
//! adversarial scenarios that try to get a training prescription through a
//! safety hold. Conflicting signals (pain + "feeling great", fever on race
//! day, pregnancy + a meet, under-18 + powerlifting, RED-S + a deficit),
//! stale / backdated / future-dated signals, report → delete → Undo, a hold
//! across midnight and a time-zone change, plan regenerate / move during a
//! hold, calculators during a hold, hybrid plans under a head cold, profile
//! edits that try to dodge a gate, the resting-HR × HRV same-day priority and
//! the `SafetyTier` ladder order (File 06 §5).
//!
//! Every view these scenarios render is also walked as JSON: each evidence
//! tag on the wire must resolve to a registered, non-myth claim whose lead
//! citation the knowledge base names (HARD RULE 1/2).
//!
//! KB: `extracted/06-autoregulation.md` (autoreg-041/043/045/046, §5 ladder),
//! `extracted/08-individualization-safety.md` (safety-000/011/022/038/040/
//! 043/044/045/046/048/049, onboard-050), `extracted/13-readiness-update.md`
//! (ready-upd-002/003/004/005/008).

use super::*;
use crate::compact_event_log;
use crate::evidence::all_claims;

/// A Monday (`mon0_weekday(TODAY) == 0`).
const TODAY: i64 = 20_752;
/// UTC+2.
const OFFSET: i64 = 7_200;

/// Unix seconds of local `hour` on local day `day` at [`OFFSET`].
fn local(day: i64, hour: i64) -> i64 {
    day * DAY_SEC + hour * 3_600 - OFFSET
}

/// The app plus the full event log fed to it, so a scenario can replay,
/// compact or "undo" (replay without the last event) like the shell.
struct Sim {
    app: Engine,
    model: Model,
    log: Vec<Event>,
}

impl Sim {
    fn new() -> Self {
        Sim {
            app: Engine,
            model: Model::default(),
            log: Vec::new(),
        }
    }

    fn ev(&mut self, e: Event) {
        self.log.push(e.clone());
        let _ = self.app.update(e, &mut self.model);
    }

    fn vm(&self) -> ViewModel {
        let vm = self.app.view(&self.model);
        assert_claims_registered(&vm);
        vm
    }

    fn today(&mut self, day: i64, offset: i64) {
        self.ev(Event::SetToday {
            epoch_day: day,
            utc_offset_sec: offset,
        });
    }

    fn report(&mut self, signal: ReadinessSignal, value: f64, at: i64) {
        self.ev(Event::SubmitReadiness(ReadinessInput {
            observed_at: at,
            ..input(signal, value)
        }));
    }

    fn pain(&mut self, kind: crate::schema::PainKind, location: Option<&str>, at: i64) {
        let mut row = pain_row(1.0, Some(kind), at);
        if let Some(d) = row.pain.as_mut() {
            d.location = location.map(str::to_string);
        }
        self.ev(Event::SubmitReadiness(row));
    }

    /// Replay `events` on a fresh model.
    fn replayed(events: &[Event]) -> Sim {
        let mut s = Sim::new();
        for e in events {
            s.ev(e.clone());
        }
        s
    }
}

/// A set-up user on [`TODAY`]: clock, a pre-plan squat baseline (the day
/// before), the profile effective today and a generated plan.
fn boot(profile: Profile) -> Sim {
    let mut s = Sim::new();
    s.today(TODAY, OFFSET);
    s.ev(log_set("Back Squat", 120.0, 3, 8.0, local(TODAY - 1, 18)));
    s.ev(log_set("Bench Press", 80.0, 5, 8.0, local(TODAY - 1, 18)));
    s.ev(log_set("Deadlift", 150.0, 3, 8.0, local(TODAY - 1, 18)));
    s.ev(Event::SetProfile(Profile {
        effective_from_epoch_day: Some(TODAY),
        ..profile
    }));
    s.ev(Event::GeneratePlan {
        start_epoch_day: TODAY,
    });
    s
}

/// Lifting four days a week plus a marathon build: every modality is planned.
fn hybrid_profile() -> Profile {
    Profile {
        lift_days_per_week: Some(3),
        ..sample_profile()
    }
}

/// Plan calculators requested (APRE next-load, hypertrophy set ramp, protein
/// with a deficit), so a hold has something to withhold.
fn request_calculators(s: &mut Sim) {
    s.ev(Event::ComputeApre {
        scheme: crate::autoreg::ApreScheme::Apre6,
        reps: 12,
        current_load_kg: 100.0,
    });
    s.ev(Event::PlanHypertrophyMeso {
        muscle: "chest".into(),
        weeks: 4,
        not_growing: true,
        recovering_easily: true,
    });
    s.ev(Event::ComputeProtein {
        bodyweight_kg: 70.0,
        masters: false,
        deficit: true,
    });
}

// ── Invariants ─────────────────────────────────────────────────────────────

/// HARD RULE 3 on a rendered day: training is blocked for `reason`, the
/// headline is the safety hold and says why with safety-critical evidence,
/// and neither the hero nor today's week-strip row carries any prescription.
#[track_caller]
fn assert_held(vm: &ViewModel, reason: HoldReason, ctx: &str) {
    assert!(vm.train_blocked, "{ctx}: training must be blocked");
    assert_eq!(vm.hold_reason, Some(reason), "{ctx}: hold reason");
    assert_eq!(
        vm.today_headline.kind,
        HeadlineKind::SafetyHold,
        "{ctx}: headline {:?}",
        vm.today_headline.summary
    );
    assert!(
        !vm.today_headline.summary.trim().is_empty(),
        "{ctx}: the hold must say why"
    );
    assert!(
        vm.today_headline.evidence.safety_critical,
        "{ctx}: the hold's evidence is safety-critical: {:?}",
        vm.today_headline
    );
    assert!(vm.safety_tier.is_some(), "{ctx}: a hold has a safety tier");
    if let Some(ns) = &vm.next_session {
        assert!(
            ns.items.is_empty() && ns.status == SessionStatus::Blocked,
            "{ctx}: the hero prescribes through a hold: {:?}",
            ns.items.iter().map(|i| &i.summary).collect::<Vec<_>>()
        );
    }
    if let Some(today) = vm.today_epoch_day {
        for row in vm.week_plan.iter().filter(|r| r.epoch_day == today) {
            assert!(
                row.items.iter().all(|i| i.done || i.sets_done > 0),
                "{ctx}: today's strip row prescribes through a hold: {:?}",
                row.items.iter().map(|i| &i.summary).collect::<Vec<_>>()
            );
        }
    }
    assert_no_load_increase(vm, ctx);
}

/// Every upcoming (after today) week-strip day carries no prescription.
#[track_caller]
fn assert_upcoming_days_blank(vm: &ViewModel, ctx: &str) {
    let today = vm.today_epoch_day.expect("clock");
    for row in vm.week_plan.iter().filter(|r| r.epoch_day > today) {
        assert!(
            row.items.is_empty(),
            "{ctx}: day {} still prescribes {:?}",
            row.epoch_day,
            row.items.iter().map(|i| &i.summary).collect::<Vec<_>>()
        );
    }
}

/// autoreg-044 / safety-049: no "add weight" call survives a hold or a
/// recovery signal.
#[track_caller]
fn assert_no_load_increase(vm: &ViewModel, ctx: &str) {
    for a in vm.adjustments.iter().chain(&vm.review_adjustments) {
        assert!(
            !a.summary.contains("add about"),
            "{ctx}: a load increase stands next to a safety signal: {}",
            a.summary
        );
    }
}

/// The plan calculators show the hold row, never numbers.
#[track_caller]
fn assert_calculators_held(vm: &ViewModel, ctx: &str) {
    assert!(
        vm.apre.iter().all(|r| r.summary.contains("on hold")) && !vm.apre.is_empty(),
        "{ctx}: APRE prescribes a next load through a hold: {:?}",
        vm.apre.iter().map(|r| &r.summary).collect::<Vec<_>>()
    );
    assert!(
        vm.hypertrophy_plan
            .iter()
            .all(|r| r.summary.contains("on hold"))
            && !vm.hypertrophy_plan.is_empty(),
        "{ctx}: the hypertrophy ramp prescribes sets through a hold: {:?}",
        vm.hypertrophy_plan
            .iter()
            .map(|r| &r.summary)
            .collect::<Vec<_>>()
    );
    assert!(
        vm.hypertrophy_ramp.is_empty(),
        "{ctx}: a set ramp is shown through a hold: {:?}",
        vm.hypertrophy_ramp
    );
}

/// safety-022: a deficit is never computed while a RED-S signal is present.
#[track_caller]
fn assert_deficit_refused(vm: &ViewModel, ctx: &str) {
    assert!(
        vm.protein_figures.iter().any(|f| f.refused),
        "{ctx}: the deficit protein target was not refused: {:?}",
        vm.protein_figures
    );
    assert!(
        !vm.protein_targets
            .iter()
            .any(|r| r.summary.starts_with("Deficit (lean-mass preserving)")),
        "{ctx}: a deficit target is shown: {:?}",
        vm.protein_targets
            .iter()
            .map(|r| &r.summary)
            .collect::<Vec<_>>()
    );
}

/// The concatenated knowledge base, if the checkout carries it.
static KB_TEXT: std::sync::LazyLock<Option<String>> = std::sync::LazyLock::new(|| {
    fn walk(dir: &std::path::Path, out: &mut String) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "md") {
                out.push_str(&std::fs::read_to_string(&p).unwrap_or_default());
                out.push('\n');
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../planning/knowledge-base");
    let mut s = String::new();
    walk(&root, &mut s);
    (!s.is_empty()).then_some(s)
});

/// Lead surname of a citation: its first alphabetic word of 3+ letters.
fn lead_author(citation: &str) -> Option<&str> {
    citation
        .split(|c: char| !c.is_alphabetic() && c != '-' && c != '\'')
        .find(|w| w.chars().count() >= 3)
}

/// HARD RULE 1/2 on the wire: walk the view JSON and resolve every evidence
/// tag (an object carrying a non-empty `grade`) to a registry row with the
/// same grade, citation, confidence, safety flag and contested flag. None may
/// be a MarketingMyth, and the row's lead citation must be named in the KB.
#[track_caller]
fn assert_claims_registered(vm: &ViewModel) {
    let json = serde_json::to_value(vm).expect("view serializes");
    let mut tags = Vec::new();
    fn walk<'a>(
        v: &'a serde_json::Value,
        path: String,
        out: &mut Vec<(String, &'a serde_json::Map<String, serde_json::Value>)>,
    ) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("grade")
                    .and_then(|g| g.as_str())
                    .is_some_and(|g| !g.is_empty())
                    && m.contains_key("citation")
                {
                    out.push((path.clone(), m));
                }
                for (k, x) in m {
                    walk(x, format!("{path}.{k}"), out);
                }
            }
            serde_json::Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    walk(x, format!("{path}[{i}]"), out);
                }
            }
            _ => {}
        }
    }
    walk(&json, String::new(), &mut tags);
    assert!(
        vm.profile.is_none() || !tags.is_empty(),
        "the walk found no evidence tags: the JSON shape changed under this check"
    );
    for (path, m) in tags {
        let grade = m["grade"].as_str().unwrap();
        let citation = m["citation"].as_str().unwrap_or_default();
        let safety = m.get("safety_critical").and_then(|b| b.as_bool());
        let contested = m.get("contested").and_then(|b| b.as_bool());
        let conf = m.get("confidence").and_then(|c| c.as_f64());
        let row = all_claims().find(|e| {
            serde_json::to_value(e.grade)
                .ok()
                .as_ref()
                .and_then(|g| g.as_str())
                == Some(grade)
                && e.reference() == citation
                && safety.is_none_or(|s| s == e.safety_critical)
                && contested.is_none_or(|c| c == e.contested.is_some())
                && conf.is_none_or(|c| (c - f64::from(e.confidence_score())).abs() < 1e-4)
        });
        let Some(row) = row else {
            panic!("{path}: evidence tag {grade:?} / {citation:?} resolves to no registered claim")
        };
        assert!(
            !row.is_blocked(),
            "{path}: MarketingMyth {} surfaced",
            row.claim_id
        );
        if let Some(kb) = KB_TEXT.as_deref()
            && citation != crate::evidence::NO_PUBLISHED_SOURCE
        {
            let author = lead_author(citation).unwrap_or(citation);
            assert!(
                kb.contains(author) || kb.contains(row.claim_id),
                "{path}: claim {} cites {citation:?}, which the knowledge base never names",
                row.claim_id
            );
        }
    }
}

// ── The ladder ─────────────────────────────────────────────────────────────

#[test]
fn safety_tier_order_is_the_hard_rule_3_ladder() {
    use SafetyTier::*;
    let ladder = [
        MedicalReferral,
        Pain,
        Illness,
        ObjectivePerformance,
        SubjectiveMultiDay,
        HrvTrend,
        SingleDayMarker,
    ];
    for w in ladder.windows(2) {
        assert!(w[0] > w[1], "{:?} must outrank {:?}", w[0], w[1]);
    }
}

#[test]
fn every_rung_pair_resolves_to_the_higher_rung() {
    // Two signals from different rungs on one morning: the tier is the higher
    // rung and a blocking rung blocks.
    use crate::schema::PainKind;
    let pain = || pain_row(1.0, Some(PainKind::SharpJoint), local(TODAY, 7));
    let fever = || ReadinessInput {
        observed_at: local(TODAY, 7),
        ..input(ReadinessSignal::Illness, 2.0)
    };
    let cardiac = || ReadinessInput {
        observed_at: local(TODAY, 7),
        ..input(ReadinessSignal::CardiacRedFlag, 1.0)
    };
    let hrv = || ReadinessInput {
        observed_at: local(TODAY, 7),
        streak: 4,
        ..input(ReadinessSignal::HrvLnRmssd, -1.5)
    };
    let rhr = || ReadinessInput {
        observed_at: local(TODAY, 7),
        ..input(ReadinessSignal::RestingHr, 12.0)
    };
    let cases: Vec<(&str, Vec<ReadinessInput>, SafetyTier, Option<HoldReason>)> = vec![
        (
            "cardiac+pain",
            vec![pain(), cardiac()],
            SafetyTier::MedicalReferral,
            Some(HoldReason::MedicalSignal),
        ),
        (
            "pain+fever",
            vec![fever(), pain()],
            SafetyTier::Pain,
            Some(HoldReason::Pain),
        ),
        (
            "fever+hrv",
            vec![hrv(), fever()],
            SafetyTier::Illness,
            Some(HoldReason::Illness),
        ),
        ("hrv+rhr", vec![rhr(), hrv()], SafetyTier::HrvTrend, None),
    ];
    for (name, inputs, tier, reason) in cases {
        let mut s = boot(strength_profile());
        for i in inputs {
            s.ev(Event::SubmitReadiness(i));
        }
        let vm = s.vm();
        assert_eq!(vm.safety_tier, Some(tier), "{name}");
        match reason {
            Some(r) => assert_held(&vm, r, name),
            None => {
                assert!(!vm.train_blocked, "{name}: a non-blocking pair blocks");
                assert_no_load_increase(&vm, name);
            }
        }
    }
}

// ── Conflicting signals ────────────────────────────────────────────────────

#[test]
fn sharp_joint_pain_holds_even_when_everything_else_says_go() {
    // File 06 §5: "Great performance BUT sharp joint pain: STOP regardless".
    // Every other channel is screaming "ready": high wellness, HRV above the
    // band, an easy first set (RPE 2 below target), an e1RM above the
    // 14-day best.
    use crate::schema::PainKind;
    let mut s = boot(strength_profile());
    s.report(ReadinessSignal::WellnessZ, 2.0, local(TODAY, 6));
    s.report(ReadinessSignal::HrvLnRmssd, 1.0, local(TODAY, 6));
    s.report(ReadinessSignal::Rpe, -2.0, local(TODAY, 7));
    s.report(ReadinessSignal::EstimatedOneRm, 1.10, local(TODAY, 7));
    let vm = s.vm();
    assert!(
        !vm.train_blocked,
        "fixture: the green signals alone don't hold training"
    );
    s.pain(PainKind::SharpJoint, None, local(TODAY, 8));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Pain, "pain + feeling great");
    assert_eq!(vm.safety_tier, Some(SafetyTier::Pain));
}

#[test]
fn fever_on_race_day_holds_the_race_and_every_upcoming_day() {
    // autoreg-046: any fever → do NOT train (absolute); a race-day goal never
    // overrides it (safety-000).
    let mut s = boot(Profile {
        goal_distance: GoalDistance::HalfMarathon,
        goal_race_epoch_day: Some(TODAY + 3),
        ..hybrid_profile()
    });
    s.today(TODAY + 3, OFFSET);
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY + 3, 6));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "fever on race day");
    assert_eq!(vm.safety_tier, Some(SafetyTier::Illness));
    assert_upcoming_days_blank(&vm, "fever on race day");
    assert!(vm.illness_check_prompt);
}

#[test]
fn pregnancy_with_a_meet_and_a_doctors_clearance_stays_deferred() {
    // safety-045: never autonomously prescribe in pregnancy; the PAR-Q
    // clearance answer clears safety-044 only. A meet date (a PR goal) never
    // overrides the deferral (safety-000).
    let mut s = boot(Profile {
        meet_epoch_day: Some(TODAY + 21),
        health: HealthScreen {
            pregnant: true,
            medically_cleared: true,
            ..HealthScreen::default()
        },
        ..strength_profile()
    });
    request_calculators(&mut s);
    let vm = s.vm();
    assert_held(&vm, HoldReason::Pregnancy, "pregnant + meet + cleared");
    assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
    assert_eq!(vm.plan_withheld, Some(PlanWithheld::HealthHold));
    assert!(vm.next_session.is_none() && vm.week_plan.is_empty());
    assert_calculators_held(&vm, "pregnant");
}

#[test]
fn an_under_18_powerlifter_gets_no_plan_however_the_age_is_stated() {
    // safety-011 / onboard-050 (owner decision 2026-10-17): no programming
    // under 18. Tried: an explicit youth=false with age 16; a birthday 16
    // years ago with a claimed age of 30 (the birthday is authoritative); a
    // NaN-poisoned age (sanitized conservatively).
    let birthday_16 = TODAY - 16 * 365 - 30;
    let cases = [
        ("age 16, youth=false", Some(16.0), None),
        (
            "birthday says 16, age says 30",
            Some(30.0),
            Some(birthday_16),
        ),
        ("age 17.99", Some(17.99), None),
        ("age NaN", Some(f64::NAN), None),
    ];
    for (name, age, birth) in cases {
        let mut s = boot(Profile {
            lift_goal: LiftGoal::MaxStrength,
            meet_epoch_day: Some(TODAY + 28),
            age_years: age,
            birth_epoch_day: birth,
            health: HealthScreen {
                youth: false,
                ..HealthScreen::default()
            },
            ..strength_profile()
        });
        request_calculators(&mut s);
        let vm = s.vm();
        assert_held(&vm, HoldReason::Youth, name);
        assert_eq!(vm.plan_withheld, Some(PlanWithheld::HealthHold), "{name}");
        assert!(vm.next_session.is_none(), "{name}");
        assert_calculators_held(&vm, name);
    }
}

#[test]
fn a_reds_answer_refuses_the_deficit_and_holds_the_hypertrophy_ramp() {
    // safety-022/049: a RED-S signal on any channel → no deficit, no volume
    // increase, defer. Channels: the onboarding answer, a readiness report,
    // the recurring weekly screen (ready-upd-005).
    type Channel = (&'static str, fn(&mut Sim));
    let channels: [Channel; 3] = [
        ("profile", |s| {
            let mut p = s.model.profile.clone().unwrap();
            p.health.reds_signal = true;
            s.ev(Event::SetProfile(p));
        }),
        ("readiness", |s| {
            s.report(ReadinessSignal::RedS, 1.0, local(TODAY, 7))
        }),
        ("weekly screen", |s| {
            s.ev(Event::SubmitWeeklyCheckin(SessionReview {
                reds_signal_reported: true,
                observed_at: local(TODAY, 7),
                ..Default::default()
            }))
        }),
    ];
    for (name, raise) in channels {
        let mut s = boot(Profile {
            lift_goal: LiftGoal::Hypertrophy,
            ..strength_profile()
        });
        request_calculators(&mut s);
        raise(&mut s);
        let vm = s.vm();
        assert_held(&vm, HoldReason::Reds, name);
        assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral), "{name}");
        assert_deficit_refused(&vm, name);
        assert_calculators_held(&vm, name);
    }
}

#[test]
fn compulsive_exercise_refuses_the_deficit_like_every_reds_channel() {
    // safety-049 lists compulsive exercise among the RED-S / disordered-eating
    // signals; the weekly check-in routes it to the RED-S referral (the hold
    // reads `Reds`). safety-022: deficit + any such signal → no deficit.
    let mut s = boot(Profile {
        lift_goal: LiftGoal::Hypertrophy,
        ..strength_profile()
    });
    request_calculators(&mut s);
    s.ev(Event::SubmitWeeklyCheckin(SessionReview {
        compulsive_flag: true,
        observed_at: local(TODAY, 7),
        ..Default::default()
    }));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Reds, "compulsive exercise");
    assert_deficit_refused(&vm, "compulsive exercise");
    assert_calculators_held(&vm, "compulsive exercise");
}

// ── Stale, backdated and future-dated signals ──────────────────────────────

#[test]
fn red_flags_never_go_stale() {
    // Fever, a cardiac red flag, bone-stress signs and RED-S have no expiry:
    // a report from five weeks ago that was never resolved still holds today
    // (autoreg-046 "rest until resolved"; safety-040/043/049 DEFER).
    let cases = [
        (ReadinessSignal::Illness, 2.0, HoldReason::Illness),
        (
            ReadinessSignal::CardiacRedFlag,
            1.0,
            HoldReason::MedicalSignal,
        ),
        (ReadinessSignal::BoneStress, 1.0, HoldReason::MedicalSignal),
        (ReadinessSignal::RedS, 1.0, HoldReason::Reds),
    ];
    for (signal, value, reason) in cases {
        let mut s = boot(hybrid_profile());
        s.report(signal, value, local(TODAY, 7));
        s.today(TODAY + 35, OFFSET);
        let vm = s.vm();
        assert_held(&vm, reason, &format!("{signal:?} 35 days later"));
        assert_upcoming_days_blank(&vm, &format!("{signal:?} 35 days later"));
    }
}

#[test]
fn a_backdated_fever_report_holds_today() {
    // The user logs last night's fever this morning, dated last night.
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY - 1, 22));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "backdated fever");
    assert_upcoming_days_blank(&vm, "backdated fever");
}

#[test]
fn a_future_dated_report_takes_hold_on_its_day() {
    // A report dated tomorrow (a wrong device clock) cannot be lost: it holds
    // from its own day on.
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY + 1, 9));
    s.today(TODAY + 1, OFFSET);
    assert_held(&s.vm(), HoldReason::Illness, "future-dated fever, its day");
    s.today(TODAY + 2, OFFSET);
    assert_held(
        &s.vm(),
        HoldReason::Illness,
        "future-dated fever, day after",
    );
}

#[test]
fn a_future_dated_all_clear_cannot_lift_a_fever_today() {
    // Illness 0 dated next week must not read as "recovered" today: on
    // today's picture only reports dated by today count.
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY, 7));
    s.report(ReadinessSignal::Illness, 0.0, local(TODAY + 7, 7));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "fever + future all-clear");
}

#[test]
fn a_future_dated_reds_all_clear_keeps_the_deficit_refused_today() {
    // RED-S reported today; a RedS 0 dated next week (wrong clock / typo).
    // Today's readiness picture (date-filtered) still defers for RED-S, so
    // the RED-S-dependent refusals must hold too (safety-022/049).
    let mut s = boot(Profile {
        lift_goal: LiftGoal::Hypertrophy,
        ..strength_profile()
    });
    request_calculators(&mut s);
    s.report(ReadinessSignal::RedS, 1.0, local(TODAY, 7));
    s.report(ReadinessSignal::RedS, 0.0, local(TODAY + 7, 7));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Reds, "RED-S + future all-clear");
    assert_deficit_refused(&vm, "RED-S + future all-clear");
    assert_calculators_held(&vm, "RED-S + future all-clear");
}

#[test]
fn a_future_dated_reds_retraction_keeps_the_deficit_refused_today() {
    // A dated RemoveReadiness takes effect from its own day; dated tomorrow,
    // the RED-S deferral is still in force today, and so must the deficit
    // refusal be.
    let mut s = boot(Profile {
        lift_goal: LiftGoal::Hypertrophy,
        ..strength_profile()
    });
    request_calculators(&mut s);
    s.report(ReadinessSignal::RedS, 1.0, local(TODAY, 7));
    s.ev(Event::RemoveReadiness {
        signal: ReadinessSignal::RedS,
        observed_at: local(TODAY + 1, 7),
    });
    let vm = s.vm();
    assert_held(&vm, HoldReason::Reds, "RED-S retracted tomorrow");
    assert_deficit_refused(&vm, "RED-S retracted tomorrow");
    assert_calculators_held(&vm, "RED-S retracted tomorrow");
}

// ── Report → delete → Undo, compaction ─────────────────────────────────────

#[test]
fn report_delete_undo_restores_the_exact_hold_and_compaction_keeps_it() {
    // The shell's "Report removed · Undo": RemoveReadiness (undated) then the
    // restore event it builds from the readiness row (SafetyBanner.kt
    // RemoveReportDialog). The fever must come back as a fever (no-train),
    // not as a head cold, and a compacted log must replay the same hold.
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY, 7));
    let held = s.vm();
    assert_held(&held, HoldReason::Illness, "fever");
    let row_value = held
        .readiness_summary
        .iter()
        .find(|r| r.signal == ReadinessSignal::Illness)
        .expect("an illness row")
        .value;
    s.ev(Event::RemoveReadiness {
        signal: ReadinessSignal::Illness,
        observed_at: 0,
    });
    assert!(!s.vm().train_blocked, "removed: the hold lifts");
    s.report(ReadinessSignal::Illness, row_value, local(TODAY, 8));
    let undone = s.vm();
    assert_held(&undone, HoldReason::Illness, "undo");
    assert_eq!(undone.safety_tier, held.safety_tier);
    assert_upcoming_days_blank(&undone, "undo");

    let compacted = Sim::replayed(&compact_event_log(s.log.clone()));
    let cvm = compacted.vm();
    assert_held(&cvm, HoldReason::Illness, "compacted");
    assert_eq!(
        cvm.week_plan, undone.week_plan,
        "compaction moved the held week"
    );
}

#[test]
fn removing_a_mistaken_head_cold_never_drops_an_earlier_fever() {
    // Two illness reports today: a fever, then a mistaken "just a cold".
    // Deleting the mistaken one must leave the fever in force.
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY, 7));
    s.report(ReadinessSignal::Illness, 1.0, local(TODAY, 8));
    s.ev(Event::RemoveReadiness {
        signal: ReadinessSignal::Illness,
        observed_at: 0,
    });
    assert_held(&s.vm(), HoldReason::Illness, "fever after removing a cold");
}

#[test]
fn undoing_the_pain_resolution_restores_the_hold() {
    // "I'm pain-free now" then the shell-level undo (replay without it).
    use crate::schema::PainKind;
    let mut s = boot(strength_profile());
    s.pain(PainKind::SharpJoint, None, local(TODAY, 7));
    s.ev(Event::ResolveHold {
        signal: ReadinessSignal::Pain,
        observed_at: local(TODAY, 8),
    });
    assert!(!s.vm().train_blocked);
    let mut log = s.log.clone();
    log.pop();
    let undone = Sim::replayed(&log);
    assert_held(&undone.vm(), HoldReason::Pain, "pain resolve undone");
}

// ── Resolution and return to play ──────────────────────────────────────────

#[test]
fn the_first_session_after_a_fever_is_the_return_check_not_the_plan() {
    // ready-upd-003/004: after "I've recovered" the first session back is
    // the return check (60–70 % for 10–20 min) / an easy session (lifts ≥ 3
    // RIR, runs easy), never the planned hard session.
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY - 2, 7));
    s.ev(Event::ResolveHold {
        signal: ReadinessSignal::Illness,
        observed_at: local(TODAY, 7),
    });
    let vm = s.vm();
    assert!(!vm.train_blocked);
    assert_no_load_increase(&vm, "return");
    let ns = vm.next_session.clone().expect("a session after recovery");
    assert!(!ns.items.is_empty());
    for it in &ns.items {
        if it.is_run() {
            assert!(
                it.run_structure.is_none(),
                "a structured quality run on the first day back: {}",
                it.summary
            );
            if let Some(RunAmount::Minutes(m)) = it.run_volume {
                assert!(m <= 20.0, "return run over 20 min: {}", it.summary);
            }
            if let Some(t) = &it.run_target {
                assert!(
                    t.rpe_high <= 4 || t.band.to_lowercase().contains("easy"),
                    "{t:?}"
                );
            }
        } else {
            match it.intensity {
                PlanIntensity::Rir { rir } => assert!(rir >= 3, "{}: RIR {rir}", it.summary),
                PlanIntensity::RirRange { lo, .. } => assert!(lo >= 3, "{}", it.summary),
                PlanIntensity::PercentE1rm { reduced_from, .. } => assert!(
                    reduced_from.is_some() || it.return_check,
                    "{}: a %1RM lift not eased on the first day back",
                    it.summary
                ),
                PlanIntensity::Prose => {}
            }
        }
    }
}

#[test]
fn a_new_fever_right_after_recovery_holds_again() {
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY, 6));
    s.ev(Event::ResolveHold {
        signal: ReadinessSignal::Illness,
        observed_at: local(TODAY, 7),
    });
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY, 9));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "relapse");
    assert_upcoming_days_blank(&vm, "relapse");
}

// ── Midnight, time zones ───────────────────────────────────────────────────

#[test]
fn a_fever_hold_survives_midnight_and_a_time_zone_change() {
    // Fever at 23:30 local in UTC+2; the next morning the user has flown to
    // UTC-5 (same local date on the new clock is still the next day), then
    // to UTC+14 two days later. No resolution was sent: every day is held.
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY, 23) + 1_800);
    assert_held(&s.vm(), HoldReason::Illness, "23:30");
    s.today(TODAY + 1, -5 * 3_600);
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "next day UTC-5");
    assert_upcoming_days_blank(&vm, "next day UTC-5");
    s.today(TODAY + 3, 14 * 3_600);
    assert_held(&s.vm(), HoldReason::Illness, "UTC+14");
}

#[test]
fn a_west_bound_flight_cannot_move_a_fever_into_the_future() {
    // Fever at 00:30 local on day D at UTC+14 (= day D-1 10:30 UTC). The user
    // flies to UTC-10 where it is still day D-1: the shell's SetToday says
    // D-1. The report, on the new clock, is dated D-1 00:30: it holds.
    let mut s = boot(hybrid_profile());
    let at = (TODAY + 1) * DAY_SEC + 1_800 - 14 * 3_600;
    s.today(TODAY + 1, 14 * 3_600);
    s.report(ReadinessSignal::Illness, 2.0, at);
    assert_held(&s.vm(), HoldReason::Illness, "UTC+14");
    // `now` on the new clock is after the report.
    let now = at + 3_600;
    let day_minus10 = (now - 10 * 3_600).div_euclid(DAY_SEC);
    s.today(day_minus10, -10 * 3_600);
    assert_held(&s.vm(), HoldReason::Illness, "flown to UTC-10");
}

// ── Plan edits during a hold ───────────────────────────────────────────────

#[test]
fn regenerating_moving_or_clearing_the_plan_cannot_escape_a_fever() {
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY, 7));
    let held = s.vm();
    // Move a later planned session onto today and another onto tomorrow.
    let later: Vec<i64> = boot(hybrid_profile())
        .vm()
        .week_plan
        .iter()
        .filter(|r| r.epoch_day > TODAY && !r.items.is_empty())
        .map(|r| r.epoch_day)
        .collect();
    assert!(later.len() >= 2, "fixture: several sessions this week");
    s.ev(Event::MoveSession {
        from_epoch_day: later[0],
        to_epoch_day: TODAY,
    });
    s.ev(Event::MoveSession {
        from_epoch_day: later[1],
        to_epoch_day: TODAY + 1,
    });
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "moved");
    assert_upcoming_days_blank(&vm, "moved");
    s.ev(Event::GeneratePlan {
        start_epoch_day: TODAY,
    });
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "regenerated");
    assert_upcoming_days_blank(&vm, "regenerated");
    s.ev(Event::ClearPlan);
    s.ev(Event::GeneratePlan {
        start_epoch_day: TODAY + 1,
    });
    let vm = s.vm();
    assert_held(
        &vm,
        HoldReason::Illness,
        "cleared + regenerated from tomorrow",
    );
    assert_upcoming_days_blank(&vm, "cleared + regenerated from tomorrow");
    assert_eq!(held.hold_reason, vm.hold_reason);
}

#[test]
fn a_head_cold_caps_every_modality_of_a_hybrid_week_even_after_moves() {
    // ready-upd-008 / autoreg-045: under a head cold every upcoming run is at
    // most one short easy run (no long run, no quality), and lifting volume is
    // cut; a moved session gets the same cap. Hybrid: neither modality may
    // slip through.
    let base = boot(hybrid_profile()).vm();
    let mut s = boot(hybrid_profile());
    s.report(ReadinessSignal::Illness, 1.0, local(TODAY, 7));
    let long = base
        .week_plan
        .iter()
        .find(|r| r.title == "Long run")
        .map(|r| r.epoch_day);
    if let Some(day) = long {
        // Move the long run one day earlier when that day is free.
        if base
            .week_plan
            .iter()
            .any(|r| r.epoch_day == day - 1 && r.items.is_empty() && day - 1 > TODAY)
        {
            s.ev(Event::MoveSession {
                from_epoch_day: day,
                to_epoch_day: day - 1,
            });
        }
    }
    let vm = s.vm();
    assert!(!vm.train_blocked, "a head cold eases, never blocks");
    assert_no_load_increase(&vm, "head cold");
    let mut runs = 0;
    let mut lifts = 0;
    for row in vm.week_plan.iter().filter(|r| r.epoch_day >= TODAY) {
        let before = base.week_plan.iter().find(|b| b.title == row.title);
        for it in &row.items {
            if it.is_run() {
                runs += 1;
                assert!(
                    it.summary.starts_with("10–20 min") && it.run_structure.is_none(),
                    "day {} {}: run not capped under a head cold: {}",
                    row.epoch_day,
                    row.title,
                    it.summary
                );
            } else {
                lifts += 1;
                if let Some(b) =
                    before.and_then(|b| b.items.iter().find(|x| x.exercise == it.exercise))
                {
                    assert!(
                        u32::from(it.sets) * 2 <= u32::from(b.sets) + 1,
                        "day {} {}: {} keeps {} of {} sets under a head cold",
                        row.epoch_day,
                        row.title,
                        it.exercise,
                        it.sets,
                        b.sets
                    );
                }
            }
        }
    }
    assert!(
        runs > 0 && lifts > 0,
        "fixture: a hybrid week ({runs} runs, {lifts} lifts)"
    );
}

#[test]
fn a_scoped_knee_stop_spares_no_leg_or_run_item_today() {
    // safety-038 "STOP that exercise": sharp knee pain stops every lower-limb
    // movement and every run today in a hybrid plan; nothing knee-loading
    // slips through on the hero or today's strip row.
    use crate::schema::PainKind;
    let mut s = boot(hybrid_profile());
    s.pain(PainKind::SharpJoint, Some("left knee"), local(TODAY, 7));
    let vm = s.vm();
    assert_eq!(vm.safety_tier, Some(SafetyTier::Pain));
    assert_no_load_increase(&vm, "knee");
    let today_items = vm
        .next_session
        .iter()
        .filter(|ns| ns.epoch_day == TODAY)
        .flat_map(|ns| ns.items.iter())
        .chain(
            vm.week_plan
                .iter()
                .filter(|r| r.epoch_day == TODAY)
                .flat_map(|r| r.items.iter()),
        );
    for it in today_items {
        let lower = it.is_run()
            || ["squat", "deadlift", "lunge", "leg", "calf", "hip", "jump"]
                .iter()
                .any(|k| it.exercise.to_lowercase().contains(k));
        if lower {
            assert!(
                matches!(it.status, ItemStatus::SkippedPain | ItemStatus::StoppedPain)
                    || it.sets == 0
                    || !it.safety_reason.is_empty(),
                "knee pain: {} still prescribed ({:?})",
                if it.is_run() {
                    &it.summary
                } else {
                    &it.exercise
                },
                it.status
            );
        }
    }
}

// ── Calculators and readiness-channel deferrals ────────────────────────────

#[test]
fn readiness_deferrals_hold_the_plan_calculators_like_profile_gates() {
    // safety-043 "STOP and DEFER for medical clearance before prescribing";
    // safety-040; autoreg-046. The same heart symptom answered on the
    // profile screen holds the calculators and the prescriptive guidance
    // (onboarding.rs `gated_profiles_get_no_prescriptive_guidance_or_calculator_numbers`);
    // reported from the readiness sheet it must too.
    let mut failures = Vec::new();
    for (signal, value) in [
        (ReadinessSignal::CardiacRedFlag, 1.0),
        (ReadinessSignal::BoneStress, 1.0),
        (ReadinessSignal::Illness, 2.0),
    ] {
        let mut s = boot(strength_profile());
        request_calculators(&mut s);
        s.report(signal, value, local(TODAY, 7));
        let vm = s.vm();
        assert!(vm.train_blocked, "{signal:?}");
        if !vm.apre.iter().all(|r| r.summary.contains("on hold")) {
            failures.push(format!("{signal:?}: APRE {:?}", vm.apre[0].summary));
        }
        if !vm.hypertrophy_ramp.is_empty() {
            failures.push(format!("{signal:?}: set ramp {:?}", vm.hypertrophy_ramp));
        }
        if let Some(g) = vm
            .guidance
            .iter()
            .find(|g| !matches!(g.section.as_str(), "Safety" | "Profile" | "Environment"))
        {
            failures.push(format!("{signal:?}: guidance {:?}", g.summary));
        }
    }
    assert!(
        failures.is_empty(),
        "prescribing through a deferral:\n{}",
        failures.join("\n")
    );
}

#[test]
fn profile_symptom_gates_hold_the_plan_calculators() {
    // The passing half of the comparison above: the profile's chest-pain
    // answer (SCREEN-ACSM-001) holds them, even with a doctor's clearance.
    let mut s = boot(Profile {
        health: HealthScreen {
            symptomatic: true,
            medically_cleared: true,
            ..HealthScreen::default()
        },
        ..strength_profile()
    });
    request_calculators(&mut s);
    let vm = s.vm();
    assert_held(&vm, HoldReason::Symptomatic, "symptomatic + cleared");
    assert_calculators_held(&vm, "symptomatic + cleared");
}

// ── Profile edits that try to dodge a gate ─────────────────────────────────

#[test]
fn editing_a_gate_away_never_rewrites_the_days_it_held() {
    // A pregnancy / injury / youth answer turned off today lifts today's
    // hold (the user's own answer), but the days the gate held stay held:
    // they never turn into missed or prescribed sessions.
    let gates: [(&str, HealthScreen, Option<f64>); 3] = [
        (
            "pregnant",
            HealthScreen {
                pregnant: true,
                ..HealthScreen::default()
            },
            None,
        ),
        (
            "injury",
            HealthScreen {
                injury_or_rehab: true,
                ..HealthScreen::default()
            },
            None,
        ),
        ("age 16", HealthScreen::default(), Some(16.0)),
    ];
    for (name, health, age) in gates {
        let mut s = boot(Profile {
            health,
            age_years: age,
            ..hybrid_profile()
        });
        assert!(s.vm().train_blocked, "{name}: fixture gate holds");
        s.today(TODAY + 3, OFFSET);
        s.ev(Event::SetProfile(Profile {
            effective_from_epoch_day: Some(TODAY + 3),
            age_years: age.map(|_| 30.0),
            ..hybrid_profile()
        }));
        let vm = s.vm();
        assert!(!vm.train_blocked, "{name}: the edit lifts today's hold");
        for row in vm.week_plan.iter().filter(|r| r.epoch_day < TODAY + 3) {
            assert!(
                row.items.is_empty() && row.status != SessionStatus::Missed,
                "{name}: held day {} rewritten to {:?} {:?}",
                row.epoch_day,
                row.status,
                row.items.iter().map(|i| &i.summary).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn a_stated_age_edit_cannot_override_the_birthday() {
    // The birthday is authoritative: a profile edit that keeps a 16-year-old
    // birthday but claims age 30 stays held.
    let birthday = TODAY - 16 * 365 - 30;
    let mut s = boot(Profile {
        birth_epoch_day: Some(birthday),
        ..strength_profile()
    });
    assert_held(&s.vm(), HoldReason::Youth, "birthday 16");
    s.ev(Event::SetProfile(Profile {
        birth_epoch_day: Some(birthday),
        age_years: Some(30.0),
        effective_from_epoch_day: Some(TODAY),
        health: HealthScreen {
            youth: false,
            ..HealthScreen::default()
        },
        ..strength_profile()
    }));
    assert_held(&s.vm(), HoldReason::Youth, "birthday 16, age edited to 30");
}

#[test]
fn a_new_parq_yes_after_clearance_gates_again() {
    // safety-044: the clearance covers the follow-up answers it was given
    // for; a Yes added later is not covered.
    let mut s = boot(Profile {
        health: HealthScreen {
            parq_yes: 0b0000_0001,
            medically_cleared: true,
            cleared_parq_mask: 0b0000_0001,
            ..HealthScreen::default()
        },
        ..strength_profile()
    });
    // PAR-Q+ follow-up bits (schema::PARQ_FOLLOWUP_BITS = Q1, Q4, Q5, Q7):
    // Q1 answered Yes and cleared; Q4 added later.
    assert!(!s.vm().train_blocked, "fixture: Q1 cleared");
    let mut p = s.model.profile.clone().unwrap();
    p.health.parq_yes |= 0b0000_1000;
    s.ev(Event::SetProfile(p));
    assert_held(&s.vm(), HoldReason::Parq, "uncovered PAR-Q Yes");
}

// ── Resting HR × HRV ───────────────────────────────────────────────────────

#[test]
fn rhr_spike_and_low_hrv_on_one_morning() {
    // ready-upd-002: one stop-level RHR morning is noise → downgrade + the
    // illness check, not rest; HRV below its band for days is tier 5 and
    // outranks the single-day marker; neither adds load.
    let mut s = boot(hybrid_profile());
    s.ev(Event::SubmitReadiness(ReadinessInput {
        observed_at: local(TODAY, 6),
        ..input(ReadinessSignal::RestingHr, 12.0)
    }));
    let vm = s.vm();
    assert!(
        !vm.train_blocked,
        "one RHR morning is noise (ready-upd-002)"
    );
    assert_eq!(vm.safety_tier, Some(SafetyTier::SingleDayMarker));
    assert!(vm.illness_check_prompt, "the spike asks the neck check");
    assert_no_load_increase(&vm, "rhr spike");

    s.ev(Event::SubmitReadiness(ReadinessInput {
        observed_at: local(TODAY, 6),
        streak: 4,
        ..input(ReadinessSignal::HrvLnRmssd, -1.5)
    }));
    let vm = s.vm();
    assert!(!vm.train_blocked);
    assert_eq!(vm.safety_tier, Some(SafetyTier::HrvTrend));
    assert!(
        vm.illness_check_prompt,
        "HRV must not swallow the neck check"
    );
    assert_no_load_increase(&vm, "rhr + hrv");

    // The neck check answered "head cold" → corroborated: rest (autoreg-041).
    s.report(ReadinessSignal::Illness, 1.0, local(TODAY, 7));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "rhr spike + head cold");
    assert_eq!(vm.safety_tier, Some(SafetyTier::Illness));
}

#[test]
fn rhr_plus_ten_on_two_mornings_is_a_rest_day() {
    let mut s = boot(hybrid_profile());
    s.ev(Event::SubmitReadiness(ReadinessInput {
        observed_at: local(TODAY, 6),
        streak: 2,
        ..input(ReadinessSignal::RestingHr, 10.0)
    }));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "RHR +10 ×2");
    assert_eq!(vm.safety_tier, Some(SafetyTier::Illness));
}

// ── Weekly referral + green readiness ──────────────────────────────────────

#[test]
fn an_overtraining_referral_holds_through_a_great_morning() {
    // safety-042: ≥2 weeks unexplained decline despite a deload → REST and
    // DEFER; a great check-in the same week cannot lift it.
    let mut s = boot(hybrid_profile());
    s.ev(Event::SubmitWeeklyCheckin(SessionReview {
        decline_weeks: Some(3),
        suppressed_wellness_domains: Some(2),
        despite_deload: true,
        observed_at: local(TODAY, 7),
        ..Default::default()
    }));
    s.report(ReadinessSignal::WellnessZ, 2.0, local(TODAY, 8));
    s.report(ReadinessSignal::Rpe, -2.0, local(TODAY, 8));
    let vm = s.vm();
    assert_held(&vm, HoldReason::MedicalSignal, "OTS + great morning");
    assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
    assert_upcoming_days_blank(&vm, "OTS");
    // A week later with no new check-in the referral is still in force.
    s.today(TODAY + 8, OFFSET);
    assert!(s.vm().train_blocked, "a deferral does not age out");
}

// ── More dodges ────────────────────────────────────────────────────────────

#[test]
fn an_unresolved_sharp_pain_still_holds_tomorrow() {
    // ready-upd-004: a pain hold ends when the athlete reports pain-free
    // through range, not by itself overnight.
    use crate::schema::PainKind;
    let mut s = boot(hybrid_profile());
    s.pain(PainKind::SharpJoint, None, local(TODAY, 18));
    s.today(TODAY + 1, OFFSET);
    assert_held(&s.vm(), HoldReason::Pain, "sharp pain, next morning");
    s.today(TODAY + 9, OFFSET);
    assert_held(&s.vm(), HoldReason::Pain, "sharp pain, 9 days on");
}

#[test]
fn chest_or_unmapped_pain_is_never_scoped_to_a_few_exercises() {
    // Chest pain may be cardiac (safety-043): it must stop the whole day,
    // never just the "chest" exercises; so must any location the engine
    // cannot map, a typo, or two regions at once.
    use crate::schema::PainKind;
    for loc in [
        "chest",
        "left chest and arm",
        "heart",
        "sternum",
        "knee and shoulder",
        "kne",
    ] {
        let mut s = boot(hybrid_profile());
        s.pain(PainKind::SharpJoint, Some(loc), local(TODAY, 7));
        assert_held(&s.vm(), HoldReason::Pain, loc);
    }
}

#[test]
fn a_session_review_cannot_overwrite_a_weekly_referral() {
    // The weekly check-in carries the clinical screens; a routine session
    // review submitted after it must not clear the bone-stress referral
    // (safety-040 DEFER, urgent).
    let mut s = boot(hybrid_profile());
    s.ev(Event::SubmitWeeklyCheckin(SessionReview {
        bone_pain_red_flag: true,
        observed_at: local(TODAY, 7),
        ..Default::default()
    }));
    s.ev(Event::SubmitReview(SessionReview {
        observed_at: local(TODAY, 8),
        ..Default::default()
    }));
    let vm = s.vm();
    assert_held(
        &vm,
        HoldReason::MedicalSignal,
        "bone stress + session review",
    );
    assert_upcoming_days_blank(&vm, "bone stress + session review");
}

#[test]
fn a_pregnancy_warning_sign_holds_without_the_pregnant_flag() {
    // safety-046: a warning sign → STOP and DEFER, even when the shell
    // forgot (or the user unticked) "pregnant".
    let mut s = boot(Profile {
        health: HealthScreen {
            pregnancy_warning_sign: true,
            medically_cleared: true,
            ..HealthScreen::default()
        },
        ..hybrid_profile()
    });
    request_calculators(&mut s);
    let vm = s.vm();
    assert_held(&vm, HoldReason::Pregnancy, "warning sign only");
    assert_calculators_held(&vm, "warning sign only");
}

#[test]
fn a_fever_blanks_a_calisthenics_week_too() {
    // The calisthenics modality goes through its own ladder views: a fever
    // must hold it exactly like weights and runs.
    let mut s = boot(calisthenics_profile());
    s.report(ReadinessSignal::Illness, 2.0, local(TODAY, 7));
    let vm = s.vm();
    assert_held(&vm, HoldReason::Illness, "calisthenics + fever");
    assert_upcoming_days_blank(&vm, "calisthenics + fever");
}
