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

//! Adds the `identifier` column to batches that predate it.
//!
//! Current encoders emit `identifier` themselves. Only legacy files and display conversions of
//! older data reach this code, where the identifier comes from the file path or the legacy
//! metadata instead of a column.

use std::sync::Arc;

use arrow::{
    array::StringArray,
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_serialization::arrow::KEY_IDENTIFIER;

/// Returns `schema` with the nullable `identifier` column appended if absent.
#[must_use]
pub(crate) fn schema_with_legacy_identifier(schema: &Schema) -> Schema {
    if schema.index_of(KEY_IDENTIFIER).is_ok() {
        return schema.clone();
    }

    let mut fields = schema.fields().iter().cloned().collect::<Vec<_>>();
    fields.push(Arc::new(Field::new(KEY_IDENTIFIER, DataType::Utf8, true)));

    Schema::new_with_metadata(fields, schema.metadata().clone())
}

/// Returns `batch` with an `identifier` column of `identifier_values` appended if absent.
///
/// # Errors
///
/// Returns an error if the value count differs from the row count or the batch cannot be rebuilt.
pub(crate) fn batch_with_legacy_identifier_values(
    batch: RecordBatch,
    identifier_values: Vec<Option<String>>,
) -> Result<RecordBatch, ArrowError> {
    if batch.schema().index_of(KEY_IDENTIFIER).is_ok() {
        return Ok(batch);
    }

    if identifier_values.len() != batch.num_rows() {
        return Err(ArrowError::InvalidArgumentError(format!(
            "identifier values length {} does not match record batch row count {}",
            identifier_values.len(),
            batch.num_rows()
        )));
    }

    let schema = schema_with_legacy_identifier(batch.schema().as_ref());
    let mut columns = batch.columns().to_vec();
    columns.push(Arc::new(StringArray::from(identifier_values)));

    RecordBatch::try_new(Arc::new(schema), columns)
}

/// Returns `batch` with `identifier` repeated in an `identifier` column appended if absent.
///
/// # Errors
///
/// Returns an error if the batch cannot be rebuilt.
pub(crate) fn batch_with_legacy_identifier(
    batch: RecordBatch,
    identifier: Option<&str>,
) -> Result<RecordBatch, ArrowError> {
    let values = vec![identifier.map(ToString::to_string); batch.num_rows()];

    batch_with_legacy_identifier_values(batch, values)
}
