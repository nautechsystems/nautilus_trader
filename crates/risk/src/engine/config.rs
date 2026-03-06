//! Provides a configuration for `RiskEngine` instances.

use ahash::AHashMap;
use nautilus_common::throttler::RateLimit;
use nautilus_core::datetime::NANOSECONDS_IN_SECOND;
use nautilus_model::identifiers::InstrumentId;
use rust_decimal::Decimal;

/// Configuration for `RiskEngineConfig` instances.
#[derive(Debug, Clone)]
pub struct RiskEngineConfig {
    pub bypass: bool,
    pub max_order_submit: RateLimit,
    pub max_order_modify: RateLimit,
    pub max_notional_per_order: AHashMap<InstrumentId, Decimal>,
    pub debug: bool,
}

impl Default for RiskEngineConfig {
    /// Creates a new [`RiskEngineConfig`] instance.
    fn default() -> Self {
        Self {
            bypass: false,
            max_order_submit: RateLimit::new(100, NANOSECONDS_IN_SECOND),
            max_order_modify: RateLimit::new(100, NANOSECONDS_IN_SECOND),
            max_notional_per_order: AHashMap::new(),
            debug: false,
        }
    }
}
