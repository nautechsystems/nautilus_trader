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

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{
        Array, ArrayRef, BooleanArray, BooleanBuilder, Float64Array, Float64Builder, StringBuilder,
        UInt64Array, UInt64Builder,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Number, Value};

use super::{EncodingError, StringColumnRef, extract_column, extract_column_string};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonFieldEncoding {
    Utf8,
    Utf8Json,
    UInt64,
    Float64,
    Boolean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonFieldSpec {
    pub name: &'static str,
    pub encoding: JsonFieldEncoding,
    pub nullable: bool,
}

impl JsonFieldSpec {
    #[must_use]
    pub const fn utf8(name: &'static str, nullable: bool) -> Self {
        Self {
            name,
            encoding: JsonFieldEncoding::Utf8,
            nullable,
        }
    }

    #[must_use]
    pub const fn utf8_json(name: &'static str, nullable: bool) -> Self {
        Self {
            name,
            encoding: JsonFieldEncoding::Utf8Json,
            nullable,
        }
    }

    #[must_use]
    pub const fn u64(name: &'static str, nullable: bool) -> Self {
        Self {
            name,
            encoding: JsonFieldEncoding::UInt64,
            nullable,
        }
    }

    #[must_use]
    pub const fn f64(name: &'static str, nullable: bool) -> Self {
        Self {
            name,
            encoding: JsonFieldEncoding::Float64,
            nullable,
        }
    }

    #[must_use]
    pub const fn boolean(name: &'static str, nullable: bool) -> Self {
        Self {
            name,
            encoding: JsonFieldEncoding::Boolean,
            nullable,
        }
    }

    fn field(self) -> Field {
        let data_type = match self.encoding {
            JsonFieldEncoding::Utf8 | JsonFieldEncoding::Utf8Json => DataType::Utf8,
            JsonFieldEncoding::UInt64 => DataType::UInt64,
            JsonFieldEncoding::Float64 => DataType::Float64,
            JsonFieldEncoding::Boolean => DataType::Boolean,
        };

        Field::new(self.name, data_type, self.nullable)
    }
}

#[must_use]
pub fn metadata_for_type(type_name: &'static str) -> HashMap<String, String> {
    HashMap::from([("type".to_string(), type_name.to_string())])
}

#[must_use]
pub fn schema_for_type(
    type_name: &'static str,
    metadata: Option<HashMap<String, String>>,
    fields: &[JsonFieldSpec],
) -> Schema {
    let mut merged = metadata.unwrap_or_default();
    merged.insert("type".to_string(), type_name.to_string());

    Schema::new_with_metadata(
        fields
            .iter()
            .copied()
            .map(JsonFieldSpec::field)
            .collect::<Vec<_>>(),
        merged,
    )
}

/// Encodes typed records into an Arrow record batch with the supplied schema metadata.
///
/// # Errors
///
/// Returns an error if JSON serialization fails or if a field cannot be encoded into
/// the requested Arrow column type.
pub fn encode_batch<T: Serialize>(
    type_name: &'static str,
    metadata: &HashMap<String, String>,
    data: &[T],
    fields: &[JsonFieldSpec],
) -> Result<RecordBatch, ArrowError> {
    let rows = serialize_rows(data)?;
    let arrays: Result<Vec<ArrayRef>, ArrowError> = fields
        .iter()
        .copied()
        .map(|field| encode_column(field, &rows))
        .collect();

    RecordBatch::try_new(
        Arc::new(schema_for_type(type_name, Some(metadata.clone()), fields)),
        arrays?,
    )
}

/// Decodes typed records from an Arrow record batch produced by encode_batch.
///
/// # Errors
///
/// Returns an error if a required column is missing, has the wrong type, contains
/// invalid JSON, or cannot be deserialized into the target type.
pub fn decode_batch<T: DeserializeOwned>(
    metadata: &HashMap<String, String>,
    record_batch: &RecordBatch,
    fields: &[JsonFieldSpec],
    fallback_type_name: Option<&'static str>,
) -> Result<Vec<T>, EncodingError> {
    let columns: Result<Vec<_>, EncodingError> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| decode_column_ref(record_batch.columns(), *field, index))
        .collect();
    let columns = columns?;

    let mut decoded = Vec::with_capacity(record_batch.num_rows());
    let type_name = metadata
        .get("type")
        .cloned()
        .or_else(|| fallback_type_name.map(str::to_string));

    for row in 0..record_batch.num_rows() {
        let mut value = Map::new();
        if let Some(type_name) = &type_name {
            value.insert("type".to_string(), Value::String(type_name.clone()));
        }

        for column in &columns {
            value.insert(column.name().to_string(), column.to_json(row)?);
        }

        let json = serde_json::to_vec(&Value::Object(value))
            .map_err(|e| EncodingError::ParseError("record_batch", format!("row {row}: {e}")))?;
        decoded.push(
            serde_json::from_slice(&json).map_err(|e| {
                EncodingError::ParseError("record_batch", format!("row {row}: {e}"))
            })?,
        );
    }

    Ok(decoded)
}

fn serialize_rows<T: Serialize>(data: &[T]) -> Result<Vec<Map<String, Value>>, ArrowError> {
    data.iter()
        .map(|item| match serde_json::to_value(item) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err(invalid_argument(
                "Expected serialized value to be a JSON object".to_string(),
            )),
            Err(e) => Err(invalid_argument(e.to_string())),
        })
        .collect()
}

fn encode_column(
    field: JsonFieldSpec,
    rows: &[Map<String, Value>],
) -> Result<ArrayRef, ArrowError> {
    match field.encoding {
        JsonFieldEncoding::Utf8 => encode_utf8_column(field, rows),
        JsonFieldEncoding::Utf8Json => encode_utf8_json_column(field, rows),
        JsonFieldEncoding::UInt64 => encode_u64_column(field, rows),
        JsonFieldEncoding::Float64 => encode_f64_column(field, rows),
        JsonFieldEncoding::Boolean => encode_bool_column(field, rows),
    }
}

fn encode_utf8_column(
    field: JsonFieldSpec,
    rows: &[Map<String, Value>],
) -> Result<ArrayRef, ArrowError> {
    let mut builder = StringBuilder::new();

    for row in rows {
        match require_value(field, row.get(field.name))? {
            Some(value) => builder.append_value(value_to_string(value)?),
            None => builder.append_null(),
        }
    }

    Ok(Arc::new(builder.finish()))
}

fn encode_utf8_json_column(
    field: JsonFieldSpec,
    rows: &[Map<String, Value>],
) -> Result<ArrayRef, ArrowError> {
    let mut builder = StringBuilder::new();

    for row in rows {
        match require_value(field, row.get(field.name))? {
            Some(value) => builder.append_value(
                serde_json::to_string(value).map_err(|e| invalid_argument(e.to_string()))?,
            ),
            None => builder.append_null(),
        }
    }

    Ok(Arc::new(builder.finish()))
}

fn encode_u64_column(
    field: JsonFieldSpec,
    rows: &[Map<String, Value>],
) -> Result<ArrayRef, ArrowError> {
    let mut builder = UInt64Builder::new();

    for row in rows {
        match require_value(field, row.get(field.name))? {
            Some(value) => builder.append_value(parse_u64(value)?),
            None => builder.append_null(),
        }
    }

    Ok(Arc::new(builder.finish()))
}

fn encode_f64_column(
    field: JsonFieldSpec,
    rows: &[Map<String, Value>],
) -> Result<ArrayRef, ArrowError> {
    let mut builder = Float64Builder::new();

    for row in rows {
        match require_value(field, row.get(field.name))? {
            Some(value) => builder.append_value(parse_f64(value)?),
            None => builder.append_null(),
        }
    }

    Ok(Arc::new(builder.finish()))
}

fn encode_bool_column(
    field: JsonFieldSpec,
    rows: &[Map<String, Value>],
) -> Result<ArrayRef, ArrowError> {
    let mut builder = BooleanBuilder::new();

    for row in rows {
        match require_value(field, row.get(field.name))? {
            Some(value) => builder.append_value(parse_bool(value)?),
            None => builder.append_null(),
        }
    }

    Ok(Arc::new(builder.finish()))
}

fn require_value(
    field: JsonFieldSpec,
    value: Option<&Value>,
) -> Result<Option<&Value>, ArrowError> {
    match value {
        Some(Value::Null) | None if !field.nullable => Err(invalid_argument(format!(
            "Missing required field `{}`",
            field.name
        ))),
        Some(Value::Null) | None => Ok(None),
        Some(value) => Ok(Some(value)),
    }
}

fn value_to_string(value: &Value) -> Result<String, ArrowError> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Null => Err(invalid_argument("Unexpected null value".to_string())),
        Value::Bool(_) | Value::Number(_) => Ok(value.to_string()),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).map_err(|e| invalid_argument(e.to_string()))
        }
    }
}

fn parse_u64(value: &Value) -> Result<u64, ArrowError> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| invalid_argument(format!("Expected u64, found `{number}`"))),
        Value::String(value) => value
            .parse::<u64>()
            .map_err(|e| invalid_argument(format!("Failed to parse u64 from `{value}`: {e}"))),
        _ => Err(invalid_argument(format!(
            "Expected u64-compatible value, found `{value}`"
        ))),
    }
}

fn parse_f64(value: &Value) -> Result<f64, ArrowError> {
    match value {
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| invalid_argument(format!("Expected f64, found `{number}`"))),
        Value::String(value) => value
            .parse::<f64>()
            .map_err(|e| invalid_argument(format!("Failed to parse f64 from `{value}`: {e}"))),
        _ => Err(invalid_argument(format!(
            "Expected f64-compatible value, found `{value}`"
        ))),
    }
}

fn parse_bool(value: &Value) -> Result<bool, ArrowError> {
    match value {
        Value::Bool(value) => Ok(*value),
        Value::String(value) => value
            .parse::<bool>()
            .map_err(|e| invalid_argument(format!("Failed to parse bool from `{value}`: {e}"))),
        _ => Err(invalid_argument(format!(
            "Expected bool-compatible value, found `{value}`"
        ))),
    }
}

enum ColumnRef<'a> {
    Utf8 {
        name: &'static str,
        values: StringColumnRef<'a>,
    },
    Utf8Json {
        name: &'static str,
        values: StringColumnRef<'a>,
    },
    UInt64 {
        name: &'static str,
        values: &'a UInt64Array,
    },
    Float64 {
        name: &'static str,
        values: &'a Float64Array,
    },
    Boolean {
        name: &'static str,
        values: &'a BooleanArray,
    },
}

impl ColumnRef<'_> {
    fn name(&self) -> &'static str {
        match self {
            Self::Utf8 { name, .. }
            | Self::Utf8Json { name, .. }
            | Self::UInt64 { name, .. }
            | Self::Float64 { name, .. }
            | Self::Boolean { name, .. } => name,
        }
    }

    fn to_json(&self, row: usize) -> Result<Value, EncodingError> {
        match self {
            Self::Utf8 { values, .. } => {
                if values_is_null(values, row) {
                    Ok(Value::Null)
                } else {
                    Ok(Value::String(values.value(row).to_string()))
                }
            }
            Self::Utf8Json { values, .. } => {
                if values_is_null(values, row) {
                    Ok(Value::Null)
                } else {
                    serde_json::from_str(values.value(row)).map_err(|e| {
                        EncodingError::ParseError(self.name(), format!("row {row}: {e}"))
                    })
                }
            }
            Self::UInt64 { values, .. } => {
                if values.is_null(row) {
                    Ok(Value::Null)
                } else {
                    Ok(Value::Number(Number::from(values.value(row))))
                }
            }
            Self::Float64 { values, .. } => {
                if values.is_null(row) {
                    Ok(Value::Null)
                } else {
                    Number::from_f64(values.value(row))
                        .map(Value::Number)
                        .ok_or_else(|| {
                            EncodingError::ParseError(
                                self.name(),
                                format!("row {row}: invalid f64 value"),
                            )
                        })
                }
            }
            Self::Boolean { values, .. } => {
                if values.is_null(row) {
                    Ok(Value::Null)
                } else {
                    Ok(Value::Bool(values.value(row)))
                }
            }
        }
    }
}

fn decode_column_ref(
    columns: &[ArrayRef],
    field: JsonFieldSpec,
    index: usize,
) -> Result<ColumnRef<'_>, EncodingError> {
    match field.encoding {
        JsonFieldEncoding::Utf8 => Ok(ColumnRef::Utf8 {
            name: field.name,
            values: extract_column_string(columns, field.name, index)?,
        }),
        JsonFieldEncoding::Utf8Json => Ok(ColumnRef::Utf8Json {
            name: field.name,
            values: extract_column_string(columns, field.name, index)?,
        }),
        JsonFieldEncoding::UInt64 => Ok(ColumnRef::UInt64 {
            name: field.name,
            values: extract_column::<UInt64Array>(columns, field.name, index, DataType::UInt64)?,
        }),
        JsonFieldEncoding::Float64 => Ok(ColumnRef::Float64 {
            name: field.name,
            values: extract_column::<Float64Array>(columns, field.name, index, DataType::Float64)?,
        }),
        JsonFieldEncoding::Boolean => Ok(ColumnRef::Boolean {
            name: field.name,
            values: extract_column::<BooleanArray>(columns, field.name, index, DataType::Boolean)?,
        }),
    }
}

fn values_is_null(values: &StringColumnRef<'_>, row: usize) -> bool {
    match values {
        StringColumnRef::Utf8(values) => values.is_null(row),
        StringColumnRef::Utf8View(values) => values.is_null(row),
    }
}

fn invalid_argument(message: String) -> ArrowError {
    ArrowError::InvalidArgumentError(message)
}
