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

//! Results from completed backtest runs.

use ahash::AHashMap;
use nautilus_core::{UUID4, UnixNanos};

/// Results from a completed backtest run.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.backtest",
        skip_from_py_object
    )
)]
pub struct BacktestResult {
    pub trader_id: String,
    pub machine_id: String,
    pub instance_id: UUID4,
    pub run_config_id: Option<String>,
    pub run_id: Option<UUID4>,
    pub run_started: Option<UnixNanos>,
    pub run_finished: Option<UnixNanos>,
    pub backtest_start: Option<UnixNanos>,
    pub backtest_end: Option<UnixNanos>,
    pub elapsed_time_secs: f64,
    pub iterations: usize,
    pub total_events: usize,
    pub total_orders: usize,
    pub total_positions: usize,
    pub stats_pnls: AHashMap<String, AHashMap<String, f64>>,
    pub stats_returns: AHashMap<String, f64>,
    pub stats_general: AHashMap<String, f64>,
}
