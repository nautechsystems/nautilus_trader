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

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::Arc,
};

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

use super::{
    EncodingError, KEY_INSTRUMENT_ID, StringColumnRef, extract_column, extract_column_string,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonFieldEncoding {
    Utf8,
    Utf8Json,
    /// Exact decimal written as `Utf8`, read back from `Utf8`, `Utf8View`, or `Float64`.
    ///
    /// The `Float64` case is what lets catalog files written before a field moved from `f64` to
    /// `Decimal` keep decoding: `Decimal`'s `Deserialize` accepts both a JSON string and a JSON
    /// number, so no version discriminator is needed.
    DecimalStr,
    UInt64,
    Float64,
    Boolean,
    BooleanDefaultTrue,
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
    pub const fn decimal_str(name: &'static str, nullable: bool) -> Self {
        Self {
            name,
            encoding: JsonFieldEncoding::DecimalStr,
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

    #[must_use]
    pub const fn boolean_default_true(name: &'static str) -> Self {
        Self {
            name,
            encoding: JsonFieldEncoding::BooleanDefaultTrue,
            nullable: true,
        }
    }

    fn field(self) -> Field {
        let data_type = match self.encoding {
            JsonFieldEncoding::Utf8
            | JsonFieldEncoding::Utf8Json
            | JsonFieldEncoding::DecimalStr => DataType::Utf8,
            JsonFieldEncoding::UInt64 => DataType::UInt64,
            JsonFieldEncoding::Float64 => DataType::Float64,
            JsonFieldEncoding::Boolean | JsonFieldEncoding::BooleanDefaultTrue => DataType::Boolean,
        };

        Field::new(self.name, data_type, self.nullable)
    }
}

const KEY_TYPE: &str = "type";

#[must_use]
pub fn metadata_for_type(type_name: &'static str) -> HashMap<String, String> {
    HashMap::from([(KEY_TYPE.to_string(), type_name.to_string())])
}

/// Builds schema metadata for `type_name` scoped to a single instrument.
#[must_use]
pub fn instrument_metadata(
    type_name: &'static str,
    instrument_id: &str,
) -> HashMap<String, String> {
    let mut metadata = metadata_for_type(type_name);
    metadata.insert(KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string());
    metadata
}

#[must_use]
pub fn schema_for_type(
    type_name: &'static str,
    metadata: Option<HashMap<String, String>>,
    fields: &[JsonFieldSpec],
) -> Schema {
    let mut merged = metadata.unwrap_or_default();
    merged.insert(KEY_TYPE.to_string(), type_name.to_string());

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
    if let Some(name) = duplicate_field_name(fields) {
        return Err(invalid_argument(format!(
            "Duplicate field specification `{name}`"
        )));
    }

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
    if let Some(name) = duplicate_field_name(fields) {
        return Err(EncodingError::ParseError(
            name,
            "duplicate field specification".to_string(),
        ));
    }

    let schema = record_batch.schema();
    let columns: Result<Vec<_>, EncodingError> = fields
        .iter()
        .enumerate()
        .map(|(expected_index, field)| {
            let column_index = column_index(&schema, field.name, expected_index)?;
            decode_column_ref(record_batch.columns(), *field, column_index)
        })
        .collect();
    let columns = columns?;

    let mut decoded = Vec::with_capacity(record_batch.num_rows());
    let type_name = metadata
        .get(KEY_TYPE)
        .cloned()
        .or_else(|| fallback_type_name.map(str::to_string));

    for row in 0..record_batch.num_rows() {
        let mut value = Map::new();
        if let Some(type_name) = &type_name {
            value.insert(KEY_TYPE.to_string(), Value::String(type_name.clone()));
        }

        for column in &columns {
            value.insert(column.name.to_string(), column.to_json(row)?);
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

/// Returns the field specifications to decode `record_batch` with.
///
/// Names in `compatible_missing` are columns added after the schema first shipped. They are
/// dropped from the specification when the batch predates all of them, and required when the
/// batch carries any of them, so a partially written schema is rejected rather than decoded
/// from shifted columns.
///
/// # Errors
///
/// Returns [`EncodingError::MissingColumn`] if only some of `compatible_missing` are present.
pub fn fields_for_schema<'a>(
    record_batch: &RecordBatch,
    fields: &'a [JsonFieldSpec],
    compatible_missing: &[&'static str],
) -> Result<Cow<'a, [JsonFieldSpec]>, EncodingError> {
    if compatible_missing.is_empty() {
        return Ok(Cow::Borrowed(fields));
    }

    let schema = record_batch.schema();
    let mut present = 0;
    let mut missing = None;

    for (index, field) in fields.iter().enumerate() {
        if !compatible_missing.contains(&field.name) {
            continue;
        }

        if schema.index_of(field.name).is_ok() {
            present += 1;
        } else if missing.is_none() {
            missing = Some((field.name, index));
        }
    }

    match missing {
        None => Ok(Cow::Borrowed(fields)),
        Some((name, index)) if present > 0 => Err(EncodingError::MissingColumn(name, index)),
        Some(_) => Ok(Cow::Owned(
            fields
                .iter()
                .copied()
                .filter(|field| !compatible_missing.contains(&field.name))
                .collect(),
        )),
    }
}

fn duplicate_field_name(fields: &[JsonFieldSpec]) -> Option<&'static str> {
    let mut names = HashSet::with_capacity(fields.len());
    fields
        .iter()
        .find_map(|field| (!names.insert(field.name)).then_some(field.name))
}

fn column_index(
    schema: &Schema,
    name: &'static str,
    expected_index: usize,
) -> Result<usize, EncodingError> {
    let mut matches = schema
        .fields()
        .iter()
        .enumerate()
        .filter(|(_, field)| field.name() == name);
    let Some((index, _)) = matches.next() else {
        return Err(EncodingError::MissingColumn(name, expected_index));
    };

    if matches.next().is_some() {
        return Err(EncodingError::ParseError(
            name,
            "duplicate column name".to_string(),
        ));
    }

    Ok(index)
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
        JsonFieldEncoding::Utf8 | JsonFieldEncoding::DecimalStr => encode_utf8_column(field, rows),
        JsonFieldEncoding::Utf8Json => encode_utf8_json_column(field, rows),
        JsonFieldEncoding::UInt64 => encode_u64_column(field, rows),
        JsonFieldEncoding::Float64 => encode_f64_column(field, rows),
        JsonFieldEncoding::Boolean | JsonFieldEncoding::BooleanDefaultTrue => {
            encode_bool_column(field, rows)
        }
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

struct ColumnRef<'a> {
    name: &'static str,
    values: ColumnValues<'a>,
}

enum ColumnValues<'a> {
    Utf8(StringColumnRef<'a>),
    Utf8Json(StringColumnRef<'a>),
    DecimalStr(DecimalColumnRef<'a>),
    UInt64(&'a UInt64Array),
    Float64(&'a Float64Array),
    Boolean {
        values: &'a BooleanArray,
        default: Option<bool>,
    },
}

impl ColumnRef<'_> {
    fn to_json(&self, row: usize) -> Result<Value, EncodingError> {
        match &self.values {
            ColumnValues::Utf8(values) => Ok(string_to_json(values, row)),
            ColumnValues::Utf8Json(values) => {
                if values_is_null(values, row) {
                    Ok(Value::Null)
                } else {
                    serde_json::from_str(values.value(row)).map_err(|e| {
                        EncodingError::ParseError(self.name, format!("row {row}: {e}"))
                    })
                }
            }
            ColumnValues::DecimalStr(DecimalColumnRef::Str(values)) => {
                Ok(string_to_json(values, row))
            }
            ColumnValues::DecimalStr(DecimalColumnRef::Float64(values)) => {
                f64_to_json(self.name, values, row)
            }
            ColumnValues::UInt64(values) => {
                if values.is_null(row) {
                    Ok(Value::Null)
                } else {
                    Ok(Value::Number(Number::from(values.value(row))))
                }
            }
            ColumnValues::Float64(values) => f64_to_json(self.name, values, row),
            ColumnValues::Boolean { values, default } => {
                if values.is_null(row) {
                    Ok(default.map_or(Value::Null, Value::Bool))
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
    let name = field.name;
    let values = match field.encoding {
        JsonFieldEncoding::Utf8 => ColumnValues::Utf8(extract_column_string(columns, name, index)?),
        JsonFieldEncoding::Utf8Json => {
            ColumnValues::Utf8Json(extract_column_string(columns, name, index)?)
        }
        JsonFieldEncoding::DecimalStr => {
            ColumnValues::DecimalStr(extract_column_decimal(columns, name, index)?)
        }
        JsonFieldEncoding::UInt64 => ColumnValues::UInt64(extract_column::<UInt64Array>(
            columns,
            name,
            index,
            DataType::UInt64,
        )?),
        JsonFieldEncoding::Float64 => ColumnValues::Float64(extract_column::<Float64Array>(
            columns,
            name,
            index,
            DataType::Float64,
        )?),
        JsonFieldEncoding::Boolean | JsonFieldEncoding::BooleanDefaultTrue => {
            ColumnValues::Boolean {
                values: extract_column::<BooleanArray>(columns, name, index, DataType::Boolean)?,
                default: (field.encoding == JsonFieldEncoding::BooleanDefaultTrue).then_some(true),
            }
        }
    };

    Ok(ColumnRef { name, values })
}

// Reference to a decimal column, either the current `Utf8`/`Utf8View` form or the `Float64`
// form written before the field became exact.
enum DecimalColumnRef<'a> {
    Str(StringColumnRef<'a>),
    Float64(&'a Float64Array),
}

fn extract_column_decimal<'a>(
    columns: &'a [ArrayRef],
    column_key: &'static str,
    column_index: usize,
) -> Result<DecimalColumnRef<'a>, EncodingError> {
    extract_column_string(columns, column_key, column_index)
        .map(DecimalColumnRef::Str)
        .or_else(|e| {
            extract_column::<Float64Array>(columns, column_key, column_index, DataType::Float64)
                .map(DecimalColumnRef::Float64)
                .map_err(|_| e)
        })
}

fn string_to_json(values: &StringColumnRef<'_>, row: usize) -> Value {
    if values_is_null(values, row) {
        Value::Null
    } else {
        Value::String(values.value(row).to_string())
    }
}

fn f64_to_json(
    name: &'static str,
    values: &Float64Array,
    row: usize,
) -> Result<Value, EncodingError> {
    if values.is_null(row) {
        return Ok(Value::Null);
    }

    Number::from_f64(values.value(row))
        .map(Value::Number)
        .ok_or_else(|| EncodingError::ParseError(name, format!("row {row}: invalid f64 value")))
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

/// Implements the Arrow schema, encode, and decode traits for a type serialized through
/// [`JsonFieldSpec`] columns.
///
/// The leading keyword selects how [`EncodeToRecordBatch::metadata`] is built: `instrument`
/// scopes the batch to `self.instrument_id`, `typed` carries only the type name. The optional
/// trailing argument lists columns added after the schema first shipped; see
/// [`fields_for_schema`].
///
/// [`EncodeToRecordBatch::metadata`]: crate::arrow::EncodeToRecordBatch::metadata
macro_rules! impl_json_arrow {
    (instrument $type:ty, $type_name:expr, $fields:expr) => {
        impl_json_arrow!(instrument $type, $type_name, $fields, &[]);
    };

    (instrument $type:ty, $type_name:expr, $fields:expr, $compatible_missing:expr) => {
        impl_json_arrow!(@schema $type, $type_name, $fields, $compatible_missing);

        impl $crate::arrow::EncodeToRecordBatch for $type {
            fn encode_batch(
                metadata: &std::collections::HashMap<String, String>,
                data: &[Self],
            ) -> Result<arrow::record_batch::RecordBatch, arrow::error::ArrowError> {
                $crate::arrow::json::encode_batch($type_name, metadata, data, $fields)
            }

            fn metadata(&self) -> std::collections::HashMap<String, String> {
                $crate::arrow::json::instrument_metadata(
                    $type_name,
                    &self.instrument_id.to_string(),
                )
            }
        }
    };

    (typed $type:ty, $type_name:expr, $fields:expr) => {
        impl_json_arrow!(typed $type, $type_name, $fields, &[]);
    };

    (typed $type:ty, $type_name:expr, $fields:expr, $compatible_missing:expr) => {
        impl_json_arrow!(@schema $type, $type_name, $fields, $compatible_missing);

        impl $crate::arrow::EncodeToRecordBatch for $type {
            fn encode_batch(
                metadata: &std::collections::HashMap<String, String>,
                data: &[Self],
            ) -> Result<arrow::record_batch::RecordBatch, arrow::error::ArrowError> {
                $crate::arrow::json::encode_batch($type_name, metadata, data, $fields)
            }

            fn metadata(&self) -> std::collections::HashMap<String, String> {
                $crate::arrow::json::metadata_for_type($type_name)
            }
        }
    };

    (@schema $type:ty, $type_name:expr, $fields:expr, $compatible_missing:expr) => {
        impl $crate::arrow::ArrowSchemaProvider for $type {
            fn get_schema(
                metadata: Option<std::collections::HashMap<String, String>>,
            ) -> arrow::datatypes::Schema {
                $crate::arrow::json::schema_for_type($type_name, metadata, $fields)
            }
        }

        impl $crate::arrow::DecodeTypedFromRecordBatch for $type {
            fn decode_typed_batch(
                metadata: &std::collections::HashMap<String, String>,
                record_batch: arrow::record_batch::RecordBatch,
            ) -> Result<Vec<Self>, $crate::arrow::EncodingError> {
                let fields = $crate::arrow::json::fields_for_schema(
                    &record_batch,
                    $fields,
                    $compatible_missing,
                )?;
                $crate::arrow::json::decode_batch(
                    metadata,
                    &record_batch,
                    &fields,
                    Some($type_name),
                )
            }
        }
    };
}

pub(crate) use impl_json_arrow;

#[cfg(test)]
mod tests {
    use arrow::array::StringViewArray;
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_encode_batch_rejects_missing_required_field() {
        let fields = [JsonFieldSpec::utf8("required", false)];

        let error = encode_batch("TestRecord", &HashMap::new(), &[json!({})], &fields)
            .expect_err("required field must be present");

        let ArrowError::InvalidArgumentError(message) = error else {
            panic!("unexpected error variant: {error:?}");
        };
        assert_eq!(message, "Missing required field `required`");
    }

    #[rstest]
    fn test_encode_batch_rejects_duplicate_field_specifications() {
        let fields = [
            JsonFieldSpec::utf8("label", false),
            JsonFieldSpec::utf8("label", false),
        ];

        let error = encode_batch(
            "TestRecord",
            &HashMap::new(),
            &[json!({"label": "value"})],
            &fields,
        )
        .expect_err("duplicate field specification must be rejected");

        let ArrowError::InvalidArgumentError(message) = error else {
            panic!("unexpected error variant: {error:?}");
        };
        assert_eq!(message, "Duplicate field specification `label`");
    }

    #[rstest]
    fn test_decode_batch_rejects_invalid_json_from_utf8_view() {
        let fields = [JsonFieldSpec::utf8_json("payload", false)];
        let schema = Schema::new(vec![Field::new("payload", DataType::Utf8View, false)]);
        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![Arc::new(StringViewArray::from(vec!["[1,"]))],
        )
        .unwrap();

        let error = decode_batch::<Value>(&HashMap::new(), &batch, &fields, None)
            .expect_err("malformed JSON must be rejected");

        let EncodingError::ParseError(field, message) = error else {
            panic!("unexpected error variant: {error:?}");
        };
        assert_eq!(field, "payload");
        assert!(message.starts_with("row 0:"));
    }

    #[rstest]
    fn test_decode_batch_matches_columns_by_name() {
        let fields = [
            JsonFieldSpec::utf8("label", false),
            JsonFieldSpec::u64("value", false),
        ];
        let schema = Schema::new(vec![
            Field::new("value", DataType::UInt64, false),
            Field::new("label", DataType::Utf8, false),
        ]);
        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(UInt64Array::from(vec![42])),
                Arc::new(arrow::array::StringArray::from(vec!["answer"])),
            ],
        )
        .unwrap();

        let decoded = decode_batch::<Value>(&HashMap::new(), &batch, &fields, None).unwrap();

        assert_eq!(decoded, vec![json!({"label": "answer", "value": 42})]);
    }

    #[rstest]
    fn test_decode_batch_uses_true_default_for_null_boolean() {
        let fields = [JsonFieldSpec::boolean_default_true("enabled")];
        let schema = Schema::new(vec![Field::new("enabled", DataType::Boolean, true)]);
        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![Arc::new(BooleanArray::from(vec![None]))],
        )
        .unwrap();

        let decoded = decode_batch::<Value>(&HashMap::new(), &batch, &fields, None).unwrap();

        assert_eq!(decoded, vec![json!({"enabled": true})]);
    }

    #[rstest]
    fn test_decode_batch_rejects_duplicate_column_names() {
        let fields = [JsonFieldSpec::utf8("label", false)];
        let schema = Schema::new(vec![
            Field::new("label", DataType::Utf8, false),
            Field::new("label", DataType::Utf8, false),
        ]);
        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(arrow::array::StringArray::from(vec!["first"])),
                Arc::new(arrow::array::StringArray::from(vec!["second"])),
            ],
        )
        .unwrap();

        let error = decode_batch::<Value>(&HashMap::new(), &batch, &fields, None)
            .expect_err("duplicate column must be rejected");

        let EncodingError::ParseError(field, message) = error else {
            panic!("unexpected error variant: {error:?}");
        };
        assert_eq!(field, "label");
        assert_eq!(message, "duplicate column name");
    }

    #[rstest]
    fn test_decode_batch_rejects_duplicate_field_specifications() {
        let fields = [
            JsonFieldSpec::utf8("label", false),
            JsonFieldSpec::utf8("label", false),
        ];
        let schema = Schema::new(vec![Field::new("label", DataType::Utf8, false)]);
        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![Arc::new(arrow::array::StringArray::from(vec!["value"]))],
        )
        .unwrap();

        let error = decode_batch::<Value>(&HashMap::new(), &batch, &fields, None)
            .expect_err("duplicate field specification must be rejected");

        let EncodingError::ParseError(field, message) = error else {
            panic!("unexpected error variant: {error:?}");
        };
        assert_eq!(field, "label");
        assert_eq!(message, "duplicate field specification");
    }
}
