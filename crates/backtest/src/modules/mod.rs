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

use nautilus_common::logging::logger::Logger;
use nautilus_core::UnixNanos;
use nautilus_model::data::Data;

use crate::exchange::SimulatedExchange;

/// Trait for custom simulation modules that extend backtesting functionality.
///
/// The `SimulationModule` trait allows for custom extensions to the backtesting
/// simulation environment. Implementations can add specialized behavior such as
/// market makers, price impact models, or other venue-specific simulation logic
/// that runs alongside the core backtesting engine.
#[warn(dead_code)]
pub trait SimulationModule {
    /// Registers a simulated exchange venue with this module.
    fn register_venue(&self, exchange: SimulatedExchange);

    /// Pre-processes market data before main simulation processing.
    fn pre_process(&self, data: Data);

    /// Processes simulation logic at the given timestamp.
    fn process(&self, ts_now: UnixNanos);

    /// Logs diagnostic information about the module's state.
    fn log_diagnostics(&self, logger: Logger);

    /// Resets the module to its initial state.
    fn reset(&self);
}
