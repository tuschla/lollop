//! Split verdicts and split feedback from the QC-filtered track; an explicit
//! review split wins over the track fallback.

use super::*;

/// A ~445 m test track stretched to ~4.4 km (4 full km splits), time scaled
/// so the speeds stay plausible and the split percentage is unchanged: long
/// enough for a pacing verdict.
fn stretch(track: &[GpsPoint]) -> Vec<GpsPoint> {
    track
        .iter()
        .map(|p| GpsPoint {
            lon: p.lon * 10.0,
            observed_at: p.observed_at * 15,
            ..*p
        })
        .collect()
}

#[test]
fn run_only_review_derives_positive_split_feedback_from_last_track() {
    // A GPS run whose back half is far slower than its front half, then a
    // run-only review (no lift, no explicit split figure). The core should
    // fall back to the track-derived split so pacing feedback still fires.
    let app = Engine;
    let mut model = Model::default();
    let points = vec![
        GpsPoint {
            lat: 0.0,
            lon: 0.000,
            observed_at: 0,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.001,
            observed_at: 20,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.002,
            observed_at: 40,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.003,
            observed_at: 90,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.004,
            observed_at: 140,
            accuracy_m: 5.0,
        },
    ];
    app.update(
        log_run_track(stretch(&points), 70.0, 12.0, 0, 0),
        &mut model,
    )
    .expect_only_render();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();

    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::IntensityDiscipline);
}

#[test]
fn a_fragment_too_short_for_a_verdict_gives_no_pacing_feedback() {
    // final-runs F3: a ~0.4 km GPS fragment (no full km split) is the latest
    // tracked run; a manual 4 km run is logged after it. Its halves say
    // nothing about pacing, so no "Even split: textbook pacing" cue.
    let app = Engine;
    let mut model = Model::default();
    let points: Vec<GpsPoint> = (0..5)
        .map(|k| GpsPoint {
            lat: 0.0,
            lon: 0.001 * f64::from(k),
            observed_at: 30 * i64::from(k),
            accuracy_m: 5.0,
        })
        .collect();
    app.update(log_run_track(points.clone(), 70.0, 12.0, 0, 0), &mut model)
        .expect_only_render();
    app.update(log_run_at(4.0, 40.0, 70.0, 600), &mut model)
        .expect_only_render();
    assert!(run_card(&model.runs[0]).split_pct.is_some());
    assert_eq!(latest_track_split(&model), None);
    let vm = app.view(&model);
    assert!(
        vm.feedback
            .as_ref()
            .is_none_or(|f| f.category != FeedbackCategory::IntensityDiscipline),
        "{:?}",
        vm.feedback
    );
    // The same even track at full length is judged.
    let mut model = Model::default();
    app.update(
        log_run_track(stretch(&points), 70.0, 12.0, 0, 0),
        &mut model,
    )
    .expect_only_render();
    assert!(latest_track_split(&model).is_some());
}

#[test]
fn negative_split_note_reads_as_a_magnitude_not_a_signed_number() {
    // A track whose back half is faster than its front half is a negative
    // split. The run summary must phrase it as a positive magnitude
    // ("N% negative split"), not a signed "-N%" that reads awkwardly next to
    // the word "negative".
    let run = LoggedRun {
        distance_km: 0.0,
        duration_min: 0.0,
        hr_pct_max: 70.0,
        longest_recent_km: 12.0,
        track: stretch(&[
            GpsPoint {
                lat: 0.0,
                lon: 0.000,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.001,
                observed_at: 50,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.002,
                observed_at: 100,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.003,
                observed_at: 120,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.004,
                observed_at: 140,
                accuracy_m: 5.0,
            },
        ]),
        track_segment_starts: Vec::new(),
        observed_at: 0,
        entry_id: 0,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
    };
    let view = run_card(&run);
    assert!(
        view.split_pct.expect("split derived") < 0.0,
        "back-half-faster track should read as a negative split"
    );
    assert!(view.summary.contains("second half"), "{}", view.summary);
    assert!(view.summary.contains("% faster"), "{}", view.summary);
    assert!(
        !view.summary.contains("-"),
        "note should show magnitude, not a signed number: {}",
        view.summary
    );
}

#[test]
fn split_feedback_and_run_row_use_the_same_qc_filtered_track() {
    // A single teleport fix (implausible >12 m/s jump) that CLEARS the
    // accuracy gate but FAILS a qc_track speed gate. The displayed run-row
    // split (to_run_view → qc_track) drops it; the pacing-feedback split
    // (latest_track_split) must derive from the SAME qc_track output, not the
    // accuracy-only usable_track, so the run chip and the coaching cue can't
    // disagree near ±3% (MEDIUM bug).
    // Clean track is evenly paced (split ≈ 0 %, "even"); the teleport in the
    // back half, if kept, flips it to a large negative split, opposite
    // verdicts across the ±3 % line.
    let pts = vec![
        GpsPoint {
            lat: 0.0,
            lon: 0.000,
            observed_at: 0,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.001,
            observed_at: 25,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.002,
            observed_at: 50,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.003,
            observed_at: 75,
            accuracy_m: 5.0,
        },
        // Teleport: ~5.2 km in 1 s (>12 m/s), accuracy fine, qc drops it.
        GpsPoint {
            lat: 0.0,
            lon: 0.050,
            observed_at: 76,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.004,
            observed_at: 100,
            accuracy_m: 5.0,
        },
    ];
    let short = pts;
    let mut pts = stretch(&short);
    // The teleport stays a 1 s jump of ~5.2 km off the stretched track.
    pts[4].lon = pts[3].lon + 0.047;
    pts[4].observed_at = pts[3].observed_at + 1;
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run_track(pts.clone(), 70.0, 12.0, 0, 0), &mut model)
        .expect_only_render();

    let row_split = run_card(&model.runs[0]).split_pct.expect("run-row split");
    let fb_split = latest_track_split(&model).expect("feedback split");
    assert_eq!(
        row_split, fb_split,
        "run chip and pacing feedback must derive the split from the same qc_track"
    );

    // An accuracy-only filter keeps the teleport fix and yields a
    // materially different split, proving the two filters actually diverge
    // on this shape, so the assertion above is not vacuous (checked on the
    // short track, where the capped teleport legs still sit in the halves).
    let short_run = LoggedRun {
        track: short.clone(),
        ..model.runs[0].clone()
    };
    let qc_split = run_card(&short_run).split_pct.expect("qc split");
    let raw_split =
        running::track_positive_split_pct(&short, running::MAX_GPS_ACCURACY_M).expect("raw split");
    assert!(
        (raw_split - qc_split).abs() > 3.0,
        "teleport must shift the accuracy-only split vs the qc split (raw {raw_split}, qc {qc_split})"
    );
}

#[test]
fn positive_split_note_reads_as_a_back_half_slowdown() {
    // A track whose back half is slower than its front half is a positive
    // split. The run summary must call it out as a "+N% back-half slowdown"
    // so the note and the coaching cue (positive_split_discipline) agree.
    let run = LoggedRun {
        distance_km: 0.0,
        duration_min: 0.0,
        hr_pct_max: 70.0,
        longest_recent_km: 12.0,
        // Front half (0 → 0.002° over 40 s) is fast; back half
        // (0.002 → 0.004° over 100 s) is slow, a clear back-half slowdown.
        track: vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.000,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.001,
                observed_at: 20,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.002,
                observed_at: 40,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.003,
                observed_at: 90,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.004,
                observed_at: 140,
                accuracy_m: 5.0,
            },
        ],
        track_segment_starts: Vec::new(),
        observed_at: 0,
        entry_id: 0,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
    };
    let view = run_card(&run);
    assert!(
        view.split_pct.expect("split derived") > feedback::POSITIVE_SPLIT_FLAG_PCT,
        "back-half-slower track should read as a positive split above the flag"
    );
    // A ~445 m track has no full km split, so it gets no pacing verdict or
    // note (too short to judge pacing); the measured split is still reported.
    assert!(view.split.is_none(), "no verdict under 2 full splits");
    assert!(
        !view.summary.contains("second half"),
        "got {}",
        view.summary
    );

    // The same shape stretched to ~4.4 km carries the note and the core-owned
    // verdict chip, in agreement.
    let long = LoggedRun {
        track: stretch(&run.track),
        ..run.clone()
    };
    let view = run_card(&long);
    assert!(
        view.summary.contains("second half") && view.summary.contains("% slower"),
        "got {}",
        view.summary
    );
    let split = view.split.expect("verdict for a measured split");
    assert_eq!(split.verdict, SplitVerdict::Fade);
    assert!(split.label.starts_with("FADE +"), "got {}", split.label);
    assert!(
        split.message.contains("as fast or faster"),
        "{}",
        split.message
    );
    assert_eq!(split.evidence.grade, Some(EvidenceGrade::Moderate));
    assert!(
        split.evidence.citation.contains("Hanley"),
        "got {}",
        split.evidence.citation
    );
    assert!(!split.evidence.safety_critical && !split.evidence.contested);
}

#[test]
fn split_verdict_bands_match_the_single_core_threshold() {
    // The verdict must flip exactly where feedback::positive_split_discipline
    // does (±POSITIVE_SPLIT_FLAG_PCT, strict): the shell renders the string
    // without re-deriving any threshold.
    let fade = split_verdict_view(3.1);
    assert_eq!(fade.verdict, SplitVerdict::Fade);
    assert_eq!(fade.label, "FADE +3%");

    let at_line = split_verdict_view(feedback::POSITIVE_SPLIT_FLAG_PCT);
    assert_eq!(
        at_line.verdict,
        SplitVerdict::Even,
        "3% exactly is not yet a fade"
    );

    let even = split_verdict_view(0.0);
    assert_eq!(even.verdict, SplitVerdict::Even);
    assert_eq!(even.label, "EVEN SPLIT");
    assert!(
        even.message.contains("about the same pace"),
        "{}",
        even.message
    );

    let at_neg_line = split_verdict_view(-feedback::POSITIVE_SPLIT_FLAG_PCT);
    assert_eq!(
        at_neg_line.verdict,
        SplitVerdict::Even,
        "-3% exactly is still even"
    );

    let neg = split_verdict_view(-4.0);
    assert_eq!(neg.verdict, SplitVerdict::Negative);
    assert_eq!(neg.label, "NEG SPLIT 4%");
    assert_eq!(
        neg.message,
        "Second half 4% faster than the first. Well paced."
    );

    // Every band carries the same FB-PACING-001 evidence tag.
    for v in [&fade, &even, &neg] {
        assert_eq!(v.evidence.grade, Some(EvidenceGrade::Moderate));
        assert!(
            v.evidence.citation.contains("Hanley"),
            "got {}",
            v.evidence.citation
        );
        assert!(v.evidence.confidence > 0.0);
    }
}

#[test]
fn manual_run_has_no_split_verdict() {
    // No GPS track → no measurable split → no verdict chip (the core never
    // invents a pacing judgment from a hand-entered distance/duration).
    let run = LoggedRun {
        distance_km: 8.0,
        duration_min: 45.0,
        hr_pct_max: 70.0,
        longest_recent_km: 10.0,
        track: vec![],
        track_segment_starts: Vec::new(),
        observed_at: 0,
        entry_id: 0,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
    };
    let view = run_card(&run);
    assert!(view.split_pct.is_none());
    assert!(view.split.is_none());
    // A hand-entered run has no track to walk → no per-unit splits either.
    assert!(view.splits_km.is_empty());
    assert!(view.splits_mi.is_empty());
}

#[test]
fn explicit_review_split_wins_over_track_fallback() {
    // GPS run finishes faster than it starts (a negative split), so the
    // track-derived fallback would not fire pacing feedback. An explicit
    // positive figure on the review must still win and fire it, proving the
    // review value takes precedence over the derived fallback.
    let app = Engine;
    let mut model = Model::default();
    let points = vec![
        GpsPoint {
            lat: 0.0,
            lon: 0.000,
            observed_at: 0,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.001,
            observed_at: 50,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.002,
            observed_at: 100,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.003,
            observed_at: 120,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.004,
            observed_at: 140,
            accuracy_m: 5.0,
        },
    ];
    app.update(log_run_track(points, 70.0, 12.0, 0, 0), &mut model)
        .expect_only_render();
    let review = SessionReview {
        positive_split_pct: Some(10.0),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();

    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::IntensityDiscipline);
}
