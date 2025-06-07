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

use std::{borrow::Cow, fmt::Display, sync::Arc};

use crate::{
    defi::{amm::Pool, chain::Chain},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{Instrument, any::InstrumentAny, currency_pair::CurrencyPair},
    types::{currency::Currency, fixed::FIXED_PRECISION, price::Price, quantity::Quantity},
};

/// Represents different types of Automated Market Makers (AMMs) in DeFi protocols.
#[derive(Debug, Clone)]
pub enum AmmType {
    /// Constant Product Automated Market Maker.
    CPAMM,
    /// Concentrated Liquidity Automated Market Maker.
    CLAMM,
    /// Enhanced CLAMM with Additional Features (Uniswap V4 with Hooks).
    CLAMEnhanced,
    /// Specialized AMM for Stable Assets (Curve Style).
    StableSwap,
    /// AMM with customizable token weights (e.g., Balancer style).
    WeightedPool,
    /// Advanced pool type that can nest other pools (Balancer V3).
    ComposablePool,
}

/// Represents a decentralized exchange (DEX) in a blockchain ecosystem.
#[derive(Debug, Clone)]
pub struct Dex {
    /// The blockchain network where this DEX operates.
    pub chain: Chain,
    /// The name of the DEX protocol.
    pub name: Cow<'static, str>,
    /// The blockchain address of the DEX factory contract.
    pub factory: Cow<'static, str>,
    /// The event signature or identifier used to detect pool creation events.
    pub pool_created_event: Cow<'static, str>,
    /// The event signature or identifier used to detect swap events.
    pub swap_created_event: Cow<'static, str>,
    /// The event signature or identifier used to detect mint events
    pub mint_created_event: Cow<'static, str>,
    /// The event signature or identifier used to detect burn events
    pub burn_created_event: Cow<'static, str>,
    /// The type of automated market maker (AMM) algorithm used by this DEX.
    pub amm_type: AmmType,
    /// Collection of liquidity pools managed by this DEX.
    #[allow(dead_code)] // TBD
    pairs: Vec<Pool>,
}

/// A thread-safe shared pointer to a `Dex`, enabling efficient reuse across multiple components.
pub type SharedDex = Arc<Dex>;

impl Dex {
    /// Creates a new [`Dex`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain: Chain,
        name: impl Into<Cow<'static, str>>,
        factory: impl Into<Cow<'static, str>>,
        amm_type: AmmType,
        pool_created_event: impl Into<Cow<'static, str>>,
        swap_created_event: impl Into<Cow<'static, str>>,
        mint_created_event: impl Into<Cow<'static, str>>,
        burn_created_event: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            chain,
            name: name.into(),
            factory: factory.into(),
            pool_created_event: pool_created_event.into(),
            swap_created_event: swap_created_event.into(),
            mint_created_event: mint_created_event.into(),
            burn_created_event: burn_created_event.into(),
            amm_type,
            pairs: vec![],
        }
    }

    /// Returns a unique identifier for this DEX, combining chain and name.
    ///
    /// Format: "{chain_id}:{name_snake_case}"
    pub fn id(&self) -> String {
        format!(
            "{}:{}",
            self.chain.name,
            self.name.to_lowercase().replace(" ", "_")
        )
    }
}

impl Display for Dex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Dex(chain={}, name={})", self.chain, self.name)
    }
}

impl From<Pool> for CurrencyPair {
    fn from(p: Pool) -> Self {
        let symbol = Symbol::from(format!("{}/{}", p.token0.symbol, p.token1.symbol));
        let id = InstrumentId::new(symbol, Venue::from(p.dex.id()));

        let size_precision = p.token0.decimals.min(FIXED_PRECISION);
        let price_precision = p.token1.decimals.min(FIXED_PRECISION);

        let price_increment = Price::new(10f64.powi(-(price_precision as i32)), price_precision);
        let size_increment = Quantity::new(10f64.powi(-(size_precision as i32)), size_precision);

        CurrencyPair::new(
            id,
            symbol,
            Currency::from(p.token0.symbol.as_str()),
            Currency::from(p.token1.symbol.as_str()),
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            0.into(),
            0.into(),
        )
    }
}

impl From<Pool> for InstrumentAny {
    fn from(p: Pool) -> Self {
        CurrencyPair::from(p).into_any()
    }
}
