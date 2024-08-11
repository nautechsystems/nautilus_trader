// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 2Nautech Systems Pty Ltd. All rights reserved.
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

use std::any::Any;

use nautilus_model::data::Data;
use pyo3::prelude::*;
use ustr::Ustr;

use crate::{messages::data::DataResponse, msgbus::handler::MessageHandler};

#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
pub struct PythonMessageHandler {
    id: Ustr,
    handler: PyObject,
}

#[pymethods]
impl PythonMessageHandler {
    #[new]
    pub fn new(id: &str, handler: PyObject) -> Self {
        let id = Ustr::from(id);
        PythonMessageHandler { id, handler }
    }
}

impl MessageHandler for PythonMessageHandler {
    #[allow(unused_variables)]
    fn handle(&self, message: &dyn Any) {
        // TODO: convert message to PyObject
        let py_event = ();
        let result =
            pyo3::Python::with_gil(|py| self.handler.call_method1(py, "handle", (py_event,)));
        if let Err(err) = result {
            eprintln!("Error calling handle method: {:?}", err);
        }
    }

    fn id(&self) -> Ustr {
        self.id
    }

    fn handle_response(&self, resp: DataResponse) {
        // TODO: convert message to PyObject
        let py_event = ();
        let result =
            pyo3::Python::with_gil(|py| self.handler.call_method1(py, "handle", (py_event,)));
        if let Err(err) = result {
            eprintln!("Error calling handle method: {:?}", err);
        }
    }

    fn handle_data(&self, data: Data) {
        let py_event = ();
        let result =
            pyo3::Python::with_gil(|py| self.handler.call_method1(py, "handle", (py_event,)));
        if let Err(err) = result {
            eprintln!("Error calling handle method: {:?}", err);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
