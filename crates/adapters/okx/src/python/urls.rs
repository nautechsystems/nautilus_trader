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

//! Python wrapper functions for OKX URL helpers.

use pyo3::prelude::*;

use crate::common::{enums::OKXEnvironment, urls};

/// Returns the OKX HTTP base URL.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.okx")]
pub fn get_okx_http_base_url() -> String {
    urls::get_http_base_url().to_string()
}

/// Returns the OKX WebSocket URL for public data (market data).
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.okx")]
pub fn get_okx_ws_url_public(environment: OKXEnvironment) -> String {
    urls::get_ws_base_url_public(environment).to_string()
}

/// Returns the OKX WebSocket URL for private data (account/order management).
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.okx")]
pub fn get_okx_ws_url_private(environment: OKXEnvironment) -> String {
    urls::get_ws_base_url_private(environment).to_string()
}

/// Returns the OKX WebSocket URL for business data (bars/candlesticks).
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.okx")]
pub fn get_okx_ws_url_business(environment: OKXEnvironment) -> String {
    urls::get_ws_base_url_business(environment).to_string()
}

/// Derives a WebSocket URL for a given channel from a base URL.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.okx")]
pub fn derive_okx_ws_url(base_url: &str, channel: &str) -> String {
    urls::derive_ws_url(base_url, channel)
}

/// Checks if OKX endpoint requires authentication.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.okx")]
pub fn okx_requires_authentication(endpoint_type: urls::OKXEndpointType) -> bool {
    urls::requires_authentication(endpoint_type)
}
