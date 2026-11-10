//! The plate grid: the bars and plates a gym has, and every rounding that puts
//! a load on them. Each rounding has its own purpose (a working load never
//! exceeds its %1RM, a warm-up lands on the nearest plate step, an added
//! calisthenics load floors to the smallest plate); the steps all come from
//! the plate sets here. Arithmetic only, no claim.

/// Pounds per kilogram. NUMERIC wire/view fields stay in kg (see the
/// `weight_kg`/`e1rm_kg`/`load_kg` field docs and `Profile::weight_lb`); ONLY
/// human-facing display strings convert, and only through the app's format
/// helpers so the factor and the "kg"/"lb" suffix can never drift per call
/// site.
pub(crate) const LB_PER_KG: f64 = 2.2046226218;

/// Kilograms per pound, exactly (the international pound). The calisthenics
/// added-load grid is stated in it.
const KG_PER_LB: f64 = 0.45359237;

/// A standard bar and the common plate set in one display unit, plates
/// largest first.
struct PlateSet {
    bar: f64,
    plates: &'static [f64],
}

const KG_PLATES: PlateSet = PlateSet {
    bar: 20.0,
    plates: &[20.0, 15.0, 10.0, 5.0, 2.5, 1.25],
};

const LB_PLATES: PlateSet = PlateSet {
    bar: 45.0,
    plates: &[45.0, 35.0, 25.0, 10.0, 5.0, 2.5],
};

impl PlateSet {
    /// The plate set of the profile's display unit.
    fn of(weight_lb: bool) -> &'static PlateSet {
        if weight_lb { &LB_PLATES } else { &KG_PLATES }
    }

    fn smallest(&self) -> f64 {
        self.plates[self.plates.len() - 1]
    }

    /// One barbell load step in the display unit: the smallest plate on each
    /// side (2.5 kg, 5 lb).
    fn bar_step(&self) -> f64 {
        2.0 * self.smallest()
    }
}

/// Below this (lb) a lb barbell load moves in one 2.5 lb step: micro-plates
/// are standard there.
const LB_MICRO_BELOW: f64 = 50.0;
const LB_MICRO_STEP: f64 = 2.5;

/// The working-load step for a load of `kg`, in the display unit: 2.5 kg; 5 lb,
/// or 2.5 lb below 50 lb.
fn working_step(kg: f64, weight_lb: bool) -> f64 {
    if !weight_lb || kg * LB_PER_KG >= LB_MICRO_BELOW {
        PlateSet::of(weight_lb).bar_step()
    } else {
        LB_MICRO_STEP
    }
}

/// Float-dust tolerance of [`round_load_grid`], in grid steps: far above the
/// ~1e-13-step error of an f64 kg↔lb round trip or an e1RM × %1RM × derate
/// product, far below any load difference (2.5e-10 kg / 5e-10 lb overshoot).
const LOAD_GRID_EPS: f64 = 1e-10;

/// Round a working load DOWN onto the profile's display-unit plate grid, then
/// return it in kg (the wire unit). kg users get the 2.5 kg grid; lb users the
/// 5 lb grid (2.5 lb below 50 lb, where micro-plates are standard). Rounding
/// DOWN — never nearest — guarantees the rendered load never exceeds the
/// prescribed %1RM, and rounding on the DISPLAY grid means a lb user never sees
/// a fractional-lb load no US plate combination can make (the wire stays kg).
/// The grade still travels with the underlying %1RM claim.
///
/// A load already ON the grid stays put: a lb entry is stored as
/// `lb / LB_PER_KG` and an e1RM × %1RM product carries float dust, so
/// `lb / step` can land at 42.99999… and a bare floor would drop a whole plate.
/// [`LOAD_GRID_EPS`] absorbs that dust; this is the ONE place it lives, so
/// every caller passes its raw load (an f32-sourced one via [`f32_load_ceil`]).
pub(crate) fn round_load_grid(kg: f64, weight_lb: bool) -> f64 {
    let floor_steps = |x: f64, step: f64| (x / step + LOAD_GRID_EPS).floor() * step;
    let step = working_step(kg, weight_lb);
    if weight_lb {
        floor_steps(kg * LB_PER_KG, step) / LB_PER_KG
    } else {
        floor_steps(kg, step)
    }
}

/// One step of the plate grid [`round_load_grid`] floors a load of `kg` onto,
/// in kg: the one plate step for completion tolerance, warm-up matching and
/// load steps.
pub(crate) fn load_grid_step_kg(kg: f64, weight_lb: bool) -> f64 {
    let step = working_step(kg, weight_lb);
    if weight_lb { step / LB_PER_KG } else { step }
}

/// A kg load that came off an f32 wire field (`LiftPrescription::load_kg`),
/// raised by its own f32 precision (half an f32 ulp, at most 2^-24 relative;
/// apply it before any factor) before it is put on the grid: the f32 cast can
/// leave an on-grid lb load ~2e-6 plate steps under its grid point, beyond
/// [`LOAD_GRID_EPS`], and the f32 cannot tell the two apart anyway.
pub(crate) fn f32_load_ceil(kg: f64) -> f64 {
    kg + kg.abs() * f64::from(f32::EPSILON) / 2.0
}

/// A display-unit warm-up figure in kg as the set screen stores it
/// (`warmUpSteps` in `SetEditorState.kt`): a lb figure converted and kept to
/// 0.01 kg.
fn warm_up_kg(display: f64, weight_lb: bool) -> f64 {
    if weight_lb {
        (display / LB_PER_KG * 100.0).round() / 100.0
    } else {
        display
    }
}

/// The first warm-up step of a barbell lift, in kg: the empty bar, or for a
/// pull from the floor the bar with one full plate a side (60 kg / 135 lb).
pub(crate) fn warm_up_start_kg(from_floor: bool, weight_lb: bool) -> f64 {
    let set = PlateSet::of(weight_lb);
    let display = if from_floor {
        set.bar + 2.0 * set.plates[0]
    } else {
        set.bar
    };
    warm_up_kg(display, weight_lb)
}

/// A warm-up ramp load `kg` rounded to the NEAREST barbell step of the
/// display unit (2.5 kg, 5 lb), in kg.
pub(crate) fn round_warm_up_kg(kg: f64, weight_lb: bool) -> f64 {
    let step = PlateSet::of(weight_lb).bar_step();
    if weight_lb {
        warm_up_kg((kg * LB_PER_KG / step).round() * step, true)
    } else {
        (kg / step).round() * step
    }
}

/// The added-load grid of a calisthenics rung in kg: one smallest plate
/// (1.25 kg; for a pounds profile the 2.5 lb plate, so a lb user never sees a
/// fractional-lb load).
pub(crate) fn added_load_grid_kg(weight_lb: bool) -> f64 {
    let smallest = PlateSet::of(weight_lb).smallest();
    if weight_lb {
        smallest * KG_PER_LB
    } else {
        smallest
    }
}

/// An added calisthenics load `kg` floored onto [`added_load_grid_kg`]
/// (tolerant of float error, so an on-grid value never drops a plate).
pub(crate) fn floor_added_load_kg(kg: f64, weight_lb: bool) -> f64 {
    let grid = added_load_grid_kg(weight_lb);
    (kg / grid + 1e-9).floor() * grid
}

/// Plates per side of a standard bar (20 kg, or 45 lb for an lb profile) for
/// a total `load_kg`, largest first, in the display unit. Greedy over the
/// common plate set. Loads are already on the display-unit grid, so the greedy
/// split is exact for every grid load; a remainder below the smallest plate is
/// dropped (never an invented plate). Empty at or below the bar, and for a
/// non-finite or implausible load (more than `MAX_PLATES_PER_SIDE` plates
/// would be needed — no real sleeve holds that), so a corrupt wire weight can
/// never blow up the view.
pub(crate) fn plates_per_side(load_kg: f64, weight_lb: bool) -> Vec<f64> {
    const MAX_PLATES_PER_SIDE: f64 = 20.0;
    let set = PlateSet::of(weight_lb);
    let total = if weight_lb {
        load_kg * LB_PER_KG
    } else {
        load_kg
    };
    // Round the per-side figure to 0.01 so a kg→lb float (e.g. 224.9999) does
    // not lose its last plate.
    let mut side = (((total - set.bar) / 2.0) * 100.0).round() / 100.0;
    if !side.is_finite() || side <= 0.0 || side / set.plates[0] > MAX_PLATES_PER_SIDE {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &p in set.plates {
        let n = ((side + 1e-9) / p).floor().max(0.0);
        out.extend(std::iter::repeat_n(p, n as usize));
        side -= n * p;
    }
    if out.len() as f64 > MAX_PLATES_PER_SIDE {
        return Vec::new();
    }
    out
}
