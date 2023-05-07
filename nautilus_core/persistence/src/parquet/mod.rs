// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

mod implementations;

use std::collections::HashMap;

use datafusion::arrow::datatypes::SchemaRef;
use datafusion::arrow::record_batch::RecordBatch;
use nautilus_model::data::Data;
use pyo3::prelude::*;

#[repr(C)]
#[pyclass]
#[derive(Debug, Clone, Copy)]
pub enum ParquetType {
    QuoteTick = 0,
    TradeTick = 1,
}

#[repr(C)]
#[pyclass]
#[derive(Debug, Clone, Copy)]
pub enum ParquetReaderType {
    File = 0,
    Buffer = 1,
}

pub trait DecodeDataFromRecordBatch
where
    Self: Sized + Into<Data>,
{
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Data>;
    fn get_schema(metadata: HashMap<String, String>) -> SchemaRef;
}
