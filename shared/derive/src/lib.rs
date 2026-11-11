//! Derives for the `shared` core's wire input types. The expansions name
//! `crate::…` paths, so they are only usable inside `shared`.
//!
//! - `#[derive(Sanitize)]` implements `crate::sanitize::Sanitize` by
//!   sanitizing every field (every variant's fields for an enum) with its own
//!   type's impl, or with `#[sanitize_with = "path"]`, a `fn(&mut T)` (not
//!   `sanitize`, which is a rustc builtin attribute on nightly toolchains).
//! - `#[derive(ReviewParts)]` generates `take_session_fields` for a review
//!   payload: every field must say which store it belongs to with
//!   `#[review(session)]`, `#[review(weekly)]` or `#[review(stamp)]`.
//! - `#[derive(Wire)]` implements `crate::wire::Wire`: the type's serde wire
//!   shape as data, for `shared/wiregen` (see [`wire`]).

mod wire;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{Attribute, Data, DeriveInput, Fields, Ident, Path, parse_macro_input};

#[proc_macro_derive(Sanitize, attributes(sanitize_with))]
pub fn derive_sanitize(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    sanitize_impl(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(ReviewParts, attributes(review))]
pub fn derive_review_parts(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    review_parts_impl(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Wire, attributes(wire))]
pub fn derive_wire(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    wire::wire_impl(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn sanitize_impl(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let body = match &input.data {
        Data::Struct(s) => {
            let (pattern, stmts) = sanitize_fields(&s.fields)?;
            quote! { match self { Self #pattern => { #(#stmts)* } } }
        }
        Data::Enum(e) if e.variants.is_empty() => quote! { match *self {} },
        Data::Enum(e) => {
            let mut arms = Vec::new();
            for v in &e.variants {
                let var = &v.ident;
                let (pattern, stmts) = sanitize_fields(&v.fields)?;
                arms.push(quote! { Self::#var #pattern => { #(#stmts)* } });
            }
            quote! {
                #[allow(clippy::match_same_arms)]
                match self { #(#arms),* }
            }
        }
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span,
                "Sanitize cannot be derived for a union",
            ));
        }
    };
    Ok(quote! {
        impl #impl_generics crate::sanitize::Sanitize for #name #ty_generics #where_clause {
            fn sanitize(&mut self) {
                #body
            }
        }
    })
}

/// The destructuring pattern binding every field, and one sanitize statement
/// per binding.
fn sanitize_fields(fields: &Fields) -> syn::Result<(Tokens, Vec<Tokens>)> {
    let bindings: Vec<Ident> = match fields {
        Fields::Named(named) => named.named.iter().filter_map(|f| f.ident.clone()).collect(),
        Fields::Unnamed(unnamed) => (0..unnamed.unnamed.len())
            .map(|i| format_ident!("f{i}"))
            .collect(),
        Fields::Unit => Vec::new(),
    };
    let pattern = match fields {
        Fields::Named(_) => quote! { { #(#bindings),* } },
        Fields::Unnamed(_) => quote! { ( #(#bindings),* ) },
        Fields::Unit => quote! {},
    };
    let mut stmts = Vec::with_capacity(bindings.len());
    for (field, binding) in fields.iter().zip(&bindings) {
        stmts.push(match sanitize_with(&field.attrs)? {
            Some(path) => quote! { #path(#binding); },
            None => quote! { crate::sanitize::Sanitize::sanitize(#binding); },
        });
    }
    Ok((pattern, stmts))
}

/// The `fn(&mut T)` a `#[sanitize_with = "path"]` attribute names.
fn sanitize_with(attrs: &[Attribute]) -> syn::Result<Option<Path>> {
    let mut with = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("sanitize_with")) {
        let value = &attr.meta.require_name_value()?.value;
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(path),
            ..
        }) = value
        else {
            return Err(syn::Error::new_spanned(value, "expected \"path\""));
        };
        with = Some(path.parse::<Path>()?);
    }
    Ok(with)
}

#[derive(PartialEq, Eq)]
enum Part {
    Session,
    Weekly,
    Stamp,
}

fn review_parts_impl(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let Data::Struct(s) = &input.data else {
        return Err(syn::Error::new_spanned(name, "ReviewParts needs a struct"));
    };
    let Fields::Named(named) = &s.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "ReviewParts needs named fields",
        ));
    };
    let mut session = Vec::new();
    for field in &named.named {
        let mut part = None;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("review")) {
            attr.parse_nested_meta(|meta| {
                part = Some(if meta.path.is_ident("session") {
                    Part::Session
                } else if meta.path.is_ident("weekly") {
                    Part::Weekly
                } else if meta.path.is_ident("stamp") {
                    Part::Stamp
                } else {
                    return Err(meta.error("expected `session`, `weekly` or `stamp`"));
                });
                Ok(())
            })?;
        }
        match part {
            Some(Part::Session) => session.extend(field.ident.clone()),
            Some(Part::Weekly | Part::Stamp) => {}
            None => {
                return Err(syn::Error::new_spanned(
                    field,
                    "every review field needs #[review(session)], #[review(weekly)] or #[review(stamp)]",
                ));
            }
        }
    }
    Ok(quote! {
        impl #name {
            /// Copy every `#[review(session)]` field of `src` into `self`.
            pub(crate) fn take_session_fields(&mut self, src: &Self) {
                #( self.#session = ::core::clone::Clone::clone(&src.#session); )*
            }
        }
    })
}
