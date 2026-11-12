//! The Kotlin wire types (`WireTypes.kt`): one kotlinx.serialization class per
//! Rust wire type, written so kotlinx reads and writes exactly serde's JSON.
//!
//! - Structs are `@Serializable data class`es; `#[serde(flatten)]` fields are
//!   inlined, and the flattened struct is a class of its own that the outer
//!   class hands out whole (a non-serialized getter named after the Rust
//!   field). `skip_serializing_if` fields are `@EncodeDefault(NEVER)` with the
//!   skipped value as their default.
//! - Unit-only enums are `enum class`es with serde's names; `Event` (the one
//!   enum with data) is a sealed interface with a serializer for serde's
//!   external tagging. A newtype variant's struct payload is inlined into the
//!   variant class (`{"SetProfile":{…}}` reads the same either way), in its
//!   `#[wire(payload_order)]`, omitting defaults when `payload_omits_defaults`;
//!   so is a struct variant's flattened struct, in its field's place.
//! - Types only the shell writes (reached from an event or an op input, not
//!   from an output) take Kotlin property names and only serde's defaults;
//!   every other type keeps the wire names and defaults every field, as the
//!   views always did. `f32` is `Double` in a type the shell writes, so the
//!   shell's figures reach the log at full precision.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

use serde_json::Value;
use shared::ffi::constants::{constants, hypertrophy_muscles};
use shared::ffi::ops::{Roots, roots};
use shared::wire::{
    Body, ConstValue, Field, Float, Int, Registry, Shape, Strings, Struct, TypeDef, Variant,
    VariantBody,
};

use crate::{HEADER, Output};

const PATH: &str = "android/lollop/src/commonMain/kotlin/app/milestone/WireTypes.kt";

/// Kotlin defaults for fields serde requires (or defaults differently) that
/// the shell fills in itself: `(owner, wire field, expression)`. An owner is
/// a type or `Event::Variant`.
const DEFAULTS: &[(&str, &str, &str)] = &[
    ("Event::RemoveReadiness", "observed_at", "AppClock.nowSec()"),
    ("Event::ResolveHold", "observed_at", "AppClock.nowSec()"),
    (
        "Event::RestartHrvBaseline",
        "observed_at",
        "AppClock.nowSec()",
    ),
    ("Event::LogSet", "observed_at", "AppClock.nowSec()"),
    ("Event::LogSet", "entry_id", "EntryIds.next()"),
    (
        "Event::SetExerciseEnded",
        "observed_at",
        "AppClock.nowSec()",
    ),
    ("Event::LogRun", "observed_at", "AppClock.nowSec()"),
    ("Event::LogRun", "entry_id", "EntryIds.next()"),
    ("Event::LogRunTrack", "observed_at", "AppClock.nowSec()"),
    ("Event::LogRunTrack", "entry_id", "EntryIds.next()"),
    ("Event::LogActivity", "observed_at", "AppClock.nowSec()"),
    ("Event::LogActivity", "entry_id", "EntryIds.next()"),
    ("Event::SubmitReadiness", "streak", "0"),
    ("Event::SubmitReview", "observed_at", "AppClock.nowSec()"),
    ("Event::SubmitReview", "bone_pain_red_flag", "false"),
    ("Event::SubmitReview", "compulsive_flag", "false"),
    ("Event::SubmitReview", "overtraining_signal_count", "0"),
    ("Event::SubmitReview", "bad_day", "false"),
    (
        "Event::SubmitWeeklyCheckin",
        "observed_at",
        "AppClock.nowSec()",
    ),
    ("Event::SubmitWeeklyCheckin", "bone_pain_red_flag", "false"),
    ("Event::SubmitWeeklyCheckin", "compulsive_flag", "false"),
    (
        "Event::SubmitWeeklyCheckin",
        "overtraining_signal_count",
        "0",
    ),
    ("Event::SubmitWeeklyCheckin", "bad_day", "false"),
];

pub(crate) fn wire_types() -> Output {
    let mut reg = Registry::default();
    let roots = roots(&mut reg);
    let mut generator = Gen::new(&reg, &roots);
    let body = generator.file();
    for (owner, field, _) in DEFAULTS {
        assert!(
            generator.used_defaults.contains(&(*owner, *field)),
            "wiregen DEFAULTS names no field: {owner}.{field}"
        );
    }
    Output {
        path: PATH,
        contents: body,
    }
}

/// How a generated class treats its fields.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// The shell writes it and reads it only as an op's answer (a run file's
    /// fixes): Kotlin names, serde's defaults only.
    Input,
    /// Read (and maybe written): wire names, every field defaulted.
    Output,
    /// Written, and read in the view model: wire names, every field
    /// defaulted, `f32` as `Double`.
    Shared,
}

impl Role {
    fn double_floats(self) -> bool {
        self != Role::Output
    }
}

struct Gen<'a> {
    reg: &'a Registry,
    roles: HashMap<&'static str, Role>,
    /// Structs reached only as an event's inlined payload: no class of their own.
    emitted: HashSet<&'static str>,
    imports: BTreeSet<&'static str>,
    used_defaults: HashSet<(&'static str, &'static str)>,
    /// `OptionAs` serializers to emit: (inner type, none label).
    option_as: Vec<(&'static str, &'static str)>,
}

impl<'a> Gen<'a> {
    fn new(reg: &'a Registry, roots: &Roots) -> Self {
        let mut input = HashSet::new();
        let mut output = HashSet::new();
        let mut viewed = HashSet::new();
        let mut emitted = HashSet::new();
        for root in &roots.outputs {
            mark(reg, root, &mut output, &mut emitted, false);
        }
        mark(reg, roots.view, &mut viewed, &mut HashSet::new(), false);
        for root in &roots.inputs {
            mark(reg, root, &mut input, &mut emitted, false);
        }
        for def in reg.types() {
            assert!(
                input.contains(def.name) || output.contains(def.name),
                "{} is reached from no op's input or answer",
                def.name
            );
        }
        let roles = reg
            .types()
            .map(|def| {
                let role = match (input.contains(def.name), viewed.contains(def.name)) {
                    (true, true) => Role::Shared,
                    (true, false) => Role::Input,
                    _ => Role::Output,
                };
                (def.name, role)
            })
            .collect();
        Gen {
            reg,
            roles,
            emitted,
            imports: BTreeSet::new(),
            used_defaults: HashSet::new(),
            option_as: Vec::new(),
        }
    }

    fn def(&self, name: &str) -> &'a TypeDef {
        self.reg
            .get(name)
            .unwrap_or_else(|| panic!("unregistered wire type {name}"))
    }

    fn import(&mut self, path: &'static str) {
        self.imports.insert(path);
    }

    fn file(&mut self) -> String {
        let mut body = String::new();
        self.constants(&mut body);
        self.import("kotlinx.serialization.json.Json");
        body.push_str(
            "/** The core's JSON: serde's shapes, unknown keys ignored as serde does. */\n\
             val coreJson = Json {\n    ignoreUnknownKeys = true\n    encodeDefaults = true\n}\n",
        );
        let types: Vec<&TypeDef> = self.reg.types().collect();
        for def in types {
            if !self.emitted.contains(def.name) {
                continue;
            }
            body.push('\n');
            match &def.body {
                Body::Struct(s) => self.struct_class(&mut body, def, s),
                Body::Enum {
                    variants,
                    predicates,
                } if variants.iter().all(|v| v.body == VariantBody::Unit) => {
                    self.unit_enum(&mut body, def, variants, predicates);
                }
                Body::Enum { variants, .. } => self.sealed(&mut body, def, variants),
                Body::Strings(s) => self.strings(&mut body, def, s),
            }
        }
        let option_as = std::mem::take(&mut self.option_as);
        for (inner, none) in option_as {
            body.push('\n');
            self.option_as_serializer(&mut body, inner, none);
        }
        let mut out = format!(
            "{HEADER}\n@file:OptIn(ExperimentalSerializationApi::class)\n\npackage app.milestone\n\n"
        );
        self.import("kotlinx.serialization.ExperimentalSerializationApi");
        for import in &self.imports {
            let _ = writeln!(out, "import {import}");
        }
        out.push('\n');
        out.push_str(&body);
        out
    }

    fn constants(&mut self, out: &mut String) {
        out.push_str("/** Rust constants the shell reads. */\nobject CoreConstants {\n");
        for c in constants() {
            doc(out, c.docs, "    ");
            match c.value {
                ConstValue::Int(v) => {
                    let _ = writeln!(out, "    const val {}: Int = {v}", c.name);
                }
                ConstValue::Float(v) => {
                    let _ = writeln!(out, "    const val {}: Double = {}", c.name, double(v));
                }
            }
        }
        out.push_str("}\n\n");
        out.push_str(
            "/** The muscles the core's hypertrophy landmarks cover, by the name a\n \
             * `PlanHypertrophyMeso` sends (`hypertrophy::LANDMARKS`). */\n\
             enum class HypertrophyMuscle(val wire: String) {\n",
        );
        let muscles: Vec<String> = hypertrophy_muscles()
            .map(|m| format!("    {}({})", pascal(m), string(m)))
            .collect();
        out.push_str(&muscles.join(",\n"));
        out.push_str(
            ";\n\n    companion object {\n        \
             /** The muscle for a wire name, matched case-insensitively as the core does; null if none. */\n        \
             fun of(wire: String?): HypertrophyMuscle? = entries.firstOrNull { it.wire.equals(wire, ignoreCase = true) }\n    \
             }\n}\n\n",
        );
    }

    fn role(&self, name: &str) -> Role {
        self.roles[name]
    }

    /// The fields a class declares for `s`: flattened structs inlined.
    fn flat_fields(&self, s: &'a Struct) -> Vec<&'a Field> {
        self.inlined(&s.fields)
    }

    /// `fields` with every flattened struct's fields inlined in its place.
    fn inlined(&self, fields: &'a [Field]) -> Vec<&'a Field> {
        let mut out = Vec::new();
        for f in fields {
            if f.flatten {
                let Shape::Named(name) = &f.shape else {
                    panic!("flattened {} is not a struct", f.name)
                };
                let Body::Struct(inner) = &self.def(name).body else {
                    panic!("flattened {name} is not a struct")
                };
                out.extend(self.flat_fields(inner));
            } else {
                out.push(f);
            }
        }
        out
    }

    fn struct_class(&mut self, out: &mut String, def: &TypeDef, s: &'a Struct) {
        let role = self.role(def.name);
        let fields = self.flat_fields(s);
        doc(out, def.docs, "");
        self.import("kotlinx.serialization.Serializable");
        let _ = writeln!(out, "@Serializable\ndata class {}(", def.name);
        for f in fields {
            self.property(out, def.name, f, role, false, "    ");
        }
        out.push(')');
        self.flattened_getters(out, def.name, s, role);
        out.push('\n');
    }

    /// `{ val <field>: <Struct> get() = <Struct>(…) }` for each flattened field
    /// of `s`, so a shell passes the flattened group as one value.
    fn flattened_getters(&self, out: &mut String, owner: &str, s: &'a Struct, role: Role) {
        let flat: Vec<&Field> = s.fields.iter().filter(|f| f.flatten).collect();
        if flat.is_empty() {
            return;
        }
        out.push_str(" {\n");
        for f in flat {
            let Shape::Named(inner) = &f.shape else {
                panic!("flattened {} is not a struct", f.name)
            };
            assert!(
                self.role(inner) == role,
                "{owner} flattens {inner}, which the shell reads or writes differently"
            );
            let Body::Struct(inner_struct) = &self.def(inner).body else {
                panic!("flattened {inner} is not a struct")
            };
            let args: Vec<String> = self
                .flat_fields(inner_struct)
                .iter()
                .map(|g| Self::name_in(g.name, role))
                .collect();
            let _ = writeln!(
                out,
                "    val {}: {inner}\n        get() = {inner}({})",
                Self::name_in(f.name, role),
                args.join(", ")
            );
        }
        out.push('}');
    }

    /// A field's Kotlin property name in a class of `role`: the shell's
    /// camelCase for the types only it writes, serde's names for the rest.
    fn name_in(name: &str, role: Role) -> String {
        if role == Role::Input {
            camel(name)
        } else {
            name.to_owned()
        }
    }

    /// One constructor property. `omit_defaults`: the shell omits it when it
    /// equals its default (an event payload's `payload_omits_defaults`).
    fn property(
        &mut self,
        out: &mut String,
        owner: &str,
        f: &Field,
        role: Role,
        omit_defaults: bool,
        indent: &str,
    ) {
        let name = Self::name_in(f.name, role);
        let ty = Self::kt_type(&f.shape, role.double_floats());
        let skipped = f.skip_if.map(|p| match p {
            "Option::is_none" => Value::Null,
            "std::ops::Not::not" => Value::Bool(false),
            "Vec::is_empty" => Value::Array(Vec::new()),
            other => panic!("{owner}.{}: unknown skip_serializing_if {other}", f.name),
        });
        let bound = DEFAULTS
            .iter()
            .find(|(o, field, _)| *o == owner && *field == f.name);
        let default = if let Some((o, field, expr)) = bound {
            self.used_defaults.insert((o, field));
            Some((*expr).to_owned())
        } else if let Some(v) = skipped.as_ref().or(f.default.as_ref()) {
            Some(self.value(&f.shape, v, role.double_floats()))
        } else if role != Role::Input {
            Some(self.zero(&f.shape, role.double_floats()))
        } else {
            None
        };
        if let (Some(skipped), Some(default)) = (&skipped, &default) {
            assert_eq!(
                *default,
                self.value(&f.shape, skipped, role.double_floats()),
                "{owner}.{}: a skipped field defaults to the value it skips",
                f.name
            );
        }
        doc(out, f.docs, indent);
        out.push_str(indent);
        if name != f.name {
            self.import("kotlinx.serialization.SerialName");
            let _ = write!(out, "@SerialName(\"{}\") ", f.name);
        }
        if skipped.is_some() || (omit_defaults && default.is_some()) {
            self.import("kotlinx.serialization.EncodeDefault");
            out.push_str("@EncodeDefault(EncodeDefault.Mode.NEVER) ");
        }
        if let Shape::OptionAs { inner, none } = &f.shape {
            let inner = named(inner);
            if !self.option_as.contains(&(inner, *none)) {
                self.option_as.push((inner, none));
            }
            self.import("kotlinx.serialization.Serializable");
            let _ = write!(out, "@Serializable(with = {inner}OrNoneSerializer::class) ");
        }
        let _ = write!(out, "val {}: {ty}", ident(&name));
        if let Some(default) = default {
            let _ = write!(out, " = {default}");
        }
        out.push_str(",\n");
    }

    fn kt_type(shape: &Shape, double_floats: bool) -> String {
        match shape {
            Shape::Bool => "Boolean".into(),
            Shape::Int(Int::U64 | Int::I64) => "Long".into(),
            Shape::Int(_) => "Int".into(),
            Shape::Float(Float::F32) if !double_floats => "Float".into(),
            Shape::Float(_) => "Double".into(),
            Shape::Str => "String".into(),
            Shape::Option(inner) => format!("{}?", Self::kt_type(inner, double_floats)),
            Shape::List(inner) => format!("List<{}>", Self::kt_type(inner, double_floats)),
            Shape::Named(name) => (*name).into(),
            Shape::OptionAs { inner, .. } => format!("{}?", named(inner)),
        }
    }

    /// `json` (a value of `shape`) as a Kotlin expression.
    fn value(&self, shape: &Shape, json: &Value, double_floats: bool) -> String {
        match (shape, json) {
            (Shape::Bool, Value::Bool(b)) => b.to_string(),
            (Shape::Int(Int::U64 | Int::I64), Value::Number(n)) => format!("{n}L"),
            (Shape::Int(_), Value::Number(n)) => n.to_string(),
            (Shape::Float(Float::F32), Value::Number(n)) if !double_floats => {
                format!("{}f", double(n.as_f64().expect("a float")))
            }
            (Shape::Float(_), Value::Number(n)) => double(n.as_f64().expect("a float")),
            (Shape::Str, Value::String(s)) => string(s),
            (Shape::Option(_), Value::Null) => "null".into(),
            (Shape::Option(inner), v) => self.value(inner, v, double_floats),
            (Shape::List(_), Value::Array(items)) if items.is_empty() => "emptyList()".into(),
            (Shape::List(inner), Value::Array(items)) => format!(
                "listOf({})",
                items
                    .iter()
                    .map(|v| self.value(inner, v, double_floats))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            (Shape::OptionAs { none, .. }, Value::String(s)) if s == none => "null".into(),
            (Shape::OptionAs { inner, .. }, v) => self.value(inner, v, double_floats),
            (Shape::Named(name), v) => self.named_value(name, v),
            (shape, v) => panic!("no Kotlin value of {shape:?} for {v}"),
        }
    }

    fn named_value(&self, name: &'static str, json: &Value) -> String {
        let def = self.def(name);
        match (&def.body, json) {
            (Body::Enum { variants, .. }, Value::String(tag)) => {
                let v = variants
                    .iter()
                    .find(|v| v.name == tag)
                    .unwrap_or_else(|| panic!("{name} has no variant {tag}"));
                format!("{name}.{}", v.ident)
            }
            (Body::Strings(s), Value::String(wire)) => {
                assert!(
                    s.entries.iter().any(|(w, _)| w == wire),
                    "{name} has no {wire}"
                );
                format!("{name}.{}", strings_entry(wire))
            }
            (Body::Struct(s), Value::Object(map)) => {
                let role = self.role(name);
                let args: Vec<String> = self
                    .flat_fields(s)
                    .into_iter()
                    .filter_map(|f| {
                        let zero = f
                            .default
                            .clone()
                            .unwrap_or_else(|| self.zero_json(&f.shape));
                        // A key a `skip_serializing_if` left out holds its default.
                        let v = map.get(f.name).unwrap_or(&zero);
                        (v != &zero).then(|| {
                            format!(
                                "{} = {}",
                                ident(f.name),
                                self.value(&f.shape, v, role.double_floats())
                            )
                        })
                    })
                    .collect();
                format!("{name}({})", args.join(", "))
            }
            (_, v) => panic!("no Kotlin value of {name} for {v}"),
        }
    }

    fn zero(&self, shape: &Shape, double_floats: bool) -> String {
        self.value(shape, &self.zero_json(shape), double_floats)
    }

    /// The JSON of a field's Kotlin default when serde gives none.
    fn zero_json(&self, shape: &Shape) -> Value {
        match shape {
            Shape::Bool => Value::Bool(false),
            Shape::Int(_) => Value::from(0),
            Shape::Float(_) => Value::from(0.0),
            Shape::Str => Value::from(""),
            Shape::Option(_) => Value::Null,
            Shape::List(_) => Value::Array(Vec::new()),
            Shape::OptionAs { none, .. } => Value::from(*none),
            Shape::Named(name) => match &self.def(name).body {
                Body::Enum { variants, .. } => Value::from(
                    variants
                        .iter()
                        .find(|v| v.default)
                        .unwrap_or(&variants[0])
                        .name,
                ),
                Body::Strings(s) => Value::from(s.default),
                Body::Struct(s) => Value::Object(
                    self.flat_fields(s)
                        .into_iter()
                        .map(|f| {
                            let v = f
                                .default
                                .clone()
                                .unwrap_or_else(|| self.zero_json(&f.shape));
                            (f.name.to_owned(), v)
                        })
                        .collect(),
                ),
            },
        }
    }

    fn unit_enum(
        &mut self,
        out: &mut String,
        def: &TypeDef,
        variants: &[Variant],
        predicates: &[&str],
    ) {
        doc(out, def.docs, "");
        self.import("kotlinx.serialization.Serializable");
        let other = variants.iter().any(|v| v.other);
        if other {
            let _ = writeln!(out, "@Serializable(with = {}Serializer::class)", def.name);
        } else {
            out.push_str("@Serializable\n");
        }
        let _ = writeln!(out, "enum class {} {{", def.name);
        for v in variants {
            doc(out, v.docs, "    ");
            if v.name != v.ident && !other {
                self.import("kotlinx.serialization.SerialName");
                let _ = writeln!(out, "    @SerialName(\"{}\")", v.name);
            }
            let _ = writeln!(out, "    {},", v.ident);
        }
        out.push_str("    ;\n\n    /** The wire string. */\n    val wire: String\n        get() = when (this) {\n");
        for v in variants {
            let _ = writeln!(out, "            {} -> {}", v.ident, string(v.name));
        }
        out.push_str("        }\n");
        for p in predicates {
            let on: Vec<String> = variants
                .iter()
                .filter(|v| v.flags.contains(p))
                .map(|v| v.ident.to_owned())
                .collect();
            let _ = write!(
                out,
                "\n    /** Rust's `{}::{p}`. */\n    val {}: Boolean\n        get() = ",
                def.name,
                camel(p)
            );
            if on.is_empty() {
                out.push_str("false\n");
            } else if on.len() == variants.len() {
                out.push_str("true\n");
            } else {
                let _ = writeln!(
                    out,
                    "when (this) {{\n            {} -> true\n            else -> false\n        }}",
                    on.join(", ")
                );
            }
        }
        out.push_str("}\n");
        if other {
            let fallback = variants.iter().find(|v| v.other).expect("an other variant");
            self.string_serializer(
                out,
                def.name,
                &format!(
                    "{}.entries.firstOrNull {{ it.wire == wire }} ?: {}.{}",
                    def.name, def.name, fallback.ident
                ),
            );
        }
    }

    /// A serializer object writing `T` as its `wire` string; `decode` is the
    /// expression reading `wire` back.
    fn string_serializer(&mut self, out: &mut String, name: &str, decode: &str) {
        for i in [
            "kotlinx.serialization.KSerializer",
            "kotlinx.serialization.descriptors.PrimitiveKind",
            "kotlinx.serialization.descriptors.PrimitiveSerialDescriptor",
            "kotlinx.serialization.encoding.Decoder",
            "kotlinx.serialization.encoding.Encoder",
        ] {
            self.import(i);
        }
        let _ = write!(
            out,
            "\ninternal object {name}Serializer : KSerializer<{name}> {{\n    \
             override val descriptor = PrimitiveSerialDescriptor(\"app.milestone.{name}\", PrimitiveKind.STRING)\n\n    \
             override fun serialize(encoder: Encoder, value: {name}) = encoder.encodeString(value.wire)\n\n    \
             override fun deserialize(decoder: Decoder): {name} {{\n        \
             val wire = decoder.decodeString()\n        \
             return {decode}\n    }}\n}}\n"
        );
    }

    fn option_as_serializer(&mut self, out: &mut String, inner: &str, none: &str) {
        for i in [
            "kotlinx.serialization.KSerializer",
            "kotlinx.serialization.SerializationException",
            "kotlinx.serialization.descriptors.PrimitiveKind",
            "kotlinx.serialization.descriptors.PrimitiveSerialDescriptor",
            "kotlinx.serialization.encoding.Decoder",
            "kotlinx.serialization.encoding.Encoder",
        ] {
            self.import(i);
        }
        let none = string(none);
        let _ = write!(
            out,
            "/** `{inner}?` written as the variant's wire string, or {none} for null. */\n\
             internal object {inner}OrNoneSerializer : KSerializer<{inner}?> {{\n    \
             override val descriptor = PrimitiveSerialDescriptor(\"app.milestone.{inner}OrNone\", PrimitiveKind.STRING)\n\n    \
             override fun serialize(encoder: Encoder, value: {inner}?) = encoder.encodeString(value?.wire ?: {none})\n\n    \
             override fun deserialize(decoder: Decoder): {inner}? {{\n        \
             val wire = decoder.decodeString()\n        \
             if (wire == {none}) return null\n        \
             return {inner}.entries.firstOrNull {{ it.wire == wire }}\n            \
             ?: throw SerializationException(\"unknown {inner} $wire\")\n    }}\n}}\n"
        );
    }

    fn strings(&mut self, out: &mut String, def: &TypeDef, s: &Strings) {
        let mut groups: Vec<&str> = Vec::new();
        for (_, g) in &s.entries {
            if !groups.contains(g) {
                groups.push(g);
            }
        }
        let _ = writeln!(
            out,
            "enum class {} {{ {} }}\n",
            s.group_type,
            groups.join(", ")
        );
        doc(out, def.docs, "");
        self.import("kotlinx.serialization.Serializable");
        self.import("kotlinx.serialization.SerialName");
        let _ = writeln!(
            out,
            "@Serializable\nenum class {}(val {}: {}) {{",
            def.name, s.group_property, s.group_type
        );
        for (wire, group) in &s.entries {
            let _ = writeln!(
                out,
                "    @SerialName({}) {}({}.{group}),",
                string(wire),
                strings_entry(wire),
                s.group_type
            );
        }
        out.push_str("    ;\n\n    /** The wire string. */\n    val wire: String\n        get() = when (this) {\n");
        for (wire, _) in &s.entries {
            let _ = writeln!(
                out,
                "            {} -> {}",
                strings_entry(wire),
                string(wire)
            );
        }
        out.push_str("        }\n}\n");
    }

    /// A data-carrying enum: a sealed interface, one class per variant, and a
    /// serializer for serde's external tagging.
    fn sealed(&mut self, out: &mut String, def: &TypeDef, variants: &'a [Variant]) {
        let name = def.name;
        for i in [
            "kotlinx.serialization.KSerializer",
            "kotlinx.serialization.Serializable",
            "kotlinx.serialization.SerializationException",
            "kotlinx.serialization.descriptors.SerialDescriptor",
            "kotlinx.serialization.encoding.Decoder",
            "kotlinx.serialization.encoding.Encoder",
            "kotlinx.serialization.json.JsonDecoder",
            "kotlinx.serialization.json.JsonElement",
            "kotlinx.serialization.json.JsonEncoder",
            "kotlinx.serialization.json.JsonObject",
            "kotlinx.serialization.json.JsonPrimitive",
        ] {
            self.import(i);
        }
        doc(out, def.docs, "");
        let _ = writeln!(
            out,
            "@Serializable(with = {name}Serializer::class)\nsealed interface {name} {{"
        );
        for v in variants {
            out.push('\n');
            doc(out, v.docs, "    ");
            let owner = format!("{name}::{}", v.ident);
            let owner = owner.as_str();
            match &v.body {
                VariantBody::Unit => {
                    let _ = write!(out, "    data object {}", v.ident);
                    let _ = write!(out, " : {name}");
                    out.push('\n');
                }
                VariantBody::Struct(fields) => {
                    let _ = writeln!(out, "    @Serializable\n    data class {}(", v.ident);
                    for f in self.inlined(fields) {
                        self.property(out, owner, f, Role::Input, false, "        ");
                    }
                    out.push_str("    )");
                    let _ = write!(out, " : {name}");
                    out.push('\n');
                }
                VariantBody::Newtype(Shape::Named(payload)) => {
                    let Body::Struct(s) = &self.def(payload).body else {
                        panic!("{owner}: a newtype variant carries a struct")
                    };
                    let mut fields = self.flat_fields(s);
                    if !s.payload_order.is_empty() {
                        fields.sort_by_key(|f| {
                            s.payload_order
                                .iter()
                                .position(|k| *k == f.name)
                                .expect("ordered")
                        });
                    }
                    let _ = writeln!(out, "    @Serializable\n    data class {}(", v.ident);
                    for f in fields {
                        self.property(
                            out,
                            owner,
                            f,
                            Role::Input,
                            s.payload_omits_defaults,
                            "        ",
                        );
                    }
                    out.push_str("    )");
                    let _ = write!(out, " : {name}");
                    out.push('\n');
                }
                VariantBody::Newtype(shape) => panic!("{owner}: unsupported newtype {shape:?}"),
            }
        }
        out.push_str("\n    companion object\n}\n\n");

        let units: Vec<&str> = variants
            .iter()
            .filter(|v| v.body == VariantBody::Unit)
            .map(|v| v.name)
            .collect();
        let payloads: Vec<&str> = variants
            .iter()
            .filter(|v| v.body != VariantBody::Unit)
            .map(|v| v.name)
            .collect();
        let _ = writeln!(
            out,
            "private val {name}UnitTags = {}\n\nprivate val {name}PayloadTags = {}\n",
            set_of(&units),
            set_of(&payloads)
        );
        let _ = writeln!(
            out,
            "/** The wire tag of `element` when it is a [{name}] in a shape serde accepts\n \
             * (a unit variant's bare string, a payload variant's single-key object), else null. */"
        );
        let _ = writeln!(
            out,
            "fun {name}.Companion.tagOf(element: JsonElement): String? = when (element) {{"
        );
        let _ = writeln!(
            out,
            "    is JsonPrimitive -> element.content.takeIf {{ element.isString && it in {name}UnitTags }}"
        );
        let _ = writeln!(
            out,
            "    is JsonObject -> element.keys.singleOrNull()?.takeIf {{ it in {name}PayloadTags && element[it] is JsonObject }}"
        );
        out.push_str("    else -> null\n}\n\n");

        let _ = writeln!(
            out,
            "/** [{name}] in serde's external tagging. */\n\
             internal object {name}Serializer : KSerializer<{name}> {{\n    \
             override val descriptor: SerialDescriptor = SerialDescriptor(\"app.milestone.{name}\", JsonElement.serializer().descriptor)\n"
        );
        out.push_str(&format!(
            "    override fun serialize(encoder: Encoder, value: {name}) {{\n        \
             val json = (encoder as JsonEncoder).json\n        \
             encoder.encodeJsonElement(\n            when (value) {{\n"
        ));
        for v in variants {
            if v.body == VariantBody::Unit {
                let _ = writeln!(
                    out,
                    "                {name}.{} -> JsonPrimitive({})",
                    v.ident,
                    string(v.name)
                );
            } else {
                let _ = writeln!(
                    out,
                    "                is {name}.{id} -> JsonObject(mapOf({tag} to json.encodeToJsonElement({name}.{id}.serializer(), value)))",
                    id = v.ident,
                    tag = string(v.name)
                );
            }
        }
        out.push_str("            },\n        )\n    }\n\n");
        let _ = write!(
            out,
            "    override fun deserialize(decoder: Decoder): {name} {{\n        \
             val input = decoder as JsonDecoder\n        \
             val element = input.decodeJsonElement()\n        \
             val tag = {name}.tagOf(element) ?: throw SerializationException(\"not a {name}: $element\")\n        \
             if (element is JsonPrimitive) {{\n            \
             return when (tag) {{\n"
        );
        for v in variants.iter().filter(|v| v.body == VariantBody::Unit) {
            let _ = writeln!(
                out,
                "                {} -> {name}.{}",
                string(v.name),
                v.ident
            );
        }
        let _ = write!(
            out,
            "                else -> throw SerializationException(\"not a {name}: $element\")\n            }}\n        }}\n        \
             val payload = (element as JsonObject).getValue(tag)\n        \
             return when (tag) {{\n"
        );
        for v in variants.iter().filter(|v| v.body != VariantBody::Unit) {
            let _ = writeln!(
                out,
                "            {} -> input.json.decodeFromJsonElement({name}.{}.serializer(), payload)",
                string(v.name),
                v.ident
            );
        }
        let _ = write!(
            out,
            "            else -> throw SerializationException(\"not a {name}: $element\")\n        }}\n    }}\n}}\n"
        );
    }
}

/// Mark `name` and everything it reaches. An event's newtype payload struct,
/// or the struct a struct variant flattens, is walked but gets no class of
/// its own unless reached another way.
fn mark(
    reg: &Registry,
    name: &'static str,
    into: &mut HashSet<&'static str>,
    emitted: &mut HashSet<&'static str>,
    inlined: bool,
) {
    if !inlined {
        emitted.insert(name);
    }
    if !into.insert(name) && !inlined {
        return;
    }
    let def = reg.get(name).expect("registered");
    match &def.body {
        Body::Struct(s) => {
            for f in &s.fields {
                match &f.shape {
                    Shape::Named(inner) if f.flatten => mark(reg, inner, into, emitted, false),
                    shape => mark_shape(reg, shape, into, emitted),
                }
            }
        }
        Body::Enum { variants, .. } => {
            for v in variants {
                match &v.body {
                    VariantBody::Unit => {}
                    VariantBody::Newtype(Shape::Named(payload)) => {
                        mark(reg, payload, into, emitted, true);
                    }
                    VariantBody::Newtype(shape) => mark_shape(reg, shape, into, emitted),
                    VariantBody::Struct(fields) => {
                        for f in fields {
                            match &f.shape {
                                Shape::Named(inner) if f.flatten => {
                                    mark(reg, inner, into, emitted, true);
                                }
                                shape => mark_shape(reg, shape, into, emitted),
                            }
                        }
                    }
                }
            }
        }
        Body::Strings(_) => {}
    }
}

fn mark_shape(
    reg: &Registry,
    shape: &Shape,
    into: &mut HashSet<&'static str>,
    emitted: &mut HashSet<&'static str>,
) {
    match shape {
        Shape::Option(inner) | Shape::List(inner) | Shape::OptionAs { inner, .. } => {
            mark_shape(reg, inner, into, emitted);
        }
        Shape::Named(name) => mark(reg, name, into, emitted, false),
        _ => {}
    }
}

/// The Kotlin type of a value the shell reads.
pub(crate) fn type_name(shape: &Shape) -> String {
    Gen::kt_type(shape, false)
}

fn named(shape: &Shape) -> &'static str {
    match shape {
        Shape::Named(name) => name,
        other => panic!("expected a named type, got {other:?}"),
    }
}

/// `"Lift(MaxEffort)"` → `LiftMaxEffort`.
fn strings_entry(wire: &str) -> String {
    wire.chars().filter(char::is_ascii_alphanumeric).collect()
}

fn set_of(items: &[&str]) -> String {
    if items.is_empty() {
        // An enum with no unit (or no payload) variant: Kotlin can't infer `setOf()`.
        return "emptySet<String>()".to_owned();
    }
    format!(
        "setOf({})",
        items
            .iter()
            .map(|s| string(s))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// `"side delts"` → `SideDelts`.
fn pascal(words: &str) -> String {
    words
        .split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect()
}

/// `snake_case` → `camelCase`.
fn camel(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

const KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

fn ident(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("`{name}`")
    } else {
        name.to_owned()
    }
}

fn double(v: f64) -> String {
    let s = format!("{v:?}");
    if s.contains(['.', 'e', 'E']) {
        s
    } else {
        format!("{s}.0")
    }
}

fn string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn doc(out: &mut String, docs: &str, indent: &str) {
    let docs = docs.trim();
    if docs.is_empty() {
        return;
    }
    let docs = docs.replace("*/", "*&#47;").replace("/*", "&#47;*");
    let lines: Vec<&str> = docs.lines().collect();
    if lines.len() == 1 {
        let _ = writeln!(out, "{indent}/** {} */", lines[0]);
        return;
    }
    let _ = writeln!(out, "{indent}/**");
    for line in lines {
        if line.is_empty() {
            let _ = writeln!(out, "{indent} *");
        } else {
            let _ = writeln!(out, "{indent} * {line}");
        }
    }
    let _ = writeln!(out, "{indent} */");
}
