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

use alloy_primitives::Address;
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{
    data::HasTsInit,
    defi::{Blockchain, SharedDex, chain::SharedChain, dex::Dex, token::Token},
    identifiers::InstrumentId,
};

/// Represents a liquidity pool in a decentralized exchange.
///
/// The instrument ID encodes with the following components:
/// `symbol` – The base/quote ticker plus the pool fee tier (in hundred-thousandths).
/// `venue`  – The DEX name plus chain name.
///
/// The string representation therefore has the form:
/// `<BASE>/<QUOTE>-<FEE>.<DEX_NAME>:<CHAIN_NAME>`
///
/// Example:
/// `WETH/USDT-3000.UniswapV3:Arbitrum`
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
    /// The blockchain address of the pool smart contract.
    pub address: Address,
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
    pub fee: u32,
    /// The minimum tick spacing for positions in concentrated liquidity AMMs.
    pub tick_spacing: u32,
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
        creation_block: u64,
        token0: Token,
        token1: Token,
        fee: u32,
        tick_spacing: u32,
        ts_init: UnixNanos,
    ) -> Self {
        let instrument_id = Self::create_instrument_id(chain.name, &dex, &token0, &token1, fee);

        Self {
            chain,
            dex,
            address,
            instrument_id,
            creation_block,
            token0,
            token1,
            fee,
            tick_spacing,
            ts_init,
        }
    }

    pub fn create_instrument_id(
        chain: Blockchain,
        dex: &Dex,
        token0: &Token,
        token1: &Token,
        fee: u32,
    ) -> InstrumentId {
        let symbol = format!(
            "{}/{}-{}",
            sanitize_symbol(&token0.symbol),
            sanitize_symbol(&token1.symbol),
            fee
        );
        let venue = format!("{}:{}", dex.name, chain);
        InstrumentId::from(format!("{symbol}.{venue}").as_str())
    }
}

fn sanitize_symbol(symbol: &str) -> String {
    symbol
        .chars()
        .map(|c| match c {
            '₮' => 'T', // Tugrik sign
            '₽' => 'R', // Ruble sign
            '₹' => 'R', // Rupee sign
            '₩' => 'W', // Won sign
            '₴' => 'H', // Hryvnia sign
            '₡' => 'C', // Colon sign
            '₪' => 'S', // Shekel sign
            '₫' => 'D', // Dong sign
            '₦' => 'N', // Naira sign
            '₨' => 'R', // Rupee sign
            '₧' => 'P', // Peseta sign
            '₭' => 'K', // Kip sign
            '₵' => 'C', // Cedi sign
            '₱' => 'P', // Peso sign
            '₲' => 'G', // Guarani sign
            '₳' => 'A', // Austral sign
            '₸' => 'T', // Tenge sign
            '₶' => 'L', // Livre sign
            '₷' => 'S', // Spesmilo sign
            '₺' => 'T', // Lira sign
            '₻' => 'N', // Nordic mark sign
            '₼' => 'M', // Manat sign
            '₾' => 'L', // Lari sign
            '₿' => 'B', // Bitcoin sign
            _ if c.is_ascii() => c,
            _ => '_', // Replace any other non-ASCII with underscore
        })
        .collect()
}

impl Display for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Pool(instrument_id={}, dex={}, fee={}, address={})",
            self.instrument_id, self.dex.name, self.fee, self.address
        )
    }
}

impl HasTsInit for Pool {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rstest::rstest;

    use super::*;
    use crate::defi::{
        chain::chains,
        dex::{AmmType, Dex},
        token::Token,
    };

    #[rstest]
    fn test_pool_constructor_and_methods() {
        let chain = Arc::new(chains::ETHEREUM.clone());
        let dex = Dex::new(
            chains::ETHEREUM.clone(),
            "UniswapV3",
            "0x1F98431c8aD98523631AE4a59f267346ea31F984",
            0,
            AmmType::CLAMM,
            "PoolCreated(address,address,uint24,int24,address)",
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
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
        let ts_init = UnixNanos::from(1_234_567_890_000_000_000u64);

        let pool = Pool::new(
            chain.clone(),
            Arc::new(dex),
            pool_address,
            12345678,
            token0,
            token1,
            3000,
            60,
            ts_init,
        );

        assert_eq!(pool.chain.chain_id, chain.chain_id);
        assert_eq!(pool.dex.name, "UniswapV3");
        assert_eq!(pool.address, pool_address);
        assert_eq!(pool.creation_block, 12345678);
        assert_eq!(pool.token0.symbol, "WETH");
        assert_eq!(pool.token1.symbol, "USDT");
        assert_eq!(pool.fee, 3000);
        assert_eq!(pool.tick_spacing, 60);
        assert_eq!(pool.ts_init, ts_init);
        assert_eq!(pool.instrument_id.symbol.as_str(), "WETH/USDT-3000");

        let instrument_id = pool.instrument_id;
        assert!(instrument_id.to_string().contains("WETH/USDT"));
        assert!(instrument_id.to_string().contains("UniswapV3"));
    }

    #[rstest]
    fn test_pool_instrument_id_format() {
        let chain = Arc::new(chains::ETHEREUM.clone());
        let factory_address = "0x1F98431c8aD98523631AE4a59f267346ea31F984";

        let dex = Dex::new(
            chains::ETHEREUM.clone(),
            "UniswapV3",
            factory_address,
            0,
            AmmType::CLAMM,
            "PoolCreated(address,address,uint24,int24,address)",
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
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

        let pool = Pool::new(
            chain.clone(),
            Arc::new(dex),
            "0x11b815efB8f581194ae79006d24E0d814B7697F6"
                .parse()
                .unwrap(),
            0,
            token0,
            token1,
            3000,
            60,
            UnixNanos::default(),
        );

        assert_eq!(
            pool.instrument_id.to_string(),
            "WETH/USDT-3000.UniswapV3:Ethereum"
        );
    }
}
