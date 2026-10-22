//! The contested-question table (File 09 global index plus file-local rows).

use super::ContestedQuestion;

/// Contested questions (File 09). Engine holds a default lean while open.
/// `question` is a plain phrase read as "experts still disagree on …";
/// `engine_default` says what the app does meanwhile. The KB source of each
/// row is kept in a comment, never in the shown text.
pub static CONTESTED_QUESTIONS: &[ContestedQuestion] = &[
    // File 09 CQ-01: hypertrophy volume ceiling.
    ContestedQuestion {
        id: "CQ-01",
        question: "how many weekly sets keep adding muscle",
        engine_default: "10–20 hard sets per muscle per week",
        other_side: Some(
            "More weekly sets keep adding growth with no clear ceiling (Schoenfeld & Krieger).",
        ),
    },
    // File 09 CQ-02: train to failure?
    ContestedQuestion {
        id: "CQ-02",
        question: "whether sets should go all the way to failure",
        engine_default: "stop with 0–3 reps left",
        // KB Position A ("failure maximizes stimulus") is an older coaching
        // position with no attributable published reference, left None.
        other_side: None,
    },
    // File 09 CQ-03: periodization model superiority.
    ContestedQuestion {
        id: "CQ-03",
        question: "which way of organizing training weeks works best",
        engine_default: "a plan that varies heavy and lighter days through the week; any structured plan works",
        // KB Position A ("DUP/undulating superior") is held by "some S&C
        // coaches" with no attributable reference, left None.
        other_side: None,
    },
    // File 09 CQ-04: HRV-guided training value.
    ContestedQuestion {
        id: "CQ-04",
        question: "how much heart-rate variability (HRV) should steer training",
        engine_default: "HRV only helps decide between an easier and a harder day",
        other_side: Some(
            "HRV-guided training improves fitness gains and leaves fewer people who barely respond (Vesterinen; Kiviniemi; Granero-Gallegos 2020).",
        ),
    },
    // File 09 CQ-05: ACWR validity.
    ContestedQuestion {
        id: "CQ-05",
        question: "whether the ratio of this week's load to your usual load predicts injury",
        engine_default: "that ratio is never used to predict injury",
        other_side: Some(
            "The ratio predicts injury, with the lowest risk at 0.8–1.3 and danger above 1.5 (Gabbett 2016).",
        ),
    },
    // File 09 CQ-06: interference real-world magnitude.
    ContestedQuestion {
        id: "CQ-06",
        question: "how much running holds back lifting gains in real training",
        engine_default: "shield only power and explosive work from it",
        // KB Position A ("large in high-volume real training") names no holder
        // or reference, left None.
        other_side: None,
    },
    // File 09 CQ-07: runner intensity distribution.
    ContestedQuestion {
        id: "CQ-07",
        question: "how runners should split their training between easy and hard",
        engine_default: "mostly easy with some moderate and a little hard work in base training, shifting to mostly easy plus hard near the race",
        other_side: Some(
            "Trained runners do best with almost only easy or very hard running, and little in between (Stoggl & Sperlich 2014; Seiler).",
        ),
    },
    // File 09 CQ-08: grade-adjusted-pace downhill validity.
    ContestedQuestion {
        id: "CQ-08",
        question: "how well hill-adjusted pace works on downhills",
        engine_default: "trust it uphill; soften and flag it downhill",
        other_side: Some(
            "The Minetti hill-adjustment model is valid on all slopes, including downhill.",
        ),
    },
    // File 09 CQ-09: menstrual-cycle periodization.
    ContestedQuestion {
        id: "CQ-09",
        question: "whether to plan training around the menstrual cycle",
        engine_default: "optional changes based on how you feel, not on cycle phase",
        // KB Position A ("phase-based programming helps") is held by "some
        // coaches" with no attributable reference, left None. (The well-cited
        // side, McNulty 2020, is the engine's own cautious lean.)
        other_side: None,
    },
    // File 09 CQ-10: optimal interval length.
    ContestedQuestion {
        id: "CQ-10",
        question: "the best length for intervals",
        engine_default: "intervals chosen to suit your goal and race",
        other_side: Some(
            "Some coaches favor short repeats of 30 seconds on, 30 seconds off (Ronnestad), others long repeats at threshold pace (Canova).",
        ),
    },
    // File 09 CQ-11: marathon philosophy.
    ContestedQuestion {
        id: "CQ-11",
        // Mirrors File 09's global contested-question table; no claim cites
        // it, so no contested chip shows it.
        question: "the best overall approach to marathon training",
        engine_default: "an easy aerobic base first, then a race-specific block",
        other_side: Some(
            "Race-pace work should lead from the start, rather than an aerobic base first (Canova).",
        ),
    },
    // File 09 CQ-12: MAF 180-formula vs measured LT1.
    ContestedQuestion {
        id: "CQ-12",
        question: "whether the 180-minus-age heart-rate formula is as good as a measured easy-effort ceiling",
        engine_default: "a measured ceiling when you have one, else the formula",
        other_side: Some(
            "The simple 180-minus-age formula is preferable to lab-measured thresholds (Maffetone).",
        ),
    },
    // File 09 CQ-13: running power / form-metric value.
    ContestedQuestion {
        id: "CQ-13",
        question: "how useful running power and running-form numbers are",
        engine_default: "shown only, never used to set training",
        // KB Position A ("actionable coaching metric") is a vendor claim with no
        // published reference, left None.
        other_side: None,
    },
    // File 09 CQ-14: consumer readiness-score trust.
    ContestedQuestion {
        id: "CQ-14",
        question: "how far to trust a wearable's daily readiness score",
        engine_default: "three simple bands only: go, caution, rest",
        // KB Position A ("actionable daily guidance") is Whoop/Oura vendor
        // marketing with no published reference, left None.
        other_side: None,
    },
    // File 09 CQ-15: concurrent session order & separation.
    ContestedQuestion {
        id: "CQ-15",
        question: "the best order and spacing for lifting and running on the same day",
        engine_default: "lift first when strength is the goal, with at least 3 hours (ideally 6–24) between sessions",
        // KB Position B ("effect small once separated >=3h") names no holder or
        // reference; the cited side (Eddens 2018) matches the engine lean -
        // left None.
        other_side: None,
    },
    // File 10 (hybrid) local CQ-01, no File 09 global counterpart
    // (knowledge-base/extracted/10-hybrid-concurrent.md; re-graded in
    // 13-hybrid-update.md hyb-upd-006).
    ContestedQuestion {
        id: "CQ-F10-01",
        question: "whether running holds back lifting gains more than cycling does",
        engine_default: "treat running as possibly more interfering than cycling, with low confidence",
        other_side: Some(
            "A 43-study meta-analysis found no difference by endurance mode (Schumann 2022), and another found cycling-based interval training trended worse for lower-body strength (Sabag 2018).",
        ),
    },
    // File-local contested questions with no File 09 global counterpart
    // (namespaced CQ-F<file>-<local>; see module docs).
    // File 03 local CQ-04.
    ContestedQuestion {
        id: "CQ-F03-04",
        question: "whether adding sets during a muscle-building block beats adding reps and weight",
        engine_default: "either works, so adding sets is optional (Enes 2024 found the same muscle growth either way, though strength favored adding sets)",
        // KB opposing side ("set-addition boosts hypertrophy") is an
        // unattributed RP coaching claim; the cited study (Enes 2024) backs the
        // engine's own lean, left None.
        other_side: None,
    },
    // File 03 local CQ-02.
    ContestedQuestion {
        id: "CQ-F03-02",
        question: "whether muscle growth needs a fixed rep range or works across a wide range of reps",
        engine_default: "anywhere from about 5–30+ reps at about 30–85% of your max, taken close to failure, avoiding very light weights under about 30%",
        other_side: Some(
            "Muscle-building sets belong in a narrower range of 6–12 reps at 70–85% of your max (older American College of Sports Medicine guidance).",
        ),
    },
    // File 02 local CQ-01.
    ContestedQuestion {
        id: "CQ-F02-01",
        question: "whether fixed percentages or effort-based loading (reps left) works better; both work, with a small, uncertain edge for effort-based loading",
        engine_default: "fixed percentages for beginners or when effort isn't rated; effort-based loading for intermediate and advanced lifters and tiring phases",
        other_side: Some(
            "Head to head, fixed percentages and effort-based loading come out effectively equal when effort is matched, with only a small, non-significant edge for effort-based loading (Helms 2018; Graham & Cleather 2019/2021).",
        ),
    },
    // File 02 local CQ-03 (strength-011).
    ContestedQuestion {
        id: "CQ-F02-03",
        question: "whether Prilepin's chart, built from elite Olympic weightlifters, carries over to powerlifting lifts and beginners",
        engine_default: "use it only as a ceiling on reps per session, checked against bar speed and technique",
        // KB names no attributable opposing reference for this local question.
        other_side: None,
    },
    // File 02 local CQ-04.
    ContestedQuestion {
        id: "CQ-F02-04",
        question: "how well the conjugate (Westside) method works; it has a strong practice record but little trial evidence",
        engine_default: "an option for advanced lifters only (2–3+ years of barbell training), never the default",
        other_side: Some(
            "The conjugate/Westside method has a strong competitive track record among elite powerlifters (Louie Simmons, Westside Barbell), despite little direct trial evidence.",
        ),
    },
    // File 02 local CQ-05.
    ContestedQuestion {
        id: "CQ-F02-05",
        question: "how strong you need to be before high-intensity depth jumps",
        engine_default: "a back squat of about 1.5x bodyweight plus solid landings first",
        other_side: Some(
            "The 1.5x bodyweight squat rule is coaching convention, not trial-tested; landing skill and gradual exposure may matter more than any fixed squat ratio.",
        ),
    },
    // File 05 feedback-003/004 "CQ-goal-specificity".
    ContestedQuestion {
        id: "CQ-F05-01",
        question: "whether goals you control (pacing, effort, technique) work better in sport than specific result goals",
        engine_default: "next steps are framed as things you control",
        other_side: Some(
            "Specific, moderately difficult goals improve sport performance in a meta-analysis (Kyllo & Landers 1995), though over a third of sport studies found them no better.",
        ),
    },
    // File 11 (calisthenics) local CQ-01 (knowledge-base/extracted/11-calisthenics.md §2-§3).
    ContestedQuestion {
        id: "CQ-F11-01",
        question: "the order of bodyweight progression steps and when to move up",
        engine_default: "move up a step after hitting the top of the rep range on every set, with at least 1 rep left, in 2 sessions in a row, then restart at the bottom of the range",
        other_side: Some(
            "Only two randomized trials have tested a step ladder, and neither compared different orders or move-up rules (Malíř 2023 review; Kotarsky 2018; Wei 2023).",
        ),
    },
    // File 11 local CQ-02.
    ContestedQuestion {
        id: "CQ-F11-02",
        question: "the strength checks before advanced bodyweight skills",
        engine_default: "12 strict pull-ups and 12 dips before muscle-up work, and at most one new straight-arm hold stage about every 4–6 weeks",
        other_side: Some(
            "Checks like these haven't been shown to predict injury (Bahr 2016), and some athletes move up earlier without injury; the numbers are safety margins, not limits of the body.",
        ),
    },
];
