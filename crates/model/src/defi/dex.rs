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

use alloy_primitives::{Address, keccak256};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

use crate::{
    defi::{amm::Pool, chain::Chain, validation::validate_address},
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
#[cfg_attr(feature = "python", pyo3::pyclass(module = "nautilus_trader.model"))]
#[cfg_attr(feature = "python", pyo3_stub_gen::derive::gen_stub_pyclass_enum)]
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
#[cfg_attr(feature = "python", pyo3::pyclass(module = "nautilus_trader.model"))]
#[cfg_attr(feature = "python", pyo3_stub_gen::derive::gen_stub_pyclass_enum)]
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
    pub fn from_dex_name(dex_name: &str) -> Option<Self> {
        Self::from_str(dex_name).ok()
    }
}

/// Represents a decentralized exchange (DEX) in a blockchain ecosystem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "python", pyo3::pyclass(module = "nautilus_trader.model"))]
#[cfg_attr(feature = "python", pyo3_stub_gen::derive::gen_stub_pyclass)]
pub struct Dex {
    /// The blockchain network where this DEX operates.
    pub chain: Chain,
    /// The variant of the DEX protocol.
    pub name: DexType,
    /// The blockchain address of the DEX factory contract.
    pub factory: Address,
    /// The block number at which the DEX factory contract was deployed.
    pub factory_creation_block: u64,
    /// The event signature or identifier used to detect pool creation events.
    pub pool_created_event: Cow<'static, str>,
    // Optional Initialize event signature emitted when pool is initialized.
    pub initialize_event: Option<Cow<'static, str>>,
    /// The event signature or identifier used to detect swap events.
    pub swap_created_event: Cow<'static, str>,
    /// The event signature or identifier used to detect mint events.
    pub mint_created_event: Cow<'static, str>,
    /// The event signature or identifier used to detect burn events.
    pub burn_created_event: Cow<'static, str>,
    /// The event signature or identifier used to detect collect fee events.
    pub collect_created_event: Cow<'static, str>,
    // Optional Flash event signature emitted when flash loan occurs.
    pub flash_created_event: Option<Cow<'static, str>>,
    /// The type of automated market maker (AMM) algorithm used by this DEX.
    pub amm_type: AmmType,
    /// Collection of liquidity pools managed by this DEX.
    #[allow(dead_code, reason = "TBD")]
    pairs: Vec<Pool>,
}

/// A thread-safe shared pointer to a `Dex`, enabling efficient reuse across multiple components.
pub type SharedDex = Arc<Dex>;

impl Dex {
    /// Creates a new [`Dex`] instance with the specified properties.
    ///
    /// # Panics
    ///
    /// Panics if the provided factory address is invalid.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain: Chain,
        name: DexType,
        factory: &str,
        factory_creation_block: u64,
        amm_type: AmmType,
        pool_created_event: &str,
        swap_event: &str,
        mint_event: &str,
        burn_event: &str,
        collect_event: &str,
    ) -> Self {
        let pool_created_event_hash = keccak256(pool_created_event.as_bytes());
        let encoded_pool_created_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(pool_created_event_hash)
        );
        let swap_event_hash: alloy_primitives::FixedBytes<32> = keccak256(swap_event.as_bytes());
        let encoded_swap_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(swap_event_hash)
        );
        let mint_event_hash = keccak256(mint_event.as_bytes());
        let encoded_mint_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(mint_event_hash)
        );
        let burn_event_hash = keccak256(burn_event.as_bytes());
        let encoded_burn_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(burn_event_hash)
        );
        let collect_event_hash = keccak256(collect_event.as_bytes());
        let encoded_collect_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(collect_event_hash)
        );
        let factory_address = validate_address(factory).unwrap();
        Self {
            chain,
            name,
            factory: factory_address,
            factory_creation_block,
            pool_created_event: encoded_pool_created_event.into(),
            initialize_event: None,
            swap_created_event: encoded_swap_event.into(),
            mint_created_event: encoded_mint_event.into(),
            burn_created_event: encoded_burn_event.into(),
            collect_created_event: encoded_collect_event.into(),
            flash_created_event: None,
            amm_type,
            pairs: vec![],
        }
    }

    /// Returns a unique identifier for this DEX, combining chain and protocol name.
    pub fn id(&self) -> String {
        format!("{}:{}", self.chain.name, self.name)
    }

    /// Sets the pool initialization event signature by hashing and encoding the provided event string.
    pub fn set_initialize_event(&mut self, event: &str) {
        let initialize_event_hash = keccak256(event.as_bytes());
        let encoded_initialized_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(initialize_event_hash)
        );
        self.initialize_event = Some(encoded_initialized_event.into());
    }

    /// Sets the flash loan event signature by hashing and encoding the provided event string.
    pub fn set_flash_event(&mut self, event: &str) {
        let flash_event_hash = keccak256(event.as_bytes());
        let encoded_flash_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(flash_event_hash)
        );
        self.flash_created_event = Some(encoded_flash_event.into());
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

        Self::new(
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
