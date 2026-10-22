# Schema design notes

The data model itself lives as Rust types in `shared/src/schema.rs`. This folder
holds design rationale only.

## Shape

```
Program { goal } → Mesocycle { phase, weeks } → Session { type, day }
  → Vec<Recommended<Prescription>>
Prescription = Lift(LiftPrescription) | Run(RunPrescription)
ReadinessInput { signal, value, observed_at } → Recommended<Adjustment>
```

## Decisions

- **`Recommended<T>` wrapper** enforces the non-negotiable rule structurally:
  a `Prescription` or `Adjustment` cannot ship without `Evidence { grade, citation }`
  and a `ConfidenceTag`. Prefer wrapping whole values over per-field evidence;
  granularity can increase later without breaking the invariant.
- **Evidence grades** are the File 09 canonical five (Strong … MarketingMyth),
  ordered so `Ord` gives "stronger evidence" comparisons. Default confidence
  scores (0.90/0.65/0.40/0.30/0.05) mirror File 09.
- **`SafetyTier`** encodes File 06 §5 priority ladder via derived `Ord`:
  Pain > Illness > ObjectivePerformance > SubjectiveMultiDay > HrvTrend >
  SingleDayMarker. The autoregulation engine (later) must resolve conflicts by
  max tier, never by score.
- **Intensity models are enums, not floats**: lifting anchors (%1RM / RPE / RIR /
  velocity) and running zone models (3-zone lactate, VDOT E-M-T-I-R, %HRmax,
  pace, %CP) are distinct variants because the knowledge base grades them
  differently and conversion between them is itself a recommendation.
- **Time**: unix seconds (`i64`) for now; no chrono dependency until a shell
  needs formatting.
- **Validation and engine logic** live in the crate alongside the schema:
  range enforcement, Prilepin lookup tables, VDOT pace functions, ACWR
  exclusion and the rest are implemented across `autoreg`, `plan`, `running`,
  `strength`, `hypertrophy`, `hybrid`, `calisthenics`, `individualization`,
  `load`, `feedback`, `evidence`, `log`, `fit` and the `app` view layer.

## Open questions

- Per-set prescriptions (wave loading, drop sets) vs. per-exercise flat sets.
- Microcycle (week) as explicit layer between Mesocycle and Session?
- Athlete profile / training history types (needed before autoregulation logic).
- Sensor-plugin data ingestion types (File on data harvesting not yet extracted).
