//! The serde wire shape of every type the shells exchange with the core, as
//! data. `#[derive(Wire)]` (`shared-derive`) describes a type from the same
//! `#[serde]` attributes serde reads; `shared/wiregen` walks the bridge's op
//! table (`ffi::ops::roots`) and generates the Kotlin types from it (rules:
//! `shared/wiregen/src/kotlin.rs`).
//!
//! `#[wire(...)]` covers what serde cannot say: how the shell writes a struct
//! that is an event's payload when that differs from its Rust serialization
//! (`Profile`, whose Rust serialization is the view's echo).

use serde::Serialize;
use serde_json::Value;

pub use shared_derive::Wire;

pub trait Wire {
    /// The shape of `Self` on the wire; named types it reaches are added to `reg`.
    fn shape(reg: &mut Registry) -> Shape;
}

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
    /// `#[default]`: the enum's `Default`, which a shell's zero value is too.
    pub default: bool,
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
    /// The entry of the type's `Default`.
    pub default: &'static str,
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
