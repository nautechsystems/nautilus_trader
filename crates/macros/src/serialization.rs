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

//! Arrow serialization generation for custom data structs.
//!
//! `#[arrow_custom_data]` adds Arrow schema, batch encoding, and batch decoding implementations
//! without generating model, Serde, constructor, display, or core PyO3 behavior. Apply it above
//! `#[nautilus_model::custom_data]`, or use it alone with manual model trait implementations.
//! `#[arrow_custom_data(pyo3)]` additionally generates `PyArrow` `RecordBatch` methods.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Fields, GenericArgument, Ident, ItemStruct, LitStr, PathArguments, Token, Type, parse::Parser,
    parse2, punctuated::Punctuated,
};

use crate::support::{CustomDataOption, FieldSpec, parse_field_options, type_for_macro, type_path};

fn optional_inner_type(ty: &Type) -> Option<&Type> {
    let segment = type_path(ty)?.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    Some(inner)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArrowFieldKind {
    Json,
    Bytes,
    Price,
    Quantity,
    Decimal,
    Money,
    Enum,
    FloatList,
    Identifier,
    Params,
    String,
    UnixNanos,
    U64,
    U32,
    F64,
    F32,
    Bool,
    I64,
    I32,
}

#[derive(Debug)]
struct ArrowFieldDescriptor {
    kind: ArrowFieldKind,
    data_type: TokenStream,
    array_type: TokenStream,
    nullable: bool,
    value_type: Type,
}

impl ArrowFieldDescriptor {
    #[expect(
        clippy::too_many_lines,
        reason = "field type classification is one exhaustive macro input mapping"
    )]
    fn new(field: &FieldSpec) -> Result<Self, syn::Error> {
        if field.options.serde {
            return Ok(Self::with_utf8(ArrowFieldKind::Json, false, &field.ty));
        }

        let unsupported = || {
            syn::Error::new_spanned(
                &field.ty,
                format!(
                    "#[arrow_custom_data] does not support field type for '{}'; use \
                     #[custom_data_field(serde)] for a Serde JSON-backed Arrow column",
                    field.ident,
                ),
            )
        };
        let (value_type, nullable) =
            optional_inner_type(&field.ty).map_or((&field.ty, false), |inner| (inner, true));
        let (outer, inner) = type_for_macro(value_type).ok_or_else(unsupported)?;
        match (outer.as_str(), inner.as_str()) {
            ("Vec", "u8") if !nullable => Ok(Self {
                kind: ArrowFieldKind::Bytes,
                data_type: quote! { arrow::datatypes::DataType::Binary },
                array_type: quote! { arrow::array::BinaryArray },
                nullable: false,
                value_type: value_type.clone(),
            }),
            ("Vec", "f64") if !nullable => Ok(Self {
                kind: ArrowFieldKind::FloatList,
                data_type: quote! {
                    arrow::datatypes::DataType::List(std::sync::Arc::new(
                        arrow::datatypes::Field::new(
                            "item",
                            arrow::datatypes::DataType::Float64,
                            true,
                        ),
                    ))
                },
                array_type: quote! { arrow::array::ListArray },
                nullable: false,
                value_type: value_type.clone(),
            }),
            _ if outer == inner => match outer.as_str() {
                "InstrumentId" | "AccountId" | "Currency" | "BarType" => Ok(Self::with_utf8(
                    ArrowFieldKind::Identifier,
                    nullable,
                    value_type,
                )),
                "Params" => Ok(Self::with_utf8(
                    ArrowFieldKind::Params,
                    nullable,
                    value_type,
                )),
                "String" => Ok(Self::with_utf8(
                    ArrowFieldKind::String,
                    nullable,
                    value_type,
                )),
                "Price" => Ok(Self::with_decimal(
                    ArrowFieldKind::Price,
                    nullable,
                    value_type,
                )),
                "Quantity" => Ok(Self::with_decimal(
                    ArrowFieldKind::Quantity,
                    nullable,
                    value_type,
                )),
                "Decimal" => Ok(Self::with_decimal(
                    ArrowFieldKind::Decimal,
                    nullable,
                    value_type,
                )),
                "Money" => Ok(Self {
                    kind: ArrowFieldKind::Money,
                    data_type: quote! { nautilus_serialization::arrow::money_data_type() },
                    array_type: quote! { arrow::array::StructArray },
                    nullable,
                    value_type: value_type.clone(),
                }),
                "UnixNanos" => Ok(Self::with_timestamp(nullable, value_type)),
                "u64" => Ok(Self::with_uint64(ArrowFieldKind::U64, nullable, value_type)),
                "u32" => Ok(Self::with_uint64(ArrowFieldKind::U32, nullable, value_type)),
                "f64" => Ok(Self::with_primitive(
                    ArrowFieldKind::F64,
                    quote! { arrow::datatypes::DataType::Float64 },
                    quote! { arrow::array::Float64Array },
                    nullable,
                    value_type,
                )),
                "f32" => Ok(Self::with_primitive(
                    ArrowFieldKind::F32,
                    quote! { arrow::datatypes::DataType::Float32 },
                    quote! { arrow::array::Float32Array },
                    nullable,
                    value_type,
                )),
                "bool" => Ok(Self::with_primitive(
                    ArrowFieldKind::Bool,
                    quote! { arrow::datatypes::DataType::Boolean },
                    quote! { arrow::array::BooleanArray },
                    nullable,
                    value_type,
                )),
                "i64" => Ok(Self::with_primitive(
                    ArrowFieldKind::I64,
                    quote! { arrow::datatypes::DataType::Int64 },
                    quote! { arrow::array::Int64Array },
                    nullable,
                    value_type,
                )),
                "i32" => Ok(Self::with_primitive(
                    ArrowFieldKind::I32,
                    quote! { arrow::datatypes::DataType::Int32 },
                    quote! { arrow::array::Int32Array },
                    nullable,
                    value_type,
                )),
                "u8" | "u16" | "i8" | "i16" | "usize" | "isize" | "char" => Err(unsupported()),
                _ if field.options.native_enum => Ok(Self {
                    kind: ArrowFieldKind::Enum,
                    data_type: quote! {
                        nautilus_serialization::arrow::enum_dictionary_data_type()
                    },
                    array_type: quote! {
                        arrow::array::DictionaryArray<arrow::datatypes::Int8Type>
                    },
                    nullable,
                    value_type: value_type.clone(),
                }),
                _ => Err(unsupported()),
            },
            _ => Err(unsupported()),
        }
    }

    fn with_primitive(
        kind: ArrowFieldKind,
        data_type: TokenStream,
        array_type: TokenStream,
        nullable: bool,
        value_type: &Type,
    ) -> Self {
        Self {
            kind,
            data_type,
            array_type,
            nullable,
            value_type: value_type.clone(),
        }
    }

    fn with_utf8(kind: ArrowFieldKind, nullable: bool, value_type: &Type) -> Self {
        Self {
            kind,
            data_type: quote! { arrow::datatypes::DataType::Utf8 },
            array_type: quote! { arrow::array::StringArray },
            nullable,
            value_type: value_type.clone(),
        }
    }

    fn with_uint64(kind: ArrowFieldKind, nullable: bool, value_type: &Type) -> Self {
        Self {
            kind,
            data_type: quote! { arrow::datatypes::DataType::UInt64 },
            array_type: quote! { arrow::array::UInt64Array },
            nullable,
            value_type: value_type.clone(),
        }
    }

    fn with_timestamp(nullable: bool, value_type: &Type) -> Self {
        Self {
            kind: ArrowFieldKind::UnixNanos,
            data_type: quote! { nautilus_serialization::arrow::timestamp_data_type() },
            array_type: quote! { arrow::array::TimestampNanosecondArray },
            nullable,
            value_type: value_type.clone(),
        }
    }

    fn with_decimal(kind: ArrowFieldKind, nullable: bool, value_type: &Type) -> Self {
        Self {
            kind,
            data_type: quote! {
                arrow::datatypes::DataType::Decimal128(
                    nautilus_serialization::arrow::FIXED_DECIMAL_PRECISION,
                    nautilus_serialization::arrow::FIXED_DECIMAL_SCALE,
                )
            },
            array_type: quote! { arrow::array::Decimal128Array },
            nullable,
            value_type: value_type.clone(),
        }
    }

    fn is_nullable(&self) -> bool {
        self.nullable || matches!(self.kind, ArrowFieldKind::Price | ArrowFieldKind::Quantity)
    }

    fn uses_string_extract(&self) -> bool {
        matches!(
            self.kind,
            ArrowFieldKind::Json
                | ArrowFieldKind::Identifier
                | ArrowFieldKind::Params
                | ArrowFieldKind::String
                | ArrowFieldKind::Enum
        )
    }

    fn builder(&self, len: &Ident) -> TokenStream {
        match self.kind {
            ArrowFieldKind::Json
            | ArrowFieldKind::Identifier
            | ArrowFieldKind::Params
            | ArrowFieldKind::String => {
                quote! { let mut builder = arrow::array::StringBuilder::new(); }
            }
            ArrowFieldKind::Enum => quote! {
                let mut builder =
                    arrow::array::StringDictionaryBuilder::<arrow::datatypes::Int8Type>::new();
            },
            ArrowFieldKind::Bytes => {
                quote! { let mut builder = arrow::array::BinaryBuilder::new(); }
            }
            ArrowFieldKind::Price
            | ArrowFieldKind::Quantity
            | ArrowFieldKind::Decimal
            | ArrowFieldKind::Money
            | ArrowFieldKind::UnixNanos => {
                quote! { let mut builder = Vec::with_capacity(#len); }
            }
            ArrowFieldKind::FloatList => quote! {
                let mut builder =
                    arrow::array::ListBuilder::new(arrow::array::Float64Builder::new());
            },
            ArrowFieldKind::U64 | ArrowFieldKind::U32 => {
                quote! { let mut builder = arrow::array::UInt64Array::builder(#len); }
            }
            ArrowFieldKind::F64 => {
                quote! { let mut builder = arrow::array::Float64Array::builder(#len); }
            }
            ArrowFieldKind::F32 => {
                quote! { let mut builder = arrow::array::Float32Array::builder(#len); }
            }
            ArrowFieldKind::Bool => {
                quote! { let mut builder = arrow::array::BooleanArray::builder(#len); }
            }
            ArrowFieldKind::I64 => {
                quote! { let mut builder = arrow::array::Int64Array::builder(#len); }
            }
            ArrowFieldKind::I32 => {
                quote! { let mut builder = arrow::array::Int32Array::builder(#len); }
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "append code generation is one exhaustive Arrow field mapping"
    )]
    fn append(&self, name: &Ident) -> TokenStream {
        let nullable = self.nullable;

        match self.kind {
            ArrowFieldKind::Json => quote! {
                let value = serde_json::to_string(&item.#name).map_err(|e| {
                    arrow::error::ArrowError::InvalidArgumentError(
                        format!("failed to serialize JSON field '{}': {e}", stringify!(#name)),
                    )
                })?;
                builder.append_value(value);
            },
            ArrowFieldKind::Bytes => {
                quote! { builder.append_value(item.#name.as_slice()); }
            }
            ArrowFieldKind::Price => {
                if nullable {
                    quote! {
                        if let Some(value) = item.#name {
                            if value.precision > nautilus_serialization::arrow::FIXED_DECIMAL_SCALE as u8 {
                                return Err(arrow::error::ArrowError::InvalidArgumentError(format!(
                                    "Price field '{}' precision {} exceeds Arrow decimal scale {}",
                                    stringify!(#name),
                                    value.precision,
                                    nautilus_serialization::arrow::FIXED_DECIMAL_SCALE,
                                )));
                            }
                            builder.push(value.raw());
                        } else {
                            builder.push(nautilus_model::types::PRICE_UNDEF);
                        }
                    }
                } else {
                    quote! {
                        if item.#name.precision > nautilus_serialization::arrow::FIXED_DECIMAL_SCALE as u8 {
                            return Err(arrow::error::ArrowError::InvalidArgumentError(format!(
                                "Price field '{}' precision {} exceeds Arrow decimal scale {}",
                                stringify!(#name),
                                item.#name.precision,
                                nautilus_serialization::arrow::FIXED_DECIMAL_SCALE,
                            )));
                        }
                        builder.push(item.#name.raw());
                    }
                }
            }
            ArrowFieldKind::Quantity => {
                if nullable {
                    quote! {
                        if let Some(value) = item.#name {
                            if value.precision > nautilus_serialization::arrow::FIXED_DECIMAL_SCALE as u8 {
                                return Err(arrow::error::ArrowError::InvalidArgumentError(format!(
                                    "Quantity field '{}' precision {} exceeds Arrow decimal scale {}",
                                    stringify!(#name),
                                    value.precision,
                                    nautilus_serialization::arrow::FIXED_DECIMAL_SCALE,
                                )));
                            }
                            builder.push(value.raw());
                        } else {
                            builder.push(nautilus_model::types::QUANTITY_UNDEF);
                        }
                    }
                } else {
                    quote! {
                        if item.#name.precision > nautilus_serialization::arrow::FIXED_DECIMAL_SCALE as u8 {
                            return Err(arrow::error::ArrowError::InvalidArgumentError(format!(
                                "Quantity field '{}' precision {} exceeds Arrow decimal scale {}",
                                stringify!(#name),
                                item.#name.precision,
                                nautilus_serialization::arrow::FIXED_DECIMAL_SCALE,
                            )));
                        }
                        builder.push(item.#name.raw());
                    }
                }
            }
            ArrowFieldKind::Decimal => {
                if nullable {
                    quote! {
                        builder.push(
                            item.#name
                                .as_ref()
                                .map(|value| {
                                    nautilus_serialization::arrow::decimal_to_arrow(
                                        value,
                                        stringify!(#name),
                                    )
                                })
                                .transpose()?,
                        );
                    }
                } else {
                    quote! {
                        builder.push(Some(
                            nautilus_serialization::arrow::decimal_to_arrow(
                                &item.#name,
                                stringify!(#name),
                            )?,
                        ));
                    }
                }
            }
            ArrowFieldKind::Money => {
                if nullable {
                    quote! { builder.push(item.#name); }
                } else {
                    quote! { builder.push(Some(item.#name)); }
                }
            }
            ArrowFieldKind::FloatList => quote! {
                for value in &item.#name {
                    builder.values().append_value(*value);
                }
                builder.append(true);
            },
            ArrowFieldKind::Identifier => {
                if nullable {
                    quote! {
                        if let Some(value) = &item.#name {
                            builder.append_value(value.to_string());
                        } else {
                            builder.append_null();
                        }
                    }
                } else {
                    quote! { builder.append_value(item.#name.to_string()); }
                }
            }
            ArrowFieldKind::Params => {
                if nullable {
                    quote! {
                        if let Some(value) = &item.#name {
                            let value = serde_json::to_string(value).map_err(|e| {
                                arrow::error::ArrowError::InvalidArgumentError(
                                    format!(
                                        "failed to serialize Params field '{}': {e}",
                                        stringify!(#name),
                                    ),
                                )
                            })?;
                            builder.append_value(value);
                        } else {
                            builder.append_null();
                        }
                    }
                } else {
                    quote! {
                        let value = serde_json::to_string(&item.#name).map_err(|e| {
                            arrow::error::ArrowError::InvalidArgumentError(
                                format!(
                                    "failed to serialize Params field '{}': {e}",
                                    stringify!(#name),
                                ),
                            )
                        })?;
                        builder.append_value(value);
                    }
                }
            }
            ArrowFieldKind::String => {
                if nullable {
                    quote! {
                        if let Some(value) = &item.#name {
                            builder.append_value(value.as_str());
                        } else {
                            builder.append_null();
                        }
                    }
                } else {
                    quote! { builder.append_value(item.#name.as_str()); }
                }
            }
            ArrowFieldKind::Enum => {
                if nullable {
                    quote! {
                        if let Some(value) = &item.#name {
                            builder.append(value.to_string())?;
                        } else {
                            builder.append_null();
                        }
                    }
                } else {
                    quote! { builder.append(item.#name.to_string())?; }
                }
            }
            ArrowFieldKind::UnixNanos => {
                if nullable {
                    quote! { builder.push(item.#name.map(|value| value.as_u64())); }
                } else {
                    quote! { builder.push(item.#name.as_u64()); }
                }
            }
            ArrowFieldKind::U32 => {
                if nullable {
                    quote! {
                        if let Some(value) = item.#name {
                            builder.append_value(value as u64);
                        } else {
                            builder.append_null();
                        }
                    }
                } else {
                    quote! { builder.append_value(item.#name as u64); }
                }
            }
            ArrowFieldKind::U64
            | ArrowFieldKind::F64
            | ArrowFieldKind::F32
            | ArrowFieldKind::Bool
            | ArrowFieldKind::I64
            | ArrowFieldKind::I32 => {
                if nullable {
                    quote! {
                        if let Some(value) = item.#name {
                            builder.append_value(value);
                        } else {
                            builder.append_null();
                        }
                    }
                } else {
                    quote! { builder.append_value(item.#name); }
                }
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "decode code generation is one exhaustive Arrow field mapping"
    )]
    fn decode(&self, name: &Ident, ty: &Type, column: &Ident) -> TokenStream {
        let precision = quote! {{
            let key = concat!(stringify!(#name), "_precision");
            metadata
                .get(key)
                .ok_or(nautilus_serialization::arrow::EncodingError::ParseError(
                    stringify!(#name),
                    format!("missing metadata key `{key}`"),
                ))?
                .parse::<u8>()
                .map_err(|e| {
                    nautilus_serialization::arrow::EncodingError::ParseError(
                        stringify!(#name),
                        format!("invalid precision metadata: {e}"),
                    )
                })?
        }};
        let value_type = &self.value_type;
        let decoded = match self.kind {
            ArrowFieldKind::Json => quote! {
                serde_json::from_str::<#ty>(#column.value(i)).map_err(|e| {
                    nautilus_serialization::arrow::EncodingError::ParseError(
                        stringify!(#name),
                        format!("row {i}: {e}"),
                    )
                })?
            },
            ArrowFieldKind::Bytes => quote! { #column.value(i).to_vec() },
            ArrowFieldKind::Price => quote! {
                nautilus_serialization::arrow::decode_decimal_price(
                    #column,
                    #precision,
                    stringify!(#name),
                    i,
                )?
            },
            ArrowFieldKind::Quantity => quote! {
                nautilus_serialization::arrow::decode_decimal_quantity(
                    #column,
                    #precision,
                    stringify!(#name),
                    i,
                )?
            },
            ArrowFieldKind::Decimal => quote! {
                nautilus_serialization::arrow::decode_decimal(
                    #column,
                    stringify!(#name),
                    i,
                )?
            },
            ArrowFieldKind::Money => quote! {
                nautilus_serialization::arrow::decode_money(
                    #column,
                    stringify!(#name),
                    i,
                )?
            },
            ArrowFieldKind::FloatList => quote! {
                {
                    let array = #column.value(i);
                    let float_array = array
                        .as_any()
                        .downcast_ref::<arrow::array::Float64Array>()
                        .ok_or_else(|| {
                            nautilus_serialization::arrow::EncodingError::ParseError(
                                stringify!(#name),
                                "expected Float64Array for list element".to_string(),
                            )
                        })?;
                    (0..float_array.len())
                        .map(|index| {
                            if arrow::array::Array::is_null(float_array, index) {
                                Err(nautilus_serialization::arrow::EncodingError::ParseError(
                                    stringify!(#name),
                                    format!("row {i}: list element {index} is null"),
                                ))
                            } else {
                                Ok(float_array.value(index))
                            }
                        })
                        .collect::<Result<Vec<f64>, nautilus_serialization::arrow::EncodingError>>()?
                }
            },
            ArrowFieldKind::Identifier => quote! {
                std::str::FromStr::from_str(#column.value(i)).map_err(|e| {
                    nautilus_serialization::arrow::EncodingError::ParseError(
                        stringify!(#name),
                        format!("expected valid identifier/type, was '{e}'"),
                    )
                })?
            },
            ArrowFieldKind::Params => quote! {
                serde_json::from_str::<#value_type>(#column.value(i)).map_err(|e| {
                    nautilus_serialization::arrow::EncodingError::ParseError(
                        stringify!(#name),
                        format!("row {i}: {e}"),
                    )
                })?
            },
            ArrowFieldKind::String => quote! { #column.value(i).to_string() },
            ArrowFieldKind::Enum => quote! {
                <#value_type as std::str::FromStr>::from_str(#column.value(i)).map_err(|e| {
                    nautilus_serialization::arrow::EncodingError::ParseError(
                        stringify!(#name),
                        format!("row {i}: {e}"),
                    )
                })?
            },
            ArrowFieldKind::UnixNanos => quote! {
                nautilus_serialization::arrow::decode_timestamp(
                    #column,
                    stringify!(#name),
                    i,
                )?.into()
            },
            ArrowFieldKind::U32 => quote! {
                u32::try_from(#column.value(i)).map_err(|_| {
                    nautilus_serialization::arrow::EncodingError::ParseError(
                        stringify!(#name),
                        format!("row {i}: value is outside the u32 range"),
                    )
                })?
            },
            ArrowFieldKind::U64
            | ArrowFieldKind::F64
            | ArrowFieldKind::F32
            | ArrowFieldKind::Bool
            | ArrowFieldKind::I64
            | ArrowFieldKind::I32 => quote! { #column.value(i) },
        };

        if !self.nullable || self.kind == ArrowFieldKind::Json {
            return decoded;
        }
        let is_null = if self.uses_string_extract() {
            quote! { #column.is_null(i) }
        } else {
            quote! { arrow::array::Array::is_null(#column, i) }
        };
        quote! {
            if #is_null {
                None
            } else {
                Some(#decoded)
            }
        }
    }

    fn finish(&self, name: &Ident) -> TokenStream {
        match self.kind {
            ArrowFieldKind::Price => {
                quote! {
                    nautilus_serialization::arrow::price_decimal_array(
                        builder,
                        stringify!(#name),
                    )?
                }
            }
            ArrowFieldKind::Quantity => {
                quote! {
                    nautilus_serialization::arrow::quantity_decimal_array(
                        builder,
                        stringify!(#name),
                    )?
                }
            }
            ArrowFieldKind::Decimal => quote! {
                arrow::array::Decimal128Array::from(builder).with_precision_and_scale(
                    nautilus_serialization::arrow::FIXED_DECIMAL_PRECISION,
                    nautilus_serialization::arrow::FIXED_DECIMAL_SCALE,
                )?
            },
            ArrowFieldKind::Money => {
                quote! { nautilus_serialization::arrow::money_array(builder)? }
            }
            ArrowFieldKind::UnixNanos => {
                if self.nullable {
                    quote! { nautilus_serialization::arrow::optional_timestamp_array(builder)? }
                } else {
                    quote! { nautilus_serialization::arrow::timestamp_array(builder)? }
                }
            }
            _ => quote! { builder.finish() },
        }
    }

    fn precision_metadata(&self, name: &Ident) -> Option<TokenStream> {
        if !matches!(self.kind, ArrowFieldKind::Price | ArrowFieldKind::Quantity) {
            return None;
        }
        let kind = if self.kind == ArrowFieldKind::Price {
            "price"
        } else {
            "quantity"
        };
        let value = if self.nullable {
            quote! { item.#name }
        } else {
            quote! { Some(item.#name) }
        };
        Some(quote! {
            let mut precision = None;

            for item in data.iter().map(std::borrow::Borrow::borrow) {
                if let Some(value) = #value
                    && !value.is_undefined()
                {
                    if let Some(expected) = precision {
                        if expected != value.precision {
                            return Err(arrow::error::ArrowError::InvalidArgumentError(format!(
                                "custom field '{}' mixes precisions {expected} and {}",
                                stringify!(#name),
                                value.precision,
                            )));
                        }
                    } else {
                        precision = Some(value.precision);
                    }
                }
            }
            final_metadata.insert(
                concat!(stringify!(#name), "_precision").to_string(),
                precision.unwrap_or(0).to_string(),
            );
            final_metadata.insert(
                concat!(stringify!(#name), "_kind").to_string(),
                #kind.to_string(),
            );
        })
    }

    fn instance_metadata(&self, name: &Ident) -> Option<TokenStream> {
        if !matches!(self.kind, ArrowFieldKind::Price | ArrowFieldKind::Quantity) {
            return None;
        }
        let kind = if self.kind == ArrowFieldKind::Price {
            "price"
        } else {
            "quantity"
        };

        if self.nullable {
            Some(quote! {
                m.insert(
                    concat!(stringify!(#name), "_precision").to_string(),
                    self.#name.map_or(0, |value| value.precision).to_string(),
                );
                m.insert(
                    concat!(stringify!(#name), "_kind").to_string(),
                    #kind.to_string(),
                );
            })
        } else {
            Some(quote! {
                m.insert(
                    concat!(stringify!(#name), "_precision").to_string(),
                    self.#name.precision.to_string(),
                );
                m.insert(
                    concat!(stringify!(#name), "_kind").to_string(),
                    #kind.to_string(),
                );
            })
        }
    }
}

#[derive(Debug, Default)]
struct ArrowCustomDataOptions {
    pyo3: bool,
    stub_module: Option<LitStr>,
}

fn parse_options(attr: &TokenStream) -> Result<ArrowCustomDataOptions, syn::Error> {
    if attr.is_empty() {
        return Ok(ArrowCustomDataOptions::default());
    }

    let mut options = ArrowCustomDataOptions::default();

    for option in
        Punctuated::<CustomDataOption, Token![,]>::parse_terminated.parse2(attr.clone())?
    {
        let name = option.ident.to_string();
        match (name.as_str(), option.value) {
            ("pyo3", None) => options.pyo3 = true,
            ("stub_module", Some(module)) => options.stub_module = Some(module),
            ("pyo3", Some(_)) => {
                return Err(syn::Error::new_spanned(
                    option.ident,
                    "option does not accept a value",
                ));
            }
            ("stub_module", None) => {
                return Err(syn::Error::new_spanned(
                    option.ident,
                    "`stub_module` requires a string value",
                ));
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    option.ident,
                    "expected `pyo3` or `stub_module`; unknown option",
                ));
            }
        }
    }

    if options.stub_module.is_some() && !options.pyo3 {
        return Err(syn::Error::new_spanned(
            attr,
            "`stub_module` requires `pyo3`",
        ));
    }
    Ok(options)
}

struct ExpansionContext<'a> {
    name: &'a Ident,
    name_str: &'a str,
    generics: &'a syn::Generics,
    field_list: &'a [FieldSpec],
    descriptors: &'a [ArrowFieldDescriptor],
    options: &'a ArrowCustomDataOptions,
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
                let mut typed: Vec<&Self> = Vec::with_capacity(items.len());
                for item in items {
                    if let Some(c) = item.as_any().downcast_ref::<Self>() {
                        typed.push(c);
                    } else {
                        anyhow::bail!("Expected {}, was different type", #name_str);
                    }
                }
                let metadata = nautilus_serialization::arrow::EncodeToRecordBatch::metadata(self);
                <Self as nautilus_serialization::arrow::EncodeToRecordBatch>::encode_batch(&metadata, &typed).map_err(Into::into)
            }
        }
    }
}

fn gen_arrow_schema_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    let field_list = ctx.field_list;
    let descriptors = ctx.descriptors;
    let arrow_schema_fields: Vec<TokenStream> = field_list
        .iter()
        .zip(descriptors)
        .map(|(f, descriptor)| {
            let ident = &f.ident;
            let arrow_dt = &descriptor.data_type;
            let nullable = descriptor.is_nullable();
            let fn_str = ident.to_string();

            if matches!(
                descriptor.kind,
                ArrowFieldKind::Json | ArrowFieldKind::Params
            ) {
                quote! {
                    nautilus_serialization::arrow::json_string_field(#fn_str, #nullable)
                }
            } else {
                quote! {
                    arrow::datatypes::Field::new(#fn_str, #arrow_dt, #nullable)
                }
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
    let descriptors = ctx.descriptors;
    let len_var = format_ident!("data_len");
    let mut col_builds = Vec::new();
    let mut col_names = Vec::new();
    let mut precision_metadata = Vec::new();
    let mut instance_metadata = Vec::new();

    for (f, descriptor) in field_list.iter().zip(descriptors) {
        let ident = &f.ident;
        let builder = descriptor.builder(&len_var);
        let append = descriptor.append(ident);
        let finish = descriptor.finish(ident);
        precision_metadata.extend(descriptor.precision_metadata(ident));
        instance_metadata.extend(descriptor.instance_metadata(ident));
        let col_name = format_ident!("col_{}", col_builds.len());
        col_names.push(col_name.clone());
        col_builds.push(quote! {
            #builder

            for item in data.iter().map(std::borrow::Borrow::borrow) {
                #append
            }
            let #col_name = std::sync::Arc::new(#finish);
        });
    }
    let metadata_map = quote! {
        let mut m = std::collections::HashMap::new();
        m.insert("type_name".to_string(), #name_str.to_string());
        #(#instance_metadata)*
        m
    };
    quote! {
        impl #generics nautilus_serialization::arrow::EncodeToRecordBatch for #name #generics {
            fn encode_batch<T>(
                metadata: &std::collections::HashMap<String, String>,
                data: &[T],
            ) -> std::result::Result<arrow::record_batch::RecordBatch, arrow::error::ArrowError>
            where
                T: std::borrow::Borrow<Self>,
            {
                let #len_var = data.len();
                let mut final_metadata = metadata.clone();
                #(#precision_metadata)*
                #(#col_builds)*
                arrow::record_batch::RecordBatch::try_new(
                    <Self as nautilus_serialization::arrow::ArrowSchemaProvider>::get_schema(Some(final_metadata)).into(),
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
    let descriptors = ctx.descriptors;
    let decode_row_fields: Vec<TokenStream> = field_list
        .iter()
        .zip(descriptors)
        .enumerate()
        .map(|(idx, (f, descriptor))| {
            let ident = &f.ident;
            let ty = &f.ty;
            let col_name = format_ident!("col_{}", idx);
            let rhs = descriptor.decode(ident, ty, &col_name);
            quote! { #ident: #rhs }
        })
        .collect();
    let decode_extracts: Vec<TokenStream> = field_list
        .iter()
        .zip(descriptors)
        .enumerate()
        .map(|(idx, (f, descriptor))| {
            let ident = &f.ident;
            let col_name = format_ident!("col_{}", idx);
            let fn_str = ident.to_string();

            if descriptor.uses_string_extract() {
                quote! {
                    let #col_name = nautilus_serialization::arrow::extract_column_string(
                        record_batch.columns(),
                        #fn_str,
                        #idx,
                    )?;
                }
            } else if matches!(descriptor.kind, ArrowFieldKind::Bytes) {
                quote! {
                    let #col_name = nautilus_serialization::arrow::extract_column_binary(
                        record_batch.columns(), #fn_str, #idx,
                    )?;
                }
            } else {
                let arrow_dt = &descriptor.data_type;
                let array_ty = &descriptor.array_type;
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
                metadata: &std::collections::HashMap<String, String>,
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

fn gen_arrow_pymethods_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    if !ctx.options.pyo3 {
        return quote! {};
    }

    let name = ctx.name;
    let generics = ctx.generics;
    let stub_attr = ctx.options.stub_module.is_some().then(|| {
        quote! { #[cfg_attr(feature = "python", pyo3_stub_gen::derive::gen_stub_pymethods)] }
    });
    quote! {
        #[cfg(feature = "python")]
        use pyo3::prelude::*;

        #[cfg(feature = "python")]
        #stub_attr
        #[pyo3::pymethods]
        impl #generics #name #generics {
            /// Decodes a PyArrow RecordBatch into custom data instances.
            #[pyo3(signature = (metadata, py_batch))]
            #[classmethod]
            fn decode_record_batch_py(
                _cls: &pyo3::Bound<'_, pyo3::types::PyType>,
                py: pyo3::Python<'_>,
                metadata: std::collections::HashMap<String, String>,
                py_batch: &pyo3::Bound<'_, pyo3::PyAny>,
            ) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
                let mut ffi_array = arrow::ffi::FFI_ArrowArray::empty();
                let mut ffi_schema = arrow::ffi::FFI_ArrowSchema::empty();
                py_batch.call_method1(
                    "_export_to_c",
                    ((&raw mut ffi_array as usize), (&raw mut ffi_schema as usize)),
                )?;

                let schema = std::sync::Arc::new(
                    arrow::datatypes::Schema::try_from(&ffi_schema)
                        .map_err(nautilus_core::python::to_pyvalue_err)?,
                );
                let struct_array_data = unsafe {
                    arrow::ffi::from_ffi_and_data_type(
                        ffi_array,
                        arrow::datatypes::DataType::Struct(schema.fields().clone()),
                    )
                    .map_err(nautilus_core::python::to_pyvalue_err)?
                };
                let batch = arrow::record_batch::RecordBatch::from(
                    &arrow::array::StructArray::from(struct_array_data),
                );
                let data = <#name as nautilus_serialization::arrow::DecodeDataFromRecordBatch>::decode_data_batch(
                    &metadata,
                    batch,
                )
                .map_err(nautilus_core::python::to_pyvalue_err)?;
                let mut items = Vec::new();

                for data in data {
                    if let nautilus_model::data::Data::Custom(custom) = data
                        && let Some(item) = custom.data.as_any().downcast_ref::<#name>()
                    {
                        items.push(pyo3::Py::new(py, item.clone())?.into_any());
                    }
                }
                Ok(pyo3::types::PyList::new(py, items)?.into_any().unbind())
            }

            /// Encodes custom data instances into a PyArrow RecordBatch.
            fn encode_record_batch_py(
                &self,
                py: pyo3::Python<'_>,
                items: &pyo3::Bound<'_, pyo3::types::PyList>,
            ) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
                let typed = items
                    .iter()
                    .map(|item| item.extract::<#name>().map_err(Into::into))
                    .collect::<pyo3::PyResult<Vec<_>>>()?;
                let metadata = <#name as nautilus_serialization::arrow::EncodeToRecordBatch>::metadata(self);
                let batch = <#name as nautilus_serialization::arrow::EncodeToRecordBatch>::encode_batch(
                    &metadata,
                    &typed,
                )
                .map_err(nautilus_core::python::to_pyvalue_err)?;
                let struct_array: arrow::array::StructArray = batch.clone().into();
                let array_data = arrow::array::Array::to_data(&struct_array);
                let mut ffi_array = arrow::ffi::FFI_ArrowArray::new(&array_data);
                let mut ffi_schema = arrow::ffi::FFI_ArrowSchema::try_from(
                    arrow::datatypes::DataType::Struct(batch.schema().fields().clone()),
                )
                .map_err(nautilus_core::python::to_pyvalue_err)?;
                let pyarrow = py.import("pyarrow")?;
                let py_batch = pyarrow.getattr("RecordBatch")?.call_method1(
                    "_import_from_c",
                    ((&raw mut ffi_array as usize), (&raw mut ffi_schema as usize)),
                )?;
                Ok(py_batch.into_any().unbind())
            }
        }
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "proc-macro entry points consume owned token streams"
)]
pub(crate) fn expand_arrow_custom_data(attr: TokenStream, item: TokenStream) -> TokenStream {
    let options = match parse_options(&attr) {
        Ok(options) => options,
        Err(e) => return e.to_compile_error(),
    };
    let input = match parse2::<ItemStruct>(item) {
        Ok(input) => input,
        Err(e) => return e.to_compile_error(),
    };
    let fields = match &input.fields {
        Fields::Named(fields) => &fields.named,
        _ => {
            return syn::Error::new_spanned(
                input,
                "#[arrow_custom_data] requires a struct with named fields",
            )
            .to_compile_error();
        }
    };
    let field_list = match fields
        .iter()
        .map(|field| {
            Ok(FieldSpec {
                ident: field.ident.clone().expect("named field"),
                ty: field.ty.clone(),
                options: parse_field_options(field)?,
            })
        })
        .collect::<Result<Vec<_>, syn::Error>>()
    {
        Ok(fields) => fields,
        Err(e) => return e.to_compile_error(),
    };

    let descriptors = match field_list
        .iter()
        .map(ArrowFieldDescriptor::new)
        .collect::<Result<Vec<_>, syn::Error>>()
    {
        Ok(descriptors) => descriptors,
        Err(e) => return e.to_compile_error(),
    };

    let name = &input.ident;
    let name_str = name.to_string();
    let ctx = ExpansionContext {
        name,
        name_str: &name_str,
        generics: &input.generics,
        field_list: &field_list,
        descriptors: &descriptors,
        options: &options,
    };
    let custom_data_serialize = gen_custom_data_serialize_impl(&ctx);
    let arrow_schema = gen_arrow_schema_impl(&ctx);
    let encode_batch = gen_encode_batch_impl(&ctx);
    let decode_batch = gen_decode_batch_impl(&ctx);
    let pymethods = gen_arrow_pymethods_impl(&ctx);
    let has_model_macro = input.attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "custom_data")
    });
    let mut output_item = input.clone();
    if !has_model_macro && let Fields::Named(fields) = &mut output_item.fields {
        for field in &mut fields.named {
            field.attrs.retain(|attr| {
                attr.path()
                    .get_ident()
                    .is_none_or(|ident| ident != "custom_data_field")
            });
        }
    }

    quote! {
        #output_item
        #custom_data_serialize
        #arrow_schema
        #encode_batch
        #decode_batch
        #pymethods
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use rstest::rstest;

    use super::*;

    fn test_item() -> TokenStream {
        quote! {
            #[custom_data(pyo3)]
            pub struct TestData {
                pub value: f64,
                pub ts_event: nautilus_core::UnixNanos,
                pub ts_init: nautilus_core::UnixNanos,
            }
        }
    }

    #[rstest]
    fn expansion_is_arrow_only() {
        let expanded = expand_arrow_custom_data(quote! {}, test_item()).to_string();

        assert!(expanded.contains("ArrowSchemaProvider"));
        assert!(expanded.contains("EncodeToRecordBatch"));
        assert!(expanded.contains("DecodeDataFromRecordBatch"));
        assert!(!expanded.contains("impl nautilus_model :: data :: HasTsInit"));
        assert!(!expanded.contains("serde :: Serialize"));
        assert!(!expanded.contains("encode_record_batch_py"));
    }

    #[rstest]
    fn pyo3_is_explicit_and_arrow_only() {
        let expanded = expand_arrow_custom_data(
            quote! { pyo3, stub_module = "nautilus_trader.test" },
            test_item(),
        )
        .to_string();

        assert!(expanded.contains("encode_record_batch_py"));
        assert!(expanded.contains("decode_record_batch_py"));
        assert!(expanded.contains("gen_stub_pymethods"));
        assert!(!expanded.contains("fn py_new"));
        assert!(!expanded.contains("gen_stub_pyclass"));
    }

    #[rstest]
    fn pyo3_is_supported_without_model_macro() {
        let item = quote! {
            pub struct ManualData {
                #[custom_data_field(serde)]
                pub values: Vec<(String, u64)>,
                pub value: f64,
                pub ts_event: nautilus_core::UnixNanos,
                pub ts_init: nautilus_core::UnixNanos,
            }
        };
        let expanded = expand_arrow_custom_data(quote! { pyo3 }, item).to_string();

        assert!(expanded.contains("encode_record_batch_py"));
        assert!(!expanded.contains("custom_data ("));
        assert!(!expanded.contains("custom_data_field"));
    }

    #[rstest]
    fn unsupported_option_is_rejected() {
        let error = parse_options(&quote! { no_arrow }).unwrap_err();

        assert_eq!(
            error.to_string(),
            "expected `pyo3` or `stub_module`; unknown option",
        );
    }

    #[rstest]
    fn unsupported_field_type_returns_spanned_error() {
        let item = quote! {
            pub struct UnsupportedData {
                pub values: Vec<String>,
            }
        };
        let expanded = expand_arrow_custom_data(quote! {}, item).to_string();

        assert!(expanded.contains("compile_error"));
        assert!(expanded.contains("does not support field type for 'values'"));
    }

    #[rstest]
    fn unmarked_single_segment_type_returns_spanned_error() {
        let item = quote! {
            pub struct UnsupportedData {
                pub value: DisplayAndFromStr,
            }
        };
        let expanded = expand_arrow_custom_data(quote! {}, item).to_string();

        assert!(expanded.contains("compile_error"));
        assert!(expanded.contains("does not support field type for 'value'"));
        assert!(!expanded.contains("enum_dictionary_data_type"));
    }

    #[rstest]
    fn marked_native_enum_uses_dictionary_encoding() {
        let item = quote! {
            pub struct SupportedData {
                #[custom_data_field(native_enum)]
                pub value: DisplayAndFromStr,
            }
        };
        let expanded = expand_arrow_custom_data(quote! {}, item).to_string();

        assert!(!expanded.contains("compile_error"));
        assert!(expanded.contains("enum_dictionary_data_type"));
        assert!(!expanded.contains("custom_data_field"));
    }
}
