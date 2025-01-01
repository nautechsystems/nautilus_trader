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
use nautilus_model::data::bar::{Bar, BarType};
use nautilus_serialization::arrow::DecodeFromRecordBatch;
use pyo3::prelude::*;

#[pyclass]
pub struct BarDataWrangler {
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
    metadata: HashMap<String, String>,
}

#[pymethods]
impl BarDataWrangler {
    #[new]
    fn py_new(bar_type: &str, price_precision: u8, size_precision: u8) -> PyResult<Self> {
        let bar_type = BarType::from_str(bar_type).map_err(to_pyvalue_err)?;
        let metadata = Bar::get_metadata(&bar_type, price_precision, size_precision);

        Ok(Self {
            bar_type,
            price_precision,
            size_precision,
            metadata,
        })
    }

    #[getter]
    fn bar_type(&self) -> String {
        self.bar_type.to_string()
    }

    #[getter]
    const fn price_precision(&self) -> u8 {
        self.price_precision
    }

    #[getter]
    const fn size_precision(&self) -> u8 {
        self.size_precision
    }

    fn process_record_batch_bytes(&self, data: &[u8]) -> PyResult<Vec<Bar>> {
        // Create a StreamReader (from Arrow IPC)
        let cursor = Cursor::new(data);
        let reader = match StreamReader::try_new(cursor, None) {
            Ok(reader) => reader,
            Err(e) => return Err(to_pyvalue_err(e)),
        };

        let mut bars = Vec::new();

        // Read the record batches
        for maybe_batch in reader {
            let record_batch = match maybe_batch {
                Ok(record_batch) => record_batch,
                Err(e) => return Err(to_pyvalue_err(e)),
            };

            let batch_bars =
                Bar::decode_batch(&self.metadata, record_batch).map_err(to_pyvalue_err)?;
            bars.extend(batch_bars);
        }

        Ok(bars)
    }
}
