//! Running claims: File 04 per-rule rows (`running.rs`, `load.rs`), the File 07
//! load and field-test rows, `13-running-update.md`, `13-run-feedback-load.md`
//! and the onboarding run anchor. KB-internal discrepancies are documented in
//! `contradicting` rather than silently resolved.

use crate::evidence::cite::{
    CITE_BOSQUET_2007, CITE_DANIELS_2022, CITE_MEEUSEN_2013, CITE_NHS_C25K,
};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Strong, Weak};

claim_group! {
    RunningClaim => Running;
    RUN_DELOAD_001 => claim(
        "RUN-DELOAD-001",
        "Train in a 3:1 cycle: three building weeks, then one lighter week that cuts both distance and intensity by 20–40% (2:1 if you're older, injury-prone or new to training).",
        Moderate,
        &[
            CITE_DANIELS_2022,
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
            "RCT protocol NCT06111144 (3:1 load:recovery structure)",
        ],
    ),
    // File 04 running-039: goal-time equivalency via BOTH Riegel and Daniels
    // VDOT, midpoint when they agree within ~2%, a range when they diverge.
    // This is the Riegel/Daniels equivalency combiner, NOT the VDOT-fitness
    // estimate (RUN-VDOT-001); `running::race_equivalency` cites this.
    RUN_EQUIV_001 => claim(
        "RUN-EQUIV-001",
        "Race predictions use two standard methods, Riegel's formula and Daniels' VDOT: when they agree within about 2% you see their midpoint, otherwise a range.",
        Moderate,
        &[
            "Riegel 1981, American Scientist 69(3):285-290",
            // No Cameron citation: that race predictor has no verifiable primary
            // publication (grey literature), and Riegel + Daniels VDOT carry the
            // equivalency.
            CITE_DANIELS_2022,
        ],
    ),
    // File 07 "Banister/Calvert impulse-response": CTL = EWMA of daily
    // TSS/TRIMP tau=42d, ATL tau=7d, TSB = CTL - ATL (yesterday's).
    // [Moderate] for CTL/ATL bookkeeping, [Weak] for prediction: graded
    // at the bookkeeping level here; the statement carries the caveat so
    // no caller can present TSB as a validated performance predictor.
    LOAD_PMC_001 => claim(
        "LOAD-PMC-001",
        "Fitness and fatigue are tracked as two running averages of your daily training load: a 42-day one (fitness) and a 7-day one (fatigue). Yesterday's gap between them shows how fresh you are. The evidence is moderate for this as bookkeeping, but weak for predicting performance.",
        Moderate,
        &[
            "Banister & Calvert 1975, Aust J Sports Med 7:57-61 (impulse-response fitness-fatigue model)",
            "Coggan & Allen 2019, Training and Racing with a Power Meter (3rd ed., VeloPress) (CTL/ATL/TSB implementation)",
        ],
    )
    .contradicting(&[
        "[Weak] for predicting performance; Moderate only as fitness-fatigue bookkeeping",
    ]),
    // knowledge-base/interval-vs-steady-training-load.md. The NEGATIVE result
    // is the strong one: by Jensen's inequality any average-only scalar (mean-HR TRIMP,
    // average %HRmax, sRPE) is mathematically incapable of separating an
    // interval session from a steady run of equal average intensity, and this
    // is empirically confirmed (mean-HR methods under-rate intermittent work).
    #[kb_only]
    RUN_INTERVAL_AVG_FAIL_001 => claim(
        "RUN-INTERVAL-AVG-FAIL-001",
        "A single average pace or average %HRmax cannot distinguish an interval session from a steady run of equal average intensity; only a convex weighting over a per-sample time-series can. Mean-HR load methods empirically under-estimate and misrank intermittent/high-intensity work.",
        Moderate,
        &[
            "Akubat & Abt 2011, J Sci Med Sport 14(3):249-253 (PMID 21237707)",
            "Sanders, Abt, Hesselink, Myers, Akubat 2017, Int J Sports Physiol Perform 12(5):668-675 (PMID 28095061)",
            "Desgorces et al. 2020, PLoS ONE 15(8):e0237027 (PMC7398532)",
        ],
    )
    .contradicting(&[
        "Small samples (n=8-15) with wide, overlapping CIs; team-sport/cycling cohorts generalized to running by analogy",
    ]),
    // The deployable, HR-free differentiator that fits the app's existing GPS
    // track: a flat-ground GOVSS / Normalized-Graded-Pace simplification. The
    // convex 4th-power step makes an interval session score above its own
    // average speed (variability index > 1). Graded WEAK/ExpertOpinion
    // honestly: Skiba's GOVSS is a self-published, non-peer-reviewed white
    // paper, and the app has no elevation → no grade adjustment (normalized
    // *speed*, not power). `running::normalized_speed_mps` /
    // `track_variability_index` compute it. See the KB file's caveats.
    RUN_INTERVAL_VI_001 => claim(
        "RUN-INTERVAL-VI-001",
        "The variability index compares your GPS speed, weighted toward the fast stretches, with your average speed. It stays near 1.0 on steady runs and rises above 1.0 on interval-like runs, so the GPS track alone tells them apart without heart rate. It assumes flat ground, with no adjustment for hills.",
        Weak,
        &[
            "Skiba 2006, Calculation of Power Output and Quantification of Training Stress in Distance Runners: The GOVSS Algorithm (self-published white paper)",
            "TrainingPeaks, Running Training Stress Score (rTSS) / Normalized Graded Pace",
        ],
    )
    .contradicting(&[
        "Self-published, non-peer-reviewed source; no elevation/grade adjustment in this implementation; 30 s fixed rolling-window smoothing (GoldenCheetah value)",
    ]),
    // File 07 "Cooper test": VO2max ~= (d_meters - 504.9)/44.73 from a
    // 12-min max run. [Moderate]
    LOAD_COOPER_001 => claim(
        "LOAD-COOPER-001",
        "The Cooper test estimates your aerobic fitness (VO2max) from how far you run in 12 minutes: (meters covered − 504.9) ÷ 44.73.",
        Moderate,
        &["Cooper 1968, JAMA 203(3):201-204 (12-min run field test of VO2max)"],
    ),
    RUN_WORKOUT_001 => claim(
        "RUN-WORKOUT-001",
        "Each run type has its own pace, heart-rate, effort and duration targets, taken from standard coaching tables.",
        ExpertOpinion,
        &[
            CITE_DANIELS_2022,
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
        ],
    ),
    RUN_MAF_001 => claim(
        "RUN-MAF-001",
        "The Maffetone heart-rate cap for base training is 180 minus your age, adjusted: add 5 if you're elite or have trained 2 or more years injury-free and kept improving; subtract 5 if you're returning from injury or illness or get 2 or more colds a year; subtract 10 if you're chronically overtrained, inactive or on medication. It's an option for base training, not the default, and moves toward a measured easy-effort ceiling (your first lactate threshold) when you have that data.",
        Weak,
        &[
            "Maffetone 2010, The Big Book of Endurance Training and Racing (Skyhorse)",
        ],
    )
    .contradicting(&[
        "Marathon Handbook; Strength Running (specific 180-age not individually validated)",
    ])
    // Global CQ-12 (MAF 180-formula vs measured LT1), not File 04's local
    // numbering.
    .contested("CQ-12"),
    RUN_PROGRESS_001 => claim(
        "RUN-PROGRESS-001",
        "Raising weekly distance by more than 30% over two weeks is linked to about 1.6 times the injury risk of rises under 10%. A safer ramp is 5–10% a week: beginners at most 10% (holding 2–3 weeks between increases), experienced runners about 5% or a fixed cap. Increase one thing at a time.",
        Weak,
        &["Nielsen et al. 2014 (>30%/2wk ~1.6x risk)"],
    )
    .contradicting(&["10%/week rule failed its RCT (Buist 2008)"]),
    RUN_HRRECALC_001 => claim(
        "RUN-HRRECALC-001",
        "Prefer a measured max HR (all-out field test) over age formulas; recalculate HR zones every 4–6 weeks.",
        Strong,
        &[
            "Tanaka, Monahan & Seals 2001, J Am Coll Cardiol 37(1):153-156 (age-predicted maximal HR; 351 studies/18,712 subjects)",
            "Sarzynski et al. HERITAGE",
        ],
    )
    .contradicting(&[
        "Shookster et al. 2020, Int J Exerc Sci 13(7):1242-1250: no age formula is uniformly best; Fox (220-age) showed the least bias in a general-population sample",
    ])
    .safety(),
    RUN_SPIKE_BLOCK_001 => claim(
        "RUN-SPIKE-BLOCK-001",
        "A single run more than 10% longer than your longest run of the previous 30 days is flagged, because one-off distance spikes are linked to overuse injury.",
        Moderate,
        &[
            "Frandsen et al. 2025, BJSM 59(17):1203-1210 (n=5,205; 588,071 sessions)",
        ],
    )
    .safety(),
    RUN_TAPER_001 => claim(
        "RUN-TAPER-001",
        "Before a race, taper for about 2 weeks: cut distance by 41–60%, most steeply at the start, keep the intensity and number of runs, and add nothing new. By race distance, the same shape runs 7–10 days with about 40–50% less for a 5K or 10K, 10–14 days with about 50% less for a half marathon, and 2–3 weeks with 40–60% less for a marathon.",
        Strong,
        &[CITE_BOSQUET_2007, "Mujika & Padilla"],
    )
    .safety(),
    RUN_QUALITY_001 => claim(
        "RUN-QUALITY-001",
        "Keep hard runs to 2–3 a week, at least 48 hours apart, and unless you're an elite runner, never do two very hard (zone 3) sessions on back-to-back days.",
        ExpertOpinion,
        &[
            CITE_DANIELS_2022,
            "Seiler & Kjerland 2006, Scand J Med Sci Sports 16(1):49-56 (polarized hard-easy distribution)",
        ],
    )
    .safety(),
    RUN_C25K_001 => claim(
        "RUN-C25K-001",
        "Couch to 5K: three run/walk sessions a week for 9 weeks (10–12 if needed), at an effort you can talk at, with rest days between. Repeat any week that feels too hard.",
        ExpertOpinion,
        &["Josh Clark 1996; NHS version"],
    )
    .safety(),
    #[kb_only]
    RUN_DOWNWEEK_001 => claim(
        "RUN-DOWNWEEK-001",
        "Insert an unscheduled down week when >=2 overtraining signals fire (RHR +5-7 bpm >=3 days, HRV downtrend, rising RPE, disrupted sleep/soreness/mood, standard-workout performance down >3-5%).",
        ExpertOpinion,
        &[
            CITE_MEEUSEN_2013,
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
        ],
    )
    .safety(),
    RUN_VOLCAP_001 => claim(
        "RUN-VOLCAP-001",
        "Daniels' weekly limits by run type: long run up to 25–30% of your weekly running (about 2–2.5 hours at most), threshold running up to 10%, intervals up to 8%, repetitions up to 5%.",
        ExpertOpinion,
        &[
            CITE_DANIELS_2022,
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
        ],
    )
    .contradicting(&[
        "The threshold and interval band caps rest on stronger evidence than the long-run share, which is a coaching convention - the whole set is registered at the more conservative expert-opinion floor",
    ]),
    RUN_TEMPO_001 => claim(
        "RUN-TEMPO-001",
        "Tempo (threshold) runs: about your 15K-to-half-marathon race pace (about 90% of marathon pace), 88–92% of your max heart rate, effort 6–7 out of 10, for 20–40 minutes in one go or as cruise intervals (long repeats with short rests). Keep threshold running to 10% of your weekly running or less.",
        Moderate,
        &[
            CITE_DANIELS_2022,
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
        ],
    ),
    RUN_INTERVAL_001 => claim(
        "RUN-INTERVAL-001",
        "Intervals (VO2max work): about your 3K–5K race pace, 95–100% of your max heart rate, effort 8–9 out of 10, repeats of 3–5 minutes (800–1600 m) with about as long to recover. Keep interval running to 8% of your weekly running or less.",
        Moderate,
        &[CITE_DANIELS_2022],
    )
    .contradicting(&["Ronnestad: short intervals may be superior for VO2max"])
    .contested("CQ-10"),
    RUN_KARVONEN_001 => claim(
        "RUN-KARVONEN-001",
        "If your resting heart rate is under 55 bpm, use heart-rate-reserve zones (Karvonen, based on max minus resting) rather than zones from max alone, because the two differ a lot. At 70 bpm or above they agree closely and either is fine. Between 55 and 69 bpm neither is established as better.",
        Moderate,
        &[
            "Karvonen, Kentala & Mustala 1957, Ann Med Exp Biol Fenn 35:307-315 (heart-rate reserve)",
            "Swain & Leutholtz 1997, Med Sci Sports Exerc 29(3):410-414 (%HRR = %VO2 reserve)",
        ],
    ),
    RUN_CS_PROTOCOL_001 => claim(
        "RUN-CS-PROTOCOL-001",
        "Critical speed, about the pace at your second lactate threshold, is estimated from 2–5 all-out efforts of different lengths: mostly 2–20 minutes, never under 2 or over 30 minutes, ideally one of 3–8 minutes and one of 12–30. A negative distance reserve in the result means the efforts weren't all-out. Avoid pairs that are too similar (1500 m and a mile) or too far apart (800 m and a half marathon).",
        Moderate,
        &["Galbraith/Nicolo et al. 2018 (linearity r=0.979-0.999)"],
    )
    .contradicting(&[
        "KB gives both a 2-20 min headline window and a 12-30 min ideal upper duration without reconciling them; the validity gate rejects only the explicitly-invalid bounds (<2 min, >30 min)",
    ]),
    #[kb_only]
    RUN_POWER_001 => claim(
        "RUN-POWER-001",
        "Running power zones (Coggan %FTP 7-zone: Z1<55, Z2 56-75, Z3 76-90, Z4 91-105, Z5 106-120, Z6 121-150, Z7>150, SweetSpot 88-94; Stryd %CP 5-zone: Z1 65-80, Z2 80-90, Z3 90-100, Z4 100-115, Z5 115-130; Stryd CP ~= 40-min power) are a consistent proxy, not a criterion metabolic measure; the two frameworks are NOT interchangeable.",
        Weak,
        &[
            "Coggan & Allen 2019, Training and Racing with a Power Meter (3rd ed., VeloPress) (%FTP zones)",
            "Stryd running power-zone documentation (stryd.com) (%Critical Power zones)",
        ],
    )
    .contradicting(&["Weak that power = metabolic cost (Moderate as a framework only)"]),
    RUN_RECOVERY_001 => claim(
        "RUN-RECOVERY-001",
        "Recovery runs: at least 20% slower than marathon pace, under 76% of your max heart rate and under 70% of your heart-rate reserve, effort 2–3 out of 10, for 20–40 minutes.",
        ExpertOpinion,
        &[
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
        ],
    )
    .contradicting(&[
        "KB internal inconsistency: the statement gives <76%HRmax AND <70%HRR (different reference scales) while the parameters line collapses them to <70-76%HRmax; the engine carries both ceilings and lets the stricter govern (conservative)",
    ]),
    RUN_EASY_001 => claim(
        "RUN-EASY-001",
        "Easy runs: 15–25% slower than marathon pace, 65–79% of your max heart rate, effort 3–4 out of 10, for 30–90 minutes.",
        ExpertOpinion,
        &[
            CITE_DANIELS_2022,
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
        ],
    ),
    RUN_LONGRUN_001 => claim(
        "RUN-LONGRUN-001",
        "Long runs: easy to steady pace (10–20% slower than marathon pace), 65–80% of your max heart rate, effort 3–5 out of 10. A long run is 25–30% of your weekly distance (Daniels caps a single run at 25%), about 2–2.5 hours at most, and when your weekly distance is low, no more than twice your average daily run.",
        ExpertOpinion,
        &[
            CITE_DANIELS_2022,
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
        ],
    )
    .contradicting(&[
        "Two KB long-run-share figures conflicted (25-30% vs 20-30%); resolved in favour of 25-30% (Daniels' prescribed long-run share for >=64 km/wk; the 20% lower bound was uncited) - the planner uses 0.25.",
    ]),
    RUN_RACEPACE_001 => claim(
        "RUN-RACEPACE-001",
        "Marathon-pace runs: your marathon race pace, 80–85% of your max heart rate, effort 5–6 out of 10, in blocks of 8–26 km (about 5–16 miles).",
        ExpertOpinion,
        &[
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
            "Canova & Arcelli 1999, Marathon Training: A Scientific Approach (IAAF/International Athletic Foundation)",
        ],
    ),
    RUN_REPETITION_001 => claim(
        "RUN-REPETITION-001",
        "Repetitions: faster than interval pace, set by pace rather than heart rate, 2 minutes or shorter each, and at most 5% of your weekly running. The best distance, number, recovery and effort for them are not established.",
        ExpertOpinion,
        &[CITE_DANIELS_2022],
    )
    .contradicting(&[
        "Band values (>100% VO2max, pace-not-HR) are Moderate via the Daniels VDOT pace tables; registered at the ExpertOpinion floor of the caps column (see RUN-VOLCAP-001)",
    ]),
    RUN_STRIDES_001 => claim(
        "RUN-STRIDES-001",
        "Strides: 4–8 controlled fast efforts (not sprints) of 15–30 seconds, effort 6–7 out of 10, with 45 seconds to 2 minutes of near-full recovery, 1–3 times a week, added a few weeks into base training.",
        ExpertOpinion,
        &[CITE_DANIELS_2022],
    ),
    RUN_HILLSPRINT_001 => claim(
        "RUN-HILLSPRINT-001",
        "Hill sprints: 4–10 near-all-out (90–95%) efforts of 8–20 seconds up a 6–10% slope, effort 9 out of 10, with full recovery (walk back down, about 2 minutes), done on easy days as strength work.",
        ExpertOpinion,
        &[
            "Magness 2014, The Science of Running (Origin Press)",
            "Hudson & Fitzgerald 2008, Run Faster from the 5K to the Marathon (Broadway Books)",
            "Lydiard & Gilmour, Running to the Top (Meyer & Meyer Sport)",
        ],
    )
    .contradicting(&[
        "Lydiard discrete 4-6-wk hill phase vs Hudson year-round (disagreement over placement, not parameters)",
    ]),
    #[kb_only]
    RUN_RUNWALK_EXT_001 => claim(
        "RUN-RUNWALK-EXT-001",
        "For obese/very deconditioned runners, extend run/walk intervals longer before continuous running to manage impact-injury risk (no numeric extension length or defining threshold stated).",
        ExpertOpinion,
        &["Galloway 2016, The Run-Walk-Run Method (Meyer & Meyer Sport)"],
    )
    .safety(),
    RUN_ONEVAR_001 => claim(
        "RUN-ONEVAR-001",
        "Increase one thing at a time: distance or intensity, not both in the same week.",
        ExpertOpinion,
        &[
            CITE_DANIELS_2022,
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
        ],
    ),
    RUN_DELOAD_DEPTH_001 => claim(
        "RUN-DELOAD-DEPTH-001",
        "On a lighter week, cut distance by 20–40% (10–30% at high mileage, up to 50% at low mileage), ease the intensity too, and drop one hard session. No source sets how much to ease intensity or where high and low mileage start.",
        ExpertOpinion,
        &[
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
            "RCT protocol NCT06111144 (3:1 load:recovery structure)",
        ],
    ),
    RUN_RETEST_001 => claim(
        "RUN-RETEST-001",
        "Set training paces from your current fitness, not your goal, re-test every 4–6 weeks, and update paces as your fitness improves. Use a recent race (within 6–8 weeks), run all-out on a flat course in cool weather. Heat above about 15 °C and altitude above about 900 m call for a correction, but how big is not established.",
        Moderate,
        &[
            CITE_DANIELS_2022,
            "Ely et al. 2007, Med Sci Sports Exerc 39(3):487-93 (PMID 17473775) (marathon pace slows first at WBGT >~15 C)",
            "El Helou et al. 2012, PLoS ONE 7(5):e37407 (PMID 22649525) (air-temp optimum ~4-10 C, continuous degradation above)",
        ],
    ),
    RUN_PERIOD_001 => claim(
        "RUN-PERIOD-001",
        "Anchor a race plan to the race date and end it with a taper of up to 3 weeks: cut volume progressively by about 41–60% while keeping intensity and session frequency; tapers of 1, 2 and 3 weeks all improved time-trial performance.",
        Strong,
        &[
            CITE_BOSQUET_2007,
            "Wang et al. 2023, PLoS One 18(5):e0282838 (taper meta-analysis, 14 studies)",
        ],
    ),
    RUN_PHASE_TID_001 => claim(
        "RUN-PHASE-TID-001",
        "Elite distance runners mostly train with a pyramidal intensity mix (lots of easy running, some threshold, a little very hard) while preparing, and shift toward a polarized mix (mostly easy, some very hard, little in between) with lower total volume in the competition period.",
        Moderate,
        &[
            "Casado, Gonzalez-Mohino, Gonzalez-Rave & Foster 2022, Int J Sports Physiol Perform 17(6):820-833",
        ],
    )
    .contradicting(&["Descriptive review of elite practice, not a controlled comparison"])
    .contested("CQ-07"),
    #[kb_only]
    RUN_VOLUME_001 => claim(
        "RUN-VOLUME-001",
        "Runners who run more each week, have a longer longest run and do more runs of 32 km or more tend to run faster marathons; weekly volume is the most consistent training predictor, so a plan keeps building volume across cycles (within safe ramp limits) rather than restarting each cycle.",
        Moderate,
        &[
            "Doherty et al. 2020, J Sci Med Sport 23(2):182-188 (marathon training predictors meta-analysis)",
        ],
    )
    .contradicting(&[
        "Observational: associations, not proof that each variable causes faster times",
    ]),
    RUN_LONGRUN_TARGET_001 => claim(
        "RUN-LONGRUN-TARGET-001",
        "Long-run goals are ceilings to work toward, not weekly musts: about 16–21 km (10–13 miles) for a half marathon and 29–32 km (18–20 miles) for a marathon. You reach them only within three limits: no run more than 10% past your recent longest, the weekly increase limit, and a share cap of at most 30% of your weekly distance below 64 km (40 miles) a week, 25% above that, and no more than about 150 minutes.",
        ExpertOpinion,
        &[
            CITE_DANIELS_2022,
            "Pfitzinger & Douglas 2009, Advanced Marathoning (2nd ed., Human Kinetics)",
            "Doherty et al. 2020, J Sci Med Sport 23(2):182-188",
        ],
    ),
    RUN_TALKTEST_001 => claim(
        "RUN-TALKTEST-001",
        "Without a measured heart-rate max or pace, keep easy runs at a pace where you can talk comfortably in full sentences: being able to talk comfortably means you're below the first breathing threshold, where easy running ends.",
        Moderate,
        &[
            "Reed & Pipe 2014, Curr Opin Cardiol 29(5):475-480 (talk test review)",
            "Persinger et al. 2004, Med Sci Sports Exerc 36(9):1632-1636",
            "Foster et al. 2008, J Cardiopulm Rehabil Prev 28(1):24-30",
        ],
    )
    .contradicting(&[
        "The talk test may not suit high-intensity interval work (Reed & Pipe 2014)",
    ]),
    RUN_STRENGTH_001 => claim(
        "RUN-STRENGTH-001",
        "Strength training helps runners, including those who only run. Studies used 1–4 sessions a week (twice is a common choice); heavy lifting at 80% of your max or more, alone or with jumps, improved time-trial results and time to exhaustion.",
        Moderate,
        &[
            "Llanos-Lagos et al. 2024, Sports Med 54(7):1801-1833 (strength training methods and endurance performance)",
            "Blagrove, Howatson & Hayes 2018, Sports Med 48(5):1117-1149",
            "Lauersen, Bertelsen & Andersen 2014, Br J Sports Med 48(11):871-877 (injury prevention, mixed sports)",
        ],
    )
    .contradicting(&[
        "Jump training alone did not significantly improve time-trial performance (Llanos-Lagos et al. 2024)",
    ]),
    RUN_C25K_002 => claim(
        "RUN-C25K-002",
        "Couch to 5K builds continuous running gradually: the first 20-minute run comes at the end of week 5, 25 minutes by the end of week 6, then 25, 28 and 30 minutes in weeks 7–9, and any week can be repeated.",
        ExpertOpinion,
        &[CITE_NHS_C25K],
    )
    .safety(),
    RUN_WARMUP_001 => claim(
        "RUN-WARMUP-001",
        "Start threshold, interval, repetition and race-pace runs with about 10–15 minutes of easy running and finish with about 10 minutes easy; a warm-up improves the performance that follows, and both count toward the week's running.",
        Moderate,
        &[
            "Fradkin, Zazryn & Smoliga 2010, J Strength Cond Res 24(1):140-148 (warm-up meta-analysis, 32 studies)",
            "Bishop 2003, Sports Med 33(7):483-498 (structuring the warm-up)",
            CITE_DANIELS_2022,
        ],
    )
    .contradicting(&[
        "A warm-up that is too hard or too hot can hurt endurance performance (Bishop 2003); the exact minutes are coaching convention",
    ]),
    RUN_REPETITION_002 => claim(
        "RUN-REPETITION-002",
        "Repetitions: 200 m or 400 m repeats at repetition pace, with an easy jog of the same distance between, keeping all repetition running to 5% of your weekly running or less.",
        ExpertOpinion,
        &[CITE_DANIELS_2022],
    )
    .contradicting(&["The number of repeats per session is coaching convention, not tested"]),
    RUN_RACEPACE_002 => claim(
        "RUN-RACEPACE-002",
        "Keep marathon-pace running to about 15–20% of the week, and any single marathon-pace run to the lesser of 110 minutes or 29 km (18 miles).",
        ExpertOpinion,
        &[CITE_DANIELS_2022],
    )
    .contradicting(&[
        "The marathon-pace segment rule (RUN-RACEPACE-001) states no weekly marathon-pace limit",
    ]),
    RUN_START_001 => claim(
        "RUN-START-001",
        "Before a running plan starts, it needs your current weekly distance and your longest run of the last 30 days; the first long run stays within 10% of that longest run, and logged runs replace the numbers you entered.",
        Moderate,
        &[
            "Frandsen et al. 2025, BJSM 59(17):1203-1210 (n=5,205; 588,071 sessions)",
        ],
    )
    .contradicting(&[
        "Week-to-week distance ratios showed no link with injury in the same study",
    ])
    .safety(),
    #[kb_only]
    RUN_LIVECUE_001 => claim(
        "RUN-LIVECUE-001",
        "Live cues tied to a target pace help runners stay on that pace, and sound or vibration cues work better than glancing at a screen.",
        Weak,
        &[
            "Dobiasch, Stafylidis & Baca 2021, Int J Perform Anal Sport 21(6):1015-1028 (feedback variants and pacing adherence)",
        ],
    )
    .contradicting(&[
        "Puleo & Abraham 2018, Int J Exerc Sci 11(5):384-390: pace readouts without a target did not change self-selected pace",
    ]),
    #[kb_only]
    RUN_EASYDRIFT_001 => claim(
        "RUN-EASYDRIFT-001",
        "Runners tend to run planned easy days harder than intended, drifting toward moderate effort, so the easy-day limit stays visible while running.",
        Weak,
        &[
            "Barnes 2017, Sports Med Open 3:38 (coach-intended vs athlete-perceived session effort)",
        ],
    )
    .contradicting(&[
        "Single observational cohort of collegiate runners; applying it to recreational runners is by analogy",
    ]),
    LOAD_SRPE_001 => claim(
        "LOAD-SRPE-001",
        "Session effort times minutes (one 0–10 rating of the whole session about 30 minutes afterwards, multiplied by its duration) is a valid, reliable training-load measure that needs no sensors. It is not interchangeable with heart-rate load, and on its own it cannot tell intervals from steady runs.",
        Moderate,
        &[
            "Foster et al. 2001, J Strength Cond Res 15(1):109-115 (session-RPE method)",
            "Haddad et al. 2017, Front Neurosci 11:612 (session-RPE validity review)",
        ],
    )
    .contradicting(&[
        "Desgorces et al. 2020, PLoS One 15(8):e0237027: load methods disagree markedly on interval sessions, so the series stay separate",
    ]),
}
