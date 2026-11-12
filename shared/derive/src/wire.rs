//! `#[derive(Wire)]`: the type's serde wire shape as `crate::wire` data, read
//! off the same `#[serde(...)]` attributes serde reads. Attributes it can't
//! mirror (`untagged`, `tag`, `transparent`, `serialize_with`, …) are compile
//! errors, so a wire type never silently diverges from its description.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, Fields, Ident, Lit, LitStr, Path, Type,
    spanned::Spanned,
};

pub(crate) fn wire_impl(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    if input.generics.type_params().next().is_some()
        || input.generics.const_params().next().is_some()
    {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "Wire types are not generic over types",
        ));
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let container = Container::parse(&input.attrs)?;
    let wire_name = name.to_string();
    let type_docs = docs(&input.attrs);
    let body = match &input.data {
        Data::Struct(s) => {
            if !container.predicates.is_empty() {
                return Err(syn::Error::new_spanned(name, "predicates are for enums"));
            }
            let Fields::Named(named) = &s.fields else {
                return Err(syn::Error::new_spanned(
                    name,
                    "Wire structs have named fields",
                ));
            };
            let mut fields = Vec::new();
            let mut names = Vec::new();
            for field in &named.named {
                let attrs = FieldAttrs::parse(&field.attrs)?;
                if attrs.skip || attrs.read_only {
                    continue;
                }
                let ident = field.ident.as_ref().expect("named field");
                let key = attrs.wire_name(ident, container.rename_all.as_deref())?;
                if !attrs.flatten {
                    names.push(key.clone());
                }
                let default = if attrs.flatten {
                    quote! { ::core::option::Option::None }
                } else if container.default && attrs.default.is_none() {
                    let value = quote! { &__defaults.#ident };
                    attrs.default_json(value)
                } else {
                    field_default(&attrs, &field.ty)
                };
                fields.push(field_tokens(
                    &key,
                    &field.attrs,
                    &field.ty,
                    &attrs,
                    &default,
                ));
            }
            let order = &container.payload_order;
            for key in order {
                if !names.iter().any(|n| n == &key.value()) {
                    return Err(syn::Error::new_spanned(key, "no such wire field"));
                }
            }
            if !order.is_empty() && order.len() != names.len() {
                return Err(syn::Error::new_spanned(
                    name,
                    "payload_order must list every wire field once",
                ));
            }
            let omits = container.payload_omits_defaults;
            let prelude = container.default.then(|| {
                quote! { let __defaults = <Self as ::core::default::Default>::default(); }
            });
            quote! {
                #prelude
                crate::wire::Body::Struct(crate::wire::Struct {
                    fields: vec![#(#fields),*],
                    payload_order: vec![#(#order),*],
                    payload_omits_defaults: #omits,
                })
            }
        }
        Data::Enum(e) => {
            if container.default || !container.payload_order.is_empty() {
                return Err(syn::Error::new_spanned(
                    name,
                    "struct-only attribute on an enum",
                ));
            }
            let mut variants = Vec::new();
            for v in &e.variants {
                let attrs = VariantAttrs::parse(&v.attrs)?;
                let tag = match attrs.rename {
                    Some(r) => r.value(),
                    None => rename(&v.ident.to_string(), container.rename_all.as_deref(), v)?,
                };
                let vdocs = docs(&v.attrs);
                let other = attrs.other;
                let default = v.attrs.iter().any(|a| a.path().is_ident("default"));
                let body = match &v.fields {
                    Fields::Unit => quote! { crate::wire::VariantBody::Unit },
                    Fields::Unnamed(u) if u.unnamed.len() == 1 => {
                        let ty = &u.unnamed[0].ty;
                        quote! { crate::wire::VariantBody::Newtype(<#ty as crate::wire::Wire>::shape(reg)) }
                    }
                    Fields::Unnamed(u) => {
                        return Err(syn::Error::new_spanned(u, "tuple variants are not Wire"));
                    }
                    Fields::Named(named) => {
                        let mut fields = Vec::new();
                        for field in &named.named {
                            let fattrs = FieldAttrs::parse(&field.attrs)?;
                            if fattrs.skip {
                                return Err(syn::Error::new_spanned(
                                    field,
                                    "skip in a variant is not Wire",
                                ));
                            }
                            if fattrs.read_only {
                                continue;
                            }
                            let ident = field.ident.as_ref().expect("named field");
                            let key = fattrs.wire_name(ident, None)?;
                            let default = if fattrs.flatten {
                                quote! { ::core::option::Option::None }
                            } else {
                                field_default(&fattrs, &field.ty)
                            };
                            fields.push(field_tokens(
                                &key,
                                &field.attrs,
                                &field.ty,
                                &fattrs,
                                &default,
                            ));
                        }
                        quote! { crate::wire::VariantBody::Struct(vec![#(#fields),*]) }
                    }
                };
                let ident = v.ident.to_string();
                let var = &v.ident;
                let preds = &container.predicates;
                let flags = if preds.is_empty() {
                    quote! { vec![] }
                } else if matches!(v.fields, Fields::Unit) {
                    let names: Vec<String> = preds.iter().map(ToString::to_string).collect();
                    quote! {
                        [#((#names, Self::#var.#preds())),*]
                            .into_iter()
                            .filter_map(|(name, on)| on.then_some(name))
                            .collect()
                    }
                } else {
                    return Err(syn::Error::new_spanned(
                        v,
                        "predicates need a unit-only enum",
                    ));
                };
                variants.push(quote! {
                    crate::wire::Variant {
                        name: #tag,
                        ident: #ident,
                        docs: #vdocs,
                        body: #body,
                        other: #other,
                        default: #default,
                        flags: #flags,
                    }
                });
            }
            let names: Vec<String> = container
                .predicates
                .iter()
                .map(ToString::to_string)
                .collect();
            quote! {
                crate::wire::Body::Enum {
                    variants: vec![#(#variants),*],
                    predicates: vec![#(#names),*],
                }
            }
        }
        Data::Union(u) => {
            return Err(syn::Error::new(u.union_token.span, "unions are not Wire"));
        }
    };
    Ok(quote! {
        impl #impl_generics crate::wire::Wire for #name #ty_generics #where_clause {
            fn shape(reg: &mut crate::wire::Registry) -> crate::wire::Shape {
                reg.define::<Self>(#wire_name, #type_docs, |reg| { #body })
            }
        }
    })
}

/// One `crate::wire::Field` literal.
fn field_tokens(
    key: &str,
    attrs: &[Attribute],
    ty: &Type,
    fattrs: &FieldAttrs,
    default: &Tokens,
) -> Tokens {
    let docs = docs(attrs);
    let shape = match &fattrs.with {
        Some(module) => quote! { #module::wire_shape(reg) },
        None => quote! { <#ty as crate::wire::Wire>::shape(reg) },
    };
    let skip_if = match &fattrs.skip_if {
        Some(p) => quote! { ::core::option::Option::Some(#p) },
        None => quote! { ::core::option::Option::None },
    };
    let flatten = fattrs.flatten;
    quote! {
        crate::wire::Field {
            name: #key,
            docs: #docs,
            shape: #shape,
            default: #default,
            skip_if: #skip_if,
            flatten: #flatten,
        }
    }
}

/// The value serde fills in for an absent field, as an `Option<Value>` expression.
fn field_default(attrs: &FieldAttrs, ty: &Type) -> Tokens {
    match &attrs.default {
        Some(Default::Trait) => {
            attrs.default_json(quote! { &<#ty as ::core::default::Default>::default() })
        }
        Some(Default::Path(path)) => attrs.default_json(quote! { &#path() }),
        None if attrs.with.is_none() && is_option(ty) => {
            quote! { ::core::option::Option::Some(::serde_json::Value::Null) }
        }
        None => quote! { ::core::option::Option::None },
    }
}

fn is_option(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none()
        && p.path.segments.last().is_some_and(|s| s.ident == "Option"))
}

fn docs(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("doc")) {
        if let syn::Meta::NameValue(nv) = &attr.meta
            && let Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) = &nv.value
        {
            let line = s.value();
            lines.push(line.strip_prefix(' ').unwrap_or(&line).to_owned());
        }
    }
    lines.join("\n")
}

/// `ident` under serde's `rename_all` rule.
fn rename(ident: &str, rule: Option<&str>, at: &impl Spanned) -> syn::Result<String> {
    Ok(match rule {
        None => ident.to_owned(),
        Some("lowercase") => ident.to_ascii_lowercase(),
        Some("snake_case") => {
            let mut out = String::new();
            for (i, c) in ident.char_indices() {
                if c.is_uppercase() && i > 0 {
                    out.push('_');
                }
                out.push(c.to_ascii_lowercase());
            }
            out
        }
        Some(other) => {
            return Err(syn::Error::new(
                at.span(),
                format!("Wire does not mirror rename_all = {other:?}"),
            ));
        }
    })
}

fn str_value(meta: &syn::meta::ParseNestedMeta) -> syn::Result<LitStr> {
    meta.value()?.parse()
}

#[derive(Default)]
struct Container {
    rename_all: Option<String>,
    default: bool,
    payload_order: Vec<LitStr>,
    payload_omits_defaults: bool,
    predicates: Vec<Ident>,
}

impl Container {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut c = Container::default();
        for attr in attrs {
            if attr.path().is_ident("serde") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rename_all") {
                        c.rename_all = Some(str_value(&meta)?.value());
                    } else if meta.path.is_ident("default") && meta.input.is_empty() {
                        c.default = true;
                    } else {
                        return Err(meta.error("Wire does not mirror this serde attribute"));
                    }
                    Ok(())
                })?;
            } else if attr.path().is_ident("wire") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("payload_order") {
                        meta.parse_nested_meta(|key| {
                            let ident: &Ident = key
                                .path
                                .get_ident()
                                .ok_or_else(|| key.error("expected a field"))?;
                            c.payload_order
                                .push(LitStr::new(&ident.to_string(), ident.span()));
                            Ok(())
                        })
                    } else if meta.path.is_ident("payload_omits_defaults") {
                        c.payload_omits_defaults = true;
                        Ok(())
                    } else if meta.path.is_ident("predicates") {
                        meta.parse_nested_meta(|p| {
                            let ident = p.path.get_ident().ok_or_else(|| p.error("expected a method"))?;
                            c.predicates.push(ident.clone());
                            Ok(())
                        })
                    } else {
                        Err(meta.error(
                            "expected payload_order(...), payload_omits_defaults or predicates(...)",
                        ))
                    }
                })?;
            }
        }
        Ok(c)
    }
}

enum Default {
    Trait,
    Path(Path),
}

#[derive(Default)]
struct FieldAttrs {
    rename: Option<LitStr>,
    default: Option<Default>,
    skip: bool,
    /// `skip_serializing`: read from old lines, never written, so no part of
    /// the shape a shell reads or writes.
    read_only: bool,
    skip_if: Option<LitStr>,
    flatten: bool,
    with: Option<Path>,
}

impl FieldAttrs {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut f = FieldAttrs::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("serde")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    f.rename = Some(str_value(&meta)?);
                } else if meta.path.is_ident("default") {
                    f.default = Some(
                        if meta.input.is_empty() || meta.input.peek(syn::Token![,]) {
                            Default::Trait
                        } else {
                            Default::Path(str_value(&meta)?.parse()?)
                        },
                    );
                } else if meta.path.is_ident("skip") {
                    f.skip = true;
                } else if meta.path.is_ident("skip_serializing") {
                    f.read_only = true;
                } else if meta.path.is_ident("skip_serializing_if") {
                    f.skip_if = Some(str_value(&meta)?);
                } else if meta.path.is_ident("flatten") {
                    f.flatten = true;
                } else if meta.path.is_ident("with") {
                    f.with = Some(str_value(&meta)?.parse()?);
                } else {
                    return Err(meta.error("Wire does not mirror this serde attribute"));
                }
                Ok(())
            })?;
        }
        Ok(f)
    }

    fn wire_name(&self, ident: &Ident, rule: Option<&str>) -> syn::Result<String> {
        match &self.rename {
            Some(r) => Ok(r.value()),
            None => rename(
                &ident.to_string(),
                rule.filter(|r| *r != "snake_case"),
                ident,
            ),
        }
    }

    /// `Some(json)` of the default value `value` (an expression of `&T`),
    /// written the way the field is written.
    fn default_json(&self, value: Tokens) -> Tokens {
        match &self.with {
            Some(module) => quote! {
                ::core::option::Option::Some(crate::wire::json_with(#value, #module::serialize))
            },
            None => quote! { ::core::option::Option::Some(crate::wire::json(#value)) },
        }
    }
}

#[derive(Default)]
struct VariantAttrs {
    rename: Option<LitStr>,
    other: bool,
}

impl VariantAttrs {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut v = VariantAttrs::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("serde")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    v.rename = Some(str_value(&meta)?);
                } else if meta.path.is_ident("other") {
                    v.other = true;
                } else {
                    return Err(meta.error("Wire does not mirror this serde attribute"));
                }
                Ok(())
            })?;
        }
        Ok(v)
    }
}
