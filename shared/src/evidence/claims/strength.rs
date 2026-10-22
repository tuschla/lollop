//! Strength and power claims: File 02 per-rule rows (`strength.rs`,
//! `plan/lift.rs`), plyometrics, and `13-strength-power-update.md` /
//! `13-logging-warmup.md`. Contested ids follow the module-doc convention: File
//! 02 local CQ-02 maps to global CQ-03, and local questions with no global
//! counterpart get `CQ-F02-<local>`.

use crate::evidence::cite::{
    CITE_BELL_2023, CITE_BOSQUET_2013, CITE_COLEMAN_2024, CITE_MARKOVIC_2007, CITE_SORIANO_2015,
};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Strong, Weak};

claim_group! {
    StrengthClaim => Strength;
    STR_PRILEPIN_001 => claim(
        "STR-PRILEPIN-001",
        "Prilepin's chart sets a ceiling on total reps per session for each weight range (as a % of your one-rep max), with bar speed and form used to check the weight.",
        Moderate,
        &[
            "A.S. Prilepin 1974 (Soviet weightlifting research, >1,000 elite lifters)",
        ],
    )
    .contradicting(&[
        "Derived from Olympic lifts and elite lifters; apply to powerlifts and novices with caution (CQ-F02-03)",
    ])
    .contested("CQ-F02-03"),
    PLYO_001 => claim(
        "PLYO-001",
        "Cap jumps per session by level: 80–100 foot contacts for beginners, 100–120 for intermediates, 120–140 for advanced. Increase either the number of jumps or how hard they are, not both.",
        Moderate,
        &["Potach & Chu 2008 (NSCA/UKSCA)", "Verkhoshansky"],
    )
    .contradicting(&["Low-dose 40-60 contacts also effective"])
    // File 02 strength-032 marks the contact caps safety_critical.
    .safety(),
    PLYO_PREREQ_001 => claim(
        "PLYO-PREREQ-001",
        "Before high-intensity depth jumps, you need to land jumps well and back squat about 1.5 times your bodyweight.",
        ExpertOpinion,
        &["Potach & Chu 2008 (NSCA/UKSCA)"],
    )
    // File 02 strength-033: ExpertOpinion prerequisite, contested (CQ-F02-05),
    // but safety_critical: depth-jump landing loads injure without the base.
    .safety()
    .contested("CQ-F02-05"),
    STR_PCTPROG_001 => claim(
        "STR-PCTPROG-001",
        "When weights follow percentages, add 2.5–5% to lower-body lifts and 1–2.5% to upper-body lifts after each successful week.",
        ExpertOpinion,
        // File 02 strength-014 lists no citation; an empty list shows as
        // `NO_PUBLISHED_SOURCE` (faithful to the KB, no reference invented).
        &[],
    ),
    STR_PWR_001 => claim(
        "STR-PWR-001",
        "Power work: move the weight as fast as you can across a range of weights (0–60% of your max for jumps and throws, 30–70% for loaded power lifts, 70–95% for weightlifting pulls), 1–5 reps, stopping before you slow down, 3–6 sets, never to failure.",
        Moderate,
        &["Cormie et al. 2011", "Suchomel", "Kawamori & Haff 2004"],
    ),
    STR_2FOR2_001 => claim(
        "STR-2FOR2-001",
        "The 2-for-2 rule: add weight, in small capped steps, once you do 2 or more reps over the goal on the last set in 2 sessions in a row.",
        ExpertOpinion,
        &[
            "Baechle & Earle 2016, Essentials of Strength Training and Conditioning (4th ed., NSCA/Human Kinetics)",
        ],
    )
    .contradicting(&[
        "The source rule grades this ExpertOpinion/Moderate (0.40); registered conservatively as ExpertOpinion",
    ])
    .safety(),
    STR_STALL_001 => claim(
        "STR-STALL-001",
        "If your one-rep max (or its estimate) stalls for 2–3 weeks despite good recovery, change how the plan is structured or take a lighter week.",
        ExpertOpinion,
        &[
            "Rhea et al. 2003, Med Sci Sports Exerc 35(3):456-464 (dose-response by training age)",
            "Zatsiorsky & Kraemer 2020, Science and Practice of Strength Training (3rd ed., Human Kinetics)",
        ],
    ),
    STR_MODEL_001 => claim(
        "STR-MODEL-001",
        "Match the plan's structure to your level: beginners progress steadily (linear); intermediates vary the focus from day to day (undulating) or train in focused blocks; advanced lifters use focused blocks or rotating exercise methods (conjugate).",
        Moderate,
        &[
            "Matveyev 1981, Fundamentals of Sport Training & Bompa, Periodization: Theory and Methodology of Training (linear/novice)",
            "Rhea et al. 2003, Med Sci Sports Exerc 35(3):456-464 & Fleck & Kraemer, Designing Resistance Training Programs (DUP)",
            "Issurin 2010, Sports Med 40(3):189-206 (block); Simmons 2007, The Westside Barbell Book of Methods (conjugate)",
        ],
    ),
    STR_LINEAR_001 => claim(
        "STR-LINEAR-001",
        "Steady (linear) progression: move from more volume at lighter weights to less volume at heavier weights over the training blocks; suited to beginners and early intermediates.",
        Moderate,
        &[
            "Matveyev 1981, Fundamentals of Sports Training (Progress Publishers)",
            "Bompa, Periodization",
            "NSCA",
        ],
    ),
    STR_BLOCK_001 => claim(
        "STR-BLOCK-001",
        "Block training: first a volume block (about 65–80% of max, 3–5 sets of 6–10), then a strength block (about 80–90%, 3–6 sets of 3–6), then a peaking block (peak and taper, 1–3 reps), each focusing on one quality.",
        Moderate,
        &[
            "Issurin 2008/2016 (block periodization reviews)",
            "Verkhoshansky",
            "Painter et al. 2012 IJSPP 7(2):161-169",
            "Bartolomei et al. 2014 JSCR 28(4):990-997",
        ],
    )
    .contradicting(&["No meta-analysis confirms block > traditional for max strength"])
    .contested("CQ-03"),
    STR_PAP_001 => claim(
        "STR-PAP-001",
        "Pairing a heavy set with an explosive one: rest 5–7 minutes after the heavy set (about 3–7 works best), and stop pairing if the explosive set gets slower than usual.",
        Moderate,
        &[
            "Seitz & Haff 2016, Sports Med 46(2):231-240",
            "Wilson et al. 2013 JSCR",
            "Docherty et al.",
        ],
    )
    .contradicting(&["Too-short rest hurts performance (fatigue > potentiation)"]),
    STR_OLY_001 => claim(
        "STR-OLY-001",
        "Olympic-lift pulls: 3–5 sets of 1–3 reps at 85–100% or more of your max in the full lift (lighter for speed-focused versions), early in the session.",
        Moderate,
        &[
            "Suchomel, Nimphius & Stone 2016, Sports Med 46(10):1419-1449 (muscular strength in athletic performance)",
        ],
    ),
    STR_DLPEAK_001 => claim(
        "STR-DLPEAK-001",
        "Do your last near-max deadlift 10–14 days before a test or meet; it's very tiring and slow to recover from.",
        Moderate,
        &["Travis et al. 2020/2021"],
    )
    .safety(),
    STR_BACKOFF_001 => claim(
        "STR-BACKOFF-001",
        "One heavy top set with about 2 reps left, then back-off sets at a fixed drop of 10–15% below it.",
        Moderate,
        &[
            "Zourdos et al. 2016, J Strength Cond Res 30(1):267-275 (RIR-based RPE)",
            "Helms et al. 2018, Front Physiol 9:247 (RPE-based load prescription)",
        ],
    ),
    // knowledge-base/hybrid-lift-session-composition.md. Same two-layer
    // discipline as HYP-VOL-DOSE-001: the coverage PRINCIPLE is citable at
    // Moderate (multi-joint compounds do NOT fully activate every muscle -> a
    // session must COVER distinct movement patterns rather than stack one),
    // while the exact exercise COUNT and the movement-pattern TAXONOMY are
    // expert-opinion practitioner frameworks (no RCT prescribes a specific N or
    // pattern list) flagged in `contradicting` per the GRADE good-practice-
    // statement precedent (knowledge-base/autoreg-citation-provenance-resolution.md).
    #[kb_only]
    STR_SESSION_BALANCE_001 => claim(
        "STR-SESSION-BALANCE-001",
        "A lift day should COVER distinct movement patterns (squat / hinge / horizontal push / horizontal pull) rather than stacking one, because multi-joint compounds do not fully activate every muscle; session STRUCTURE (full-body vs split, exact exercise count) is not itself the adaptation driver at equated volume, so balance is a sensible default, not a mandate.",
        Moderate,
        &[
            "Schoenfeld, Grgic, Haun, Itagaki & Helms 2019, Sports 7(7):177 (PMID 31336594; multi-joint EMG under-activates some muscles -> cover the patterns)",
            "Evangelista et al. 2024, J Strength Cond Res 38(7):e363-e373 (split vs full-body equivalent for strength/hypertrophy at equated volume)",
            "12-week concurrent-training RCT in recreational runners, PMC9518107 (a small ~3-exercise lower-body-dominant strength set alongside running)",
        ],
    )
    .contradicting(&[
        "Exercise COUNT (~3-4/day) and the movement-pattern TAXONOMY are expert-opinion practitioner frameworks - no RCT prescribes a specific N or pattern list (GRADE good-practice-statement precedent; Schoenfeld et al. 2019 counts sets 1:1 plus logical rationale and personal expertise)",
        "Balance is a DEFAULT, not a mandate: a running-focused hybrid may legitimately keep a compound/leg emphasis (concurrent RCT PMC9518107 used a small lower-body-dominant strength set for a running-performance goal)",
        "Frequency is negligible at equated volume, so this adds pattern coverage within ~2 lift days, not more lift days (consistent with HYP-VOL-DOSE-001 / HYP-FREQ-001)",
    ]),
    STR_E1RM_CHECK_001 => claim(
        "STR-E1RM-CHECK-001",
        "Above 10 reps or on single-joint lifts, an estimated one-rep max isn't reliable; compare 2 or more formulas and prefer test sets of 3–6 reps.",
        Moderate,
        &["DiStasio 2014"],
    ),
    #[kb_only]
    STR_LOADSEL_001 => claim(
        "STR-LOADSEL-001",
        "Prefer fixed % loading for novices, teaching, or when no monitoring is available; prefer RPE/RIR autoregulation for intermediate/advanced and fatigue-sensitive phases.",
        Moderate,
        &[
            "Helms et al. 2018 Front Physiol",
            "Graham & Cleather 2019/2021 JSCR",
        ],
    )
    .contradicting(&[
        "RPE vs %1RM matched shows both effective, RPE small non-significant advantage",
    ])
    .contested("CQ-F02-01"),
    #[kb_only]
    STR_VZONE_001 => claim(
        "STR-VZONE-001",
        "Set velocity zones from the individual load-velocity relationship; mean concentric velocity maps inversely and near-perfectly to %1RM (R^2 ~0.98 bench; MVT ~0.15 m/s bench, ~0.30 m/s squat).",
        Moderate,
        &["Gonzalez-Badillo & Sanchez-Medina 2010 Int J Sports Med"],
    )
    .contradicting(&["Zone boundaries approximate and individual"]),
    #[kb_only]
    STR_LVP_001 => claim(
        "STR-LVP-001",
        "Estimate 1RM via a 4-7 (recommend 5-7) incremental-load velocity profile extrapolated to MVT; monitoring only (SEE ~9.8% of 1RM); deadlift LVP must not predict 1RM.",
        Moderate,
        &[
            "Greig et al. 2023 (434 participants, 20 studies)",
            "Jovanovic & Flanagan",
            "Lake et al. (deadlift caveat)",
        ],
    ),
    #[kb_only]
    STR_VBT_DAILY_001 => claim(
        "STR-VBT-DAILY-001",
        "Autoregulate daily load by first-rep bar speed: add load if first-rep velocity exceeds the target-zone upper bound, reduce load if below the lower bound (true 1RM varies +/-18% day-to-day).",
        Moderate,
        &["Jovanovic & Flanagan 2014 J Aust Strength Cond 22(2):58-69"],
    ),
    STR_DUP_001 => claim(
        "STR-DUP-001",
        "Vary each day's focus through the week (heavy: 3–5 sets of 3–5 at 85–90%; power: 3–6 sets of 3–5 at 50–70%, moved fast; muscle: 3–4 sets of 8–12 at 70–75%). This is best supported for intermediate and advanced lifters training a lift 2–3 or more times a week.",
        Moderate,
        &["Rhea et al. 2002 JSCR 16(2):250-255", "Fleck & Kraemer"],
    )
    .contradicting(&["Harries/Grgic meta-analyses find no consistent DUP advantage"])
    .contested("CQ-03"),
    #[kb_only]
    STR_CONJ_001 => claim(
        "STR-CONJ-001",
        "Reserve conjugate/Westside for advanced lifters (>=2-3 yr barbell training): rotate Max Effort variations to 1-3RM (90%+), run Dynamic Effort speed work with accommodating resistance, and use Repetition-method accessories for weak points.",
        ExpertOpinion,
        &[
            "Simmons 2007, The Westside Barbell Book of Methods",
            "Prilepin",
        ],
    )
    .contradicting(&["Little direct RCT support (Weak experimental)"])
    .safety()
    .contested("CQ-F02-04"),
    #[kb_only]
    STR_WAVE_001 => claim(
        "STR-WAVE-001",
        "Use wave loading (ascending/descending load waves across sets, e.g. 3-2-1/3-2-1 rising) to exploit acute potentiation.",
        ExpertOpinion,
        &[
            "Zatsiorsky & Kraemer 2020, Science and Practice of Strength Training (3rd ed., Human Kinetics) (potentiation)",
            "Simmons 2007, The Westside Barbell Book of Methods",
        ],
    ),
    #[kb_only]
    STR_PEAK_001 => claim(
        "STR-PEAK-001",
        "For strength peaking, reduce volume 30-70%, maintain or slightly increase intensity >=85% 1RM, taper (step or exponential) over 1-2 wk, then 2-7 days cessation.",
        Moderate,
        &[
            "Pritchard et al. 2015 Strength Cond J 37(2):72-83",
            "Travis et al. 2020 Sports 8(9):125",
            "Travis et al. 2021",
            "Pritchard et al. 2019 IJSPP",
        ],
    ),
    #[kb_only]
    STR_PWRSPEC_001 => claim(
        "STR-PWRSPEC-001",
        "Train power across a load spectrum matched to force-velocity needs, not a single optimal load: jump squat ~0% 1RM, loaded squat power >30-<70% 1RM, power clean ~40-80% (>=70% for clean variations), weightlifting pulls 90-95%.",
        Moderate,
        &[
            "Cormie et al. 2007/2011",
            "Soriano, Jimenez-Reyes, Rhea & Marin 2015 Sports Med 45:1191-1205 (27 studies)",
            "Suchomel",
            "Kawamori & Haff",
        ],
    ),
    #[kb_only]
    STR_PLYO_SCHED_001 => claim(
        "STR-PLYO-SCHED-001",
        "Schedule plyometrics 1-3x/wk with 48-72 h between sessions; rest 2-3 min/set (depth jumps up to ~1:10 work:rest, 5-10 s between reps).",
        Moderate,
        &["Potach & Chu 2008 (NSCA/UKSCA)", "Verkhoshansky"],
    )
    // Same File 02 rule (strength-032) as PLYO-001, which the KB marks
    // safety_critical.
    .safety(),
    #[kb_only]
    STR_COMP_ANCHOR_001 => claim(
        "STR-COMP-ANCHOR-001",
        "Anchor strength-goal training with the competition lift (highest specificity); adaptations are specific to trained movement, velocity, and ROM; over-specialized variations improve the variation, not the comp lift.",
        Strong,
        &[
            "Zatsiorsky & Kraemer 2020, Science and Practice of Strength Training (3rd ed., Human Kinetics) (specificity / SAID)",
            "Simmons 2007, The Westside Barbell Book of Methods (over-specialization caution)",
        ],
    )
    .contradicting(&["Effect magnitudes only Moderate"]),
    #[kb_only]
    STR_VARIATION_001 => claim(
        "STR-VARIATION-001",
        "Select variations to bias weak points (ROM, stance/grip, tempo, bar) with best carryover when stance/grip/ROM match the target movement; use accessories for lagging muscles (single-joint, muscle-focused).",
        Moderate,
        &[
            "Simmons 2007, The Westside Barbell Book of Methods (conjugate variation selection)",
            "Zatsiorsky & Kraemer 2020, Science and Practice of Strength Training (3rd ed., Human Kinetics)",
        ],
    ),
    #[kb_only]
    STR_WEAKPOINT_001 => claim(
        "STR-WEAKPOINT-001",
        "Pick assistance exercises by where a lift stalls. Bench stalling off the chest: paused, Spoto or incline press plus chest and front-shoulder work. Bench lockout: close-grip or floor press plus triceps. Squat out of the hole: pause or front squats plus quads. Squat mid-range: low-bar squats plus hips and hamstrings. Deadlift off the floor: deficit pulls plus quads and upper back. Deadlift lockout: rack or block pulls plus glutes, hamstrings and upper back.",
        ExpertOpinion,
        &[
            "Simmons 2007, The Westside Barbell Book of Methods (weak-point/sticking-point training)",
        ],
    ),
    #[kb_only]
    STR_SUBST_EQUIP_001 => claim(
        "STR-SUBST-EQUIP-001",
        "When substituting equipment, preserve the movement pattern and velocity/load intent, ordering substitutions by specificity and preferring free-weight over machine for transfer to free-weight tests.",
        ExpertOpinion,
        &[
            "Zatsiorsky & Kraemer 2020, Science and Practice of Strength Training (3rd ed., Human Kinetics) (specificity of transfer)",
            "Haff & Triplett 2016, NSCA Essentials of Strength Training and Conditioning (4th ed.)",
        ],
    )
    .contradicting(&[
        "Per-pattern substitution table referenced but not reproduced in the KB extract",
    ]),
    STR_1RMTEST_001 => claim(
        "STR-1RMTEST-001",
        "Test a one-rep max only when your technique is solid and you're recovered and warmed up (beginners with supervision; lifts that load the spine only once you can brace well). Beginners raise weights by no more than 2.5–5% on upper-body lifts and 5–10% on lower-body lifts, and should prefer an estimated max at first.",
        ExpertOpinion,
        &[
            "Baechle & Earle 2016, Essentials of Strength Training and Conditioning (4th ed., NSCA/Human Kinetics)",
        ],
    )
    .contradicting(&[
        "The source rule grades this ExpertOpinion/Moderate (0.40); registered conservatively as ExpertOpinion",
    ])
    .safety(),
    #[kb_only]
    STR_HEAVYFREQ_001 => claim(
        "STR-HEAVYFREQ-001",
        "For maximal strength, lift at 80% of your max or more on each main lift at least twice a week; lighter days add to those heavy days rather than replace them.",
        Moderate,
        &[
            "Currier et al. 2026, Med Sci Sports Exerc 58(4):851-872 (ACSM position stand on resistance training prescription)",
            "Lopez et al. 2021, Med Sci Sports Exerc 53(6):1206-1216 (load meta-analysis)",
        ],
    )
    .contradicting(&[
        "Head-to-head comparisons of periodization models remain non-significant (see PERIOD-001)",
    ]),
    STR_BASE_POWER_001 => claim(
        "STR-BASE-POWER-001",
        "In weaker or newer lifters, heavy strength training improves jumping and sprinting about as much as fast power training and builds far more strength (squat max +31% vs +4.5% over 10 weeks), so the week leans toward heavy work until the squat reaches about 1.5 times bodyweight.",
        Moderate,
        &[
            "Cormie, McGuigan & Newton 2010, Med Sci Sports Exerc 42(8):1582-1598",
            "James et al. 2018, Scand J Med Sci Sports 28(5):1494-1505",
            "Suchomel, Nimphius & Stone 2016, Sports Med 46(10):1419-1449",
            "Suchomel et al. 2018, Sports Med 48(4):765-785",
        ],
    )
    .contradicting(&[
        "The 1.5x bodyweight cut-off is a coaching convention, not a tested threshold",
        "James et al. 2018: stronger athletes responded faster to ballistic training only mid-program, not at the end",
    ]),
    STR_POWER_SELECT_001 => claim(
        "STR-POWER-SELECT-001",
        "Power training should include jumps or throws and, if you have the skill, Olympic-lift variations, each at its own best weight: jump squats at or below 30% of max, squats at 30–70%, power cleans at 70% or more.",
        Moderate,
        &[
            CITE_SORIANO_2015,
            "Cormie, McGuigan & Newton 2011, Sports Med 41(2):125-146 (developing maximal neuromuscular power, part 2)",
            CITE_MARKOVIC_2007,
        ],
    ),
    STR_DELOAD_VOL_001 => claim(
        "STR-DELOAD-VOL-001",
        "For strength goals, a lighter week cuts volume (about half the sets) while keeping weights close to normal (at most about 10% lighter); a full week off in the middle of a block slightly slowed lower-body strength gains in a trial.",
        Weak,
        &[CITE_COLEMAN_2024, CITE_BELL_2023],
    )
    .contradicting(&["The exact set and load cuts come from expert consensus, not trials"]),
    STR_LOADPROG_001 => claim(
        "STR-LOADPROG-001",
        "Add 2–10% weight once you can do 1–2 reps more than the target; adding reps first and then weight also works, though adding weight favors max strength slightly.",
        ExpertOpinion,
        &[
            "American College of Sports Medicine 2009, Med Sci Sports Exerc 41(3):687-708 (progression models position stand)",
            "Plotkin et al. 2022, PeerJ 10:e14142 (load vs repetition progression RCT)",
        ],
    )
    .contradicting(&[
        "Plotkin et al. 2022: the strength advantage of load progression was small (about 2 kg) and not decisive",
    ]),
    DETRAIN_STR_001 => claim(
        "DETRAIN-STR-001",
        "Strength loss after stopping training grows with the length of the break and is larger in people over 65 or otherwise inactive; a break of one to two months costs a modest amount, never everything.",
        Moderate,
        &[CITE_BOSQUET_2013],
    ),
    #[kb_only]
    STR_WARMUP_001 => claim(
        "STR-WARMUP-001",
        "Before heavy compound sets, warm up on the same exercise and finish with a heavier warm-up set at around 80% of the working weight; light-only warm-ups may not improve the work sets. A typical ramp is about 40% for 5-8 reps, 60% for 3-5 and 80% for 1-3 of the working weight, and warm-up sets do not count as work sets.",
        Moderate,
        &[
            "McCrary, Ackermann & Halaki 2015, Br J Sports Med 49(14):935-942 (warm-up systematic review)",
        ],
    )
    .contradicting(&[
        "The exact ramp percentages are a convention built on the heavy-warm-up finding; warm-up sets have not been shown to prevent lifting injuries",
    ]),
}
