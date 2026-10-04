// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Shared `syn` parsing and type inspection utilities for Nautilus procedural macros.

#![warn(clippy::pedantic)]

use syn::{
    Field, Ident, LitStr, Token, Type,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

/// Returns the path of a `Type::Path`, or `None` for other type kinds.
#[must_use]
pub(crate) fn type_path(ty: &Type) -> Option<&syn::Path> {
    match ty {
        Type::Path(path) => Some(&path.path),
        _ => None,
    }
}

/// Returns the last path segment of a type, such as `InstrumentId`, `UnixNanos`, or `f64`.
#[must_use]
pub(crate) fn type_last_segment(ty: &Type) -> Option<String> {
    type_path(ty)?
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

/// Extracts the inner type from `Vec<T>`, e.g. `Vec<f64>` -> `f64`, `Vec<u8>` -> `u8`.
#[must_use]
pub(crate) fn vec_inner_type(ty: &Type) -> Option<&Type> {
    let path = type_path(ty)?;
    if path.segments.len() != 1 {
        return None;
    }

    let segment = path.segments.last()?;
    if segment.ident != "Vec" {
        return None;
    }

    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };

    if args.args.len() != 1 {
        return None;
    }

    match &args.args[0] {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    }
}

/// Returns `(outer_type, inner_type)` for `Vec<T>`: `("Vec", "f64")` or `("Vec", "u8")`.
/// For non-`Vec` types, returns `(seg, seg)` where `seg` is the last path segment.
#[must_use]
pub(crate) fn type_for_macro(ty: &Type) -> Option<(String, String)> {
    if let Some(inner) = vec_inner_type(ty) {
        return Some(("Vec".to_string(), type_last_segment(inner)?));
    }

    let segment = type_last_segment(ty)?;
    Some((segment.clone(), segment))
}

/// Options parsed from `#[custom_data_field(...)]` attributes.
#[derive(Clone, Copy, Default)]
pub(crate) struct FieldOptions {
    /// Whether the field uses generic Serde handling.
    pub serde: bool,
    /// Whether the field is a native enum encoded as its display name.
    pub native_enum: bool,
}

/// A named struct field with its parsed `#[custom_data_field(...)]` options.
pub(crate) struct FieldSpec {
    /// The field identifier.
    pub ident: Ident,
    /// The field type.
    pub ty: Type,
    /// The parsed field options.
    pub options: FieldOptions,
}

/// A single macro option of the form `ident` or `ident = "value"`.
pub(crate) struct CustomDataOption {
    /// The option name.
    pub ident: Ident,
    /// The optional string value.
    pub value: Option<LitStr>,
}

impl Parse for CustomDataOption {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident = input.parse()?;
        let value = if input.parse::<Option<Token![=]>>()?.is_some() {
            Some(input.parse()?)
        } else {
            None
        };
        Ok(Self { ident, value })
    }
}

/// Parses `#[custom_data_field(...)]` options from a struct field.
///
/// # Errors
///
/// Returns an error if an attribute fails to parse or contains an unknown option.
pub(crate) fn parse_field_options(field: &Field) -> Result<FieldOptions, syn::Error> {
    let mut options = FieldOptions::default();

    for attr in field.attrs.iter().filter(|attr| {
        attr.path()
            .get_ident()
            .is_some_and(|ident| ident == "custom_data_field")
    }) {
        for ident in attr.parse_args_with(Punctuated::<Ident, Token![,]>::parse_terminated)? {
            if ident == "serde" {
                options.serde = true;
            } else if ident == "native_enum" {
                options.native_enum = true;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `serde` or `native_enum`; unknown field option",
                ));
            }
        }
    }
    Ok(options)
}
