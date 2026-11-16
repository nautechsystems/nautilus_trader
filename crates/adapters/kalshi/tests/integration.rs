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

#[cfg(not(all(feature = "simulation", madsim)))]
use nautilus_kalshi::config::KalshiDataClientConfig;

#[path = "integration/enums.rs"]
mod enums;

#[path = "common/http_request.rs"]
mod http_request;

#[path = "integration/instruments.rs"]
mod instruments;

#[allow(
    dead_code,
    reason = "shared key fixtures also serve the crate unit tests"
)]
#[path = "integration/keys.rs"]
mod keys;

#[path = "integration/rest.rs"]
mod rest;

#[cfg(not(all(feature = "simulation", madsim)))]
#[path = "integration/data.rs"]
mod data;

#[cfg(not(all(feature = "simulation", madsim)))]
#[allow(
    dead_code,
    reason = "shared peer fields also serve data and execution schedules"
)]
#[path = "common/data_peer.rs"]
mod data_peer;

#[cfg(all(feature = "simulation", madsim))]
#[path = "integration/http.rs"]
mod dst;

#[cfg(not(all(feature = "simulation", madsim)))]
#[path = "integration/execution.rs"]
mod execution;

#[cfg(not(all(feature = "simulation", madsim)))]
#[path = "integration/execution_stream.rs"]
mod execution_stream;

#[cfg(not(all(feature = "simulation", madsim)))]
#[path = "integration/history.rs"]
mod history;

#[cfg(not(all(feature = "simulation", madsim)))]
#[path = "integration/http.rs"]
mod http;

#[cfg(all(feature = "python", not(all(feature = "simulation", madsim))))]
#[path = "integration/python.rs"]
mod python;

#[cfg(not(all(feature = "simulation", madsim)))]
#[path = "integration/websocket.rs"]
mod websocket;

#[cfg(not(all(feature = "simulation", madsim)))]
#[path = "integration/ws_lifecycle.rs"]
mod ws_lifecycle;
