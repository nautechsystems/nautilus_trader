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

//! Python bindings for Massive configuration.

use nautilus_network::websocket::TransportBackend;
use pyo3::pymethods;

use crate::{common::enums::MassiveDataFeed, config::MassiveDataClientConfig};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl MassiveDataClientConfig {
    /// Configuration for the Massive live data client.
    #[new]
    #[pyo3(signature = (
        api_key = None,
        base_url_rest = None,
        base_url_ws = None,
        feed = None,
        symbols = None,
        http_timeout_secs = None,
        adjusted_bars = None,
        bars_timestamp_on_close = None,
        transport_backend = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        base_url_rest: Option<String>,
        base_url_ws: Option<String>,
        feed: Option<MassiveDataFeed>,
        symbols: Option<Vec<String>>,
        http_timeout_secs: Option<u64>,
        adjusted_bars: Option<bool>,
        bars_timestamp_on_close: Option<bool>,
        transport_backend: Option<TransportBackend>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            base_url_rest,
            base_url_ws,
            feed: feed.unwrap_or(defaults.feed),
            symbols: symbols.unwrap_or(defaults.symbols),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            adjusted_bars: adjusted_bars.unwrap_or(defaults.adjusted_bars),
            bars_timestamp_on_close: bars_timestamp_on_close
                .unwrap_or(defaults.bars_timestamp_on_close),
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        }
    }

    #[getter]
    const fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    fn __repr__(&self) -> String {
        format!(
            "MassiveDataClientConfig(api_key={}, base_url_rest={:?}, base_url_ws={:?}, feed={:?}, symbols={:?}, http_timeout_secs={}, adjusted_bars={}, bars_timestamp_on_close={})",
            if self.api_key.is_some() {
                "<redacted>"
            } else {
                "None"
            },
            self.base_url_rest,
            self.base_url_ws,
            self.feed,
            self.symbols,
            self.http_timeout_secs,
            self.adjusted_bars,
            self.bars_timestamp_on_close,
        )
    }
}
