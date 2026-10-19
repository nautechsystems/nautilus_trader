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

pub mod cfd_swap;
pub mod fx_rollover;

use std::{
    fmt::{Debug, Display},
    rc::Rc,
};

use ahash::AHashMap;
pub use cfd_swap::{CfdSwapModule, CfdSwapRate};
pub use fx_rollover::FXRolloverInterestModule;
use indexmap::IndexMap;
use nautilus_common::cache::Cache;
use nautilus_core::UnixNanos;
use nautilus_execution::matching_engine::OrderMatchingEngine;
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
    CfdSwap(CfdSwapModule),
    FXRolloverInterest(FXRolloverInterestModule),
    #[cfg(feature = "python")]
    Python(crate::python::modules::PythonSimulationModule),
}

impl SimulationModule for SimulationModuleAny {
    fn pre_process(&self, data: &Data) -> anyhow::Result<()> {
        match self {
            Self::CfdSwap(module) => module.pre_process(data),
            Self::FXRolloverInterest(module) => module.pre_process(data),
            #[cfg(feature = "python")]
            Self::Python(module) => module.pre_process(data),
        }
    }

    fn process(
        &self,
        ts_now: UnixNanos,
        ctx: &ExchangeContext,
    ) -> anyhow::Result<SimulationModuleResult> {
        match self {
            Self::CfdSwap(module) => module.process(ts_now, ctx),
            Self::FXRolloverInterest(module) => module.process(ts_now, ctx),
            #[cfg(feature = "python")]
            Self::Python(module) => module.process(ts_now, ctx),
        }
    }

    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
        match self {
            Self::CfdSwap(module) => module.acknowledge(outcomes),
            Self::FXRolloverInterest(module) => module.acknowledge(outcomes),
            #[cfg(feature = "python")]
            Self::Python(module) => module.acknowledge(outcomes),
        }
    }

    fn log_diagnostics(&self) -> anyhow::Result<()> {
        match self {
            Self::CfdSwap(module) => module.log_diagnostics(),
            Self::FXRolloverInterest(module) => module.log_diagnostics(),
            #[cfg(feature = "python")]
            Self::Python(module) => module.log_diagnostics(),
        }
    }

    fn reset(&self) -> anyhow::Result<()> {
        match self {
            Self::CfdSwap(module) => module.reset(),
            Self::FXRolloverInterest(module) => module.reset(),
            #[cfg(feature = "python")]
            Self::Python(module) => module.reset(),
        }
    }
}

/// Shared runtime handle for a simulation module.
///
/// Clones share the same module instance and state. Create a separate module for each venue or
/// run that requires isolated state.
#[derive(Clone)]
pub struct SimulationModuleHandle(Rc<dyn SimulationModule>);

impl SimulationModuleHandle {
    /// Creates a new [`SimulationModuleHandle`] from a simulation module.
    #[must_use]
    pub fn new<T>(module: T) -> Self
    where
        T: SimulationModule + 'static,
    {
        Self(Rc::new(module))
    }

    /// Creates a new [`SimulationModuleHandle`] from an existing reference-counted module.
    #[must_use]
    pub fn from_rc(module: Rc<dyn SimulationModule>) -> Self {
        Self(module)
    }
}

impl Debug for SimulationModuleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(stringify!(SimulationModuleHandle))
            .field(&"<dyn SimulationModule>")
            .finish()
    }
}

impl SimulationModule for SimulationModuleHandle {
    fn pre_process(&self, data: &Data) -> anyhow::Result<()> {
        self.0.pre_process(data)
    }

    fn process(
        &self,
        ts_now: UnixNanos,
        ctx: &ExchangeContext,
    ) -> anyhow::Result<SimulationModuleResult> {
        self.0.process(ts_now, ctx)
    }

    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
        self.0.acknowledge(outcomes)
    }

    fn log_diagnostics(&self) -> anyhow::Result<()> {
        self.0.log_diagnostics()
    }

    fn reset(&self) -> anyhow::Result<()> {
        self.0.reset()
    }
}

impl From<SimulationModuleAny> for SimulationModuleHandle {
    fn from(module: SimulationModuleAny) -> Self {
        Self::new(module)
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

impl AccountAdjustmentError {
    pub(crate) const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::TotalOverflow(_) | Self::FreeBalanceOverflow(_) | Self::AccountStateGeneration(_)
        )
    }
}

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
    ///
    /// # Errors
    ///
    /// Returns an error if the module cannot accept the data.
    fn pre_process(&self, data: &Data) -> anyhow::Result<()>;

    /// Processes simulation logic at the given timestamp.
    ///
    /// Returns a complete batch of account balance adjustments, or indicates
    /// that the module is not ready.
    ///
    /// # Errors
    ///
    /// Returns an error if the module cannot process the exchange state.
    fn process(
        &self,
        ts_now: UnixNanos,
        ctx: &ExchangeContext,
    ) -> anyhow::Result<SimulationModuleResult>;

    /// Acknowledges the ordered application outcomes for a completed batch.
    ///
    /// This is called exactly once for every [`SimulationModuleResult::Completed`],
    /// including an empty batch.
    ///
    /// # Errors
    ///
    /// Returns an error if the outcomes do not match the pending completed batch or the module
    /// cannot record them. The exchange treats an acknowledgement failure as terminal until reset
    /// because account adjustments may already have been applied.
    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()>;

    /// Logs diagnostic information about the module's state.
    ///
    /// # Errors
    ///
    /// Returns an error if the module cannot produce its diagnostics.
    fn log_diagnostics(&self) -> anyhow::Result<()>;

    /// Resets the module to its initial state.
    ///
    /// # Errors
    ///
    /// Returns an error if the module cannot reset its state.
    fn reset(&self) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use rstest::rstest;

    use super::*;

    #[derive(Debug)]
    struct CountingModule {
        resets: Rc<Cell<u32>>,
    }

    impl SimulationModule for CountingModule {
        fn pre_process(&self, _data: &Data) -> anyhow::Result<()> {
            Ok(())
        }

        fn process(
            &self,
            _ts_now: UnixNanos,
            _ctx: &ExchangeContext,
        ) -> anyhow::Result<SimulationModuleResult> {
            Ok(SimulationModuleResult::NotReady)
        }

        fn acknowledge(&self, _outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
            Ok(())
        }

        fn log_diagnostics(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&self) -> anyhow::Result<()> {
            self.resets.set(self.resets.get() + 1);
            Ok(())
        }
    }

    #[rstest]
    fn simulation_module_handle_from_rc_clones_shared_module() {
        let resets = Rc::new(Cell::new(0));
        let module: Rc<dyn SimulationModule> = Rc::new(CountingModule {
            resets: resets.clone(),
        });
        let handle = SimulationModuleHandle::from_rc(module);
        let cloned = handle.clone();

        handle.reset().unwrap();
        cloned.reset().unwrap();

        assert_eq!(resets.get(), 2);
        assert_eq!(
            format!("{handle:?}"),
            "SimulationModuleHandle(\"<dyn SimulationModule>\")"
        );
    }
}
