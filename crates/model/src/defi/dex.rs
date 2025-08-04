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

use std::{borrow::Cow, fmt::Display, str::FromStr, sync::Arc};

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

use crate::{
    defi::{amm::Pool, chain::Chain},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{Instrument, any::InstrumentAny, currency_pair::CurrencyPair},
    types::{currency::Currency, fixed::FIXED_PRECISION, price::Price, quantity::Quantity},
};

/// Represents different types of Automated Market Makers (AMMs) in DeFi protocols.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Serialize,
    Deserialize,
    strum::EnumString,
    strum::Display,
    strum::EnumIter,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[non_exhaustive]
pub enum AmmType {
    /// Constant Product Automated Market Maker.
    CPAMM,
    /// Concentrated Liquidity Automated Market Maker.
    CLAMM,
    /// Concentrated liquidity AMM **with hooks** (e.g. upcoming Uniswap v4).
    CLAMEnhanced,
    /// Specialized Constant-Sum AMM for low-volatility assets (Curve-style “StableSwap”).
    StableSwap,
    /// AMM with customizable token weights (e.g., Balancer style).
    WeightedPool,
    /// Advanced pool type that can nest other pools (Balancer V3).
    ComposablePool,
}

/// Represents different types of decentralized exchanges (DEXes) supported by Nautilus.
#[derive(
    Debug,
    Clone,
    Copy,
    Hash,
    PartialOrd,
    PartialEq,
    Ord,
    Eq,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub enum DexType {
    AerodromeSlipstream,
    AerodromeV1,
    BalancerV2,
    BalancerV3,
    BaseSwapV2,
    BaseX,
    CamelotV3,
    CurveFinance,
    FluidDEX,
    MaverickV1,
    MaverickV2,
    PancakeSwapV3,
    SushiSwapV2,
    SushiSwapV3,
    UniswapV2,
    UniswapV3,
    UniswapV4,
}

impl DexType {
    /// Returns a reference to the `DexType` corresponding to the given dex name, or `None` if it is not found.
    pub fn from_dex_name(dex_name: &str) -> Option<DexType> {
        DexType::from_str(dex_name).ok()
    }
}

/// Represents a decentralized exchange (DEX) in a blockchain ecosystem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Dex {
    /// The blockchain network where this DEX operates.
    pub chain: Chain,
    /// The variant of the DEX protocol.
    pub name: DexType,
    /// The blockchain address of the DEX factory contract.
    pub factory: Cow<'static, str>,
    /// The block number at which the DEX factory contract was deployed.
    pub factory_creation_block: u64,
    /// The event signature or identifier used to detect pool creation events.
    pub pool_created_event: Cow<'static, str>,
    /// The event signature or identifier used to detect swap events.
    pub swap_created_event: Cow<'static, str>,
    /// The event signature or identifier used to detect mint events.
    pub mint_created_event: Cow<'static, str>,
    /// The event signature or identifier used to detect burn events.
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
        name: DexType,
        factory: impl Into<Cow<'static, str>>,
        factory_creation_block: u64,
        amm_type: AmmType,
        pool_created_event: impl Into<Cow<'static, str>>,
        swap_created_event: impl Into<Cow<'static, str>>,
        mint_created_event: impl Into<Cow<'static, str>>,
        burn_created_event: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            chain,
            name,
            factory: factory.into(),
            factory_creation_block,
            pool_created_event: pool_created_event.into(),
            swap_created_event: swap_created_event.into(),
            mint_created_event: mint_created_event.into(),
            burn_created_event: burn_created_event.into(),
            amm_type,
            pairs: vec![],
        }
    }

    /// Returns a unique identifier for this DEX, combining chain and protocol name.
    pub fn id(&self) -> String {
        format!("{}:{}", self.chain.name, self.name)
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::DexType;

    #[rstest]
    fn test_dex_type_from_dex_name_valid() {
        // Test some known DEX names
        assert!(DexType::from_dex_name("UniswapV3").is_some());
        assert!(DexType::from_dex_name("SushiSwapV2").is_some());
        assert!(DexType::from_dex_name("BalancerV2").is_some());
        assert!(DexType::from_dex_name("CamelotV3").is_some());

        // Verify specific DEX type
        let uniswap_v3 = DexType::from_dex_name("UniswapV3").unwrap();
        assert_eq!(uniswap_v3, DexType::UniswapV3);

        // Verify compound names
        let aerodrome_slipstream = DexType::from_dex_name("AerodromeSlipstream").unwrap();
        assert_eq!(aerodrome_slipstream, DexType::AerodromeSlipstream);

        // Verify specialized names
        let fluid_dex = DexType::from_dex_name("FluidDEX").unwrap();
        assert_eq!(fluid_dex, DexType::FluidDEX);
    }

    #[rstest]
    fn test_dex_type_from_dex_name_invalid() {
        // Test unknown DEX names
        assert!(DexType::from_dex_name("InvalidDEX").is_none());
        assert!(DexType::from_dex_name("").is_none());
        assert!(DexType::from_dex_name("NonExistentDEX").is_none());
    }

    #[rstest]
    fn test_dex_type_from_dex_name_case_sensitive() {
        // Test case sensitivity - should be case sensitive
        assert!(DexType::from_dex_name("UniswapV3").is_some());
        assert!(DexType::from_dex_name("uniswapv3").is_none()); // lowercase
        assert!(DexType::from_dex_name("UNISWAPV3").is_none()); // uppercase
        assert!(DexType::from_dex_name("UniSwapV3").is_none()); // mixed case

        assert!(DexType::from_dex_name("SushiSwapV2").is_some());
        assert!(DexType::from_dex_name("sushiswapv2").is_none()); // lowercase
    }

    #[rstest]
    fn test_dex_type_all_variants_mappable() {
        // Test that all DEX variants can be mapped from their string representation
        let all_dex_names = vec![
            "AerodromeSlipstream",
            "AerodromeV1",
            "BalancerV2",
            "BalancerV3",
            "BaseSwapV2",
            "BaseX",
            "CamelotV3",
            "CurveFinance",
            "FluidDEX",
            "MaverickV1",
            "MaverickV2",
            "PancakeSwapV3",
            "SushiSwapV2",
            "SushiSwapV3",
            "UniswapV2",
            "UniswapV3",
            "UniswapV4",
        ];

        for dex_name in all_dex_names {
            assert!(
                DexType::from_dex_name(dex_name).is_some(),
                "DEX name '{dex_name}' should be valid but was not found",
            );
        }
    }

    #[rstest]
    fn test_dex_type_display() {
        // Test that DexType variants display correctly (using strum::Display)
        assert_eq!(DexType::UniswapV3.to_string(), "UniswapV3");
        assert_eq!(DexType::SushiSwapV2.to_string(), "SushiSwapV2");
        assert_eq!(
            DexType::AerodromeSlipstream.to_string(),
            "AerodromeSlipstream"
        );
        assert_eq!(DexType::FluidDEX.to_string(), "FluidDEX");
    }
}
