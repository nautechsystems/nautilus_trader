use alloy::primitives::U160;
use nautilus_model::defi::{PoolIdentifier, SharedDex};

/// Event emitted when a liquidity pool is initialized on a DEX.
///
/// This event typically occurs when a new pool is created and
/// the initial price and tick are set.
#[derive(Debug, Clone)]
pub struct InitializeEvent {
    /// The decentralized exchange where the event happened.
    pub dex: SharedDex,
    /// The unique identifier for the pool.
    pub pool_identifier: PoolIdentifier,
    /// The square root of the price ratio encoded as a fixed point number with 96 fractional bits.
    pub sqrt_price_x96: U160,
    /// The current tick of the pool.
    pub tick: i32,
}

impl InitializeEvent {
    pub fn new(
        dex: SharedDex,
        pool_identifier: PoolIdentifier,
        sqrt_price_x96: U160,
        tick: i32,
    ) -> Self {
        Self {
            dex,
            pool_identifier,
            sqrt_price_x96,
            tick,
        }
    }
}
