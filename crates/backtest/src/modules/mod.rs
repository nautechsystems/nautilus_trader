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

use std::fmt::Display;

use ahash::AHashMap;
pub use fx_rollover::FXRolloverInterestModule;
use indexmap::IndexMap;
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
    pub matching_engines: &'a IndexMap<InstrumentId, OrderMatchingEngine>,
    /// Read-only cache access for querying positions and other state.
    pub cache: &'a Cache,
}

#[derive(Debug, Clone)]
pub enum SimulationModuleAny {
    FXRolloverInterest(FXRolloverInterestModule),
}

impl SimulationModule for SimulationModuleAny {
    fn pre_process(&self, data: &Data) {
        match self {
            Self::FXRolloverInterest(module) => module.pre_process(data),
        }
    }

    fn process(&self, ts_now: UnixNanos, ctx: &ExchangeContext) -> SimulationModuleResult {
        match self {
            Self::FXRolloverInterest(module) => module.process(ts_now, ctx),
        }
    }

    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]) {
        match self {
            Self::FXRolloverInterest(module) => module.acknowledge(outcomes),
        }
    }

    fn log_diagnostics(&self) {
        match self {
            Self::FXRolloverInterest(module) => module.log_diagnostics(),
        }
    }

    fn reset(&self) {
        match self {
            Self::FXRolloverInterest(module) => module.reset(),
        }
    }
}

impl From<SimulationModuleAny> for Box<dyn SimulationModule> {
    fn from(value: SimulationModuleAny) -> Self {
        match value {
            SimulationModuleAny::FXRolloverInterest(module) => Box::new(module),
        }
    }
}

/// Result of processing a simulation module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulationModuleResult {
    /// The module does not yet have a complete batch of adjustments.
    NotReady,
    /// The module produced a complete batch, which may be empty.
    Completed(Vec<Money>),
}

/// Failure applying an account adjustment produced by a simulation module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountAdjustmentError {
    /// The adjusted total balance would exceed [`Money`] bounds.
    TotalOverflow(Currency),
    /// The adjusted free balance would exceed [`Money`] bounds.
    FreeBalanceOverflow(Currency),
    /// The account has no balance for the adjustment currency.
    MissingBalance(Currency),
    /// The exchange has no account for the venue.
    MissingAccount(Venue),
    /// Generating the updated account state failed.
    AccountStateGeneration(String),
}

impl Display for AccountAdjustmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TotalOverflow(currency) => {
                write!(
                    f,
                    "Cannot adjust account: {currency} total exceeds Money bounds"
                )
            }
            Self::FreeBalanceOverflow(currency) => write!(
                f,
                "Cannot adjust account: {currency} free balance exceeds Money bounds"
            ),
            Self::MissingBalance(currency) => {
                write!(
                    f,
                    "Cannot adjust account: no balance for currency {currency}"
                )
            }
            Self::MissingAccount(venue) => {
                write!(f, "Cannot adjust account: no account for venue {venue}")
            }
            Self::AccountStateGeneration(error) => {
                write!(
                    f,
                    "Cannot adjust account: failed to generate account state: {error}"
                )
            }
        }
    }
}

impl std::error::Error for AccountAdjustmentError {}

/// Outcome of applying an account adjustment produced by a simulation module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountAdjustmentOutcome {
    /// The adjustment was applied successfully.
    Applied,
    /// The adjustment could not be applied.
    Failed(AccountAdjustmentError),
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
    /// Returns a complete batch of account balance adjustments, or indicates
    /// that the module is not ready.
    fn process(&self, ts_now: UnixNanos, ctx: &ExchangeContext) -> SimulationModuleResult;

    /// Acknowledges the ordered application outcomes for a completed batch.
    ///
    /// This is called exactly once for every [`SimulationModuleResult::Completed`],
    /// including an empty batch.
    ///
    /// # Panics
    ///
    /// Implementations may panic if the outcome count does not match the
    /// completed batch or if no completed batch is awaiting acknowledgement.
    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]);

    /// Logs diagnostic information about the module's state.
    fn log_diagnostics(&self);

    /// Resets the module to its initial state.
    fn reset(&self);
}
