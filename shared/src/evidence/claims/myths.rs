//! File 09's 21 marketing myths. Recorded so the engine can recognize them;
//! `MarketingMyth` rows are hard-blocked and can never back a recommendation.

use crate::evidence::claim;
use crate::schema::EvidenceGrade::MarketingMyth;

claim_group! {
    MythClaim => Myth;
    LOAD_ACWR_001 => claim(
        "LOAD-ACWR-001",
        "Acute:chronic workload ratio predicts injury risk with a 0.8-1.3 'sweet spot'.",
        MarketingMyth,
        &["Gabbett 2016, BJSM"],
    )
    .contradicting(&[
        "Impellizzeri et al. 2019 (retraction request)",
        "Lolli et al. 2019 (mathematical coupling)",
    ])
    .contested("CQ-05")
    .review(6),
    RUN_CAD_180 => claim(
        "RUN-CAD-180",
        "All runners should run at 180 steps per minute.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Daniels 1984 observation misread as universal target"])
    .review(24),
    MYTH_POSITIVITY => claim(
        "MYTH-POSITIVITY",
        "The 2.9:1 'positivity ratio' governs feedback.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Brown, Sokal & Friedman 2013; American Psychologist 2013 correction"]),
    MYTH_10PCT => claim(
        "MYTH-10PCT",
        "10% weekly mileage rule as a hard injury predictor.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Buist 2008 (failed RCT); Nielsen"]),
    MYTH_LIFTING_SLOW => claim(
        "MYTH-LIFTING-SLOW",
        "Lifting makes runners slow / bulky.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Blagrove, Howatson & Hayes 2018"]),
    MYTH_MUST_FAIL => claim(
        "MYTH-MUST-FAIL",
        "You must train to failure for growth.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Grgic 2022; Refalo 2023; Robinson et al. 2024"]),
    MYTH_MENSTRUAL_MANDATORY => claim(
        "MYTH-MENSTRUAL-MANDATORY",
        "Mandatory menstrual-cycle-phase periodization.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["McNulty et al. 2020"]),
    MYTH_TONING_SPOT => claim(
        "MYTH-TONING-SPOT",
        "'Toning' vs 'bulking' / spot reduction.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Ramirez-Campillo et al. 2022; Vispute et al. 2011"]),
    MYTH_MUSCLE_CONFUSION => claim(
        "MYTH-MUSCLE-CONFUSION",
        "'Muscle confusion' drives adaptation.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Progressive-overload RCT literature"]),
    MYTH_PROTEIN_WINDOW => claim(
        "MYTH-PROTEIN-WINDOW",
        "Anabolic / 30-minute protein window.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Aragon & Schoenfeld 2013; Schoenfeld/Aragon/Krieger 2013 meta"]),
    MYTH_LACTIC_DOMS => claim(
        "MYTH-LACTIC-DOMS",
        "Lactic acid causes DOMS.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Cheung, Hume & Maxwell 2003"]),
    MYTH_STATIC_STRETCH => claim(
        "MYTH-STATIC-STRETCH",
        "Static stretching prevents injury / boosts performance.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Lauersen, Bertelsen & Andersen 2014 (25 RCTs)"]),
    MYTH_FAT_BURN_ZONE => claim(
        "MYTH-FAT-BURN-ZONE",
        "The 'fat-burning zone'.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["ACSM position; Treuth et al."]),
    MYTH_FASTED_CARDIO => claim(
        "MYTH-FASTED-CARDIO",
        "Fasted cardio is superior for fat loss.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Schoenfeld et al. 2014; Hackett & Hagstrom 2017 meta"]),
    MYTH_BAREFOOT => claim(
        "MYTH-BAREFOOT",
        "Barefoot/minimalist running universally reduces injury.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Ryan et al. 2014; multiple systematic reviews"]),
    MYTH_BMI_COMP => claim(
        "MYTH-BMI-COMP",
        "BMI = body-composition truth.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Itani et al. (DXA; 24.9% of athletes misclassified)"]),
    MYTH_NO_PAIN_JOINT => claim(
        "MYTH-NO-PAIN-JOINT",
        "'No pain, no gain' applied to joint pain.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Musculoskeletal safety consensus - joint pain is a stop signal"])
    .safety(),
    MYTH_WOMEN_HEAVY => claim(
        "MYTH-WOMEN-HEAVY",
        "Women shouldn't lift heavy.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Rhea et al. 2003; Hagstrom 2020"]),
    MYTH_SWEAT => claim(
        "MYTH-SWEAT",
        "More sweat = better workout.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Thermoregulation physiology"]),
    MYTH_WAIST_TRAINER => claim(
        "MYTH-WAIST-TRAINER",
        "Waist trainers / spot fat loss devices.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Ramirez-Campillo 2022; Vispute 2011"]),
    MYTH_HR_ZONE_DOGMA => claim(
        "MYTH-HR-ZONE-DOGMA",
        "Heart-rate-zone dogmatism.",
        MarketingMyth,
        &[],
    )
    .contradicting(&["Tanaka et al. 2001 (HRmax is an individual value, SEE ~ +/-10 bpm)"]),
}
