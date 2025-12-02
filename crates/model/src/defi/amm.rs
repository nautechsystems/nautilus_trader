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

//! Data types specific to automated-market-maker (AMM) protocols.

use std::{fmt::Display, sync::Arc};

use alloy_primitives::{Address, U160};
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{
    data::HasTsInit,
    defi::{
        Blockchain, PoolIdentifier, SharedDex, chain::SharedChain, dex::Dex,
        tick_map::tick_math::get_tick_at_sqrt_ratio, token::Token,
    },
    identifiers::{InstrumentId, Symbol, Venue},
};

/// Represents a liquidity pool in a decentralized exchange.
///
/// ## Pool Identification Architecture
///
/// Pools are identified differently depending on the DEX protocol version:
///
/// **UniswapV2/V3**: Each pool has its own smart contract deployed at a unique address.
/// - `address` = pool contract address
/// - `pool_identifier` = same as address (hex string)
///
/// **UniswapV4**: All pools share a singleton PoolManager contract. Pools are distinguished
/// by a unique Pool ID (keccak256 hash of currencies, fee, tick spacing, and hooks).
/// - `address` = PoolManager contract address (shared by all pools)
/// - `pool_identifier` = Pool ID (bytes32 as hex string)
///
/// ## Instrument ID Format
///
/// The instrument ID encodes with the following components:
/// - `symbol` – The pool identifier (address for V2/V3, Pool ID for V4)
/// - `venue`  – The chain name plus DEX ID
///
/// String representation: `<POOL_IDENTIFIER>.<CHAIN_NAME>:<DEX_ID>`
///
/// Example: `0x11b815efB8f581194ae79006d24E0d814B7697F6.Ethereum:UniswapV3`
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pool {
    /// The blockchain network where this pool exists.
    pub chain: SharedChain,
    /// The decentralized exchange protocol that created and manages this pool.
    pub dex: SharedDex,
    /// The blockchain address where the pool smart contract code is deployed.
    pub address: Address,
    /// The unique identifier for this pool across all pools on the DEX.
    pub pool_identifier: PoolIdentifier,
    /// The instrument ID for the pool.
    pub instrument_id: InstrumentId,
    /// The block number when this pool was created on the blockchain.
    pub creation_block: u64,
    /// The first token in the trading pair.
    pub token0: Token,
    /// The second token in the trading pair.
    pub token1: Token,
    /// The trading fee tier used by the pool expressed in hundred-thousandths
    /// (1e-6) of one unit – identical to Uniswap-V3’s fee representation.
    ///
    /// Examples:
    /// • `500`   →  0.05 %  (5 bps)
    /// • `3_000` →  0.30 %  (30 bps)
    /// • `10_000`→  1.00 %
    pub fee: Option<u32>,
    /// The minimum tick spacing for positions in concentrated liquidity AMMs.
    pub tick_spacing: Option<u32>,
    /// The initial tick when the pool was first initialized.
    pub initial_tick: Option<i32>,
    /// The initial square root price when the pool was first initialized.
    pub initial_sqrt_price_x96: Option<U160>,
    /// The hooks contract address for Uniswap V4 pools.
    /// For V2/V3 pools, this will be None. For V4, it contains the hooks contract address.
    pub hooks: Option<Address>,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

/// A thread-safe shared pointer to a `Pool`, enabling efficient reuse across multiple components.
pub type SharedPool = Arc<Pool>;

impl Pool {
    /// Creates a new [`Pool`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain: SharedChain,
        dex: SharedDex,
        address: Address,
        pool_identifier: PoolIdentifier,
        creation_block: u64,
        token0: Token,
        token1: Token,
        fee: Option<u32>,
        tick_spacing: Option<u32>,
        ts_init: UnixNanos,
    ) -> Self {
        let instrument_id = Self::create_instrument_id(chain.name, &dex, pool_identifier.as_str());

        Self {
            chain,
            dex,
            address,
            pool_identifier,
            instrument_id,
            creation_block,
            token0,
            token1,
            fee,
            tick_spacing,
            initial_tick: None,
            initial_sqrt_price_x96: None,
            hooks: None,
            ts_init,
        }
    }

    /// Returns a formatted string representation of the pool for display purposes.
    pub fn to_full_spec_string(&self) -> String {
        format!(
            "{}/{}-{}.{}",
            self.token0.symbol,
            self.token1.symbol,
            self.fee.unwrap_or(0),
            self.instrument_id.venue
        )
    }

    /// Initializes the pool with the initial tick and square root price.
    ///
    /// This method should be called when an Initialize event is processed
    /// to set the initial price and tick values for the pool.
    ///
    /// # Panics
    ///
    /// Panics if the provided tick does not match the tick calculated from sqrt_price_x96.
    pub fn initialize(&mut self, sqrt_price_x96: U160, tick: i32) {
        let calculated_tick = get_tick_at_sqrt_ratio(sqrt_price_x96);

        assert_eq!(
            tick, calculated_tick,
            "Provided tick {tick} does not match calculated tick {calculated_tick} for sqrt_price_x96 {sqrt_price_x96}",
        );

        self.initial_sqrt_price_x96 = Some(sqrt_price_x96);
        self.initial_tick = Some(tick);
    }

    /// Sets the hooks contract address for this pool.
    ///
    /// This is typically called for Uniswap V4 pools that have hooks enabled.
    pub fn set_hooks(&mut self, hooks: Address) {
        self.hooks = Some(hooks);
    }

    pub fn create_instrument_id(
        chain: Blockchain,
        dex: &Dex,
        pool_identifier: &str,
    ) -> InstrumentId {
        let symbol = Symbol::new(pool_identifier);
        let venue = Venue::new(format!("{}:{}", chain, dex.name));
        InstrumentId::new(symbol, venue)
    }

    /// Returns the base token based on token priority.
    ///
    /// The base token is the asset being traded/priced. Token priority determines
    /// which token becomes base vs quote:
    /// - Lower priority number (1=stablecoin, 2=native, 3=other) = quote token
    /// - Higher priority number = base token
    pub fn get_base_token(&self) -> &Token {
        let priority0 = self.token0.get_token_priority();
        let priority1 = self.token1.get_token_priority();

        if priority0 < priority1 {
            &self.token1
        } else {
            &self.token0
        }
    }

    /// Returns the quote token based on token priority.
    ///
    /// The quote token is the pricing currency. Token priority determines
    /// which token becomes quote:
    /// - Lower priority number (1=stablecoin, 2=native, 3=other) = quote token
    pub fn get_quote_token(&self) -> &Token {
        let priority0 = self.token0.get_token_priority();
        let priority1 = self.token1.get_token_priority();

        if priority0 < priority1 {
            &self.token0
        } else {
            &self.token1
        }
    }

    /// Returns whether the base/quote order is inverted from token0/token1 order.
    ///
    /// # Returns
    /// - `true` if base=token1, quote=token0 (inverted from pool order)
    /// - `false` if base=token0, quote=token1 (matches pool order)
    ///
    /// # Use Case
    /// This is useful for knowing whether prices need to be inverted when
    /// converting from pool convention (token1/token0) to market convention (base/quote).
    pub fn is_base_quote_inverted(&self) -> bool {
        let priority0 = self.token0.get_token_priority();
        let priority1 = self.token1.get_token_priority();

        // Inverted when token0 has higher priority (becomes quote instead of base)
        priority0 < priority1
    }
}

impl Display for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Pool(instrument_id={}, dex={}, fee={}, address={})",
            self.instrument_id,
            self.dex.name,
            self.fee
                .map_or("None".to_string(), |fee| format!("fee={fee}, ")),
            self.address
        )
    }
}

impl HasTsInit for Pool {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rstest::rstest;

    use super::*;
    use crate::defi::{
        chain::chains,
        dex::{AmmType, Dex, DexType},
        token::Token,
    };

    #[rstest]
    fn test_pool_constructor_and_methods() {
        let chain = Arc::new(chains::ETHEREUM.clone());
        let dex = Dex::new(
            chains::ETHEREUM.clone(),
            DexType::UniswapV3,
            "0x1F98431c8aD98523631AE4a59f267346ea31F984",
            0,
            AmmType::CLAMM,
            "PoolCreated(address,address,uint24,int24,address)",
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
            "Collect(address,address,int24,int24,uint128,uint128)",
        );

        let token0 = Token::new(
            chain.clone(),
            "0xA0b86a33E6441b936662bb6B5d1F8Fb0E2b57A5D"
                .parse()
                .unwrap(),
            "Wrapped Ether".to_string(),
            "WETH".to_string(),
            18,
        );

        let token1 = Token::new(
            chain.clone(),
            "0xdAC17F958D2ee523a2206206994597C13D831ec7"
                .parse()
                .unwrap(),
            "Tether USD".to_string(),
            "USDT".to_string(),
            6,
        );

        let pool_address: Address = "0x11b815efB8f581194ae79006d24E0d814B7697F6"
            .parse()
            .unwrap();
        let pool_identifier = PoolIdentifier::from_address(pool_address);
        let ts_init = UnixNanos::from(1_234_567_890_000_000_000u64);

        let pool = Pool::new(
            chain.clone(),
            Arc::new(dex),
            pool_address,
            pool_identifier,
            12345678,
            token0,
            token1,
            Some(3000),
            Some(60),
            ts_init,
        );

        assert_eq!(pool.chain.chain_id, chain.chain_id);
        assert_eq!(pool.dex.name, DexType::UniswapV3);
        assert_eq!(pool.address, pool_address);
        assert_eq!(pool.creation_block, 12345678);
        assert_eq!(pool.token0.symbol, "WETH");
        assert_eq!(pool.token1.symbol, "USDT");
        assert_eq!(pool.fee.unwrap(), 3000);
        assert_eq!(pool.tick_spacing.unwrap(), 60);
        assert_eq!(pool.ts_init, ts_init);
        assert_eq!(
            pool.instrument_id.symbol.as_str(),
            "0x11b815efB8f581194ae79006d24E0d814B7697F6"
        );
        assert_eq!(pool.instrument_id.venue.as_str(), "Ethereum:UniswapV3");
        // We expect WETH to be a base and USDT a quote token
        assert_eq!(pool.get_base_token().symbol, "WETH");
        assert_eq!(pool.get_quote_token().symbol, "USDT");
        assert!(!pool.is_base_quote_inverted());
        assert_eq!(
            pool.to_full_spec_string(),
            "WETH/USDT-3000.Ethereum:UniswapV3"
        );
    }

    #[rstest]
    fn test_pool_instrument_id_format() {
        let chain = Arc::new(chains::ETHEREUM.clone());
        let factory_address = "0x1F98431c8aD98523631AE4a59f267346ea31F984";

        let dex = Dex::new(
            chains::ETHEREUM.clone(),
            DexType::UniswapV3,
            factory_address,
            0,
            AmmType::CLAMM,
            "PoolCreated(address,address,uint24,int24,address)",
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
            "Collect(address,address,int24,int24,uint128,uint128)",
        );

        let token0 = Token::new(
            chain.clone(),
            "0xA0b86a33E6441b936662bb6B5d1F8Fb0E2b57A5D"
                .parse()
                .unwrap(),
            "Wrapped Ether".to_string(),
            "WETH".to_string(),
            18,
        );

        let token1 = Token::new(
            chain.clone(),
            "0xdAC17F958D2ee523a2206206994597C13D831ec7"
                .parse()
                .unwrap(),
            "Tether USD".to_string(),
            "USDT".to_string(),
            6,
        );

        let pool_address = "0x11b815efB8f581194ae79006d24E0d814B7697F6"
            .parse()
            .unwrap();
        let pool = Pool::new(
            chain,
            Arc::new(dex),
            pool_address,
            PoolIdentifier::from_address(pool_address),
            0,
            token0,
            token1,
            Some(3000),
            Some(60),
            UnixNanos::default(),
        );

        assert_eq!(
            pool.instrument_id.to_string(),
            "0x11b815efB8f581194ae79006d24E0d814B7697F6.Ethereum:UniswapV3"
        );
    }
}
