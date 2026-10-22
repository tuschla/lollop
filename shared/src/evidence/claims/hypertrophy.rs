//! Hypertrophy claims: File 03 per-rule rows (`hypertrophy.rs`,
//! `plan/hypertrophy.rs`), the caloric-deficit rule, and
//! `13-hypertrophy-update.md`.

use crate::evidence::cite::{
    CITE_BELL_2023, CITE_COLEMAN_2024, CITE_ISRAETEL_2021, CITE_PELLAND_2026, CITE_ROBINSON_2024,
    CITE_SINGER_2024, CONTRA_COLEMAN_DELOAD,
};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Strong, Weak};

claim_group! {
    HypertrophyClaim => Hypertrophy;
    DEFICIT_001 => claim(
        "DEFICIT-001",
        "When you eat less than you burn, keep your muscle by eating 1.8–2.7 g of protein per kg of bodyweight a day, keeping weights heavy, and cutting sets toward the fewest that still build muscle rather than lightening the weight.",
        Strong,
        &[
            "Helms et al. 2014, Int J Sport Nutr Exerc Metab 24(2):127-138",
            "Longland et al. 2016, Am J Clin Nutr",
        ],
    )
    .contradicting(&["Volume-sparing ordering itself is Moderate"]),
    HYP_REPLOAD_001 => claim(
        "HYP-REPLOAD-001",
        "Reps and weight by exercise: heavy multi-joint lifts 5–10 reps at 75–85% of your one-rep max, lighter multi-joint and machine lifts 8–15 at 65–75%, single-joint lifts 12–25 at 50–70%.",
        Moderate,
        &[
            "Schoenfeld, Grgic, Van Every & Plotkin 2021, Sports 9(2):32 (loading recommendations)",
            "Schoenfeld, Grgic, Ogborn & Krieger 2017, J Strength Cond Res 31(12):3508-3523 (load meta-analysis)",
        ],
    )
    .contradicting(&["Load interchangeable when effort matched (HYP-LOAD-001)"]),
    HYP_FREQ_001 => claim(
        "HYP-FREQ-001",
        "Train each muscle 2–3 times a week. Twice a week or more beats once because it fits in more weekly sets; with the same weekly sets, how often you train barely matters.",
        Strong,
        &[
            "Schoenfeld/Ogborn/Krieger 2016",
            "Schoenfeld/Grgic/Krieger 2019",
            CITE_PELLAND_2026,
        ],
    )
    .contradicting(&["3x specifically Moderate"]),
    HYP_REST_001 => claim(
        "HYP-REST-001",
        "Rest 2–3 minutes on multi-joint lifts and 1–2 on single-joint lifts, long enough to keep at least 90% of your first-set reps on later sets.",
        Moderate,
        &[
            "Schoenfeld, Pope et al. 2016, JSCR 30(7):1805-1812",
            CITE_SINGER_2024,
        ],
    )
    .contradicting(&[
        "Singer et al. 2024: the added benefit of resting beyond ~90 s is uncertain, and 60 s still produced substantial growth",
    ]),
    HYP_RIR_RAMP_001 => claim(
        "HYP-RIR-RAMP-001",
        "Across a 4-week block, finish sets closer to failure each week (4, 3, 2, then 1 rep left), then take a lighter week.",
        ExpertOpinion,
        &["Renaissance Periodization (rpstrength.com 2018)"],
    )
    .contradicting(&["Consistent with, not established by, Robinson 2024"]),
    HYP_MEV_AGE_001 => claim(
        "HYP-MEV-AGE-001",
        "The fewest weekly sets that build muscle go up with experience: about 6–10 per muscle for beginners, 10–18 for intermediates and 12–20+ for advanced lifters, varying by person.",
        Weak,
        &[
            CITE_ISRAETEL_2021,
            "Rhea et al. 2003, Med Sci Sports Exerc 35(3):456-464 (training-age dose-response)",
        ],
    ),
    HYP_MESO_ADD_001 => claim(
        "HYP-MESO-ADD-001",
        "If a muscle isn't growing and recovery feels easy, add 2 weekly sets next block: it's below the fewest sets that make it grow.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    ),
    HYP_MRV_DELOAD_001 => claim(
        "HYP-MRV-DELOAD-001",
        "More than about 20 weekly sets for a muscle, with falling performance or aching joints, means you're at or past what you can recover from. Take a lighter week.",
        Moderate,
        &[CITE_ISRAETEL_2021],
    ),
    HYP_RECOVOL_001 => claim(
        "HYP-RECOVOL-001",
        "When eating less than you burn, sleeping badly or under high stress, cut weekly sets by 20–30% and go to failure less often; with good recovery you can handle more sets.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    ),
    HYP_SETRAMP_001 => claim(
        "HYP-SETRAMP-001",
        "Set progression: start each muscle near its minimum effective volume and add sets week by week toward the most it can recover from (for example 10, 13, 16, then 20 sets), while moving from about 4 reps left down to about 1.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    )
    .contradicting(&[
        "Enes, De Souza & Souza-Junior 2024, MSSE 56(3):553-563 (no significant hypertrophy difference vs constant volume)",
    ])
    .contested("CQ-F03-04"),
    HYP_DELOAD_TRIG_001 => claim(
        "HYP-DELOAD-TRIG-001",
        "Take a lighter week when 2 or more signs show up together (performance dropping, sets drifting to failure without meaning to, lasting joint or tendon aches, poor sleep, a higher resting heart rate, low mood or motivation), or every 4–8 weeks as planned.",
        Moderate,
        &[CITE_ISRAETEL_2021],
    )
    .contradicting(&[CONTRA_COLEMAN_DELOAD]),
    HYP_DELOAD_RX_001 => claim(
        "HYP-DELOAD-RX-001",
        "A lighter week: one week at about maintenance volume (roughly half the sets), with 2–4 or more reps left, at about 60–70% of your working weights, with the same movements.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    )
    .contradicting(&[CONTRA_COLEMAN_DELOAD]),
    HYP_SPLIT_001 => claim(
        "HYP-SPLIT-001",
        "Spread any weekly target above about 12 sets per muscle over 2 or more sessions.",
        ExpertOpinion,
        &[
            "Remmert et al. 2025, SportRxiv preprint 537 (per-session volume meta-regressions; not peer reviewed)",
            "Krieger/Weightology analyses",
        ],
    )
    .contradicting(&[
        "Preprint only: graded expert opinion until a peer-reviewed version is verified",
    ]),
    HYP_SESSCAP_001 => claim(
        "HYP-SESSCAP-001",
        "Keep each session to about 11 sets per muscle (an indirect set counts as half); move extra sets to another session rather than adding them.",
        ExpertOpinion,
        &[
            "Remmert, Pelland, Robinson, Hinson & Zourdos 2025, SportRxiv preprint 537 (per-session volume meta-regressions; not peer reviewed)",
        ],
    )
    .contradicting(&[
        "Preprint only: graded expert opinion until a peer-reviewed version is verified",
    ]),
    // KB rule hypertrophy-046 (planning/design/user-decisions.md, 2026-10-14):
    // the default weekly set ramp ends at each muscle's MAV high end, and one
    // hypertrophy lift day holds at most 32 working sets.
    HYP_VOL_CAP_001 => claim(
        "HYP-VOL-CAP-001",
        "By default each muscle's weekly sets rise only to the top of its productive range, not to its recovery limit, and one lift day holds about 32 working sets at most. Your body can't recover with every muscle at its limit at once, and extra sets add less and less.",
        ExpertOpinion,
        &[
            CITE_ISRAETEL_2021,
            CITE_PELLAND_2026,
            "Schoenfeld, Ogborn & Krieger 2017, J Sports Sci 35(11):1073-1082 (PMID 27433992; weekly-volume dose-response meta-analysis)",
        ],
    )
    .contradicting(&[
        "Coaching ruling, not a trial result: no study tests a whole-session set cap; 32 is four core movement patterns times the per-session top of the 10-18 weekly-set band (8)",
    ]),
    // knowledge-base/weekly-set-volume-to-prescription-mapping.md. The
    // dose-response PRINCIPLE ("more weekly volume -> more growth, diminishing
    // returns") is citable at Moderate; the weekly-target -> per-session/
    // per-exercise ALLOCATION has no validated formula and is an expert-opinion
    // heuristic. This entry follows the GRADE good-practice-statement precedent
    // (see knowledge-base/autoreg-citation-provenance-resolution.md): keep the
    // Moderate principle grade and flag the unstudied allocation parameter as
    // expert-opinion in `contradicting`, rather than overstate the split or drop
    // the rule. Volume-sized lift prescriptions (`plan::lift`) cite this.
    HYP_VOL_DOSE_001 => claim(
        "HYP-VOL-DOSE-001",
        "More weekly sets per muscle build more muscle (and strength), with smaller gains from each extra set. A plan can size each session's sets from a weekly target per muscle, within the coaching set ranges.",
        Moderate,
        &[
            CITE_PELLAND_2026,
            "Schoenfeld, Ogborn & Krieger 2017, J Sports Sci 35(11):1073-1082 (PMID 27433992; weekly-volume dose-response meta-analysis)",
        ],
    )
    .contradicting(&[
        "Allocation is expert-opinion: no validated formula exists for splitting a weekly target into per-exercise/per-day sets (Schoenfeld, Grgic, Haun, Itagaki & Helms 2019, Sports 7(7):177, PMID 31336594: count 1:1 plus logical rationale and personal expertise)",
        "100% posterior slope means the SIGN is near-certain, NOT a magnitude license or unbounded extrapolation; diminishing returns more pronounced for strength",
        "Numeric landmarks (MEV possibly <5, optimal 10+ weekly sets/muscle) are Weak-Moderate; the MEV/MAV/MRV framework itself is expert-opinion, not a validated construct",
    ])
    .review(6),
    HYP_PAIN_SHIFT_001 => claim(
        "HYP-PAIN-SHIFT-001",
        "If a joint hurts under heavy weights, switch that muscle's work to lighter weights for 12–25 reps; muscle growth is preserved.",
        Strong,
        &[
            "Schoenfeld/Grgic/Ogborn/Krieger 2017 (load-interchangeability basis)",
        ],
    )
    .safety(),
    HYP_LOADRANGE_001 => claim(
        "HYP-LOADRANGE-001",
        "Muscle grows equally well across about 30–85% of your one-rep max (about 5–30+ reps) when sets end close to failure; heavier weights build more strength.",
        Strong,
        &[
            "Schoenfeld/Grgic/Ogborn/Krieger 2017 load meta",
            "Morton 2016",
            "Lasevicius 2018",
        ],
    )
    .contradicting(&[
        "Older ACSM 6-12 rep / 70-85% 1RM guidance",
        "Very light loads (<~30% 1RM) underperform",
    ])
    .contested("CQ-F03-02"),
    HYP_VOLRAMP_SAFE_001 => claim(
        "HYP-VOLRAMP-SAFE-001",
        "Raise weekly sets gradually from a low start and never jump straight to your recovery limit; big week-to-week jumps raise injury risk.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    )
    .safety(),
    HYP_SKILL_RIR_001 => claim(
        "HYP-SKILL-RIR-001",
        "On high-skill exercises or ones that demand a lot of balance, do at least 5 reps and stop with 1–2 or more reps left to protect your form.",
        ExpertOpinion,
        &[
            CITE_ISRAETEL_2021,
            "Zourdos et al. 2016, J Strength Cond Res 30(1):267-275 (RIR accuracy)",
        ],
    )
    .safety(),
    HYP_RIR_DEFAULT_001 => claim(
        "HYP-RIR-DEFAULT-001",
        "End most sets with 1–3 reps left. Muscle growth rises steadily as sets get closer to failure, but going all the way isn't needed and isn't worth the extra fatigue.",
        Moderate,
        &[
            "Robinson et al. 2024, Sports Medicine 54(9):2209-31",
            "Grgic 2022",
            "Refalo et al. 2023, Sports Medicine 53(3):649-65",
        ],
    )
    .contradicting(&[
        "Refalo 2023 trivial failure advantage ES=0.19 (CI 0.00,0.37)",
        "Consistent-failure advocates",
    ])
    .contested("CQ-02"),
    HYP_RIR_ACC_001 => claim(
        "HYP-RIR-ACC-001",
        "Guesses of reps left are accurate to within about 1 rep only near failure (0–5 left); further away they miss by more than 2. Beginners should start with 3–4 reps left and check against a set taken to failure.",
        Moderate,
        &["Hackett 2017", "Zourdos 2016/2021", "Refalo/Remmert 2023"],
    ),
    HYP_FAIL_SAFE_001 => claim(
        "HYP-FAIL-SAFE-001",
        "Save going to failure (0 reps left) for machines and single-joint lifts, where it's safe; avoid it on heavy free-weight multi-joint lifts like an unspotted squat or bench, because of injury risk.",
        ExpertOpinion,
        &[
            CITE_ISRAETEL_2021,
            "Zourdos et al. 2016, J Strength Cond Res 30(1):267-275 (autoregulated proximity to failure)",
        ],
    )
    .safety(),
    #[kb_only]
    HYP_CUT_OBJ_001 => claim(
        "HYP-CUT-OBJ-001",
        "On a cut, trust rep count and bar speed over perceived effort because RPE inflates in a deficit.",
        Moderate,
        &[
            CITE_ISRAETEL_2021,
            "Zourdos et al. 2016, J Strength Cond Res 30(1):267-275 (RPE/velocity as objective effort marker)",
        ],
    ),
    #[kb_only]
    HYP_VEL_CHECK_001 => claim(
        "HYP-VEL-CHECK-001",
        "Use last-rep bar-speed slowdown as an objective cross-check on proximity to failure.",
        Moderate,
        &[
            "Gonzalez-Badillo & Sanchez-Medina 2010, Int J Sports Med 31:347-352 (bar velocity as effort marker)",
            "Zourdos et al. 2016, J Strength Cond Res 30(1):267-275",
        ],
    ),
    #[kb_only]
    HYP_SFR_001 => claim(
        "HYP-SFR-001",
        "Select exercises maximizing stimulus-to-fatigue ratio (SFR): favor machines, cables, and stable isolation; reserve lower-SFR heavy deadlifts and high-skill free weights for compound stimulus/strength.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    ),
    #[kb_only]
    HYP_LENGTHSEL_001 => claim(
        "HYP-LENGTHSEL-001",
        "Bias exercise selection toward long muscle lengths (lengthened positions/partials), e.g., seated over prone leg curl (+14% vs +9% hamstrings) and overhead over pushdown triceps.",
        Moderate,
        &[
            "Maeo 2021 MSSE 53(4):825-837",
            "Maeo et al. 2023, Eur J Sport Sci 23(7):1240-1250 (overhead vs neutral triceps extension)",
            "Wolf et al. 2023, Int J Strength Cond 3(1) (partial vs full range-of-motion meta-analysis)",
            "Pedrosa 2022",
            "Kassiano 2023",
        ],
    )
    .contradicting(&[
        "Effect shrinks in a trained-subject replication of lengthened partials (Wolf et al., SportRxiv preprint); most studies untrained",
        "Overall, partial vs full range of motion differs trivially, slightly favouring full range (Wolf et al. 2023)",
    ]),
    #[kb_only]
    HYP_SUBST_001 => claim(
        "HYP-SUBST-001",
        "Substitute exercises by filtering to same primary muscle and available equipment, then ranking by long-length bias, SFR, and stability; fall back to bodyweight variants if no equipment match.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    ),
    #[kb_only]
    HYP_PAIN_SWAP_001 => claim(
        "HYP-PAIN-SWAP-001",
        "On movement-specific joint pain, substitute a same-muscle exercise with higher stability or a different resistance profile.",
        ExpertOpinion,
        &[
            CITE_ISRAETEL_2021,
            "Cook & Purdam 2009, Br J Sports Med 43:409-416 (load-related joint/tendon pain)",
        ],
    )
    .safety(),
    HYP_MESO_STRUCT_001 => claim(
        "HYP-MESO-STRUCT-001",
        "Train in blocks of 4–6 hard weeks plus 1 lighter week (3 to 6 hard weeks per lighter week), with a lighter week every 4–8 weeks.",
        Moderate,
        &[CITE_ISRAETEL_2021, CITE_BELL_2023],
    )
    .contradicting(&[CONTRA_COLEMAN_DELOAD]),
    HYP_DOUBLEPROG_001 => claim(
        "HYP-DOUBLEPROG-001",
        "Progress with a rep range: add reps each week until you reach the top of the range (for example 10–15 reps with 2–0 reps left), then add the smallest step, about 2.5 kg (5 lb), and start again at the bottom, keeping the same number of sets.",
        Weak,
        &[
            "Plotkin et al. 2022, PeerJ 10:e14142 (load vs repetition progression RCT)",
            "Helms, Morgan & Valdez 2019, The Muscle and Strength Pyramid: Training (2nd ed.)",
        ],
    )
    .contradicting(&[
        "The trial compared rep vs load progression only; the exact rep range and load step are coaching convention",
    ])
    .contested("CQ-F03-04"),
    HYP_LAYOFF_001 => claim(
        "HYP-LAYOFF-001",
        "After a break, restart at a lower set count: the fewest weekly sets that build muscle, and your other set ranges, drop for a while.",
        ExpertOpinion,
        &[
            CITE_ISRAETEL_2021,
            "Mujika & Padilla 2000, Sports Med 30(2):79-87 & 30(3):145-154 (detraining lowers landmarks)",
        ],
    ),
    HYP_TEMPO_001 => claim(
        "HYP-TEMPO-001",
        "Lift at a controlled pace of about 0.5–8 seconds per rep (for example 1–2 seconds up, 2–3 down); speed matters little for muscle growth. Avoid super-slow reps over 10 seconds, which force lighter weights.",
        Moderate,
        &[
            "Schoenfeld/Ogborn/Krieger 2015 tempo meta",
            "2025 meta-analyses",
        ],
    )
    .contradicting(&["Superslow/HIT time-under-tension proponents (unsupported)"]),
    #[kb_only]
    HYP_SUPERSET_001 => claim(
        "HYP-SUPERSET-001",
        "When time-limited, use antagonist/non-competing supersets to save time without cutting rest for the working muscle (accept small volume loss at >=90 s).",
        Moderate,
        &[
            "Iversen, Norum, Schoenfeld & Fimland 2021, Sports Medicine 51:2079-2095 (No Time to Lift; time-efficient antagonist supersets)",
        ],
    ),
    HYP_DEFAULT_PROG_001 => claim(
        "HYP-DEFAULT-PROG-001",
        "Default plan for intermediates: each muscle twice a week near the fewest sets that build muscle (about 8–10 sets a week, up to about 8 per session). Multi-joint lifts 5–10 reps, single-joint 10–20. Start week 1 at 3 reps left and go about 1 rep closer each week, down to 1. Rest 2–3 minutes on multi-joint lifts and 1–2 on single-joint, at a controlled pace. Pick 1–2 exercises per muscle that give a lot of growth for little fatigue or train it at long muscle lengths, and take a lighter week in week 5 or 6.",
        Moderate,
        &[CITE_ISRAETEL_2021],
    ),
    HYP_SPEC_BLOCK_001 => claim(
        "HYP-SPEC-BLOCK-001",
        "If you're advanced and a muscle isn't responding to about 15 sets, run a focus block: raise that muscle's sets and drop the others to maintenance sets.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    ),
    HYP_REST_002 => claim(
        "HYP-REST-002",
        "For muscle growth, rest more than 60 seconds between sets: longer rest gives a small benefit, likely because you keep more reps per set. Beyond about 90 seconds the extra benefit is uncertain, so about 90 seconds suits isolation and machine work and 2–3 minutes suits heavy compound lifts.",
        Moderate,
        &[
            CITE_SINGER_2024,
            "Schoenfeld, Pope et al. 2016, JSCR 30(7):1805-1812",
        ],
    )
    .contradicting(&[
        "Singer et al. 2024: whole-body lean-mass estimates slightly favoured shorter rest (uncertain)",
    ]),
    #[kb_only]
    HYP_RIR_DOSE_001 => claim(
        "HYP-RIR-DOSE-001",
        "Muscle growth increases as sets end closer to failure, while strength gains barely depend on it at a given load; stopping 1-2 reps short grew muscle about as well as going to failure in trained lifters. Sets ending 5 or more reps short, like speed work, are a weak growth stimulus.",
        Moderate,
        &[
            CITE_ROBINSON_2024,
            "Refalo et al. 2024, J Sports Sci 42(1):85-101 (failure vs 1-2 reps in reserve in trained lifters)",
        ],
    )
    .contradicting(&["Close to failure, the extra growth per rep is small (Refalo 2023)"]),
    #[kb_only]
    HYP_VL_001 => claim(
        "HYP-VL-001",
        "Sets stopped after a larger slowdown in bar speed (40%) grew parts of the thigh more than sets stopped at 20%, with similar strength gains, while the 20% sets improved jumping more; low-fatigue speed sets are a strength and power tool rather than a muscle-building one.",
        Weak,
        &[
            "Pareja-Blanco et al. 2017, Scand J Med Sci Sports 27(7):724-735 (velocity-loss RCT)",
        ],
    )
    .contradicting(&[
        "Single trial; both groups grew the whole quadriceps, and the 40% group grew more only in two of its four heads",
    ]),
    #[kb_only]
    HYP_PERIOD_001 => claim(
        "HYP-PERIOD-001",
        "Linear and daily-undulating plans build similar muscle, so a muscle-building plan does not need a strength plan's heavy and power days.",
        Moderate,
        &[
            "Grgic et al. 2017, PeerJ 5:e3695 (linear vs undulating periodization meta-analysis)",
        ],
    ),
    HYP_DELOAD_CADENCE_001 => claim(
        "HYP-DELOAD-CADENCE-001",
        "Taking one lighter week in the middle of a 9-week program did not change muscle growth and slightly slowed lower-body strength gains. Experts place a lighter week about every 4–6 weeks, lasting about a week and made by cutting volume, effort or both, so build in blocks of 4–6 weeks and take a lighter week earlier only when 2 or more fatigue signs appear.",
        Weak,
        &[CITE_COLEMAN_2024, CITE_BELL_2023],
    )
    .contradicting(&[
        "Some coaching systems use shorter cycles of three weeks on, one week off (Israetel et al. 2021)",
    ]),
}
