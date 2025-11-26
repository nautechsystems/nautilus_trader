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

//! Python bindings for URL builder functions.

use pyo3::prelude::*;

use crate::common::{
    enums::{KrakenEnvironment, KrakenProductType},
    urls::{get_http_base_url, get_ws_private_url, get_ws_public_url},
};

#[pyfunction]
#[pyo3(name = "get_http_base_url")]
pub fn py_get_http_base_url(
    product_type: KrakenProductType,
    environment: KrakenEnvironment,
) -> String {
    get_http_base_url(product_type, environment).to_string()
}

#[pyfunction]
#[pyo3(name = "get_ws_public_url")]
pub fn py_get_ws_public_url(
    product_type: KrakenProductType,
    environment: KrakenEnvironment,
) -> String {
    get_ws_public_url(product_type, environment).to_string()
}

#[pyfunction]
#[pyo3(name = "get_ws_private_url")]
pub fn py_get_ws_private_url(
    product_type: KrakenProductType,
    environment: KrakenEnvironment,
) -> String {
    get_ws_private_url(product_type, environment).to_string()
}
