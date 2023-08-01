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

pub mod http;
pub mod socket;
pub mod websocket;

use http::{HttpClient, HttpResponse};
use pyo3::prelude::*;
use socket::SocketClient;
use tracing::metadata::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
use websocket::WebSocketClient;

#[pyclass]
struct LogGuard {
    guard: WorkerGuard,
}

#[pyfunction]
fn set_global_tracing_collector(file_path: Option<String>) -> LogGuard {
    if let Some(_file_path) = file_path {
        todo!()
    } else {
        let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());
        tracing_subscriber::fmt().with_max_level(LevelFilter::DEBUG).with_writer(non_blocking).init();
        LogGuard { guard }
    }
}

/// Loaded as nautilus_pyo3.network
#[pymodule]
pub fn network(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<HttpClient>()?;
    m.add_class::<HttpResponse>()?;
    m.add_class::<WebSocketClient>()?;
    m.add_class::<SocketClient>()?;
    m.add_class::<LogGuard>()?;
    m.add_function(wrap_pyfunction!(set_global_tracing_collector, m)?)?;
    Ok(())
}
