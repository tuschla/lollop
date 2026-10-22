//! File 09 canonical claims (`knowledge-base/extracted/09-evidence-map.md`),
//! with statements reworded into plain language (meaning and numbers kept) and
//! re-stated or re-graded where the 2026-10 update did. File 09 grades Strong
//! only the five referral deferrals among its safety rows
//! (SAFE-REDS/OTS/BSI/PREG/CVD-001).

use crate::evidence::cite::{
    CITE_BOSQUET_2007, CITE_ISRAETEL_2021, CITE_MEEUSEN_2013, CITE_MOUNTJOY_2023,
    CITE_PELLAND_2026, CITE_SCHUMANN_2022,
};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Strong, Weak};

claim_group! {
    CanonicalClaim => Canonical;
    HYP_VOL_001 => claim(
        "HYP-VOL-001",
        "More weekly sets build more muscle, with smaller gains from each extra set.",
        Strong,
        &[
            "Schoenfeld, Ogborn & Krieger 2017, J Sports Sci",
            CITE_PELLAND_2026,
        ],
    )
    .contradicting(&[
        "Undetectable superiority beyond ~31 fractional weekly sets (Pelland 2026)",
    ])
    .contested("CQ-01")
    .review(6),
    #[kb_only]
    HYP_LOAD_001 => claim(
        "HYP-LOAD-001",
        "Load interchangeable ~30-85% 1RM when effort matched (hypertrophy).",
        Strong,
        &["Schoenfeld/Grgic 2017", "Morton 2016", "Lasevicius 2018"],
    )
    .contradicting(&["Max-strength gains still favor heavier loads"]),
    #[kb_only]
    PERIOD_001 => claim(
        "PERIOD-001",
        "Periodization beats no plan.",
        Strong,
        &["Williams et al. 2017"],
    )
    .contradicting(&["Model-vs-model comparisons equivocal (CQ-03)"])
    .contested("CQ-03"),
    STR_INTENT_001 => claim(
        "STR-INTENT-001",
        "Lifting heavy is the main driver of maximal strength; moving the weight fast, or trying to, drives power.",
        Strong,
        &[
            "Zatsiorsky & Kraemer 2020, Science and Practice of Strength Training (3rd ed., Human Kinetics); Haff & Triplett 2016, NSCA Essentials of Strength Training and Conditioning (4th ed.)",
            "Cormie, McGuigan & Newton 2011, Sports Med 41(1):17-38; Kawamori & Haff 2004, J Strength Cond Res 18(3):675-684",
        ],
    ),
    CONC_RE_001 => claim(
        "CONC-RE-001",
        "Strength training makes running about 2–8% more economical, so the same pace costs less energy.",
        Strong,
        &["Blagrove, Howatson & Hayes 2018, Sports Med"],
    )
    .contradicting(&["Magnitude speed- and method-dependent (Llanos-Lagos 2024)"]),
    #[kb_only]
    CONC_INTERF_001 => claim(
        "CONC-INTERF-001",
        "Interference is real but modest and quality-specific.",
        Strong,
        &[CITE_SCHUMANN_2022],
    )
    .contradicting(&["Wilson et al. 2012 (nuance)"])
    .contested("CQ-06"),
    AUTOREG_RIR_001 => claim(
        "AUTOREG-RIR-001",
        "Adjusting the weight by how hard sets feel (reps left or a 0–10 effort rating) or by bar speed is the best-supported way to match training to how ready you are.",
        Strong,
        // Helms 2018 leads: it is the KB source for the first-set effort bands
        // graded with this claim (File 06 autoreg-001…005).
        &["Helms 2018", "Zourdos et al. 2016, JSCR", "Banyard 2017"],
    )
    .contradicting(&["Novices estimate RIR poorly"]),
    STR_TRAGE_001 => claim(
        "STR-TRAGE-001",
        "How long you've been training matters most for how much training it takes to keep getting stronger.",
        Strong,
        &["Rhea et al. 2003, MSSE", "Peterson, Rhea & Alvar 2004"],
    ),
    #[kb_only]
    GOAL_PROCESS_001 => claim(
        "GOAL-PROCESS-001",
        "Process goals beat outcome goals.",
        Strong,
        &["Williamson et al. 2022, Int Rev Sport Exerc Psychol"],
    ),
    FEEDBACK_001 => claim(
        "FEEDBACK-001",
        "How feedback is given matters: it works best when it explains, leaves you the choice and focuses on what you did.",
        Strong,
        &[
            "Deci, Koestner & Ryan 1999",
            "Carpentier & Mageau 2013",
            "Mouratidis 2010",
        ],
    ),
    TAPER_001 => claim(
        "TAPER-001",
        "A taper is the best-supported way to peak for an event: 2 weeks, cutting volume by 41–60% on an exponential curve while keeping intensity and how often you train.",
        Strong,
        &[CITE_BOSQUET_2007],
    ),
    SAFE_REDS_001 => claim(
        "SAFE-REDS-001",
        "Signs of under-eating (RED-S) put training on hold; see a professional.",
        Strong,
        &[CITE_MOUNTJOY_2023],
    )
    .contradicting(&["Some critique of diagnostic specificity (does not affect deferral)"])
    .safety(),
    SAFE_OTS_001 => claim(
        "SAFE-OTS-001",
        "Overtraining syndrome calls for real recovery and no increase in training.",
        Strong,
        &[CITE_MEEUSEN_2013],
    )
    .safety(),
    SAFE_BSI_001 => claim(
        "SAFE-BSI-001",
        "A bone stress injury means no impact or loading on it; see a doctor.",
        Strong,
        &["Warden/Davis/Fredericson 2014", "Nattiv 2013"],
    )
    .safety(),
    SAFE_PREG_001 => claim(
        "SAFE-PREG-001",
        "In pregnancy, training follows pregnancy-specific limits, agreed with your doctor or midwife.",
        Strong,
        &["ACOG Committee Opinion 804 (2020)"],
    )
    .safety(),
    SAFE_CVD_001 => claim(
        "SAFE-CVD-001",
        "If the health questions show a heart risk, see a doctor before you start training.",
        Strong,
        &[
            "ACSM's Guidelines for Exercise Testing and Prescription (10th ed., 2018, Wolters Kluwer)",
            "PAR-Q+ (Warburton et al. 2011)",
        ],
    )
    .safety(),
    RUN_HRMAX_001 => claim(
        "RUN-HRMAX-001",
        "Max heart rate is estimated from age as 208 minus 0.7 × age (Tanaka). The estimate is well validated, accurate on average and better than 220 minus age, but it can be off for any one person.",
        Moderate,
        &[
            "Tanaka, Monahan & Seals 2001, J Am Coll Cardiol 37(1):153-6 (PMID 11153730)",
            "Sarzynski et al. 2013 (HERITAGE), Am J Hum Biol 25(5):695-701 (PMID 23913510)",
            "Nes et al. 2013 (HUNT), Scand J Med Sci Sports 23(6):697-704 (PMID 22376273)",
        ],
    )
    .contradicting(&[
        "SEE ~ 10-11 bpm; 95% LOA ~ +/-21 bpm; wider in Black/male/low-fitness subgroups - imprecise per individual",
    ]),
    #[kb_only]
    HYP_FAIL_001 => claim(
        "HYP-FAIL-001",
        "Proximity to failure drives growth continuously; absolute failure not required.",
        Moderate,
        &[
            "Grgic 2022",
            "Refalo 2023",
            "Robinson et al. 2024, Sports Med",
        ],
    )
    .contradicting(&["Marginal effect small near failure"])
    .contested("CQ-02"),
    #[kb_only]
    HYP_LENGTH_001 => claim(
        "HYP-LENGTH-001",
        "Long-muscle-length training is the strongest recent exercise-selection finding.",
        Moderate,
        &["Maeo 2021/2023", "Pedrosa 2022", "Kassiano 2023"],
    )
    .contradicting(&["Some measures site-specific"]),
    #[kb_only]
    AUTOREG_PCT_001 => claim(
        "AUTOREG-PCT-001",
        "Autoregulation matches or slightly beats fixed % load.",
        Moderate,
        &["Gonzalez-Badillo & Sanchez-Medina 2010", "Banyard 2017"],
    )
    .contradicting(&["True 1RM fluctuates +/-18% day-to-day (Jovanovic & Flanagan)"]),
    AUTOREG_VL_001 => claim(
        "AUTOREG-VL-001",
        "Ending a set once bar speed drops by a fixed amount is the best-supported way to adjust how many reps you do.",
        Moderate,
        &[
            "Pareja-Blanco et al. 2017, Scand J Med Sci Sports 27(7):724-735 (velocity-loss thresholds)",
            "Gonzalez-Badillo & Sanchez-Medina 2010, Int J Sports Med 31:347-352 (bar velocity tracks the load lifted, %1RM)",
        ],
    )
    .contradicting(&[
        "The source graded this both Moderate and Moderate-Strong; reconciled to Moderate",
    ]),
    #[kb_only]
    TAPER_STR_001 => claim(
        "TAPER-STR-001",
        "Tapering yields ~2-6% strength gains.",
        Moderate,
        &[
            "Travis et al. 2020, Sports 8(9):125",
            "Pritchard et al. 2015, Strength Cond J 37(2):72-83",
            CITE_BOSQUET_2007,
        ],
    ),
    RUN_DIST_001 => claim(
        "RUN-DIST-001",
        "About 80% easy running is the shared core of every good mix of easy and hard training. A polarized mix (mostly easy plus some very hard work, little in between) gave a small edge in aerobic capacity (VO2max) only to highly trained athletes over blocks shorter than 12 weeks, and no time-trial advantage over other common mixes. The best mix changes with the training phase.",
        Moderate,
        &[
            "Silva Oliveira, Boppre & Fonseca 2024, Sports Med 54(8):2071-2095 (intensity-distribution meta-analysis)",
            "Stoggl & Sperlich 2014, Front Physiol",
            "Rosenblat, Perrotta & Vicenzino 2019, J Strength Cond Res 33(12):3491-3500",
            "Filipas 2022",
        ],
    )
    .contradicting(&[
        "Phase-dependent; recreational responses mixed",
        "Rosenblat et al. 2019 pooled three trials and found a time-trial advantage for polarized over threshold training",
        "Stoggl & Sperlich 2014: larger polarized effects in a single trial",
    ])
    .contested("CQ-07"),
    RUN_SPIKE_001 => claim(
        "RUN-SPIKE-001",
        "One run more than 10% longer than your longest in the last 30 days is the strongest warning sign of a running injury.",
        Moderate,
        &["Frandsen et al. 2025, BJSM"],
    )
    .contradicting(&["ACWR showed a negative dose-response; week-to-week ratio none"]),
    CONC_ORDER_001 => claim(
        "CONC-ORDER-001",
        "Lifting before endurance work gives a small edge in lower-body strength and power.",
        Moderate,
        &["Eddens 2018"],
    )
    .contradicting(&["Effect small"])
    .contested("CQ-15"),
    CONC_SEP_001 => claim(
        "CONC-SEP-001",
        "Spacing lifting and endurance sessions at least 3 hours apart (ideally 6–24) reduces how much one blunts the other in the short term.",
        Moderate,
        &[
            "Baar 2014, Sports Med 44(Suppl 2):S117-S125 (AMPK-mTOR molecular interference)",
            "Robineau et al. 2016, J Strength Cond Res 30(3):672-683 (recovery interval between concurrent sessions)",
        ],
    )
    .contested("CQ-15"),
    #[kb_only]
    CONC_MODE_001 => claim(
        "CONC-MODE-001",
        "Running may interfere more than cycling with long-term muscle and strength gains, but the evidence is preliminary and mostly concerns slow-twitch muscle fibres.",
        Weak,
        &[
            "Lundberg et al. 2022, Sports Med 52(10):2391-2403 (fibre-hypertrophy meta-analysis)",
            "Wilson et al. 2012, J Strength Cond Res 26(8):2293-2307 (concurrent-training meta-analysis)",
        ],
    )
    .contradicting(&[
        "Schumann et al. 2022 (43-study meta-analysis): endurance mode did not moderate interference",
        "Sabag et al. 2018, J Sports Sci 36(21):2472-2483: cycling-based interval training trended worse for lower-body strength than running",
    ])
    .contested("CQ-F10-01"),
    HRV_001 => claim(
        "HRV-001",
        "Training guided by heart-rate variability (HRV) means fewer people fail to improve, and it improves fitness measured below max effort.",
        Moderate,
        &["Vesterinen 2016", "Nuuttila 2017", "Granero-Gallegos 2020"],
    )
    .contradicting(&["Group-average performance NS (Duking 2021)"])
    .contested("CQ-04")
    .review(6),
    WELLNESS_001 => claim(
        "WELLNESS-001",
        "How you feel over several days is the best early warning of overdoing it.",
        Moderate,
        &["Saw et al. 2016"],
    )
    .contradicting(&["Never veto a single day on it"]),
    RUN_VDOT_001 => claim(
        "RUN-VDOT-001",
        "Training zones and race predictions come from a fitness estimate (VDOT or critical speed) worked out from race or time-trial results.",
        Moderate,
        &[
            "Daniels & Gilbert 1979, Oxygen Power: Performance Tables for Distance Runners",
            "Jones & Vanhatalo",
        ],
    ),
    #[kb_only]
    LOAD_TRIMP_001 => claim(
        "LOAD-TRIMP-001",
        "Training-load quantification (TRIMP, TSS/rTSS) is standard and implementable.",
        Moderate,
        &[
            "Banister & Calvert 1975, Aust J Sports Med 7:57-61 (impulse-response fitness-fatigue model)",
            "Coggan & Allen 2019, Training and Racing with a Power Meter (3rd ed., VeloPress)",
        ],
    ),
    HYP_LANDMARKS_001 => claim(
        "HYP-LANDMARKS-001",
        "Weekly set targets use coaching ranges: the fewest sets that build muscle, the range that works best, and the most you can recover from.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    )
    .contradicting(&[
        "Pelland et al. 2026, Sports Med 56(2):481-505: dose-response meta shows no per-muscle landmark validation; the bands are practitioner constants",
    ]),
    #[kb_only]
    RUN_GAP_001 => claim(
        "RUN-GAP-001",
        "Grade-adjusted pace (downhill).",
        Weak,
        &["Minetti et al. 2002, J Appl Physiol"],
    )
    .contradicting(&[
        "Errs up to ~3x on steep downhill; Strava switched to HR-equivalency 2017",
    ])
    .contested("CQ-08"),
    #[kb_only]
    RUN_FORM_001 => claim(
        "RUN-FORM-001",
        "Running power + form metrics (GCT, vertical oscillation, Stryd/Garmin).",
        Weak,
        &[
            "Moore 2016, Sports Med 46(6):793-807 (running economy/biomechanics review)",
        ],
    )
    .contradicting(&["Not RCT-backed"])
    .contested("CQ-13"),
    RUN_DECOUPLE_001 => claim(
        "RUN-DECOUPLE-001",
        "Heart-rate drift: how much your heart rate rises against your pace from the first half of a steady run to the second. Under 5% is good; over 10% is a warning sign.",
        Weak,
        &[
            "Friel 2009, The Triathlete's Training Bible (3rd ed., VeloPress) (<5% good, >10% flag)",
        ],
    )
    .contradicting(&["Confounded by heat/dehydration"]),
    #[kb_only]
    RUN_10PCT_001 => claim(
        "RUN-10PCT-001",
        "10%/week mileage rule.",
        Weak,
        &[
            "Buist et al. 2008, Am J Sports Med 36(1):33-39 (validation RCT; rule failed)",
            "Nielsen et al. 2014, J Orthop Sports Phys Ther 44(10):739-747",
        ],
    )
    .contradicting(&["Failed RCT (Buist 2008); Nielsen"]),
    #[kb_only]
    FEM_MENSTRUAL_001 => claim(
        "FEM-MENSTRUAL-001",
        "Menstrual-cycle-based periodization.",
        Weak,
        &[
            "McNulty et al. 2020, Sports Medicine 50:1813-1827",
            "Elliott-Sale et al. 2021, Sports Medicine 51:843-861",
        ],
    )
    .contradicting(&["McNulty et al. 2020 (78 studies; trivial, highly variable)"])
    .contested("CQ-09"),
    #[kb_only]
    READY_CONSUMER_001 => claim(
        "READY-CONSUMER-001",
        "Consumer readiness scores (Whoop / Oura / Garmin Body Battery).",
        Weak,
        &[
            "Peake, Kerr & Sullivan 2018, Front Physiol 9:743 (critical review of consumer wearables)",
        ],
    )
    .contradicting(&["Black-box algorithms"])
    .contested("CQ-14"),
}
