# Schema design notes

The data model itself lives as Rust types in `shared/src/schema.rs`. This folder
holds design rationale only.

## Shape

```
Program { goal, mesocycle } → Mesocycle { phase, weeks, sessions }
  → Session { session_type, day, prescriptions: Vec<Rx> }
Rx = Recommended<Prescription, RxBasis>
Prescription = Lift(LiftPrescription) | Run(RunPrescription)
             | Calisthenics(CalisthenicsPrescription)
ReadinessInput { signal, value, observed_at } → Recommended<Adjustment, AdjustmentKind>
```

## Decisions

- `Recommended<T, K>` makes "every recommendation carries its evidence" a type
  rule: a `Prescription` or `Adjustment` cannot ship without a registered
  `ClaimId` (private, set only by `Recommended::new`), whose registry row carries
  the grade, citation and confidence. `K` is the typed meaning consumers branch
  on (`RxBasis`, `AdjustmentKind`); the claim is only its citation. It wraps
  whole values, not single fields; finer granularity can come later without
  breaking the invariant.
- Evidence grades are the File 09 canonical five (Strong … MarketingMyth),
  ordered so `Ord` gives "stronger evidence" comparisons. Default confidence
  scores (0.90/0.65/0.40/0.30/0.05) are File 09's.
- `SafetyTier` encodes the File 06 §5 priority ladder via derived `Ord`:
  MedicalReferral (File 08 §5 red flags) > Pain > Illness >
  ObjectivePerformance > SubjectiveMultiDay > HrvTrend > SingleDayMarker. The
  autoregulation engine resolves conflicts by max tier, never by score.
- Intensity models are enums, not floats: lifting anchors (%1RM / RPE / RIR /
  velocity) and running zone models (3-zone lactate, VDOT E-M-T-I-R, %HRmax,
  pace, %CP) are distinct variants because the knowledge base grades them
  differently and converting between them is itself a recommendation.
- Time is unix seconds (`i64`). `shared` doesn't depend on chrono itself
  (fitparser pulls it in for FIT timestamps); local days, weeks and formatting
  are the core's own (`app/format.rs`).
- Validation and engine logic live in the crate next to the schema: range
  checks, Prilepin lookup tables, VDOT pace functions, ACWR exclusion and the
  rest are in `autoreg`, `plan`, `running`, `strength`, `hypertrophy`,
  `hybrid`, `calisthenics`, `individualization`, `load`, `feedback`,
  `evidence`, `log`, `fit` and the `app` view layer.

## Open questions

- Per-set prescriptions (wave loading, drop sets) vs. per-exercise flat sets.
- Microcycle (week) as explicit layer between Mesocycle and Session?
- Sensor-plugin data ingestion types (tracker plugins come later; runs import
  from FIT, GPX and TCX files today, `shared/src/import`).
