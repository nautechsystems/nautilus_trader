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

use std::collections::HashMap;

use nautilus_core::{
    python::{to_pyruntime_err, to_pyvalue_err},
    uuid::UUID4,
};
use nautilus_model::identifiers::trader_id::TraderId;
use pyo3::{prelude::*, PyResult};
use serde_json::Value;

use crate::{cache::CacheDatabase, redis::RedisCacheDatabase};

#[pymethods]
impl RedisCacheDatabase {
    #[new]
    fn py_new(trader_id: TraderId, instance_id: UUID4, config_json: Vec<u8>) -> PyResult<Self> {
        let config: HashMap<String, Value> =
            serde_json::from_slice(&config_json).map_err(to_pyvalue_err)?;

        match Self::new(trader_id, instance_id, config) {
            Ok(cache) => Ok(cache),
            Err(e) => Err(to_pyruntime_err(e.to_string())),
        }
    }

    #[pyo3(name = "read")]
    fn py_read(&mut self, op_type: String) -> PyResult<Vec<Vec<u8>>> {
        match self.read(op_type) {
            Ok(payload) => Ok(payload),
            Err(e) => Err(to_pyruntime_err(e)),
        }
    }

    #[pyo3(name = "write")]
    fn py_write(&mut self, op_type: String, payload: Vec<Vec<u8>>) -> PyResult<()> {
        match self.write(op_type, payload) {
            Ok(_) => Ok(()),
            Err(e) => Err(to_pyvalue_err(e)),
        }
    }
}
