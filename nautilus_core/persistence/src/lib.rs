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

mod kmerge_batch;
pub mod parquet;
pub mod session;

use parquet::ParquetType;
use pyo3::prelude::*;
use session::{DataBackendSession, DataQueryResult};

/// Loaded as nautilus_pyo3.persistence
#[pymodule]
pub fn persistence(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<ParquetType>()?;
    m.add_class::<DataBackendSession>()?;
    m.add_class::<DataQueryResult>()?;
    Ok(())
}
