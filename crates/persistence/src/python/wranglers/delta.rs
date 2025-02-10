// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, io::Cursor, str::FromStr};

use datafusion::arrow::ipc::reader::StreamReader;
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{data::OrderBookDelta, identifiers::InstrumentId};
use nautilus_serialization::arrow::DecodeFromRecordBatch;
use pyo3::prelude::*;

#[pyclass()]
pub struct OrderBookDeltaDataWrangler {
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    metadata: HashMap<String, String>,
}

#[pymethods]
impl OrderBookDeltaDataWrangler {
    #[new]
    fn py_new(instrument_id: &str, price_precision: u8, size_precision: u8) -> PyResult<Self> {
        let instrument_id = InstrumentId::from_str(instrument_id).map_err(to_pyvalue_err)?;
        let metadata =
            OrderBookDelta::get_metadata(&instrument_id, price_precision, size_precision);

        Ok(Self {
            instrument_id,
            price_precision,
            size_precision,
            metadata,
        })
    }

    #[getter]
    fn instrument_id(&self) -> String {
        self.instrument_id.to_string()
    }

    #[getter]
    const fn price_precision(&self) -> u8 {
        self.price_precision
    }

    #[getter]
    const fn size_precision(&self) -> u8 {
        self.size_precision
    }

    fn process_record_batch_bytes(&self, data: &[u8]) -> PyResult<Vec<OrderBookDelta>> {
        // Create a StreamReader (from Arrow IPC)
        let cursor = Cursor::new(data);
        let reader = match StreamReader::try_new(cursor, None) {
            Ok(reader) => reader,
            Err(e) => return Err(to_pyvalue_err(e)),
        };

        let mut deltas = Vec::new();

        // Read the record batches
        for maybe_batch in reader {
            let record_batch = match maybe_batch {
                Ok(record_batch) => record_batch,
                Err(e) => return Err(to_pyvalue_err(e)),
            };

            let batch_deltas = OrderBookDelta::decode_batch(&self.metadata, record_batch)
                .map_err(to_pyvalue_err)?;
            deltas.extend(batch_deltas);
        }

        Ok(deltas)
    }
}
