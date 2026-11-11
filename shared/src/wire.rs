//! The serde wire shape of every type the shells exchange with the core, as
//! data. `#[derive(Wire)]` (`shared-derive`) describes a type from the same
//! `#[serde]` attributes serde reads; `shared/wiregen` walks [`roots`] and
//! generates the Kotlin types from it (rules: `shared/wiregen/src/kotlin.rs`).
//!
//! `#[wire(...)]` covers what serde cannot say: how the shell writes a struct
//! that is an event's payload when that differs from its Rust serialization
//! (`Profile`, whose Rust serialization is the view's echo).

use serde::Serialize;
use serde_json::Value;

pub use shared_derive::Wire;

/// A type whose wire shape is known.
pub trait Wire {
    /// The shape of `Self` on the wire; named types it reaches are added to `reg`.
    fn shape(reg: &mut Registry) -> Shape;
}

/// The wire shape of one value.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Bool,
    Int(Int),
    Float(Float),
    Str,
    Option(Box<Shape>),
    List(Box<Shape>),
    /// A type in the [`Registry`].
    Named(&'static str),
    /// `Option<T>` written as `T`'s own string, and `None` as `none`
    /// (a `#[serde(with)]` module; `T` is a unit-only enum).
    OptionAs {
        inner: Box<Shape>,
        none: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Int {
    U8,
    U16,
    U32,
    U64,
    Usize,
    I32,
    I64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Float {
    F32,
    F64,
}

/// A named type: a struct, an externally tagged enum, or a value written as
/// one of a fixed set of strings.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: &'static str,
    /// The Rust path (`std::any::type_name`), to catch two types sharing a name.
    pub rust_path: &'static str,
    pub docs: &'static str,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Struct(Struct),
    /// An externally tagged enum. `predicates` are the `fn(self) -> bool`
    /// methods of a unit-only enum the shell reads (`#[wire(predicates(...))]`);
    /// each variant lists the ones true for it in [`Variant::flags`].
    Enum {
        variants: Vec<Variant>,
        predicates: Vec<&'static str>,
    },
    Strings(Strings),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub fields: Vec<Field>,
    /// As an event's payload the shell writes the fields in this order
    /// (`#[wire(payload_order(...))]`); empty: declaration order.
    pub payload_order: Vec<&'static str>,
    /// As an event's payload the shell omits every field equal to its default
    /// (`#[wire(payload_omits_defaults)]`).
    pub payload_omits_defaults: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// The wire key.
    pub name: &'static str,
    pub docs: &'static str,
    pub shape: Shape,
    /// The value serde fills in when the key is absent; `None`: required.
    pub default: Option<Value>,
    /// The `skip_serializing_if` predicate, as written.
    pub skip_if: Option<&'static str>,
    /// `#[serde(flatten)]`: the struct's fields sit in the parent object.
    pub flatten: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// The wire tag.
    pub name: &'static str,
    /// The Rust variant name.
    pub ident: &'static str,
    pub docs: &'static str,
    pub body: VariantBody,
    /// `#[serde(other)]`: any unknown tag decodes to this variant.
    pub other: bool,
    /// The enum's predicates true for this (unit) variant.
    pub flags: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantBody {
    Unit,
    Newtype(Shape),
    Struct(Vec<Field>),
}

/// A value written as one of fixed strings, each in a group (a planned day's
/// session type: `"Lift(MaxEffort)"` in the group `Lift`).
#[derive(Debug, Clone, PartialEq)]
pub struct Strings {
    pub entries: Vec<(&'static str, &'static str)>,
    /// The groups' type name and the property naming an entry's group.
    pub group_type: &'static str,
    pub group_property: &'static str,
}

/// Every named type reached so far, in first-reached order.
#[derive(Debug, Default)]
pub struct Registry {
    types: Vec<Option<TypeDef>>,
    index: std::collections::HashMap<&'static str, (usize, &'static str)>,
}

impl Registry {
    /// Register `T` as `name`, building its body once (a type reached again,
    /// recursively included, is only named).
    ///
    /// # Panics
    /// When two different Rust types share a wire name.
    pub fn define<T: ?Sized>(
        &mut self,
        name: &'static str,
        docs: &'static str,
        body: impl FnOnce(&mut Self) -> Body,
    ) -> Shape {
        let rust_path = std::any::type_name::<T>();
        if let Some(&(_, path)) = self.index.get(name) {
            assert_eq!(path, rust_path, "two wire types are named {name}");
            return Shape::Named(name);
        }
        let slot = self.types.len();
        self.types.push(None);
        self.index.insert(name, (slot, rust_path));
        let body = body(self);
        self.types[slot] = Some(TypeDef {
            name,
            rust_path,
            docs,
            body,
        });
        Shape::Named(name)
    }

    /// The registered types, in first-reached order.
    pub fn types(&self) -> impl Iterator<Item = &TypeDef> {
        self.types.iter().flatten()
    }

    pub fn get(&self, name: &str) -> Option<&TypeDef> {
        self.index
            .get(name)
            .and_then(|&(slot, _)| self.types[slot].as_ref())
    }
}

/// `value` as JSON, for a field's default.
///
/// # Panics
/// When serde cannot write it (never for a wire type).
pub fn json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("a wire default serializes")
}

/// `value` as JSON written by a `#[serde(with)]` module's `serialize`.
///
/// # Panics
/// When the module cannot write it (never for a wire type).
pub fn json_with<T>(
    value: &T,
    serialize: impl FnOnce(&T, serde_json::value::Serializer) -> Result<Value, serde_json::Error>,
) -> Value {
    serialize(value, serde_json::value::Serializer).expect("a wire default serializes")
}

macro_rules! leaf {
    ($($t:ty => $shape:expr),* $(,)?) => {
        $(impl Wire for $t {
            fn shape(_: &mut Registry) -> Shape {
                $shape
            }
        })*
    };
}

leaf! {
    bool => Shape::Bool,
    u8 => Shape::Int(Int::U8),
    u16 => Shape::Int(Int::U16),
    u32 => Shape::Int(Int::U32),
    u64 => Shape::Int(Int::U64),
    usize => Shape::Int(Int::Usize),
    i32 => Shape::Int(Int::I32),
    i64 => Shape::Int(Int::I64),
    f32 => Shape::Float(Float::F32),
    f64 => Shape::Float(Float::F64),
    String => Shape::Str,
}

impl<T: Wire> Wire for Option<T> {
    fn shape(reg: &mut Registry) -> Shape {
        Shape::Option(Box::new(T::shape(reg)))
    }
}

impl<T: Wire> Wire for Vec<T> {
    fn shape(reg: &mut Registry) -> Shape {
        Shape::List(Box::new(T::shape(reg)))
    }
}

impl<T: Wire + ?Sized> Wire for &T {
    fn shape(reg: &mut Registry) -> Shape {
        T::shape(reg)
    }
}

impl Wire for std::borrow::Cow<'_, str> {
    fn shape(_: &mut Registry) -> Shape {
        Shape::Str
    }
}

impl<T: Wire + Clone> Wire for std::borrow::Cow<'_, [T]> {
    fn shape(reg: &mut Registry) -> Shape {
        Shape::List(Box::new(T::shape(reg)))
    }
}

/// A Rust constant the shell reads, by name.
#[derive(Debug, Clone, PartialEq)]
pub struct Constant {
    pub name: &'static str,
    pub docs: &'static str,
    pub value: ConstValue,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConstValue {
    Int(i64),
    Float(f64),
}

macro_rules! constants {
    ($($(#[doc = $doc:literal])* $path:path as $kind:ident;)*) => {
        /// The Rust constants the shell reads.
        pub fn constants() -> Vec<Constant> {
            vec![$(Constant {
                name: {
                    let path = stringify!($path);
                    match path.rfind(':') {
                        Some(i) => path.split_at(i + 1).1,
                        None => path,
                    }
                },
                docs: concat!($($doc, "\n"),*),
                value: ConstValue::$kind($path.into()),
            }),*]
        }
    };
}

constants! {
    /// Longest single activity `LogActivity` stores, minutes.
    crate::event::entry::ACTIVITY_MAX_MINUTES as Float;
    /// Lowest plausible resting HR, bpm (the Karvonen anchor's band).
    crate::running::RESTING_HR_MIN_BPM as Float;
    /// Highest plausible resting HR, bpm (the Karvonen anchor's band).
    crate::running::RESTING_HR_MAX_BPM as Float;
    /// Youngest age the Tanaka HRmax estimate covers, years.
    crate::running::TANAKA_AGE_MIN_YEARS as Float;
    /// Oldest age the Tanaka HRmax estimate covers, years.
    crate::running::TANAKA_AGE_MAX_YEARS as Float;
    /// The highest average heart rate a logged run states, % of HRmax.
    crate::running::HR_PCT_MAX_CEILING as Float;
    /// Fewest lift days a week a chosen count is held to.
    crate::profile::LIFT_DAYS_MIN as Int;
    /// Most lift days a week a chosen count is held to.
    crate::profile::LIFT_DAYS_MAX as Int;
    /// Lift days a lifting-only profile that chose none trains.
    crate::profile::LIFT_ONLY_DEFAULT_LIFT_DAYS as Int;
    /// Lift days a lifting-and-running profile that chose none trains.
    crate::profile::HYBRID_DEFAULT_LIFT_DAYS as Int;
    /// Most lift days of a run-led hybrid week (maintenance dose, CONC-RE-001).
    crate::hybrid::MAINTENANCE_LIFT_SESSIONS_MAX as Int;
    /// Most run days of a lift-led hybrid week (HYB-RUNCAP-001).
    crate::hybrid::ENDURANCE_FREQUENCY_CAP_DAYS as Int;
    /// One international mile, metres (exact).
    crate::running::MILE_M as Float;
    /// Race distances, metres (the half marathon is half the marathon).
    crate::running::FIVE_K_M as Float;
    crate::running::TEN_K_M as Float;
    crate::running::MARATHON_M as Float;
    /// Pounds per kilogram.
    crate::plates::LB_PER_KG as Float;
    /// Worst horizontal accuracy (m) a fix may carry to count (the QC gate).
    crate::gps::MAX_GPS_ACCURACY_M as Float;
    /// A ground speed above this (m/s) is GPS jitter, not running.
    crate::gps::MAX_PLAUSIBLE_SPEED_MPS as Float;
    /// A leg slower than this (m/s) is standing still: no moving time or distance.
    crate::gps::MIN_MOVING_SPEED_MPS as Float;
    /// Mean Earth radius (m) of the haversine distance.
    crate::gps::EARTH_RADIUS_M as Float;
    /// A gap between fixes longer than this (s) opens a new recording segment.
    crate::gps::MAX_FIX_GAP_SEC as Int;
    /// Most GPS fixes a saved run keeps.
    crate::gps::TRACK_DECIMATION_CAP as Int;
    /// A run under this distance (km) or [MIN_RUN_SEC] is too short to keep.
    crate::gps::MIN_RUN_KM as Float;
    crate::gps::MIN_RUN_SEC as Int;
    /// The largest run file an import reads, bytes.
    crate::import::MAX_RUN_IMPORT_BYTES as Int;
    /// RPE above target (actual − target) that trims the load (autoreg-001/002).
    crate::autoreg::RPE_DELTA_ABOVE as Float;
    /// RPE above target (actual − target) that trims the load further (autoreg-001/002).
    crate::autoreg::RPE_DELTA_WELL_ABOVE as Float;
    /// e1RM ratio (today ÷ baseline) below which the e1RM gate trims the top-set
    /// load (autoreg-006).
    crate::autoreg::E1RM_REDUCE_RATIO as Float;
    /// e1RM ratio (today ÷ baseline) above which the e1RM gate adds load (autoreg-007).
    crate::autoreg::E1RM_INCREASE_RATIO as Float;
    /// Velocity-loss fraction ending a power set, the lowest goal threshold (AUTOREG-VL-001).
    crate::strength::VL_STOP_POWER_FRAC as Float;
    /// Velocity-loss fraction ending a hypertrophy set, the highest goal threshold
    /// (AUTOREG-VL-001).
    crate::strength::VL_STOP_HYPERTROPHY_FRAC as Float;
    /// Wellness composite z-score at or below which one day eases the session
    /// (autoreg-030).
    crate::autoreg::WELLNESS_SINGLE_DAY_Z as Float;
    /// HRV lnRMSSD z-score below which the session eases (autoreg-028).
    crate::autoreg::HRV_SWC_LOWER_Z as Float;
    /// Aerobic-decoupling drift, %, at which the run stays easy (RUN-DECOUPLE-001).
    crate::load::DECOUPLING_HIGH_PCT as Float;
    /// Resting-HR elevation, bpm above baseline, that eases training once it
    /// lasts [RHR_DOWNGRADE_STREAK] days (autoreg-040/041).
    crate::autoreg::RHR_DOWNGRADE_FLOOR_BPM as Float;
    crate::autoreg::RHR_DOWNGRADE_STREAK as Int;
    /// Resting-HR elevation, bpm above baseline, that stops training (autoreg-040/041).
    crate::autoreg::RHR_STOP_BPM as Float;
    /// Soreness (1–7) at or above which intensity eases (autoreg-030).
    crate::autoreg::SORENESS_HIGH as Float;
    /// Shortest steady effort, minutes, whose decoupling counts (File 06 signal spec).
    crate::autoreg::DECOUPLING_MIN_EFFORT_MIN as Float;
    /// Age, years, where the masters bracket begins (MASTERS-001).
    crate::individualization::MASTERS_AGE_YEARS as Float;
    /// Weeks the overtraining signs must last for the recovery concern (feedback-036).
    crate::feedback::OVERTRAINING_MIN_WEEKS as Float;
    /// Weeks of unexplained performance decline at which the overtraining
    /// referrals fire (safety-042, autoreg-042).
    crate::autoreg::NFOR_DECLINE_WEEKS as Int;
}

/// The shapes the shells exchange: the view model, the events, the bridge's
/// error envelope and op payloads, the log compaction verdict, the run
/// import verdict, a run's route fixes, the warm-up ramp and the recorder's
/// track thinning.
pub fn roots(reg: &mut Registry) {
    crate::app::ViewModel::shape(reg);
    crate::event::Event::shape(reg);
    crate::envelope::ErrorEnvelope::shape(reg);
    crate::envelope::RunRef::shape(reg);
    crate::envelope::TrackFigures::shape(reg);
    crate::envelope::WarmUpQuery::shape(reg);
    crate::envelope::ProfileDraft::shape(reg);
    crate::envelope::DraftFacts::shape(reg);
    crate::envelope::TrackThinning::shape(reg);
    crate::envelope::ThinnedTrack::shape(reg);
    crate::envelope::ImportedHr::shape(reg);
    crate::log::Compaction::shape(reg);
    crate::import::ImportVerdict::shape(reg);
    crate::envelope::RunTrack::shape(reg);
    crate::strength::WarmUpStep::shape(reg);
}
