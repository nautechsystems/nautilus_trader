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

//! Simulation module trait for extending backtesting with custom venue behaviors.

pub mod fx_rollover;

use ahash::AHashMap;
pub use fx_rollover::FXRolloverInterestModule;
use nautilus_common::cache::Cache;
use nautilus_core::UnixNanos;
use nautilus_execution::matching_engine::engine::OrderMatchingEngine;
use nautilus_model::{
    data::Data,
    identifiers::{InstrumentId, Venue},
    instruments::InstrumentAny,
    types::{Currency, Money},
};

/// Read-only view of exchange state passed to simulation modules during processing.
#[derive(Debug)]
pub struct ExchangeContext<'a> {
    /// The venue identifier.
    pub venue: Venue,
    /// The optional base currency for single-currency accounts.
    pub base_currency: Option<Currency>,
    /// All instruments registered on the exchange.
    pub instruments: &'a AHashMap<InstrumentId, InstrumentAny>,
    /// All matching engines, providing order book access.
    pub matching_engines: &'a AHashMap<InstrumentId, OrderMatchingEngine>,
    /// Read-only cache access for querying positions and other state.
    pub cache: &'a Cache,
}

/// Trait for custom simulation modules that extend backtesting functionality.
///
/// Implementations can add specialized behavior such as rollover interest,
/// market makers, price impact models, or other venue-specific simulation
/// logic that runs alongside the core backtesting engine.
///
/// Modules use interior mutability (`Cell`/`RefCell`) for state since they
/// are stored inside `SimulatedExchange` and invoked through shared references.
pub trait SimulationModule {
    /// Pre-processes market data before matching engine processing.
    fn pre_process(&self, data: &Data);

    /// Processes simulation logic at the given timestamp.
    ///
    /// Returns account balance adjustments to be applied by the exchange.
    fn process(&self, ts_now: UnixNanos, ctx: &ExchangeContext) -> Vec<Money>;

    /// Logs diagnostic information about the module's state.
    fn log_diagnostics(&self);

    /// Resets the module to its initial state.
    fn reset(&self);
}
