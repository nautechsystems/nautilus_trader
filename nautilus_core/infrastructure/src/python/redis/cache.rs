// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use bytes::Bytes;
use nautilus_core::{
    python::{to_pyruntime_err, to_pyvalue_err},
    uuid::UUID4,
};
use nautilus_model::identifiers::TraderId;
use pyo3::{prelude::*, types::PyBytes};

use crate::redis::cache::RedisCacheDatabase;

#[pymethods]
impl RedisCacheDatabase {
    #[new]
    fn py_new(trader_id: TraderId, instance_id: UUID4, config_json: Vec<u8>) -> PyResult<Self> {
        let config = serde_json::from_slice(&config_json).map_err(to_pyvalue_err)?;
        Self::new(trader_id, instance_id, config).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close()
    }

    #[pyo3(name = "flushdb")]
    fn py_flushdb(&mut self) {
        self.flushdb()
    }

    #[pyo3(name = "keys")]
    fn py_keys(&mut self, pattern: &str) -> PyResult<Vec<String>> {
        match self.keys(pattern) {
            Ok(keys) => Ok(keys),
            Err(e) => Err(to_pyruntime_err(e)),
        }
    }

    #[pyo3(name = "read")]
    fn py_read(&mut self, py: Python, key: &str) -> PyResult<Vec<PyObject>> {
        match self.read(key) {
            Ok(result) => {
                let vec_py_bytes = result
                    .into_iter()
                    .map(|r| PyBytes::new(py, &r.as_ref()).into())
                    .collect::<Vec<PyObject>>();
                Ok(vec_py_bytes)
            }
            Err(e) => Err(to_pyruntime_err(e)),
        }
    }

    #[pyo3(name = "insert")]
    fn py_insert(&mut self, key: String, payload: Vec<Vec<u8>>) -> PyResult<()> {
        let payload: Vec<Bytes> = payload.into_iter().map(Bytes::from).collect();
        self.insert(key, Some(payload)).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "update")]
    fn py_update(&mut self, key: String, payload: Vec<Vec<u8>>) -> PyResult<()> {
        let payload: Vec<Bytes> = payload.into_iter().map(Bytes::from).collect();
        self.update(key, Some(payload)).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "delete")]
    fn py_delete(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> PyResult<()> {
        let payload: Option<Vec<Bytes>> =
            payload.map(|vec| vec.into_iter().map(Bytes::from).collect());
        self.delete(key, payload).map_err(to_pyvalue_err)
    }
}
