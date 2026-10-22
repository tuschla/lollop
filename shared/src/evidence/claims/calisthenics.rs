//! Calisthenics claims: File 11 (`knowledge-base/extracted/11-calisthenics.md`
//! §1; source reports in `knowledge-base/calisthenics/`) and
//! `13-calisthenics-addendum.md`. Grades transcribed verbatim; MEASURED vs
//! INFERRED load fractions per §1 "Load quantification".

use crate::evidence::cite::{
    CITE_BELL_2024, CITE_BOSQUET_2013, CITE_COLEMAN_2024, CITE_HALPERIN_2022, CITE_ISRAETEL_2021,
    CITE_LOW_2016, CITE_MARKOVIC_2007, CITE_PELLAND_2026, CITE_ROBINSON_2024, CITE_SORIANO_2015,
};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Strong, Weak};

claim_group! {
    CalisthenicsClaim => Calisthenics;
    #[kb_only]
    CAL_CONCEPT_001 => claim(
        "CAL-CONCEPT-001",
        "Progressive bodyweight training builds maximal strength comparably to progressive external load, shown across the push-up AND squat patterns.",
        Moderate,
        &[
            "Kotarsky 2018 (PMID 29466268)",
            "Wei 2023 (PMID 37598268)",
            "Calatayud 2015 (PMID 24983847)",
        ],
    ),
    #[kb_only]
    CAL_CONCEPT_002 => claim(
        "CAL-CONCEPT-002",
        "Hypertrophy is load/modality-independent when sets are taken to/near failure (external-load continuum to ~30% 1RM).",
        Strong,
        &[
            "Schoenfeld/Grgic/Ogborn/Krieger 2017 (PMID 28834797)",
            "Lopez 2021 (PMID 33433148)",
        ],
    ),
    #[kb_only]
    CAL_CONCEPT_003 => claim(
        "CAL-CONCEPT-003",
        "For literally-unweighted calisthenics, hypertrophy equivalence holds by INFERENCE (no calisthenics-vs-barbell hypertrophy RCT).",
        Moderate,
        &[
            "Liu 2022 (PMID 35250604)",
            "Kikuchi & Nakazato 2017 (PMID 29541130)",
        ],
    ),
    #[kb_only]
    CAL_STR_LOAD_001 => claim(
        "CAL-STR-LOAD-001",
        "Maximal strength is load-dependent; a bodyweight modality must raise effective load (harder tier / added load), not just add reps.",
        Strong,
        &[
            "Lopez 2021 (PMID 33433148)",
            "Currier 2023 BJSM (PMID 37414459)",
            "Csapo & Alegre 2016 (PMID 26302881)",
        ],
    ),
    #[kb_only]
    CAL_MECH_001 => claim(
        "CAL-MECH-001",
        "Muscle-fibre activation is independent of external load when a set reaches failure; surface EMG amplitude does NOT track true fibre activation (so EMG cannot be used as a load proxy).",
        Strong,
        &["Morton 2019 (PMID 31294822)"],
    ),
    #[kb_only]
    CAL_MECH_002 => claim(
        "CAL-MECH-002",
        "Mechanical tension is the direct signal for muscle growth; where it comes from (bodyweight leverage or a loaded bar) does not matter if the tension is the same.",
        Strong,
        &[
            "Hornberger 2011 (PMID 21621634)",
            "Wackerhage 2019 (PMID 30335577)",
        ],
    ),
    CAL_LOAD_PUSHUP_001 => claim(
        "CAL-LOAD-PUSHUP-001",
        "Push-up load, as measured: a standard push-up carries about 64% of bodyweight at the top and about 70–75% at the bottom; incline about 41–55%, knee about 49–53%, decline about 70–74%; it varies with body angle and range of motion.",
        Moderate,
        &["Ebben 2011", "Suprak 2011 (PMID 20179649)"],
    ),
    CAL_LOAD_ROW_001 => claim(
        "CAL-LOAD-ROW-001",
        "Inverted row, as measured: about 69.5–75.7% of bodyweight at the hands, depending on the body angle.",
        Moderate,
        &["Vural 2023 (PMID 37738266)"],
    ),
    CAL_LOAD_SPLITSQ_001 => claim(
        "CAL-LOAD-SPLITSQ-001",
        "Bulgarian split squat, as measured: front leg about 59% of bodyweight, rear about 28% (bodyweight only); the front share rises with added weight.",
        Moderate,
        &["Arakawa 2025 (PMID 40909869)", "Helme 2022 (PMID 32796420)"],
    ),
    CAL_LOAD_PISTOL_001 => claim(
        "CAL-LOAD-PISTOL-001",
        "Single-leg (pistol) squat, as measured: about 100% of bodyweight on the working leg, with a knee-straightening torque of about 1.52 N·m per kg of bodyweight.",
        Moderate,
        &["Khuu 2016 (PMID 27104053)", "DeForest 2014 (PMC4831851)"],
    ),
    #[kb_only]
    CAL_LOAD_PULL_001 => claim(
        "CAL-LOAD-PULL-001",
        "Pull-ups and chin-ups (muscle activation, not measured load) move about 90-100% of bodyweight; lats reach about 117-130% of maximal activation, chin-ups raise biceps and chest activity, pull-ups raise lower-trapezius activity.",
        Moderate,
        &["Youdas 2010 (PMID 21068680)"],
    ),
    #[kb_only]
    CAL_LOAD_IRONCROSS_001 => claim(
        "CAL-LOAD-IRONCROSS-001",
        "Iron cross, as measured: about 100% of bodyweight as straight-arm shoulder force; elite hold benchmarks 56.7-94.1% of bodyweight; the load at intermediate lever positions roughly follows the sine of the body angle. Bench-press strength correlates only moderately with the iron cross (r = 0.41), so pressing strength is necessary but not sufficient.",
        Moderate,
        &[
            "Dunlavy 2007 (PMC3778705)",
            "Tom/Lecocq 2025 (PMID 40638591)",
            "Schärer 2021 (doi:10.3390/sports9060078)",
        ],
    ),
    CAL_LOAD_INFERRED_001 => claim(
        "CAL-LOAD-INFERRED-001",
        "Dips, the share of bodyweight at the hands in a moving pull-up, handstand and pike push-ups, and planche, front lever, back lever, L-sit and muscle-up work have no force-plate or load-cell data; their loads are estimated from body mechanics, so they are graded expert opinion.",
        ExpertOpinion,
        &[
            "McKenzie 2022 (PMID 36293792, dip EMG-only)",
            "absence of primary kinetics confirmed by exhaustive search",
        ],
    ),
    CAL_INTENSITY_001 => claim(
        "CAL-INTENSITY-001",
        "Bodyweight difficulty comes first from a harder version of the move, then added weight at the top of the ladder, then how many reps you leave within a version. Your max reps on a version replaces percent of your one-rep max as the day-to-day measure.",
        ExpertOpinion,
        &["Low 2016"],
    ),
    CAL_INTENSITY_FLOOR_001 => claim(
        "CAL-INTENSITY-FLOOR-001",
        "Bodyweight sets with no or little added weight must end close to failure, with few reps left, to build muscle; a comfortable, easy set isn't enough.",
        Strong,
        &[
            "Lasevicius 2022 (PMID 31895290)",
            "Refalo 2023 (PMID 36334240)",
            "Mollinedo 2019",
            "Bays-Moneo 2023",
        ],
    ),
    CAL_EFAIL_SAFE_001 => claim(
        "CAL-EFAIL-SAFE-001",
        "Going to failure is fine on moves you can safely stop (push-ups, dips, pull-ups, squats, rows). Keep at least 1–2 reps left on freestanding handstand push-ups, high-skill holds, weighted work, Nordic curls and ring dips.",
        ExpertOpinion,
        &["McKenzie 2022 (PMID 36293792)", "NSCA PTQ 13.1"],
    )
    .safety(),
    CAL_E1RM_001 => claim(
        "CAL-E1RM-001",
        "The estimated one-rep max (Epley or Brzycki formula) is used only for weighted bodyweight moves, on your bodyweight plus the added weight, at about 12 reps or fewer. It isn't shown for high-rep unweighted sets, as the formulas don't hold beyond about 10 reps.",
        Strong,
        &["LeSuer 1997 (JSCR 11(4):211)"],
    ),
    #[kb_only]
    CAL_VBT_001 => claim(
        "CAL-VBT-001",
        "Bar-speed (velocity-based) autoregulation works on loaded lifts measured with a device, including the weighted pull-up (Sánchez-Moreno 2017), but needs a measured implement, so absolute bar-speed zones are not used for unweighted bodyweight work.",
        Strong,
        &["Jukic 2023 (PMID 36178597)", "Sánchez-Moreno 2017"],
    ),
    #[kb_only]
    CAL_RIR_001 => claim(
        "CAL-RIR-001",
        "RIR-anchored RPE is a valid proximity gauge (tracks bar velocity r~=-0.77 to -0.88) but was validated AGAINST velocity on loaded lifts, so unweighted-bodyweight RIR is uncorroborated self-report; trust most at 0-3 RIR.",
        Moderate,
        &[
            "Zourdos 2016 (PMID 26049792)",
            "Ormsbee 2019 (PMID 28301439)",
        ],
    ),
    CAL_PROG_MECHANIC_001 => claim(
        "CAL-PROG-MECHANIC-001",
        "A ladder of fixed steps with a clear move-up rule (move to a harder version, not more reps) works as a progression: two randomized trials showed it for push-ups and squats.",
        Moderate,
        &["Kotarsky 2018 (PMID 29466268)", "Wei 2023 (PMID 37598268)"],
    ),
    CAL_PROG_TRIGGER_001 => claim(
        "CAL-PROG-TRIGGER-001",
        "The exact order of steps and the move-up rules (rep range, two sessions in a row, reps-left cutoff) are untested convention; no trial has compared alternatives, and only two true experiments exist in all men's rings research.",
        ExpertOpinion,
        &[
            "Low 2016",
            "NSCA 2-for-2 (Baechle & Earle)",
            "Malíř 2023 PRISMA",
        ],
    )
    .contested("CQ-F11-01"),
    #[kb_only]
    CAL_PROG_DOUBLEPROG_001 => claim(
        "CAL-PROG-DOUBLEPROG-001",
        "Double progression implements progressive overload (Strong principle) but the scheme itself has no head-to-head trial; it is textbook convention.",
        ExpertOpinion,
        &[
            "Baechle & Earle 2016, Essentials of Strength Training and Conditioning (4th ed., NSCA/Human Kinetics)",
        ],
    ),
    #[kb_only]
    CAL_PROG_ROM_001 => claim(
        "CAL-PROG-ROM-001",
        "Deepening ROM / lengthened-partials is a valid FIXED-LOAD overload for hypertrophy (LP~=full-ROM; the lengthened portion drives growth).",
        Strong,
        &[
            "Schoenfeld 2017 ROM discussion",
            "Wolf 2025 (lengthened-partials RCT)",
        ],
    ),
    CAL_PROG_TEMPO_001 => claim(
        "CAL-PROG-TEMPO-001",
        "Slower reps or added pauses are a valid way to make a move harder, within about 0.5–8 seconds per rep; slower than 10 seconds per rep builds less muscle, so the plan stays below that.",
        Strong,
        &["Schoenfeld 2015 (PMID 25601394)"],
    ),
    #[kb_only]
    CAL_PROG_BRIDGE_001 => claim(
        "CAL-PROG-BRIDGE-001",
        "A discrete ladder cannot let load track rising strength continuously (the mechanism behind RIR-autoregulation's benefit), so RIR can only TIME a tier jump; smooth the quantized step with intra-tier ROM/tempo/added-load bridges.",
        ExpertOpinion,
        &["Graham & Cleather 2021 (PMID 31009432)"],
    ),
    CAL_PROG_STATIC_001 => claim(
        "CAL-PROG-STATIC-001",
        "Holds progress by lever length and seconds held (about 4–5 holds of 5–10 seconds; move up when clean at the target time). Strength from a hold is specific to that joint angle, so it isn't assumed to carry over to full movements.",
        Moderate,
        &["Oranchuk 2019 (doi:10.1111/sms.13375)"],
    ),
    #[kb_only]
    CAL_PROG_GTG_001 => claim(
        "CAL-PROG-GTG-001",
        "Grease-the-Groove (frequent submaximal practice) is an expert-opinion protocol; its frequency \"edge\" comes from extra volume (no benefit beyond equated volume).",
        ExpertOpinion,
        &["Grgic 2018 (PMID 29470825)", "Tsatsouline 2000"],
    ),
    #[kb_only]
    CAL_VOL_001 => claim(
        "CAL-VOL-001",
        "Weekly hard-set volume per muscle has a graded dose-response with hypertrophy (multiset > single by ~40% ES); practical optimum ~12-20 sets/muscle/wk.",
        Strong,
        &[
            "Schoenfeld 2017 (PMID 27433992)",
            "Krieger 2010 (PMID 20300012)",
            "Schoenfeld 2019 (PMID 30153194)",
        ],
    ),
    #[kb_only]
    CAL_FREQ_001 => claim(
        "CAL-FREQ-001",
        "At equated weekly volume, frequency does not independently affect hypertrophy/strength; >=2x/wk is a volume-distribution vehicle.",
        Strong,
        &[
            "Schoenfeld 2019 (PMID 30558493)",
            "Grgic 2018 (PMID 29470825)",
        ],
    ),
    #[kb_only]
    CAL_VOL_LANDMARK_001 => claim(
        "CAL-VOL-LANDMARK-001",
        "The MEV/MAV/MRV numeric landmarks (and the MRV ceiling) are practitioner heuristics; only the underlying dose-response is Strong.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    ),
    #[kb_only]
    CAL_DELOAD_001 => claim(
        "CAL-DELOAD-001",
        "Planned deloads have no direct adaptation-benefit RCT (the one RCT: a cessation deload did not aid hypertrophy and impaired strength); deloads are a fatigue-management convention.",
        ExpertOpinion,
        &[CITE_COLEMAN_2024, CITE_BELL_2024],
    ),
    #[kb_only]
    CAL_VOLCOUNT_SKILL_001 => claim(
        "CAL-VOLCOUNT-SKILL-001",
        "Submaximal skill/static practice should be discounted from the failure-driven hypertrophy volume count (low-fatigue, non-failure, neural/tendon-biased).",
        ExpertOpinion,
        &[
            "Grgic 2018 (PMID 29470825)",
            "Schoenfeld 2019 (PMID 30558493)",
        ],
    ),
    CAL_TENDON_001 => claim(
        "CAL-TENDON-001",
        "Tendons stiffen only under high loads (above about 70% of maximal effort), adapting over about 12 weeks whatever the contraction type, so heavy straight-arm and lever stages advance on the tendon's slower timescale (48–72 hours between sessions, high effort required).",
        Strong,
        &[
            "Bohm 2015 (doi:10.1186/s40798-015-0009-9)",
            "Arampatzis 2007 (doi:10.1242/jeb.003814)",
        ],
    )
    .safety(),
    #[kb_only]
    CAL_INJURY_STATIC_001 => claim(
        "CAL-INJURY-STATIC-001",
        "Distal-biceps rupture mechanism is a supinated, near-extended, isometric contraction - exactly the planche/front-lever/iron-cross posture (calisthenics documented as a mechanism); pace straight-arm static progression.",
        Moderate,
        &["Jukes 2022"],
    )
    .safety(),
    CAL_INJURY_WRIST_001 => claim(
        "CAL-INJURY-WRIST-001",
        "About 57% of adult hand-balancers report wrist pain from pinching at the back of the wrist when it bends back under load, whatever the volume. So handstand, handstand push-up and planche work needs pain-free wrist bending under load and a wrist warm-up. (The growth-plate condition \"gymnast's wrist\" affects only young athletes, not adults; Kox 2015.)",
        Moderate,
        &[
            "Hanson 2022 (PMID 35480067)",
            "Martonovich 2025 (PMID 41133562)",
            "Kox 2015 (PMID 25872521)",
        ],
    )
    .safety(),
    #[kb_only]
    CAL_PREVENT_NORDIC_001 => claim(
        "CAL-PREVENT-NORDIC-001",
        "Nordic eccentric hamstring training roughly halves hamstring injuries (RR ~0.49).",
        Strong,
        &["van Dyk 2019 (BJSM)"],
    ),
    #[kb_only]
    CAL_PREVENT_STRENGTH_001 => claim(
        "CAL-PREVENT-STRENGTH-001",
        "Strength training reduces sports injuries dose-dependently; warm-ups reduce overuse/severe injuries.",
        Strong,
        &["Lauersen 2014/2018", "Soligard 2008"],
    ),
    CAL_GATE_NUMBERS_001 => claim(
        "CAL-GATE-NUMBERS-001",
        "Strength checks before advanced moves (pull-up and dip counts before a muscle-up, weeks to adapt between hold stages) haven't been shown to predict injury, and training-load ratios are unproven. So these numbers are expert starting points; a clinician's assessment is the alternative.",
        ExpertOpinion,
        &["Bahr 2016 (PMID 27343238)", "Impellizzeri 2020"],
    )
    .contested("CQ-F11-02"),
    #[kb_only]
    CAL_INJURY_EPI_001 => claim(
        "CAL-INJURY-EPI-001",
        "Only one calisthenics-specific injury dataset exists (street-workout survey, n=93: 62% injured/12 mo, tendinopathy 31%, shoulder/back predominant, prior injury OR 4.08); the adult signature is shoulder/upper-back overuse tendinopathy.",
        Weak,
        &["Ngo 2021 (PMID 34189146)"],
    ),
    #[kb_only]
    CAL_GUIDE_001 => claim(
        "CAL-GUIDE-001",
        "Baseline: muscle-strengthening of all major groups >=2 d/wk with progressive overload; bodyweight is an accepted modality.",
        Strong,
        &["WHO 2020", "US PAG 2018", "ACSM 2011"],
    ),
    #[kb_only]
    CAL_YOUTH_001 => claim(
        "CAL-YOUTH-001",
        "Youth position stands endorse technique/bodyweight mastery BEFORE external load (a direct external endorsement of tier-before-added_kg); supervised age-appropriate RT is safe/effective (does not harm growth plates).",
        ExpertOpinion,
        &[
            "Faigenbaum 2009 (PMID 19620931)",
            "Lloyd 2014 (PMID 24055781)",
            "Behringer 2010 (PMID 20974785)",
        ],
    ),
    #[kb_only]
    CAL_SEX_001 => claim(
        "CAL-SEX-001",
        "Men and women gain muscle and lower-body strength similarly, and women show a larger relative gain in upper-body strength, so pull-up, dip and push-up goals are not scaled down for women.",
        Strong,
        &["Roberts 2020 (PMID 32218059)"],
    ),
    #[kb_only]
    CAL_OLDER_001 => claim(
        "CAL-OLDER-001",
        "RT builds large strength/function gains in older adults, but for the FALLS goal must be paired with balance/functional work.",
        Strong,
        &[
            "Peterson 2010 (PMID 20385254)",
            "Sherrington 2020 (PMID 31792067)",
        ],
    ),
    // Two rows the synthesis lacks but the source reports state (allowed; both
    // live in knowledge-base/calisthenics/). CAL-TEMPLATE-001: calis-programming.md
    // §1.3. CAL-BP-VALSALVA-001: rt-populations.md (statics are this hazard class).
    CAL_TEMPLATE_001 => claim(
        "CAL-TEMPLATE-001",
        "Weekly bodyweight templates. Strength: full body about 3 times a week, 3–5 sets of 3–6 reps on a hard version near your limit (0–2 reps left), resting 3–5 minutes. Muscle: 3–4 sets of 6–15 reps at 0–1 reps left, resting 1.5–3 minutes. Train every movement pattern at least twice a week, with skill work first while fresh.",
        ExpertOpinion,
        &[
            "Low 2016, Overcoming Gravity (2nd ed.)",
            "Schoenfeld, Ogborn & Krieger 2016, Sports Med (frequency meta)",
        ],
    ),
    CAL_BP_VALSALVA_001 => claim(
        "CAL-BP-VALSALVA-001",
        "Holding your breath and straining (Valsalva) during a long near-max hold sends blood pressure very high: in-artery measurements averaged 320/250 mmHg, with peaks above 480/350. Keep breathing during holds; if you have high blood pressure, prefer lighter, higher-rep work with steady breathing.",
        Moderate,
        &[
            "MacDougall et al. 1985, J Appl Physiol (PMID 3980383)",
            "Pescatello et al. 2004, ACSM position stand (PMID 15076798)",
        ],
    )
    .safety(),
    CAL_PLACE_001 => claim(
        "CAL-PLACE-001",
        "Start each bodyweight progression at the hardest version you can do for at least the bottom of its rep range with clean form (one max-reps test set), and skip ahead more than one step when your reps beat the top of the range by 50% or more (or pass about 15 reps on a strength goal).",
        ExpertOpinion,
        &[CITE_LOW_2016, "Lasevicius 2022 (PMID 31895290)"],
    )
    .contradicting(&["The placement and skip thresholds are coaching convention, not tested"])
    .contested("CQ-F11-01"),
    #[kb_only]
    CAL_RIR_ACC_001 => claim(
        "CAL-RIR-ACC-001",
        "People underestimate how many reps they have left by about one rep on average, and are much less accurate on sets above 12 reps, so high-rep bodyweight progressions advance on reps or hold time achieved, not on a reported reps-in-reserve ceiling.",
        Moderate,
        &[CITE_HALPERIN_2022],
    ),
    CAL_GOAL_RIR_001 => claim(
        "CAL-GOAL-RIR-001",
        "Going closer to failure modestly increases muscle growth but barely changes strength gains, so muscle-building bodyweight sets on safe-to-fail exercises aim for 0–1 reps left while strength sets can stay at 1–2.",
        Moderate,
        &[CITE_ROBINSON_2024],
    ),
    #[kb_only]
    CAL_FREQ_VOL_002 => claim(
        "CAL-FREQ-VOL-002",
        "More weekly sets build both muscle and strength, with diminishing returns; training a movement more often adds strength but little muscle on its own, and indirect sets count about half. So every movement pattern is trained at least twice a week, with its weekly sets sized to the muscle target.",
        Moderate,
        &[CITE_PELLAND_2026],
    ),
    CAL_POWER_001 => claim(
        "CAL-POWER-001",
        "Bodyweight jump training improves vertical jump (countermovement jump +8.7% on average), so a power goal on a bodyweight plan uses low-volume, maximal-intent jumps and explosive moves rather than sets near failure.",
        Strong,
        &[
            CITE_MARKOVIC_2007,
            "Kons et al. 2023, Sports Med Open 9:4 (plyometric training umbrella review)",
        ],
    )
    .contradicting(&[
        "Many trials in the umbrella review lacked a control condition (Kons et al. 2023)",
    ]),
    #[kb_only]
    CAL_POWER_LADDER_001 => claim(
        "CAL-POWER-LADDER-001",
        "The order of bodyweight explosive progressions is coaching convention: jumps go countermovement jump, broad jump, single-leg hops and bounds, then depth jumps only once cleared; pushing goes incline plyometric push-up, plyometric push-up, clap push-up; pulling uses explosive chest-to-bar pull-ups.",
        ExpertOpinion,
        &[CITE_LOW_2016, CITE_SORIANO_2015],
    ),
    CAL_REENTRY_001 => claim(
        "CAL-REENTRY-001",
        "After a break from a bodyweight progression: under 2 weeks, stay on the same step; 2–8 weeks, drop one step (straight-arm holds also restart their tendon adaptation period); over 8 weeks, re-test where to start.",
        ExpertOpinion,
        &[CITE_BOSQUET_2013, CITE_LOW_2016],
    )
    .contradicting(&[
        "The week brackets are an app default mapped from general detraining data",
    ]),
    CAL_EQUIP_001 => claim(
        "CAL-EQUIP-001",
        "Without a bar, dip bars, rings or an anchor, swap to the nearest same-movement version that needs no equipment (table or inverted rows, bench or box dips, box squats, partner- or furniture-anchored Nordic curls) and give the session slot to the other direction of the same movement family.",
        ExpertOpinion,
        &[CITE_LOW_2016],
    ),
}
