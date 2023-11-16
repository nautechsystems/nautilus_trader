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

use pyo3::prelude::*;

use crate::{http, ratelimiter, socket, websocket};

/// Loaded as nautilus_pyo3.network
#[pymodule]
pub fn network(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<http::HttpClient>()?;
    m.add_class::<http::HttpMethod>()?;
    m.add_class::<http::HttpResponse>()?;
    m.add_class::<ratelimiter::quota::Quota>()?;
    m.add_class::<websocket::WebSocketClient>()?;
    m.add_class::<websocket::WebSocketConfig>()?;
    m.add_class::<socket::SocketClient>()?;
    m.add_class::<socket::SocketConfig>()?;
    Ok(())
}
