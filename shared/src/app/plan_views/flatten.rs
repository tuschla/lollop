//! Flattening a graded plan prescription into the shell-facing item.

use crate::app::plan_views::calis_views::flatten_calisthenics_prescription;
use crate::app::plan_views::flatten_lift::flatten_lift_prescription;
use crate::app::plan_views::flatten_run::flatten_run_prescription;
use crate::app::view_types::PrescriptionView;
use crate::evidence::claims;
use crate::schema::{
    DupDay, LiftSessionType, Prescription, Recommended, RunSessionType, Session, SessionType,
};

/// Flatten one graded `Prescription` into a shell-facing `PrescriptionView`,
/// resolving the load from the anchor when the intensity is %1RM.
pub(in crate::app) fn flatten_prescription(
    rx: &Recommended<Prescription>,
    anchors: &crate::plan::Anchors,
    bodyweight_kg: Option<f64>,
    weight_lb: bool,
) -> PrescriptionView {
    match &rx.value {
        Prescription::Lift(l) => flatten_lift_prescription(l, &anchors.lift, rx, weight_lb),
        Prescription::Run(r) => flatten_run_prescription(r, rx),
        Prescription::Calisthenics(c) => {
            flatten_calisthenics_prescription(c, rx, anchors, bodyweight_kg, weight_lb)
        }
    }
}

/// A session's title by its type and, for a DUP lift day, its emphasis. Two
/// heavy DUP days of one week read apart: the second, 5–6-rep exposure
/// (strength-022 block band) is the "heavy volume" day.
fn day_title(st: SessionType, dup_day: Option<DupDay>) -> &'static str {
    match (st, dup_day) {
        (SessionType::Lift(LiftSessionType::MaxEffort), Some(DupDay::HeavyVolume)) => {
            "Heavy volume day"
        }
        (SessionType::Lift(LiftSessionType::MaxEffort), _) => "Heavy day",
        (SessionType::Lift(LiftSessionType::DynamicEffort), _) => "Power day",
        (SessionType::Lift(LiftSessionType::Repetition), _) => "Volume day",
        (SessionType::Lift(LiftSessionType::Accessory), _) => "Accessory",
        (SessionType::Lift(LiftSessionType::Calisthenics), _) => "Calisthenics",
        (SessionType::Lift(LiftSessionType::FullBody), _) => "Strength day",
        (SessionType::Run(RunSessionType::LongRun), _) => "Long run",
        (SessionType::Run(RunSessionType::Tempo), _) => "Tempo run",
        (SessionType::Run(RunSessionType::Interval), _) => "Intervals",
        (SessionType::Run(RunSessionType::Recovery), _) => "Recovery run",
        (SessionType::Run(RunSessionType::RacePace), _) => "Race-pace run",
        (SessionType::Run(RunSessionType::Repetition), _) => "Repetitions",
        (SessionType::Run(RunSessionType::Strides), _) => "Strides",
        (SessionType::Run(RunSessionType::Hills), _) => "Hill repeats",
        (SessionType::Run(RunSessionType::Easy), _) => "Easy run",
        (SessionType::Rest, _) => "Rest",
    }
}

/// The session's human title ([`day_title`]), marking a lift deload day as
/// part of a lighter week ("Heavy day (lighter week)") so a deload is never
/// titled like the full session. A lift session is a deload day when every
/// lift/calisthenics item carries a deload rule (strength STR-DELOAD-VOL-001,
/// hypertrophy HYP-DELOAD-RX-001), the claims `apply_deload_week` grades
/// them with.
pub(in crate::app) fn plan_session_title(s: &Session) -> String {
    let title = day_title(s.session_type, s.dup_day);
    let mut lifts = s
        .prescriptions
        .iter()
        .filter(|rx| !matches!(rx.value, Prescription::Run(_)))
        .peekable();
    let deload = matches!(s.session_type, SessionType::Lift(_))
        && lifts.peek().is_some()
        && lifts.all(|rx| {
            matches!(
                rx.claim_id(),
                claims::STR_DELOAD_VOL_001 | claims::HYP_DELOAD_RX_001
            )
        });
    if deload {
        format!("{title} (lighter week)")
    } else {
        title.to_string()
    }
}
