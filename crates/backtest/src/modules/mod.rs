//! Simulation module trait for extending backtesting with custom venue behaviors.

use nautilus_core::UnixNanos;
use nautilus_model::data::Data;

use crate::exchange::SimulatedExchange;

/// Trait for custom simulation modules that extend backtesting functionality.
///
/// The `SimulationModule` trait allows for custom extensions to the backtesting
/// simulation environment. Implementations can add specialized behavior such as
/// market makers, price impact models, or other venue-specific simulation logic
/// that runs alongside the core backtesting engine.
pub trait SimulationModule {
    /// Registers a simulated exchange venue with this module.
    fn register_venue(&self, exchange: SimulatedExchange);

    /// Pre-processes market data before main simulation processing.
    fn pre_process(&self, data: Data);

    /// Processes simulation logic at the given timestamp.
    fn process(&self, ts_now: UnixNanos);

    /// Logs diagnostic information about the module's state.
    fn log_diagnostics(&self);

    /// Resets the module to its initial state.
    fn reset(&self);
}
