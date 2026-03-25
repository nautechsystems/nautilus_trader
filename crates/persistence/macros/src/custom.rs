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
//! - Supported field types: InstrumentId, AccountId, Currency, BarType, UnixNanos, f64, f32, bool,
//!   String, u64, i64, u32, i32, `Vec<f64>`, `Vec<u8>`
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

use proc_macro2::Span;
use zyn::{
    Arg, Args, Output, format_ident,
    syn::{self, Field, Fields, Ident, ItemStruct, LitStr, Type},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    InstrumentId,
    AccountId,
    Currency,
    BarType,
    UnixNanos,
    F64,
    F32,
    Bool,
    String,
    U64,
    I64,
    U32,
    I32,
    VecU8,
    VecF64,
}

impl FieldKind {
    fn uses_string_extract(self) -> bool {
        matches!(
            self,
            Self::InstrumentId | Self::AccountId | Self::Currency | Self::BarType | Self::String
        )
    }
}

fn classify_field_kind(ty: &Type) -> Option<FieldKind> {
    let (outer, inner) = type_for_macro(ty)?;
    match (outer.as_str(), inner.as_str()) {
        ("InstrumentId", "InstrumentId") => Some(FieldKind::InstrumentId),
        ("AccountId", "AccountId") => Some(FieldKind::AccountId),
        ("Currency", "Currency") => Some(FieldKind::Currency),
        ("BarType", "BarType") => Some(FieldKind::BarType),
        ("UnixNanos", "UnixNanos") => Some(FieldKind::UnixNanos),
        ("f64", "f64") => Some(FieldKind::F64),
        ("f32", "f32") => Some(FieldKind::F32),
        ("bool", "bool") => Some(FieldKind::Bool),
        ("String", "String") => Some(FieldKind::String),
        ("u64", "u64") => Some(FieldKind::U64),
        ("i64", "i64") => Some(FieldKind::I64),
        ("u32", "u32") => Some(FieldKind::U32),
        ("i32", "i32") => Some(FieldKind::I32),
        ("Vec", "u8") => Some(FieldKind::VecU8),
        ("Vec", "f64") => Some(FieldKind::VecF64),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct FieldSpec {
    ident: Ident,
    ty: Type,
    kind: FieldKind,
}

impl FieldSpec {
    fn parse(field: &Field) -> Option<Self> {
        Some(Self {
            ident: field.ident.as_ref()?.clone(),
            ty: field.ty.clone(),
            kind: classify_field_kind(&field.ty)?,
        })
    }

    fn is_ts_field(&self) -> bool {
        self.ident == "ts_event" || self.ident == "ts_init"
    }

    fn name_str(&self) -> String {
        self.ident.to_string()
    }
}

/// Parsed options from #[custom_data(...)] attribute.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CustomDataOptions {
    pyo3: bool,
    no_display: bool,
}

fn parse_options(args: &Args) -> syn::Result<CustomDataOptions> {
    let mut options = CustomDataOptions::default();

    for arg in args {
        match arg {
            Arg::Flag(ident) if ident == "pyo3" || ident == "python" => {
                options.pyo3 = true;
            }
            Arg::Flag(ident) if ident == "no_display" => {
                options.no_display = true;
            }
            Arg::Flag(ident) => {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `pyo3`, `python`, or `no_display`; unknown option",
                ));
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    arg,
                    "expected bare flags `pyo3`, `python`, or `no_display`",
                ));
            }
        }
    }

    Ok(options)
}

fn arrow_data_type(kind: FieldKind) -> Output {
    match kind {
        FieldKind::VecU8 => zyn::zyn!(arrow::datatypes::DataType::Binary),
        FieldKind::VecF64 => zyn::zyn!(arrow::datatypes::DataType::List(std::sync::Arc::new(
            arrow::datatypes::Field::new("item", arrow::datatypes::DataType::Float64, true,),
        ))),
        FieldKind::InstrumentId
        | FieldKind::AccountId
        | FieldKind::Currency
        | FieldKind::BarType
        | FieldKind::String => zyn::zyn!(arrow::datatypes::DataType::Utf8),
        FieldKind::UnixNanos | FieldKind::U64 | FieldKind::U32 => {
            zyn::zyn!(arrow::datatypes::DataType::UInt64)
        }
        FieldKind::F64 => zyn::zyn!(arrow::datatypes::DataType::Float64),
        FieldKind::F32 => zyn::zyn!(arrow::datatypes::DataType::Float32),
        FieldKind::Bool => zyn::zyn!(arrow::datatypes::DataType::Boolean),
        FieldKind::I64 => zyn::zyn!(arrow::datatypes::DataType::Int64),
        FieldKind::I32 => zyn::zyn!(arrow::datatypes::DataType::Int32),
    }
}

fn arrow_array_type(kind: FieldKind) -> Output {
    match kind {
        FieldKind::VecU8 => zyn::zyn!(arrow::array::BinaryArray),
        FieldKind::VecF64 => zyn::zyn!(arrow::array::ListArray),
        FieldKind::InstrumentId
        | FieldKind::AccountId
        | FieldKind::Currency
        | FieldKind::BarType
        | FieldKind::String => zyn::zyn!(arrow::array::StringArray),
        FieldKind::UnixNanos | FieldKind::U64 | FieldKind::U32 => {
            zyn::zyn!(arrow::array::UInt64Array)
        }
        FieldKind::F64 => zyn::zyn!(arrow::array::Float64Array),
        FieldKind::F32 => zyn::zyn!(arrow::array::Float32Array),
        FieldKind::Bool => zyn::zyn!(arrow::array::BooleanArray),
        FieldKind::I64 => zyn::zyn!(arrow::array::Int64Array),
        FieldKind::I32 => zyn::zyn!(arrow::array::Int32Array),
    }
}

fn py_param_ty(field: &FieldSpec) -> Output {
    match field.kind {
        FieldKind::UnixNanos => zyn::zyn!(u64),
        FieldKind::VecU8 => zyn::zyn!(Vec<u8>),
        FieldKind::VecF64 => zyn::zyn!(Vec<f64>),
        _ => {
            let ty = &field.ty;
            zyn::zyn!({ { ty } })
        }
    }
}

fn py_field_init(field: &FieldSpec) -> Output {
    let name = &field.ident;
    match field.kind {
        FieldKind::UnixNanos => zyn::zyn!({ { name } }.into()),
        _ => zyn::zyn!({ { name } }),
    }
}

fn py_getter_ret_ty(field: &FieldSpec) -> Output {
    match field.kind {
        FieldKind::UnixNanos => zyn::zyn!(u64),
        _ => {
            let ty = &field.ty;
            zyn::zyn!({ { ty } })
        }
    }
}

fn py_getter_body(field: &FieldSpec) -> Output {
    let name = &field.ident;
    match field.kind {
        FieldKind::UnixNanos => zyn::zyn!(self.{{ name }}.as_u64()),
        FieldKind::VecU8 | FieldKind::VecF64 | FieldKind::String => {
            zyn::zyn!(self.{{ name }}.clone())
        }
        _ => zyn::zyn!(self.{{ name }}),
    }
}

fn repr_arg(field: &FieldSpec) -> Output {
    let ident = &field.ident;
    if field.is_ts_field() {
        zyn::zyn!(nautilus_core::datetime::unix_nanos_to_iso8601(self.{{ ident }}))
    } else {
        zyn::zyn!(self.{{ ident }})
    }
}

fn finish_builder() -> Output {
    zyn::zyn!(std::sync::Arc::new(builder.finish()))
}

#[zyn::element]
fn arrow_schema_field<'a>(field: &'a FieldSpec) -> zyn::TokenStream {
    let field_name = field.name_str();
    zyn::zyn! {
        arrow::datatypes::Field::new(
            {{ field_name }},
            {{ arrow_data_type(field.kind) }},
            false
        )
    }
}

#[zyn::element]
fn encode_builder<'a>(len_var: Ident, field: &'a FieldSpec) -> zyn::TokenStream {
    match field.kind {
        FieldKind::VecU8 => zyn::zyn!(let mut builder = arrow::array::BinaryBuilder::new();),
        FieldKind::VecF64 => zyn::zyn! {
            let mut builder = arrow::array::ListBuilder::new(arrow::array::Float64Builder::new());
        },
        FieldKind::InstrumentId
        | FieldKind::AccountId
        | FieldKind::Currency
        | FieldKind::BarType
        | FieldKind::String => {
            zyn::zyn!(let mut builder = arrow::array::StringBuilder::new();)
        }
        FieldKind::UnixNanos | FieldKind::U64 | FieldKind::U32 => {
            zyn::zyn!(let mut builder = arrow::array::UInt64Array::builder({{ len_var }});)
        }
        FieldKind::F64 => {
            zyn::zyn!(let mut builder = arrow::array::Float64Array::builder({{ len_var }});)
        }
        FieldKind::F32 => {
            zyn::zyn!(let mut builder = arrow::array::Float32Array::builder({{ len_var }});)
        }
        FieldKind::Bool => {
            zyn::zyn!(let mut builder = arrow::array::BooleanArray::builder({{ len_var }});)
        }
        FieldKind::I64 => {
            zyn::zyn!(let mut builder = arrow::array::Int64Array::builder({{ len_var }});)
        }
        FieldKind::I32 => {
            zyn::zyn!(let mut builder = arrow::array::Int32Array::builder({{ len_var }});)
        }
    }
}

#[zyn::element]
fn encode_field_append<'a>(field: &'a FieldSpec) -> zyn::TokenStream {
    let ident = &field.ident;
    match field.kind {
        FieldKind::VecU8 => zyn::zyn!(builder.append_value(item.{{ ident }}.as_slice());),
        FieldKind::VecF64 => zyn::zyn! {
            for v in item.{{ ident }}.iter() {
                builder.values().append_value(*v);
            }
            builder.append(true);
        },
        FieldKind::InstrumentId
        | FieldKind::AccountId
        | FieldKind::Currency
        | FieldKind::BarType => zyn::zyn!(builder.append_value(item.{{ ident }}.to_string());),
        FieldKind::UnixNanos => zyn::zyn!(builder.append_value(item.{{ ident }}.as_u64());),
        FieldKind::F64
        | FieldKind::F32
        | FieldKind::Bool
        | FieldKind::U64
        | FieldKind::I64
        | FieldKind::I32 => zyn::zyn!(builder.append_value(item.{{ ident }});),
        FieldKind::String => zyn::zyn!(builder.append_value(item.{{ ident }}.as_str());),
        FieldKind::U32 => zyn::zyn!(builder.append_value(item.{{ ident }} as u64);),
    }
}

#[zyn::element]
fn encode_column_block<'a>(idx: usize, len_var: Ident, field: &'a FieldSpec) -> zyn::TokenStream {
    let col_name = format_ident!("col_{idx}");
    zyn::zyn! {
        @encode_builder(len_var = len_var.clone(), field = field)
        for item in data {
            let _append_guard = ();
            @encode_field_append(field = field)
        }
        let {{ col_name }} = {{ finish_builder() }};
    }
}

#[zyn::element]
fn decode_field_value<'a>(col_ident: Ident, field: &'a FieldSpec) -> zyn::TokenStream {
    let field_name = field.name_str();
    match field.kind {
        FieldKind::VecU8 => zyn::zyn!({ { col_ident } }.value(i).to_vec()),
        FieldKind::VecF64 => zyn::zyn! {
            {
                let arr = {{ col_ident }}.value(i);
                let float_arr = arr.as_any().downcast_ref::<arrow::array::Float64Array>()
                    .ok_or_else(|| nautilus_serialization::arrow::EncodingError::ParseError(
                        {{ field_name }},
                        format!("expected Float64Array for list element"),
                    ))?;
                (0..float_arr.len()).map(|j| float_arr.value(j)).collect::<Vec<f64>>()
            }
        },
        FieldKind::InstrumentId
        | FieldKind::AccountId
        | FieldKind::Currency
        | FieldKind::BarType => zyn::zyn! {
            std::str::FromStr::from_str({{ col_ident }}.value(i)).map_err(|e| nautilus_serialization::arrow::EncodingError::ParseError(
                {{ field_name }},
                format!("expected valid identifier/type, was '{}'", e),
            ))?
        },
        FieldKind::UnixNanos => zyn::zyn!({ { col_ident } }.value(i).into()),
        FieldKind::F64
        | FieldKind::F32
        | FieldKind::Bool
        | FieldKind::U64
        | FieldKind::I64
        | FieldKind::I32 => zyn::zyn!({ { col_ident } }.value(i)),
        FieldKind::U32 => zyn::zyn!({ { col_ident } }.value(i) as u32),
        FieldKind::String => zyn::zyn!({ { col_ident } }.value(i).to_string()),
    }
}

#[zyn::element]
fn decode_extract<'a>(idx: usize, field: &'a FieldSpec) -> zyn::TokenStream {
    let col_ident = format_ident!("col_{idx}");
    let field_name = field.name_str();

    if field.kind.uses_string_extract() {
        zyn::zyn! {
            let {{ col_ident }} = nautilus_serialization::arrow::extract_column_string(
                record_batch.columns(),
                {{ field_name }},
                {{ idx }},
            )?;
        }
    } else {
        zyn::zyn! {
            let {{ col_ident }} = nautilus_serialization::arrow::extract_column::<{{ arrow_array_type(field.kind) }}>(
                record_batch.columns(),
                {{ field_name }},
                {{ idx }},
                {{ arrow_data_type(field.kind) }},
            )?;
        }
    }
}

#[zyn::element]
fn decode_row_field<'a>(idx: usize, field: &'a FieldSpec) -> zyn::TokenStream {
    let ident = &field.ident;
    let col_ident = format_ident!("col_{idx}");
    zyn::zyn! {
        {{ ident }}: @decode_field_value(col_ident = col_ident, field = field)
    }
}

#[zyn::element]
fn py_getter<'a>(field: &'a FieldSpec) -> zyn::TokenStream {
    let ident = &field.ident;
    zyn::zyn! {
        #[getter]
        fn {{ ident }}(&self) -> {{ py_getter_ret_ty(field) }} {
            let value = {{ py_getter_body(field) }};
            value
        }
    }
}

#[zyn::element]
fn new_fn<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    vis: &'a syn::Visibility,
    field_list: &'a [FieldSpec],
    pyo3_enabled: bool,
) -> zyn::TokenStream {
    let rust_ctor_name = format_ident!("new");
    let rust_ctor_doc = if *pyo3_enabled {
        "Constructor from all fields. Use from Rust; Python __init__ forwards to this."
    } else {
        "Constructor."
    };

    zyn::zyn! {
        impl {{ generics }} {{ name }} {{ generics }} {
            #[allow(dead_code)]
            #[allow(clippy::too_many_arguments)]
            #[doc = {{ rust_ctor_doc }}]
            {{ vis }} fn {{ rust_ctor_name }}(
                @for (field in field_list.iter()) {
                    {{ field.ident }}: {{ field.ty }},
                }
            ) -> Self {
                Self {
                    @for (field in field_list.iter()) {
                        {{ field.ident }},
                    }
                }
            }
        }
    }
}

#[zyn::element]
fn repr_impl<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    name_str: &'a str,
    field_list: &'a [FieldSpec],
) -> zyn::TokenStream {
    let repr_format_parts: Vec<String> = field_list
        .iter()
        .map(|field: &FieldSpec| {
            let name = field.name_str();
            if field.is_ts_field() {
                format!("{name}={{}}")
            } else {
                format!("{name}={{:?}}")
            }
        })
        .collect();
    let repr_format_str = format!("{}({})", name_str, repr_format_parts.join(", "));
    let repr_format_lit = LitStr::new(&repr_format_str, Span::call_site());
    let repr_args: Vec<Output> = field_list.iter().map(repr_arg).collect();

    zyn::zyn! {
        impl {{ generics }} {{ name }} {{ generics }} {
            /// Returns a string representation in the same style as Python CustomDataClass (fields and ts_event/ts_init as ISO8601).
            pub fn repr(&self) -> String {
                format!(
                    {{ repr_format_lit }},
                    @for (arg in repr_args.iter()) {
                        {{ arg }},
                    }
                )
            }
        }
        impl {{ generics }} std::fmt::Display for {{ name }} {{ generics }} {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::write!(f, "{}", self.repr())
            }
        }
    }
}

#[zyn::element]
fn ts_init_impl<'a>(name: &'a Ident, generics: &'a syn::Generics) -> zyn::TokenStream {
    zyn::zyn! {
        impl {{ generics }} nautilus_model::data::HasTsInit for {{ name }} {{ generics }} {
            fn ts_init(&self) -> nautilus_core::UnixNanos {
                self.ts_init
            }
        }
    }
}

#[zyn::element]
fn custom_data_trait_impl<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    name_str: &'a str,
) -> zyn::TokenStream {
    zyn::zyn! {
        impl {{ generics }} nautilus_model::data::CustomDataTrait for {{ name }} {{ generics }} {
            fn type_name(&self) -> &'static str {
                let value = {{ name_str }};
                value
            }
            fn type_name_static() -> &'static str {
                let value = {{ name_str }};
                value
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

#[zyn::element]
fn custom_data_serialize_impl<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    name_str: &'a str,
) -> zyn::TokenStream {
    zyn::zyn! {
        impl {{ generics }} nautilus_serialization::arrow::custom::CustomDataSerialize for {{ name }} {{ generics }} {
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
                        anyhow::bail!("Expected {}, was different type", {{ name_str }});
                    }
                }
                let metadata = nautilus_serialization::arrow::EncodeToRecordBatch::metadata(self);
                nautilus_serialization::arrow::EncodeToRecordBatch::encode_batch(&metadata, &typed).map_err(Into::into)
            }
        }
    }
}

#[zyn::element]
fn arrow_schema_impl<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    field_list: &'a [FieldSpec],
) -> zyn::TokenStream {
    zyn::zyn! {
        impl {{ generics }} nautilus_serialization::arrow::ArrowSchemaProvider for {{ name }} {{ generics }} {
            fn get_schema(metadata: Option<std::collections::HashMap<String, String>>) -> arrow::datatypes::Schema {
                let fields = vec![
                    @for (field in field_list.iter()) {
                        @arrow_schema_field(field = field),
                    }
                ];
                match metadata {
                    Some(m) => arrow::datatypes::Schema::new_with_metadata(fields, m),
                    None => arrow::datatypes::Schema::new(fields),
                }
            }
        }
    }
}

#[zyn::element]
fn encode_batch_impl<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    name_str: &'a str,
    field_list: &'a [FieldSpec],
) -> zyn::TokenStream {
    let len_var = format_ident!("data_len");
    zyn::zyn! {
        impl {{ generics }} nautilus_serialization::arrow::EncodeToRecordBatch for {{ name }} {{ generics }} {
            fn encode_batch(
                metadata: &std::collections::HashMap<String, String>,
                data: &[Self],
            ) -> std::result::Result<arrow::record_batch::RecordBatch, arrow::error::ArrowError> {
                let {{ len_var }} = data.len();
                @for (idx in 0..field_list.len()) {
                    @encode_column_block(idx = idx, len_var = len_var.clone(), field = &field_list[idx])
                }
                arrow::record_batch::RecordBatch::try_new(
                    <Self as nautilus_serialization::arrow::ArrowSchemaProvider>::get_schema(Some(metadata.clone())).into(),
                    vec![
                        @for (idx in 0..field_list.len()) {
                            {{ format_ident!("col_{}", idx) }},
                        }
                    ],
                )
            }
            fn metadata(&self) -> std::collections::HashMap<String, String> {
                let mut m = std::collections::HashMap::new();
                m.insert("type_name".to_string(), {{ name_str }}.to_string());
                m
            }
        }
    }
}

#[zyn::element]
fn decode_batch_impl<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    field_list: &'a [FieldSpec],
) -> zyn::TokenStream {
    zyn::zyn! {
        impl {{ generics }} nautilus_serialization::arrow::DecodeDataFromRecordBatch for {{ name }} {{ generics }} {
            fn decode_data_batch(
                _metadata: &std::collections::HashMap<String, String>,
                record_batch: arrow::record_batch::RecordBatch,
            ) -> std::result::Result<Vec<nautilus_model::data::Data>, nautilus_serialization::arrow::EncodingError> {
                @for (idx in 0..field_list.len()) {
                    @decode_extract(idx = idx, field = &field_list[idx])
                }
                let num_rows = record_batch.num_rows();
                let mut results = Vec::with_capacity(num_rows);
                for i in 0..num_rows {
                    let row = Self {
                        @for (idx in 0..field_list.len()) {
                            @decode_row_field(idx = idx, field = &field_list[idx]),
                        }
                    };
                    results.push(nautilus_model::data::Data::Custom(nautilus_model::data::CustomData::from_arc(std::sync::Arc::new(row))));
                }
                Ok(results)
            }
        }
    }
}

#[zyn::element]
fn catalog_path_prefix_impl<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    catalog_path: &'a str,
) -> zyn::TokenStream {
    zyn::zyn! {
        impl {{ generics }} nautilus_model::data::CatalogPathPrefix for {{ name }} {{ generics }} {
            fn path_prefix() -> &'static str {
                let value = {{ catalog_path }};
                value
            }
        }
    }
}

#[zyn::element]
fn from_impl<'a>(name: &'a Ident, generics: &'a syn::Generics) -> zyn::TokenStream {
    zyn::zyn! {
        impl {{ generics }} std::convert::From<{{ name }} {{ generics }}> for nautilus_model::data::Data {
            fn from(value: {{ name }} {{ generics }}) -> Self {
                nautilus_model::data::Data::Custom(nautilus_model::data::CustomData::from_arc(std::sync::Arc::new(value)))
            }
        }
    }
}

#[zyn::element]
fn try_from_impl<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    name_str: &'a str,
) -> zyn::TokenStream {
    zyn::zyn! {
        impl {{ generics }} std::convert::TryFrom<nautilus_model::data::Data> for {{ name }} {{ generics }} {
            type Error = anyhow::Error;
            fn try_from(value: nautilus_model::data::Data) -> std::result::Result<Self, Self::Error> {
                match value {
                    nautilus_model::data::Data::Custom(custom) => {
                        if let Some(c) = custom.data.as_any().downcast_ref::<Self>() {
                            Ok(std::clone::Clone::clone(c))
                        } else {
                            anyhow::bail!("Expected {}", {{ name_str }})
                        }
                    }
                    _ => anyhow::bail!("Expected Custom data variant"),
                }
            }
        }
    }
}

#[zyn::element]
fn pymethods_impl<'a>(
    name: &'a Ident,
    generics: &'a syn::Generics,
    field_list: &'a [FieldSpec],
    no_display: bool,
) -> zyn::TokenStream {
    zyn::zyn! {
        #[cfg(feature = "python")]
        use pyo3::prelude::*;
        /// PyO3 bindings (constructor, getters, to_json, from_json, record batch encode/decode). Only compiled when `feature = "python"`.
        #[cfg(feature = "python")]
        #[pyo3::pymethods]
        #[allow(clippy::needless_pass_by_value)]
        impl {{ generics }} {{ name }} {{ generics }} {
            #[allow(clippy::too_many_arguments)]
            #[new]
            #[pyo3(signature = (
                @for (field in field_list.iter()) {
                    {{ field.ident }},
                }
            ))]
            fn py_new(
                @for (field in field_list.iter()) {
                    {{ field.ident }}: {{ py_param_ty(field) }},
                }
            ) -> Self {
                @for (field in field_list.iter()) {
                    let {{ field.ident }} = {{ py_field_init(field) }};
                }
                Self::new(
                    @for (field in field_list.iter()) {
                        {{ field.ident }},
                    }
                )
            }
            @for (field in field_list.iter()) {
                @py_getter(field = field)
            }

            @if (!*no_display) {
                /// Python `repr()`: uses the Rust `Display` implementation.
                fn __repr__(&self) -> pyo3::PyResult<String> {
                    Ok(std::fmt::format(std::format_args!("{}", self)))
                }

                /// Python `str()`: uses the Rust `Display` implementation.
                fn __str__(&self) -> pyo3::PyResult<String> {
                    Ok(std::fmt::format(std::format_args!("{}", self)))
                }
            }

            /// Serializes to JSON string. Used by CustomData.to_json_bytes and PythonCustomDataWrapper.
            fn to_json(&self) -> pyo3::PyResult<String> {
                <{{ name }} as nautilus_model::data::CustomDataTrait>::to_json_py(self)
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
                let arc = <{{ name }} as nautilus_model::data::CustomDataTrait>::from_json(value)
                    .map_err(nautilus_core::python::to_pyvalue_err)?;
                let inner = arc.as_any().downcast_ref::<{{ name }}>()
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

                let data_list = <{{ name }} as nautilus_serialization::arrow::DecodeDataFromRecordBatch>::decode_data_batch(
                    &metadata,
                    batch,
                ).map_err(nautilus_core::python::to_pyvalue_err)?;
                let mut py_items = Vec::new();
                for d in data_list {
                    if let nautilus_model::data::Data::Custom(custom) = d {
                        if let Some(m) = custom.data.as_any().downcast_ref::<{{ name }}>() {
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
                let typed: Vec<{{ name }}> = items
                    .iter()
                    .map(|obj| obj.extract::<{{ name }}>().map_err(|e| e.into()))
                    .collect::<pyo3::PyResult<Vec<_>>>()?;
                let metadata = <{{ name }} as nautilus_serialization::arrow::EncodeToRecordBatch>::metadata(self);
                let batch = <{{ name }} as nautilus_serialization::arrow::EncodeToRecordBatch>::encode_batch(
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

pub fn expand_custom_data(item: ItemStruct, args: &Args) -> Output {
    let options = match parse_options(args) {
        Ok(options) => options,
        Err(e) => return e.to_compile_error().into(),
    };

    let name = &item.ident;
    let name_str = name.to_string();
    let vis = &item.vis;
    let generics = &item.generics;

    let fields = match &item.fields {
        Fields::Named(fields) => &fields.named,
        _ => {
            return syn::Error::new_spanned(
                item,
                "#[custom_data] requires a struct with named fields",
            )
            .to_compile_error()
            .into();
        }
    };

    let mut field_specs = Vec::with_capacity(fields.len());
    for field in fields {
        match FieldSpec::parse(field) {
            Some(field_spec) => field_specs.push(field_spec),
            None => {
                let ident = field.ident.as_ref().expect("named field");
                return syn::Error::new_spanned(
                    &field.ty,
                    format!(
                        "#[custom_data] does not support field type for '{ident}'; supported: InstrumentId, AccountId, Currency, BarType, UnixNanos, f64, f32, bool, String, u64, i64, u32, i32, Vec<f64>, Vec<u8>"
                    ),
                )
                .to_compile_error()
                .into();
            }
        }
    }

    let has_ts_event = field_specs.iter().any(|field| field.ident == "ts_event");
    let has_ts_init = field_specs.iter().any(|field| field.ident == "ts_init");

    if !has_ts_event || !has_ts_init {
        return syn::Error::new_spanned(
            item,
            "#[custom_data] requires fields ts_event and ts_init (e.g. nautilus_core::UnixNanos)",
        )
        .to_compile_error()
        .into();
    }

    let struct_attrs: Vec<syn::Attribute> = item
        .attrs
        .iter()
        .filter(|attr| {
            attr.path()
                .get_ident()
                .is_none_or(|ident| *ident != "custom_data")
        })
        .cloned()
        .collect();
    let fields_vec: Vec<Field> = fields.iter().cloned().collect();
    let catalog_path = format!("custom/{name_str}");
    let input: zyn::Input = syn::Item::Struct(item.clone()).into();

    zyn::zyn! {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
        @for (attr in struct_attrs.iter()) {
            {{ attr }}
        }
        @if (options.pyo3) {
            #[cfg_attr(feature = "python", pyo3::pyclass(from_py_object))]
        }
        {{ vis }} struct {{ name }} {{ generics }} {
            @for (field in fields_vec.iter()) {
                {{ field }},
            }
        }

        @new_fn(
            name = name,
            generics = generics,
            vis = vis,
            field_list = field_specs.as_slice(),
            pyo3_enabled = options.pyo3,
        )

        @if (!options.no_display) {
            @repr_impl(
                name = name,
                generics = generics,
                name_str = name_str.as_str(),
                field_list = field_specs.as_slice(),
            )
        }

        @ts_init_impl(name = name, generics = generics)
        @custom_data_trait_impl(name = name, generics = generics, name_str = name_str.as_str())
        @custom_data_serialize_impl(name = name, generics = generics, name_str = name_str.as_str())
        @arrow_schema_impl(name = name, generics = generics, field_list = field_specs.as_slice())
        @encode_batch_impl(
            name = name,
            generics = generics,
            name_str = name_str.as_str(),
            field_list = field_specs.as_slice(),
        )
        @decode_batch_impl(name = name, generics = generics, field_list = field_specs.as_slice())
        @catalog_path_prefix_impl(name = name, generics = generics, catalog_path = catalog_path.as_str())
        @from_impl(name = name, generics = generics)
        @try_from_impl(name = name, generics = generics, name_str = name_str.as_str())

        @if (options.pyo3) {
            @pymethods_impl(
                name = name,
                generics = generics,
                field_list = field_specs.as_slice(),
                no_display = options.no_display,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use proc_macro2::TokenStream;
    use rstest::rstest;
    use zyn::syn::{self, ItemStruct};

    use super::{expand_custom_data, parse_options};

    fn normalize_tokens(tokens: &impl ToString) -> String {
        tokens
            .to_string()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect()
    }

    #[rstest]
    #[case(TokenStream::from_str("pyo3").unwrap(), true, false)]
    #[case(TokenStream::from_str("python").unwrap(), true, false)]
    #[case(TokenStream::from_str("python, pyo3").unwrap(), true, false)]
    #[case(TokenStream::from_str("pyo3, no_display").unwrap(), true, true)]
    #[case(TokenStream::new(), false, false)]
    fn parse_options_supports_zyn_attribute_parser(
        #[case] tokens: TokenStream,
        #[case] pyo3_enabled: bool,
        #[case] no_display: bool,
    ) {
        let args: zyn::Args = zyn::parse!(tokens => zyn::Args).unwrap();
        let options = parse_options(&args).unwrap();

        assert_eq!(options.pyo3, pyo3_enabled);
        assert_eq!(options.no_display, no_display);
    }

    #[rstest]
    fn parse_options_rejects_unknown_option() {
        let args: zyn::Args =
            zyn::parse!(TokenStream::from_str("bogus").unwrap() => zyn::Args).unwrap();
        let error = parse_options(&args).unwrap_err();

        assert_eq!(
            format!("{error}"),
            "expected `pyo3`, `python`, or `no_display`; unknown option"
        );
    }

    #[rstest]
    fn expand_custom_data_generates_valid_return_bodies() {
        let item: ItemStruct = syn::parse_str(
            "
            pub struct ExampleCustomData {
                pub instrument_id: InstrumentId,
                pub payload: String,
                pub ts_event: UnixNanos,
                pub ts_init: UnixNanos,
            }
            ",
        )
        .unwrap();
        let args: zyn::Args = syn::parse_str("python").unwrap();

        let expanded = expand_custom_data(item, &args);
        let normalized = normalize_tokens(&expanded);

        assert!(
            normalized.contains("#[cfg_attr(feature=\"python\",pyo3::pyclass(from_py_object))]")
        );
        assert!(normalized.contains("letvalue=\"ExampleCustomData\";value"));
        assert!(normalized.contains("letvalue=\"custom/ExampleCustomData\";value"));
        assert!(normalized.contains("\"ExampleCustomData\""));
        assert!(normalized.contains("\"custom/ExampleCustomData\""));
        assert!(normalized.contains("letvalue=self.payload.clone();value"));
        assert!(normalized.contains("_append_guard"));
    }
}
