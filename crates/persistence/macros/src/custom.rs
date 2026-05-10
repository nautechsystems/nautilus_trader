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

//! The `#[custom_data]` procedural macro generates a complete custom data type implementation.
//!
//! # Overview
//!
//! Applied to a struct with named fields, the macro implements:
//! - [`CustomDataTrait`] (including `type_name_static`, `from_json` for JSON deserialization)
//! - [`HasTsInit`]
//! - [`ArrowSchemaProvider`], [`EncodeToRecordBatch`], [`DecodeDataFromRecordBatch`]
//! - [`CatalogPathPrefix`], `From<Self> for Data`, `TryFrom<Data>`
//! - `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]` on the struct
//!
//! Call [`nautilus_serialization::ensure_custom_data_registered::<T>()`] once per type for JSON
//! and Arrow registration; for Python bindings also call
//! [`nautilus_model::data::register_rust_extractor::<T>()`].
//!
//! # Requirements
//!
//! - Struct must have named fields
//! - Must include `ts_event` and `ts_init` fields (e.g. `nautilus_core::UnixNanos`)
//! - Supported field types: InstrumentId, AccountId, Currency, BarType, Params, UnixNanos, f64,
//!   f32, bool, String, u64, i64, u32, i32, `Vec<f64>`, `Vec<u8>`
//!
//! # Options
//!
//! - `#[custom_data(pyo3)]` or `#[custom_data(python)]`: Adds `#[pyclass]` and `#[pymethods]`
//!   with constructor and getters; Rust and Python both use constructor `new` (Python __init__ forwards to it).
//!   Python `__repr__` and `__str__` are generated to use the Rust `Display` implementation.
//! - `no_display`: Do not generate `repr()` or `Display`; the user may implement them manually.
//!
//! # Example
//!
//! ```ignore
//! #[custom_data(pyo3)]
//! pub struct MyCustomData {
//!     pub instrument_id: InstrumentId,
//!     pub value: f64,
//!     pub ts_event: UnixNanos,
//!     pub ts_init: UnixNanos,
//! }
//! ```
//! (The macro adds `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]`.)

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    Field, Fields, Ident, ItemStruct, LitStr, Token, Type,
    parse::{Parse, ParseStream},
    parse2,
};

/// Last path segment of a type (e.g. "InstrumentId", "UnixNanos", "f64").
fn type_last_segment(ty: &Type) -> Option<String> {
    let path = match ty {
        Type::Path(p) => &p.path,
        _ => return None,
    };
    path.segments.last().map(|s| s.ident.to_string())
}

/// Extracts inner type from Vec<T>, e.g. Vec<f64> -> f64, Vec<u8> -> u8.
fn vec_inner_type(ty: &Type) -> Option<&Type> {
    let path = match ty {
        Type::Path(p) => &p.path,
        _ => return None,
    };

    if path.segments.len() != 1 {
        return None;
    }
    let seg = path.segments.last()?;
    if seg.ident != "Vec" {
        return None;
    }
    let args = match &seg.arguments {
        syn::PathArguments::AngleBracketed(a) => &a.args,
        _ => return None,
    };

    if args.len() != 1 {
        return None;
    }

    match &args[0] {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    }
}

/// Returns (outer_type, inner_type) for Vec<T>: ("Vec", "f64") or ("Vec", "u8").
/// For non-Vec types, returns (seg, seg) where seg is the last path segment.
fn type_for_macro(ty: &Type) -> Option<(String, String)> {
    if let Some(inner) = vec_inner_type(ty) {
        let inner_seg = type_last_segment(inner)?;
        return Some(("Vec".to_string(), inner_seg));
    }
    let seg = type_last_segment(ty)?;
    Some((seg.clone(), seg))
}

/// Returns true if the type uses string extraction (Utf8 or Utf8View).
fn use_string_extract(ty: &Type) -> bool {
    if let Some((outer, inner)) = type_for_macro(ty) {
        matches!(
            (outer.as_str(), inner.as_str()),
            ("InstrumentId", "InstrumentId")
                | ("AccountId", "AccountId")
                | ("Currency", "Currency")
                | ("BarType", "BarType")
                | ("Params", "Params")
                | ("String", "String")
        )
    } else {
        false
    }
}

/// Arrow DataType and array type for encoding/decoding. Emits token streams that reference
/// arrow::datatypes::DataType and arrow array types.
fn arrow_type_for_rust_type(ty: &Type) -> Option<(TokenStream, TokenStream, TokenStream)> {
    let (outer, inner) = type_for_macro(ty)?;
    let (arrow_dt, array_type, extract_array_type): (TokenStream, TokenStream, TokenStream) = match (
        outer.as_str(),
        inner.as_str(),
    ) {
        ("Vec", "u8") => (
            quote! { arrow::datatypes::DataType::Binary },
            quote! { arrow::array::BinaryArray },
            quote! { arrow::array::BinaryArray },
        ),
        ("Vec", "f64") => (
            quote! { arrow::datatypes::DataType::List(std::sync::Arc::new(arrow::datatypes::Field::new("item", arrow::datatypes::DataType::Float64, true))) },
            quote! { arrow::array::ListArray },
            quote! { arrow::array::ListArray },
        ),
        _ if outer == inner => match outer.as_str() {
            "InstrumentId" | "AccountId" | "Currency" | "BarType" | "Params" => (
                quote! { arrow::datatypes::DataType::Utf8 },
                quote! { arrow::array::StringArray },
                quote! { arrow::array::StringArray },
            ),
            "UnixNanos" => (
                quote! { arrow::datatypes::DataType::UInt64 },
                quote! { arrow::array::UInt64Array },
                quote! { arrow::array::UInt64Array },
            ),
            "f64" => (
                quote! { arrow::datatypes::DataType::Float64 },
                quote! { arrow::array::Float64Array },
                quote! { arrow::array::Float64Array },
            ),
            "f32" => (
                quote! { arrow::datatypes::DataType::Float32 },
                quote! { arrow::array::Float32Array },
                quote! { arrow::array::Float32Array },
            ),
            "bool" => (
                quote! { arrow::datatypes::DataType::Boolean },
                quote! { arrow::array::BooleanArray },
                quote! { arrow::array::BooleanArray },
            ),
            "String" => (
                quote! { arrow::datatypes::DataType::Utf8 },
                quote! { arrow::array::StringArray },
                quote! { arrow::array::StringArray },
            ),
            "u64" | "u32" => (
                quote! { arrow::datatypes::DataType::UInt64 },
                quote! { arrow::array::UInt64Array },
                quote! { arrow::array::UInt64Array },
            ),
            "i64" => (
                quote! { arrow::datatypes::DataType::Int64 },
                quote! { arrow::array::Int64Array },
                quote! { arrow::array::Int64Array },
            ),
            "i32" => (
                quote! { arrow::datatypes::DataType::Int32 },
                quote! { arrow::array::Int32Array },
                quote! { arrow::array::Int32Array },
            ),
            _ => return None,
        },
        _ => return None,
    };
    Some((arrow_dt, array_type, extract_array_type))
}

/// How to encode a field value into an Arrow builder (append call).
fn encode_field_expr(field_name: &syn::Ident, ty: &Type) -> Option<TokenStream> {
    let (outer, inner) = type_for_macro(ty)?;
    let name = field_name;
    match (outer.as_str(), inner.as_str()) {
        ("Vec", "u8") => Some(quote! { builder.append_value(item.#name.as_slice()); }),
        ("Vec", "f64") => Some(quote! {
            for v in item.#name.iter() {
                builder.values().append_value(*v);
            }
            builder.append(true);
        }),
        _ if outer == inner => match outer.as_str() {
            "InstrumentId" | "AccountId" | "Currency" | "BarType" => {
                Some(quote! { builder.append_value(item.#name.to_string()); })
            }
            "Params" => Some(quote! {
                let value = serde_json::to_string(&item.#name).map_err(|e| {
                    arrow::error::ArrowError::InvalidArgumentError(
                        format!("failed to serialize Params field '{}': {e}", stringify!(#name)),
                    )
                })?;
                builder.append_value(value);
            }),
            "UnixNanos" => Some(quote! { builder.append_value(item.#name.as_u64()); }),
            "f64" | "f32" => Some(quote! { builder.append_value(item.#name); }),
            "bool" => Some(quote! { builder.append_value(item.#name); }),
            "String" => Some(quote! { builder.append_value(item.#name.as_str()); }),
            "u64" | "i64" => Some(quote! { builder.append_value(item.#name); }),
            "u32" => Some(quote! { builder.append_value(item.#name as u64); }),
            "i32" => Some(quote! { builder.append_value(item.#name); }),
            _ => None,
        },
        _ => None,
    }
}

/// RHS of a struct field when decoding from Arrow: uses col_ident.value(i) with optional conversion.
fn decode_field_rhs(
    field_name: &syn::Ident,
    ty: &Type,
    col_ident: &syn::Ident,
) -> Option<TokenStream> {
    let (outer, inner) = type_for_macro(ty)?;
    let name = field_name;
    let col = col_ident;
    match (outer.as_str(), inner.as_str()) {
        ("Vec", "u8") => Some(quote! { #col.value(i).to_vec() }),
        ("Vec", "f64") => Some(quote! {
            {
                let arr = #col.value(i);
                let float_arr = arr.as_any().downcast_ref::<arrow::array::Float64Array>()
                    .ok_or_else(|| nautilus_serialization::arrow::EncodingError::ParseError(
                        stringify!(#name),
                        format!("expected Float64Array for list element"),
                    ))?;
                (0..float_arr.len()).map(|j| float_arr.value(j)).collect::<Vec<f64>>()
            }
        }),
        _ if outer == inner => match outer.as_str() {
            "InstrumentId" | "AccountId" | "Currency" | "BarType" => Some(quote! {
                std::str::FromStr::from_str(#col.value(i)).map_err(|e| nautilus_serialization::arrow::EncodingError::ParseError(
                    stringify!(#name),
                    format!("expected valid identifier/type, was '{}'", e),
                ))?
            }),
            "Params" => Some(quote! {
                serde_json::from_str::<nautilus_core::Params>(#col.value(i)).map_err(|e| {
                    nautilus_serialization::arrow::EncodingError::ParseError(
                        stringify!(#name),
                        format!("row {i}: {e}"),
                    )
                })?
            }),
            "UnixNanos" => Some(quote! { #col.value(i).into() }),
            "f64" | "f32" | "bool" | "u64" | "i64" => Some(quote! { #col.value(i) }),
            "u32" => Some(quote! { #col.value(i) as u32 }),
            "i32" => Some(quote! { #col.value(i) }),
            "String" => Some(quote! { #col.value(i).to_string() }),
            _ => None,
        },
        _ => None,
    }
}

/// Builder type and initialisation for a field (e.g. StringBuilder::new() or Float64Array::builder(len)).
fn encode_builder_for_field(ty: &Type, len_var: &syn::Ident) -> Option<TokenStream> {
    let (outer, inner) = type_for_macro(ty)?;
    let len = len_var;

    match (outer.as_str(), inner.as_str()) {
        ("Vec", "u8") => Some(quote! { let mut builder = arrow::array::BinaryBuilder::new(); }),
        ("Vec", "f64") => Some(quote! {
            let mut builder = arrow::array::ListBuilder::new(arrow::array::Float64Builder::new());
        }),
        _ if outer == inner => match outer.as_str() {
            "InstrumentId" | "AccountId" | "Currency" | "BarType" | "Params" | "String" => {
                Some(quote! { let mut builder = arrow::array::StringBuilder::new(); })
            }
            "UnixNanos" | "u64" | "u32" => {
                Some(quote! { let mut builder = arrow::array::UInt64Array::builder(#len); })
            }
            "f64" => Some(quote! { let mut builder = arrow::array::Float64Array::builder(#len); }),
            "f32" => Some(quote! { let mut builder = arrow::array::Float32Array::builder(#len); }),
            "bool" => Some(quote! { let mut builder = arrow::array::BooleanArray::builder(#len); }),
            "i64" => Some(quote! { let mut builder = arrow::array::Int64Array::builder(#len); }),
            "i32" => Some(quote! { let mut builder = arrow::array::Int32Array::builder(#len); }),
            _ => None,
        },
        _ => None,
    }
}

/// Python constructor param type: UnixNanos -> u64, Params -> PyDict, Vec<u8> -> Vec<u8>, rest unchanged.
fn py_param_ty(ty: &Type) -> Option<TokenStream> {
    let (outer, inner) = type_for_macro(ty)?;
    if outer == "UnixNanos" {
        return Some(quote! { u64 });
    }

    if outer == inner && outer == "Params" {
        return Some(quote! { pyo3::Py<pyo3::types::PyDict> });
    }

    if outer == "Vec" && inner == "u8" {
        return Some(quote! { Vec<u8> });
    }

    if outer == "Vec" && inner == "f64" {
        return Some(quote! { Vec<f64> });
    }
    Some(quote! { #ty })
}

/// Python constructor body RHS: UnixNanos fields use arg.into(), rest use arg.
fn py_field_init(ident: &syn::Ident, ty: &Type) -> Option<TokenStream> {
    let (outer, inner) = type_for_macro(ty)?;
    let name = ident;

    if outer == "UnixNanos" {
        return Some(quote! { #name.into() });
    }

    if outer == inner && outer == "Params" {
        return Some(quote! {
            pyo3::Python::attach(|py| nautilus_core::from_pydict(py, #name))?.unwrap_or_default()
        });
    }
    Some(quote! { #name })
}

/// Python getter return type: UnixNanos -> u64, rest unchanged.
fn py_getter_ret_ty(ty: &Type) -> Option<TokenStream> {
    let (outer, inner) = type_for_macro(ty)?;

    if outer == "UnixNanos" {
        return Some(quote! { u64 });
    }

    if outer == inner && outer == "Params" {
        return Some(quote! { pyo3::PyResult<pyo3::Py<pyo3::types::PyDict>> });
    }
    Some(quote! { #ty })
}

/// Python getter body: UnixNanos -> self.x.as_u64(), Vec -> clone, String -> clone, rest -> self.x.
fn py_getter_body(ident: &syn::Ident, ty: &Type) -> Option<TokenStream> {
    let (outer, inner) = type_for_macro(ty)?;
    let name = ident;

    if outer == "UnixNanos" {
        return Some(quote! { self.#name.as_u64() });
    }

    if outer == inner && outer == "Params" {
        return Some(quote! { pyo3::Python::attach(|py| self.#name.to_pydict(py)) });
    }

    if outer == "Vec" || outer == "String" {
        return Some(quote! { self.#name.clone() });
    }
    Some(quote! { self.#name })
}

/// Finish the builder and wrap in Arc for RecordBatch::try_new columns.
fn encode_finish_builder(ty: &Type) -> Option<TokenStream> {
    let (outer, inner) = type_for_macro(ty)?;
    match (outer.as_str(), inner.as_str()) {
        ("Vec", "u8" | "f64") => Some(quote! { std::sync::Arc::new(builder.finish()) }),
        _ if outer == inner => match outer.as_str() {
            "InstrumentId" | "AccountId" | "Currency" | "BarType" | "Params" | "String" => {
                Some(quote! { std::sync::Arc::new(builder.finish()) })
            }
            "UnixNanos" | "u64" | "u32" | "f64" | "f32" | "bool" | "i64" | "i32" => {
                Some(quote! { std::sync::Arc::new(builder.finish()) })
            }
            _ => None,
        },
        _ => None,
    }
}

/// Parsed options from #[custom_data(...)] attribute.
struct CustomDataOptions {
    pyo3: bool,
    no_display: bool,
}

fn parse_option_ident(
    ident: &syn::Ident,
    options: &mut CustomDataOptions,
) -> Result<(), syn::Error> {
    let s = ident.to_string();
    match s.as_str() {
        "pyo3" | "python" => options.pyo3 = true,
        "no_display" => options.no_display = true,
        _ => {
            return Err(syn::Error::new_spanned(
                ident,
                "expected `pyo3`, `python`, or `no_display`; unknown option",
            ));
        }
    }
    Ok(())
}

struct OptionIdents {
    idents: Vec<Ident>,
}

impl Parse for OptionIdents {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut idents = Vec::new();
        idents.push(input.parse()?);
        while input.parse::<Option<Token![,]>>()?.is_some() {
            idents.push(input.parse()?);
        }
        Ok(Self { idents })
    }
}

/// Parse #[custom_data(pyo3)] or #[custom_data(pyo3, no_display)] etc.
fn parse_options(attr: &TokenStream) -> Result<CustomDataOptions, syn::Error> {
    let mut options = CustomDataOptions {
        pyo3: false,
        no_display: false,
    };
    let attr_str = attr.to_string();
    let attr_str = attr_str.trim();
    if attr_str.is_empty() {
        return Ok(options);
    }
    let option_idents: OptionIdents = parse2(attr.clone())?;
    for ident in &option_idents.idents {
        parse_option_ident(ident, &mut options)?;
    }
    Ok(options)
}

/// Context passed to each expansion generator for readability and testability.
struct ExpansionContext<'a> {
    name: &'a Ident,
    name_str: &'a str,
    generics: &'a syn::Generics,
    vis: &'a syn::Visibility,
    field_list: &'a [(Ident, Type)],
    options: &'a CustomDataOptions,
}

fn gen_new_fn(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    let vis = ctx.vis;
    let field_list = ctx.field_list;
    let (rust_ctor_name, rust_ctor_doc) = if ctx.options.pyo3 {
        (
            quote! { new },
            quote! { "Constructor from all fields. Use from Rust; Python __init__ forwards to this." },
        )
    } else {
        (quote! { new }, quote! { "Constructor." })
    };
    let constructor_params = field_list.iter().map(|(i, ty)| quote! { #i: #ty });
    let constructor_fields = field_list.iter().map(|(i, _)| quote! { #i });
    quote! {
        impl #generics #name #generics {
            #[allow(dead_code)]
            #[expect(clippy::too_many_arguments)]
            #[doc = #rust_ctor_doc]
            #vis fn #rust_ctor_name(#(#constructor_params),*) -> Self {
                Self { #(#constructor_fields),* }
            }
        }
    }
}

fn gen_repr_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    if ctx.options.no_display {
        return quote! {};
    }
    let name = ctx.name;
    let generics = ctx.generics;
    let name_str = ctx.name_str;
    let field_list = ctx.field_list;
    let repr_format_parts: Vec<String> = field_list
        .iter()
        .map(|(ident, _)| {
            let s = ident.to_string();
            if s == "ts_event" || s == "ts_init" {
                format!("{s}={{}}")
            } else {
                format!("{s}={{:?}}")
            }
        })
        .collect();
    let repr_format_str = format!("{}({})", name_str, repr_format_parts.join(", "));
    let repr_format_lit = LitStr::new(&repr_format_str, Span::call_site());
    let repr_args: Vec<TokenStream> = field_list
        .iter()
        .map(|(ident, _)| {
            let s = ident.to_string();
            if s == "ts_event" || s == "ts_init" {
                quote! { nautilus_core::datetime::unix_nanos_to_iso8601(self.#ident) }
            } else {
                quote! { self.#ident }
            }
        })
        .collect();
    quote! {
        impl #generics #name #generics {
            /// Returns a string representation in the same style as Python CustomDataClass (fields and ts_event/ts_init as ISO8601).
            pub fn repr(&self) -> String {
                format!(#repr_format_lit, #(#repr_args),*)
            }
        }
        impl #generics std::fmt::Display for #name #generics {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::write!(f, "{}", self.repr())
            }
        }
    }
}

fn gen_ts_init_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    quote! {
        impl #generics nautilus_model::data::HasTsInit for #name #generics {
            fn ts_init(&self) -> nautilus_core::UnixNanos {
                self.ts_init
            }
        }
    }
}

fn gen_custom_data_trait_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    let name_str = ctx.name_str;
    quote! {
        impl #generics nautilus_model::data::CustomDataTrait for #name #generics {
            fn type_name(&self) -> &'static str {
                    #name_str
                }
                fn type_name_static() -> &'static str {
                    #name_str
                }
                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }
                fn ts_event(&self) -> nautilus_core::UnixNanos {
                    self.ts_event
                }
                fn to_json(&self) -> anyhow::Result<String> {
                    serde_json::to_string(self).map_err(Into::into)
                }
                fn clone_arc(&self) -> std::sync::Arc<dyn nautilus_model::data::CustomDataTrait> {
                    std::sync::Arc::new(std::clone::Clone::clone(self))
                }
                fn eq_arc(&self, other: &dyn nautilus_model::data::CustomDataTrait) -> bool {
                    if let Some(other) = other.as_any().downcast_ref::<Self>() {
                        self == other
                    } else {
                        false
                    }
                }
                fn from_json(value: serde_json::Value) -> anyhow::Result<std::sync::Arc<dyn nautilus_model::data::CustomDataTrait>> {
                    let t: Self = serde_json::from_value(value)?;
                    Ok(std::sync::Arc::new(t))
                }
                #[cfg(feature = "python")]
                fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
                    nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
                }
        }
    }
}

fn gen_custom_data_serialize_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    let name_str = ctx.name_str;
    quote! {
        impl #generics nautilus_serialization::arrow::custom::CustomDataSerialize for #name #generics {
            fn schema(&self) -> anyhow::Result<arrow::datatypes::Schema> {
                Ok(<Self as nautilus_serialization::arrow::ArrowSchemaProvider>::get_schema(
                    Some(nautilus_serialization::arrow::EncodeToRecordBatch::metadata(self))
                ).into())
            }
            fn encode_record_batch(
                &self,
                items: &[std::sync::Arc<dyn nautilus_model::data::CustomDataTrait>],
            ) -> anyhow::Result<arrow::record_batch::RecordBatch> {
                let mut typed: Vec<Self> = Vec::with_capacity(items.len());
                for item in items {
                    if let Some(c) = item.as_any().downcast_ref::<Self>() {
                        typed.push(std::clone::Clone::clone(c));
                    } else {
                        anyhow::bail!("Expected {}, was different type", #name_str);
                    }
                }
                let metadata = nautilus_serialization::arrow::EncodeToRecordBatch::metadata(self);
                nautilus_serialization::arrow::EncodeToRecordBatch::encode_batch(&metadata, &typed).map_err(Into::into)
            }
        }
    }
}

fn gen_arrow_schema_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    let field_list = ctx.field_list;
    let arrow_schema_fields: Vec<TokenStream> = field_list
        .iter()
        .map(|(ident, ty)| {
            let (arrow_dt, _, _) = arrow_type_for_rust_type(ty).unwrap();
            let fn_str = ident.to_string();
            quote! {
                arrow::datatypes::Field::new(#fn_str, #arrow_dt, false)
            }
        })
        .collect();
    quote! {
        impl #generics nautilus_serialization::arrow::ArrowSchemaProvider for #name #generics {
            fn get_schema(metadata: Option<std::collections::HashMap<String, String>>) -> arrow::datatypes::Schema {
                let fields = vec![ #(#arrow_schema_fields),* ];
                match metadata {
                    Some(m) => arrow::datatypes::Schema::new_with_metadata(fields, m),
                    None => arrow::datatypes::Schema::new(fields),
                }
            }
        }
    }
}

fn gen_encode_batch_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    let name_str = ctx.name_str;
    let field_list = ctx.field_list;
    let len_var = format_ident!("data_len");
    let mut col_builds = Vec::new();
    let mut col_names = Vec::new();

    for (ident, ty) in field_list {
        let builder = encode_builder_for_field(ty, &len_var).unwrap();
        let append = encode_field_expr(ident, ty).unwrap();
        let finish = encode_finish_builder(ty).unwrap();
        let col_name = format_ident!("col_{}", col_builds.len());
        col_names.push(col_name.clone());
        col_builds.push(quote! {
            #builder

            for item in data {
                #append
            }
            let #col_name = #finish;
        });
    }
    let metadata_map = quote! {
        let mut m = std::collections::HashMap::new();
        m.insert("type_name".to_string(), #name_str.to_string());
        m
    };
    quote! {
        impl #generics nautilus_serialization::arrow::EncodeToRecordBatch for #name #generics {
            fn encode_batch(
                metadata: &std::collections::HashMap<String, String>,
                data: &[Self],
            ) -> std::result::Result<arrow::record_batch::RecordBatch, arrow::error::ArrowError> {
                let #len_var = data.len();
                #(#col_builds)*
                arrow::record_batch::RecordBatch::try_new(
                    <Self as nautilus_serialization::arrow::ArrowSchemaProvider>::get_schema(Some(metadata.clone())).into(),
                    vec![ #(#col_names),* ],
                )
            }
            fn metadata(&self) -> std::collections::HashMap<String, String> {
                #metadata_map
            }
        }
    }
}

fn gen_decode_batch_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    let field_list = ctx.field_list;
    let decode_row_fields: Vec<TokenStream> = field_list
        .iter()
        .enumerate()
        .map(|(idx, (ident, ty))| {
            let col_name = format_ident!("col_{}", idx);
            let rhs = decode_field_rhs(ident, ty, &col_name).unwrap();
            quote! { #ident: #rhs }
        })
        .collect();
    let decode_extracts: Vec<TokenStream> = field_list
        .iter()
        .enumerate()
        .map(|(idx, (ident, ty))| {
            let col_name = format_ident!("col_{}", idx);
            let fn_str = ident.to_string();

            if use_string_extract(ty) {
                quote! {
                    let #col_name = nautilus_serialization::arrow::extract_column_string(
                        record_batch.columns(),
                        #fn_str,
                        #idx,
                    )?;
                }
            } else {
                let (arrow_dt, _, array_ty) = arrow_type_for_rust_type(ty).unwrap();
                quote! {
                    let #col_name = nautilus_serialization::arrow::extract_column::<#array_ty>(
                        record_batch.columns(),
                        #fn_str,
                        #idx,
                        #arrow_dt,
                    )?;
                }
            }
        })
        .collect();
    quote! {
        impl #generics nautilus_serialization::arrow::DecodeDataFromRecordBatch for #name #generics {
            fn decode_data_batch(
                _metadata: &std::collections::HashMap<String, String>,
                record_batch: arrow::record_batch::RecordBatch,
            ) -> std::result::Result<Vec<nautilus_model::data::Data>, nautilus_serialization::arrow::EncodingError> {
                #(#decode_extracts)*
                let num_rows = record_batch.num_rows();
                let mut results = Vec::with_capacity(num_rows);
                for i in 0..num_rows {
                    let row = Self {
                        #(#decode_row_fields),*
                    };
                    results.push(nautilus_model::data::Data::Custom(nautilus_model::data::CustomData::from_arc(std::sync::Arc::new(row))));
                }
                Ok(results)
            }
        }
    }
}

fn gen_catalog_path_and_conversions(
    ctx: &ExpansionContext<'_>,
) -> (TokenStream, TokenStream, TokenStream) {
    let name = ctx.name;
    let generics = ctx.generics;
    let name_str = ctx.name_str;
    let catalog_path = format!("custom/{name_str}");
    let catalog_path_prefix_impl = quote! {
        impl #generics nautilus_model::data::CatalogPathPrefix for #name #generics {
            fn path_prefix() -> &'static str {
                #catalog_path
            }
        }
    };
    let from_impl = quote! {
        impl #generics std::convert::From<#name #generics> for nautilus_model::data::Data {
            fn from(value: #name #generics) -> Self {
                nautilus_model::data::Data::Custom(nautilus_model::data::CustomData::from_arc(std::sync::Arc::new(value)))
            }
        }
    };
    let try_from_impl = quote! {
        impl #generics std::convert::TryFrom<nautilus_model::data::Data> for #name #generics {
            type Error = anyhow::Error;
            fn try_from(value: nautilus_model::data::Data) -> std::result::Result<Self, Self::Error> {
                match value {
                    nautilus_model::data::Data::Custom(custom) => {
                        if let Some(c) = custom.data.as_any().downcast_ref::<Self>() {
                            Ok(std::clone::Clone::clone(c))
                        } else {
                            anyhow::bail!("Expected {}", #name_str)
                        }
                    }
                    _ => anyhow::bail!("Expected Custom data variant"),
                }
            }
        }
    };
    (catalog_path_prefix_impl, from_impl, try_from_impl)
}

fn gen_pymethods_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    let field_list = ctx.field_list;
    if !ctx.options.pyo3 {
        return quote! {};
    }
    let py_new_params: Vec<TokenStream> = field_list
        .iter()
        .map(|(ident, ty)| {
            let py_ty = py_param_ty(ty).unwrap();
            quote! { #ident: #py_ty }
        })
        .collect();
    let py_let_bindings: Vec<TokenStream> = field_list
        .iter()
        .map(|(ident, ty)| {
            let init = py_field_init(ident, ty).unwrap();
            quote! { let #ident = #init; }
        })
        .collect();
    let py_new_call_args: Vec<TokenStream> = field_list
        .iter()
        .map(|(ident, _)| quote! { #ident })
        .collect();
    let getters: Vec<TokenStream> = field_list
        .iter()
        .map(|(ident, ty)| {
            let ret_ty = py_getter_ret_ty(ty).unwrap();
            let body = py_getter_body(ident, ty).unwrap();
            quote! {
                #[getter]
                fn #ident(&self) -> #ret_ty {
                    #body
                }
            }
        })
        .collect();
    let repr_str_methods = if ctx.options.no_display {
        quote! {}
    } else {
        quote! {
            /// Python `repr()`: uses the Rust `Display` implementation.
            fn __repr__(&self) -> pyo3::PyResult<String> {
                Ok(std::fmt::format(std::format_args!("{}", self)))
            }

            /// Python `str()`: uses the Rust `Display` implementation.
            fn __str__(&self) -> pyo3::PyResult<String> {
                Ok(std::fmt::format(std::format_args!("{}", self)))
            }
        }
    };
    quote! {
        #[cfg(feature = "python")]
        use pyo3::prelude::*;
        /// PyO3 bindings (constructor, getters, to_json, from_json, record batch encode/decode). Only compiled when `feature = "python"`.
        #[cfg(feature = "python")]
        #[pyo3::pymethods]
        #[expect(clippy::needless_pass_by_value)]
        impl #generics #name #generics {
            #[expect(clippy::too_many_arguments)]
            #[new]
            #[pyo3(signature = (#(#py_new_call_args),*))]
            fn py_new(#(#py_new_params),*) -> pyo3::PyResult<Self> {
                #(#py_let_bindings)*
                Ok(Self::new(#(#py_new_call_args),*))
            }
            #(#getters)*

            #repr_str_methods

            /// Serializes to JSON string. Used by CustomData.to_json_bytes and PythonCustomDataWrapper.
            fn to_json(&self) -> pyo3::PyResult<String> {
                <#name as nautilus_model::data::CustomDataTrait>::to_json_py(self)
                    .map_err(nautilus_core::python::to_pyvalue_err)
            }

            /// Class method for JSON deserialization. Used by register_custom_data_class.
            #[classmethod]
            fn from_json(
                _cls: pyo3::Bound<'_, pyo3::types::PyType>,
                py: pyo3::Python<'_>,
                data: &pyo3::Bound<'_, pyo3::PyAny>,
            ) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
                let json_module = py.import("json")
                    .map_err(|e| nautilus_core::python::to_pyvalue_err(format!("import json failed: {e}")))?;
                let json_str: String = json_module
                    .call_method1("dumps", (data,))
                    .map_err(|e| nautilus_core::python::to_pyvalue_err(format!("json.dumps failed: {e}")))?
                    .extract()?;
                let value: serde_json::Value = serde_json::from_str(&json_str)
                    .map_err(|e| nautilus_core::python::to_pyvalue_err(format!("serde_json::from_str failed: {e}")))?;
                let arc = <#name as nautilus_model::data::CustomDataTrait>::from_json(value)
                    .map_err(nautilus_core::python::to_pyvalue_err)?;
                let inner = arc.as_any().downcast_ref::<#name>()
                    .ok_or_else(|| nautilus_core::python::to_pyvalue_err("from_json downcast failed"))?;
                Ok(pyo3::Py::new(py, inner.clone())?.into_any())
            }

            /// Decodes a RecordBatch from a PyArrow batch into a list of instances.
            /// Class method: call via MarketTickData.decode_record_batch_py(metadata, batch).
            #[pyo3(signature = (metadata, py_batch))]
            #[classmethod]
            fn decode_record_batch_py(
                _cls: pyo3::Bound<'_, pyo3::types::PyType>,
                py: pyo3::Python<'_>,
                metadata: std::collections::HashMap<String, String>,
                py_batch: &pyo3::Bound<'_, pyo3::PyAny>,
            ) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
                let mut ffi_array = arrow::ffi::FFI_ArrowArray::empty();
                let mut ffi_schema = arrow::ffi::FFI_ArrowSchema::empty();

                py_batch.call_method1("_export_to_c", (
                    (&raw mut ffi_array as usize),
                    (&raw mut ffi_schema as usize)
                ))?;

                let schema = std::sync::Arc::new(arrow::datatypes::Schema::try_from(&ffi_schema).map_err(nautilus_core::python::to_pyvalue_err)?);
                let struct_array_data = unsafe { arrow::ffi::from_ffi_and_data_type(ffi_array, arrow::datatypes::DataType::Struct(schema.fields().clone())).map_err(nautilus_core::python::to_pyvalue_err)? };
                let struct_array = arrow::array::StructArray::from(struct_array_data);
                let batch = arrow::record_batch::RecordBatch::from(&struct_array);

                let data_list = <#name as nautilus_serialization::arrow::DecodeDataFromRecordBatch>::decode_data_batch(
                    &metadata,
                    batch,
                ).map_err(nautilus_core::python::to_pyvalue_err)?;
                let mut py_items = Vec::new();

                for d in data_list {
                    if let nautilus_model::data::Data::Custom(custom) = d {
                        if let Some(m) = custom.data.as_any().downcast_ref::<#name>() {
                            py_items.push(pyo3::Py::new(py, m.clone())?.into_any());
                        }
                    }
                }
                let list = pyo3::types::PyList::new(py, py_items)?;
                Ok(list.into_any().unbind())
            }

            /// Encodes a batch of items to an Arrow RecordBatch. Returns a PyArrow RecordBatch
            /// using zero-copy C Data interface.
            fn encode_record_batch_py(
                &self,
                py: pyo3::Python<'_>,
                items: &pyo3::Bound<'_, pyo3::types::PyList>,
            ) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
                use std::collections::HashMap;
                let typed: Vec<#name> = items
                    .iter()
                    .map(|obj| obj.extract::<#name>().map_err(|e| e.into()))
                    .collect::<pyo3::PyResult<Vec<_>>>()?;
                let metadata = <#name as nautilus_serialization::arrow::EncodeToRecordBatch>::metadata(self);
                let batch = <#name as nautilus_serialization::arrow::EncodeToRecordBatch>::encode_batch(
                    &metadata,
                    &typed,
                ).map_err(nautilus_core::python::to_pyvalue_err)?;

                let struct_array: arrow::array::StructArray = batch.clone().into();
                let array_data = arrow::array::Array::to_data(&struct_array);
                let mut ffi_array = arrow::ffi::FFI_ArrowArray::new(&array_data);
                let mut ffi_schema = arrow::ffi::FFI_ArrowSchema::try_from(arrow::datatypes::DataType::Struct(batch.schema().fields().clone())).map_err(nautilus_core::python::to_pyvalue_err)?;

                let pyarrow = py.import("pyarrow")?;
                let cls = pyarrow.getattr("RecordBatch")?;
                let py_batch = cls.call_method1("_import_from_c", (
                    (&raw mut ffi_array as usize),
                    (&raw mut ffi_schema as usize)
                ))?;

                Ok(py_batch.into_any().unbind())
            }
        }
    }
}

#[expect(clippy::needless_pass_by_value)]
pub fn expand_custom_data(attr: TokenStream, item: TokenStream) -> TokenStream {
    let options = match parse_options(&attr) {
        Ok(o) => o,
        Err(e) => return e.to_compile_error(),
    };

    let input: ItemStruct = match parse2(item) {
        Ok(i) => i,
        Err(e) => return e.to_compile_error(),
    };

    let name = &input.ident;
    let name_str = name.to_string();
    let vis = &input.vis;
    let generics = &input.generics;

    let fields = match &input.fields {
        Fields::Named(n) => &n.named,
        _ => {
            return syn::Error::new_spanned(
                input,
                "#[custom_data] requires a struct with named fields",
            )
            .to_compile_error();
        }
    };

    let field_list: Vec<_> = fields
        .iter()
        .map(|f| {
            let ident = f.ident.as_ref().expect("named field");
            let ty = &f.ty;
            (ident.clone(), ty.clone())
        })
        .collect();

    for (ident, ty) in &field_list {
        if arrow_type_for_rust_type(ty).is_none() {
            return syn::Error::new_spanned(
                ty,
                format!(
                    "#[custom_data] does not support field type for '{ident}'; supported: InstrumentId, AccountId, Currency, BarType, Params, UnixNanos, f64, f32, bool, String, u64, i64, u32, i32, Vec<f64>, Vec<u8>"
                ),
            )
            .to_compile_error();
        }
    }

    let ts_init_field = field_list
        .iter()
        .find(|(i, _)| *i == "ts_init")
        .map(|(i, _)| i);
    let ts_event_field = field_list
        .iter()
        .find(|(i, _)| *i == "ts_event")
        .map(|(i, _)| i);

    if ts_init_field.is_none() || ts_event_field.is_none() {
        return syn::Error::new_spanned(
            input,
            "#[custom_data] requires fields ts_event and ts_init (e.g. nautilus_core::UnixNanos)",
        )
        .to_compile_error();
    }

    let ctx = ExpansionContext {
        name,
        name_str: &name_str,
        generics,
        vis,
        field_list: &field_list,
        options: &options,
    };

    let new_fn = gen_new_fn(&ctx);
    let repr_impl = gen_repr_impl(&ctx);
    let ts_init_impl = gen_ts_init_impl(&ctx);
    let custom_data_trait_impl = gen_custom_data_trait_impl(&ctx);
    let custom_data_serialize_impl = gen_custom_data_serialize_impl(&ctx);
    let arrow_schema_impl = gen_arrow_schema_impl(&ctx);
    let encode_batch_impl = gen_encode_batch_impl(&ctx);
    let decode_batch_impl = gen_decode_batch_impl(&ctx);
    let (catalog_path_prefix_impl, from_impl, try_from_impl) =
        gen_catalog_path_and_conversions(&ctx);
    let pymethods_impl = gen_pymethods_impl(&ctx);

    let struct_attrs: Vec<syn::Attribute> = input
        .attrs
        .iter()
        .filter(|a| a.path().get_ident().is_none_or(|i| *i != "custom_data"))
        .cloned()
        .collect();
    let pyclass_attr_ts: TokenStream = if options.pyo3 {
        quote! {
            #[cfg_attr(feature = "python", pyo3::pyclass(from_py_object))]
        }
    } else {
        quote! {}
    };
    let fields_vec: Vec<&Field> = fields.iter().collect();

    let derived_attr = quote! {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
    };
    quote! {
        #derived_attr
        #(#struct_attrs)*
        #pyclass_attr_ts
        #vis struct #name #generics {
            #(#fields_vec),*
        }

        #new_fn
        #repr_impl
        #ts_init_impl
        #custom_data_trait_impl
        #custom_data_serialize_impl
        #arrow_schema_impl
        #encode_batch_impl
        #decode_batch_impl
        #catalog_path_prefix_impl
        #from_impl
        #try_from_impl
        #pymethods_impl
    }
}
