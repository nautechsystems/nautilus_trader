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

//! Arrow IPC conversion for Python catalog bindings.

use std::{io::Cursor, sync::Arc};

use arrow::{datatypes::Schema, ipc::reader::StreamReader, record_batch::RecordBatch};
use nautilus_model::data::{NautilusDataType, NautilusRecordType};
use nautilus_serialization::arrow::catalog_display::{catalog_display_schema, catalog_raw_schema};
use pyo3::{exceptions::PyIOError, prelude::*};

use super::to_pyio_err;
use crate::common::arrow::catalog_record_schema;

pub(crate) fn arrow_ipc_data_schema(
    data_type: &NautilusDataType,
    batches: &[RecordBatch],
    display: bool,
) -> PyResult<Schema> {
    arrow_ipc_schema(batches, || {
        if display {
            catalog_display_schema(data_type).map_err(to_pyio_err)
        } else {
            catalog_raw_schema(data_type).map_err(to_pyio_err)
        }
    })
}

pub(crate) fn arrow_ipc_record_schema(
    record_type: &NautilusRecordType,
    batches: &[RecordBatch],
) -> PyResult<Schema> {
    arrow_ipc_schema(batches, || {
        catalog_record_schema(record_type).map_err(to_pyio_err)
    })
}

fn arrow_ipc_schema(
    batches: &[RecordBatch],
    empty_schema: impl FnOnce() -> PyResult<Schema>,
) -> PyResult<Schema> {
    match batches.first() {
        Some(first) => {
            let fields = first.schema().fields().clone();

            if batches
                .iter()
                .any(|batch| batch.schema().fields() != &fields)
            {
                return Err(PyIOError::new_err(
                    "Arrow IPC result batches do not share one physical schema",
                ));
            }
            let metadata = first.schema().metadata().clone();

            if batches
                .iter()
                .any(|batch| batch.schema().metadata() != &metadata)
            {
                Ok(Schema::new(fields))
            } else {
                Ok(first.schema().as_ref().clone())
            }
        }
        None => empty_schema(),
    }
}

pub(crate) fn arrow_ipc_batches(
    schema: &Schema,
    batches: Vec<RecordBatch>,
) -> PyResult<Vec<RecordBatch>> {
    let schema = Arc::new(schema.clone());
    batches
        .into_iter()
        .map(|batch| {
            if batch.schema().as_ref() == schema.as_ref() {
                Ok(batch)
            } else {
                RecordBatch::try_new(Arc::clone(&schema), batch.columns().to_vec())
                    .map_err(to_pyio_err)
            }
        })
        .collect()
}

pub(crate) fn arrow_record_batches_from_pybytes(data: Vec<u8>) -> PyResult<Vec<RecordBatch>> {
    let cursor = Cursor::new(data);
    let reader = StreamReader::try_new(cursor, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to decode Arrow IPC bytes: {e}")))?;
    reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| PyIOError::new_err(format!("Failed to decode Arrow IPC bytes: {e}")))
}
