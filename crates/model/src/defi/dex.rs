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

use std::{borrow::Cow, fmt::Display, str::FromStr, sync::Arc};

use alloy_primitives::{Address, keccak256};
use nautilus_core::{
    correctness::{CorrectnessError, CorrectnessResultExt, FAILED},
    hex,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

use crate::{
    defi::{amm::Pool, chain::Chain, validation::validate_address},
    enums::CurrencyType,
    instruments::{Instrument, any::InstrumentAny, currency_pair::CurrencyPair},
    types::{currency::Currency, fixed::FIXED_PRECISION, price::Price, quantity::Quantity},
};

/// Represents different types of Automated Market Makers (AMMs) in DeFi protocols.
#[derive(
    Debug,
    Clone,
    Copy,
    Hash,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum::EnumString,
    strum::Display,
    strum::EnumIter,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(feature = "python", pyo3_stub_gen::derive::gen_stub_pyclass_enum)]
#[non_exhaustive]
pub enum AmmType {
    /// Constant Product Automated Market Maker.
    CPAMM,
    /// Concentrated Liquidity Automated Market Maker.
    CLAMM,
    /// Concentrated liquidity AMM **with hooks** (e.g. upcoming Uniswap v4).
    CLAMEnhanced,
    /// Specialized Constant-Sum AMM for low-volatility assets (Curve-style "`StableSwap`").
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
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
    #[must_use]
    pub fn from_dex_name(dex_name: &str) -> Option<Self> {
        Self::from_str(dex_name).ok()
    }
}

/// Represents a decentralized exchange (DEX) in a blockchain ecosystem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
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
    // Optional SetFeeProtocol event signature emitted when the protocol-fee config changes.
    pub fee_protocol_update_event: Option<Cow<'static, str>>,
    // Optional CollectProtocol event signature emitted when protocol fees are withdrawn.
    pub fee_protocol_collect_event: Option<Cow<'static, str>>,
    /// The type of automated market maker (AMM) algorithm used by this DEX.
    pub amm_type: AmmType,
    /// Collection of liquidity pools managed by this DEX.
    #[allow(dead_code)]
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
    #[expect(clippy::too_many_arguments)]
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
        let encoded_pool_created_event =
            hex::encode_prefixed(keccak256(pool_created_event.as_bytes()));
        let encoded_swap_event = hex::encode_prefixed(keccak256(swap_event.as_bytes()));
        let encoded_mint_event = hex::encode_prefixed(keccak256(mint_event.as_bytes()));
        let encoded_burn_event = hex::encode_prefixed(keccak256(burn_event.as_bytes()));
        let encoded_collect_event = hex::encode_prefixed(keccak256(collect_event.as_bytes()));
        let factory_address = match validate_address(factory) {
            Ok(address) => address,
            Err(e) => panic!(
                "Invalid factory address for DEX {name} on chain {chain} for factory address {factory}: {e}"
            ),
        };
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
            fee_protocol_update_event: None,
            fee_protocol_collect_event: None,
            amm_type,
            pairs: vec![],
        }
    }

    /// Returns a unique identifier for this DEX, combining chain and protocol name.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}:{}", self.chain.name, self.name)
    }

    /// Sets the pool initialization event signature by hashing and encoding the provided event string.
    pub fn set_initialize_event(&mut self, event: &str) {
        self.initialize_event = Some(hex::encode_prefixed(keccak256(event.as_bytes())).into());
    }

    /// Sets the flash loan event signature by hashing and encoding the provided event string.
    pub fn set_flash_event(&mut self, event: &str) {
        self.flash_created_event = Some(hex::encode_prefixed(keccak256(event.as_bytes())).into());
    }

    /// Sets the protocol-fee change event signature by hashing and encoding the provided event string.
    pub fn set_fee_protocol_update_event(&mut self, event: &str) {
        self.fee_protocol_update_event =
            Some(hex::encode_prefixed(keccak256(event.as_bytes())).into());
    }

    /// Sets the protocol-fee withdrawal event signature by hashing and encoding the provided event string.
    pub fn set_fee_protocol_collect_event(&mut self, event: &str) {
        self.fee_protocol_collect_event =
            Some(hex::encode_prefixed(keccak256(event.as_bytes())).into());
    }
}

impl Display for Dex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Dex(chain={}, name={})", self.chain, self.name)
    }
}

impl TryFrom<&Pool> for CurrencyPair {
    type Error = CorrectnessError;

    fn try_from(p: &Pool) -> Result<Self, Self::Error> {
        let size_precision = p.token0.decimals.min(FIXED_PRECISION);
        let price_precision = p.token1.decimals.min(FIXED_PRECISION);

        let price_increment =
            Price::from_mantissa_exponent(1, -price_precision.cast_signed(), price_precision);
        let size_increment =
            Quantity::from_mantissa_exponent(1, -size_precision.cast_signed(), size_precision);
        let base_currency = Currency::new_checked(
            p.token0.symbol.as_str(),
            size_precision,
            0,
            p.token0.name.as_str(),
            CurrencyType::Crypto,
        )?;
        let quote_currency = Currency::new_checked(
            p.token1.symbol.as_str(),
            price_precision,
            0,
            p.token1.name.as_str(),
            CurrencyType::Crypto,
        )?;
        let taker_fee = p.fee.map(|fee| Decimal::new(i64::from(fee), 6));

        let pair = Self::builder()
            .instrument_id(p.instrument_id)
            .raw_symbol(p.instrument_id.symbol)
            .base_currency(base_currency)
            .quote_currency(quote_currency)
            .price_precision(price_precision)
            .size_precision(size_precision)
            .price_increment(price_increment)
            .size_increment(size_increment)
            .maybe_taker_fee(taker_fee)
            .ts_event(p.ts_event)
            .ts_init(p.ts_init)
            .build()?;

        for currency in [base_currency, quote_currency] {
            if let Err(e) = Currency::register(currency, false) {
                log::error!(
                    "Failed to register DeFi token currency '{}': {e}",
                    currency.code
                );
            }
        }

        Ok(pair)
    }
}

impl From<Pool> for CurrencyPair {
    fn from(p: Pool) -> Self {
        Self::try_from(&p).expect_display(FAILED)
    }
}

impl From<Pool> for InstrumentAny {
    fn from(p: Pool) -> Self {
        CurrencyPair::from(p).into_any()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::correctness::CorrectnessError;
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::{CurrencyPair, DexType};
    use crate::{
        defi::{SharedPool, stubs::rain_pool},
        enums::CurrencyType,
        types::{currency::Currency, fixed::FIXED_PRECISION},
    };

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

    #[rstest]
    #[case(0, 6, 0, 6)]
    #[case(6, FIXED_PRECISION, 6, FIXED_PRECISION)]
    #[case(FIXED_PRECISION, 0, FIXED_PRECISION, 0)]
    #[case(
        FIXED_PRECISION + 1,
        FIXED_PRECISION + 2,
        FIXED_PRECISION,
        FIXED_PRECISION
    )]
    fn test_pool_to_currency_pair_constructs_exact_increments(
        #[case] size_precision: u8,
        #[case] price_precision: u8,
        #[case] expected_size_precision: u8,
        #[case] expected_price_precision: u8,
        rain_pool: SharedPool,
    ) {
        let mut pool = (*rain_pool).clone();
        pool.token0.symbol = "BTC".to_string();
        pool.token1.symbol = "USDC".to_string();
        pool.token0.decimals = size_precision;
        pool.token1.decimals = price_precision;

        let expected_id = pool.instrument_id;
        let expected_taker_fee = pool.fee.map(|fee| Decimal::new(i64::from(fee), 6));
        let expected_ts_event = pool.ts_event;
        let expected_ts_init = pool.ts_init;
        let pair = CurrencyPair::from(pool);
        let price_scale_exponent = u32::from(FIXED_PRECISION - expected_price_precision);
        let size_scale_exponent = u32::from(FIXED_PRECISION - expected_size_precision);

        assert_eq!(pair.id, expected_id);
        assert_eq!(pair.raw_symbol, expected_id.symbol);
        assert_eq!(pair.base_currency.code.as_str(), "BTC");
        assert_eq!(pair.base_currency.precision, expected_size_precision);
        assert_eq!(pair.quote_currency.code.as_str(), "USDC");
        assert_eq!(pair.quote_currency.precision, expected_price_precision);
        assert_eq!(pair.price_precision, expected_price_precision);
        assert_eq!(pair.size_precision, expected_size_precision);
        assert_eq!(pair.price_increment.raw, 10_i128.pow(price_scale_exponent));
        assert_eq!(pair.price_increment.precision, expected_price_precision);
        assert_eq!(pair.size_increment.raw, 10_u128.pow(size_scale_exponent));
        assert_eq!(pair.size_increment.precision, expected_size_precision);
        assert_eq!(pair.maker_fee, Decimal::ZERO);
        assert_eq!(pair.taker_fee, expected_taker_fee.unwrap());
        assert_eq!(pair.ts_event, expected_ts_event);
        assert_eq!(pair.ts_init, expected_ts_init);
    }

    #[rstest]
    fn test_pool_to_currency_pair_registers_token_currencies(rain_pool: SharedPool) {
        let mut pool = (*rain_pool).clone();
        pool.token0.symbol = "ENG444BASE".to_string();
        pool.token0.name = "ENG-444 Base Token".to_string();
        pool.token0.decimals = 8;
        pool.token1.symbol = "ENG444QUOTE".to_string();
        pool.token1.name = "ENG-444 Quote Token".to_string();
        pool.token1.decimals = 6;

        let _ = CurrencyPair::from(pool);

        let base = Currency::try_from_str("ENG444BASE").unwrap();
        let quote = Currency::try_from_str("ENG444QUOTE").unwrap();
        assert_eq!(base.code.as_str(), "ENG444BASE");
        assert_eq!(base.precision, 8);
        assert_eq!(base.iso4217, 0);
        assert_eq!(base.name.as_str(), "ENG-444 Base Token");
        assert_eq!(base.currency_type, CurrencyType::Crypto);
        assert_eq!(quote.code.as_str(), "ENG444QUOTE");
        assert_eq!(quote.precision, 6);
        assert_eq!(quote.iso4217, 0);
        assert_eq!(quote.name.as_str(), "ENG-444 Quote Token");
        assert_eq!(quote.currency_type, CurrencyType::Crypto);
    }

    #[rstest]
    fn test_pool_to_currency_pair_rejects_invalid_token_metadata(rain_pool: SharedPool) {
        let mut missing_symbol = (*rain_pool).clone();
        missing_symbol.token0.symbol.clear();
        let mut blank_symbol = (*rain_pool).clone();
        blank_symbol.token0.symbol = "  ".to_string();
        let mut missing_name = (*rain_pool).clone();
        missing_name.token0.name.clear();

        let missing_symbol_result = CurrencyPair::try_from(&missing_symbol);
        let blank_symbol_result = CurrencyPair::try_from(&blank_symbol);
        let missing_name_result = CurrencyPair::try_from(&missing_name);

        assert_eq!(
            missing_symbol_result.unwrap_err(),
            CorrectnessError::EmptyString {
                param: "code".to_string(),
            }
        );
        assert_eq!(
            blank_symbol_result.unwrap_err(),
            CorrectnessError::WhitespaceString {
                param: "code".to_string(),
            }
        );
        assert_eq!(
            missing_name_result.unwrap_err(),
            CorrectnessError::EmptyString {
                param: "name".to_string(),
            }
        );
    }
}
