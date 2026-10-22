//! Every `f32`/`f64` in a rendered `ViewModel` must be finite.
//!
//! serde_json writes a NaN or ±∞ as `null`, and the shell decodes the view with
//! kotlinx `Json` without `coerceInputValues`, so a `null` in a non-null
//! `Double` field makes Kotlin reject the whole view. The check runs at the Rust
//! type level, through a serde `Serializer` that records every non-finite float
//! with its path, instead of scanning the JSON for `null`: an `Option::None`
//! never reaches `serialize_f64`, so no hand-kept list of which keys are
//! optional can drift out of date.

use std::fmt;

use serde::Serialize;
use serde::ser;

use super::view_equivalence::{END_MON, big_history, ev};
use super::*;

/// A serializer that discards everything except non-finite floats, which it
/// records as `field.path = value`. Sequence elements show as `[]`, map values
/// as `{}`, enum variants by name.
#[derive(Default)]
struct FiniteFloats {
    path: Vec<&'static str>,
    floats: usize,
    bad: Vec<String>,
}

impl FiniteFloats {
    fn float(&mut self, v: f64) -> Result<(), fmt::Error> {
        self.floats += 1;
        if !v.is_finite() {
            self.bad.push(format!("{} = {v}", self.path.join(".")));
        }
        Ok(())
    }

    fn nested<T: ?Sized + Serialize>(
        &mut self,
        seg: &'static str,
        v: &T,
    ) -> Result<(), fmt::Error> {
        self.path.push(seg);
        let out = v.serialize(&mut *self);
        self.path.pop();
        out
    }

    fn enter(&mut self, seg: &'static str) -> Result<&mut Self, fmt::Error> {
        self.path.push(seg);
        Ok(self)
    }

    fn leave(&mut self) -> Result<(), fmt::Error> {
        self.path.pop();
        Ok(())
    }
}

macro_rules! ignore {
    ($($method:ident: $ty:ty),* $(,)?) => {
        $(fn $method(self, _: $ty) -> Result<(), fmt::Error> {
            Ok(())
        })*
    };
}

impl ser::Serializer for &mut FiniteFloats {
    type Ok = ();
    type Error = fmt::Error;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    ignore!(
        serialize_bool: bool,
        serialize_i8: i8,
        serialize_i16: i16,
        serialize_i32: i32,
        serialize_i64: i64,
        serialize_i128: i128,
        serialize_u8: u8,
        serialize_u16: u16,
        serialize_u32: u32,
        serialize_u64: u64,
        serialize_u128: u128,
        serialize_char: char,
        serialize_str: &str,
        serialize_bytes: &[u8],
        serialize_unit_struct: &'static str,
    );

    fn serialize_f32(self, v: f32) -> Result<(), fmt::Error> {
        self.float(f64::from(v))
    }

    fn serialize_f64(self, v: f64) -> Result<(), fmt::Error> {
        self.float(v)
    }

    fn serialize_none(self) -> Result<(), fmt::Error> {
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> Result<(), fmt::Error> {
        v.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), fmt::Error> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), fmt::Error> {
        Ok(())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        v: &T,
    ) -> Result<(), fmt::Error> {
        v.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        v: &T,
    ) -> Result<(), fmt::Error> {
        self.nested(variant, v)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self, fmt::Error> {
        Ok(self)
    }

    fn serialize_tuple(self, _: usize) -> Result<Self, fmt::Error> {
        Ok(self)
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, fmt::Error> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self, fmt::Error> {
        self.enter(variant)
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self, fmt::Error> {
        Ok(self)
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, fmt::Error> {
        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self, fmt::Error> {
        self.enter(variant)
    }
}

impl ser::SerializeSeq for &mut FiniteFloats {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), fmt::Error> {
        self.nested("[]", v)
    }

    fn end(self) -> Result<(), fmt::Error> {
        Ok(())
    }
}

impl ser::SerializeTuple for &mut FiniteFloats {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), fmt::Error> {
        self.nested("[]", v)
    }

    fn end(self) -> Result<(), fmt::Error> {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for &mut FiniteFloats {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), fmt::Error> {
        self.nested("[]", v)
    }

    fn end(self) -> Result<(), fmt::Error> {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for &mut FiniteFloats {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), fmt::Error> {
        self.nested("[]", v)
    }

    fn end(self) -> Result<(), fmt::Error> {
        self.leave()
    }
}

impl ser::SerializeMap for &mut FiniteFloats {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, k: &T) -> Result<(), fmt::Error> {
        self.nested("{key}", k)
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), fmt::Error> {
        self.nested("{}", v)
    }

    fn end(self) -> Result<(), fmt::Error> {
        Ok(())
    }
}

impl ser::SerializeStruct for &mut FiniteFloats {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        v: &T,
    ) -> Result<(), fmt::Error> {
        self.nested(key, v)
    }

    fn end(self) -> Result<(), fmt::Error> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut FiniteFloats {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        v: &T,
    ) -> Result<(), fmt::Error> {
        self.nested(key, v)
    }

    fn end(self) -> Result<(), fmt::Error> {
        self.leave()
    }
}

#[test]
fn finite_floats_flags_a_non_finite_value_with_its_path() {
    #[derive(Serialize)]
    struct Row {
        ok: f64,
        none: Option<f64>,
        open: Vec<f32>,
    }
    let mut check = FiniteFloats::default();
    Row {
        ok: 1.0,
        none: None,
        open: vec![2.0, f32::INFINITY],
    }
    .serialize(&mut check)
    .expect("walk succeeds");
    assert_eq!(check.floats, 3);
    assert_eq!(check.bad, ["open.[] = inf"]);
}

#[test]
fn rich_view_model_serializes_only_finite_floats() {
    // The 420-day hybrid history behind the view-equivalence pins: lifts, GPS
    // runs, check-ins, a plan and the calculators all populated.
    let (app, mut model) = big_history(420);
    let _ = app.update(
        ev(serde_json::json!({"SetToday": {"epoch_day": END_MON, "utc_offset_sec": 0}})),
        &mut model,
    );
    let mut check = FiniteFloats::default();
    app.view(&model)
        .serialize(&mut check)
        .expect("walk succeeds");
    assert!(
        check.floats > 1_000,
        "the walk reached the view's numbers: {}",
        check.floats
    );
    assert!(
        check.bad.is_empty(),
        "non-finite floats serialize as JSON null, which Kotlin rejects: {:?}",
        check.bad
    );
}
