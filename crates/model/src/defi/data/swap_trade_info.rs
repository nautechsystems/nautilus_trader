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

use alloy_primitives::{U160, U256};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;

use crate::{
    defi::{
        Token,
        data::swap::RawSwapData,
        tick_map::{
            full_math::FullMath, sqrt_price_math::decode_sqrt_price_x96_to_price_tokens_adjusted,
        },
    },
    enums::OrderSide,
    types::{Price, Quantity, fixed::FIXED_PRECISION, quantity::QuantityRaw},
};

/// Trade information derived from raw swap data, normalized to market conventions.
///
/// This structure represents a Uniswap V3 swap translated into standard trading terminology
/// (base/quote, buy/sell) for consistency with traditional financial data systems.
///
/// # Base/Quote Token Convention
///
/// Tokens are assigned base/quote roles based on their priority:
/// - Higher priority token → base (asset being traded)
/// - Lower priority token → quote (pricing currency)
///
/// This may differ from the pool's token0/token1 ordering. When token priority differs
/// from pool ordering, we say the market is "inverted":
/// - NOT inverted: token0=base, token1=quote
/// - Inverted: token0=quote, token1=base
///
/// # Prices
///
/// - `spot_price`: Instantaneous pool price after the swap (from sqrt_price_x96)
/// - `execution_price`: Average realized price for this swap (from amount ratio)
///
/// Both prices are in quote/base direction (e.g., USDC per WETH) and adjusted for token decimals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapTradeInfo {
    /// The direction of the trade from the base token perspective.
    pub order_side: OrderSide,
    /// The absolute quantity of the base token involved in the swap.
    pub quantity_base: Quantity,
    /// The absolute quantity of the quote token involved in the swap.
    pub quantity_quote: Quantity,
    /// The instantaneous pool price after the swap (quote per base).
    pub spot_price: Price,
    /// The average realized execution price for this swap (quote per base).
    pub execution_price: Price,
    /// Whether the base/quote assignment differs from token0/token1 ordering.
    pub is_inverted: bool,
    /// The pool price before that swap executed(optional).
    pub spot_price_before: Option<Price>,
}

impl SwapTradeInfo {
    /// Sets the spot price before the swap for price impact and slippage calculations.
    pub fn set_spot_price_before(&mut self, price: Price) {
        self.spot_price_before = Some(price);
    }

    /// Calculates price impact in basis points (requires token references for decimal adjustment).
    ///
    /// Price impact measures the market movement caused by the swap size,
    /// excluding fees. This is the percentage change in spot price from
    /// before to after the swap.
    ///
    /// # Returns
    /// Price impact in basis points (10000 = 100%)
    ///
    /// # Errors
    /// Returns error if price calculations fail
    pub fn get_price_impact_bps(&self) -> anyhow::Result<u32> {
        if let Some(spot_price_before) = self.spot_price_before {
            let price_change = self.spot_price - spot_price_before;
            let price_impact =
                (price_change.as_decimal() / spot_price_before.as_decimal()).abs() * dec!(10_000);

            Ok(price_impact.round().to_u32().unwrap_or(0))
        } else {
            anyhow::bail!("Cannot calculate price impact, the spot price before is not set");
        }
    }

    /// Calculates slippage in basis points (requires token references for decimal adjustment).
    ///
    /// Slippage includes both price impact and fees, representing the total
    /// deviation from the spot price before the swap. This measures the total
    /// cost to the trader.
    ///
    /// # Returns
    /// Total slippage in basis points (10000 = 100%)
    ///
    /// # Errors
    /// Returns error if price calculations fail
    pub fn get_slippage_bps(&self) -> anyhow::Result<u32> {
        if let Some(spot_price_before) = self.spot_price_before {
            let price_change = self.execution_price - spot_price_before;
            let slippage =
                (price_change.as_decimal() / spot_price_before.as_decimal()).abs() * dec!(10_000);

            Ok(slippage.round().to_u32().unwrap_or(0))
        } else {
            anyhow::bail!("Cannot calculate slippage, the spot price before is not set")
        }
    }
}

/// Computation engine for deriving market-oriented trade info from raw swap data.
///
/// This calculator translates DEX's token0/token1 representation into standard
/// trading terminology (base/quote, buy/sell) based on token priority.
///
/// # Token Priority and Inversion
///
/// The calculator determines which token is base vs quote by comparing token priorities.
/// When the higher-priority token is token1 (not token0), the market is "inverted":
///
/// # Precision Handling
///
/// For tokens with more than 16 decimals, quantities and prices are automatically
/// scaled down to MAX_FLOAT_PRECISION (16) to ensure safe f64 conversion while
/// maintaining reasonable precision for practical trading purposes.
#[derive(Debug)]
pub struct SwapTradeInfoCalculator<'a> {
    /// Reference to token0 from the pool.
    token0: &'a Token,
    /// Reference to token1 from the pool.
    token1: &'a Token,
    /// Whether the base/quote assignment differs from token0/token1 ordering.
    ///
    /// - `true`: token0=quote, token1=base (inverted)
    /// - `false`: token0=base, token1=quote (normal)
    pub is_inverted: bool,
    /// Raw swap amounts and resulting sqrt price from the blockchain event.
    raw_swap_data: RawSwapData,
}

impl<'a> SwapTradeInfoCalculator<'a> {
    pub fn new(token0: &'a Token, token1: &'a Token, raw_swap_data: RawSwapData) -> Self {
        let is_inverted = token0.get_token_priority() < token1.get_token_priority();
        Self {
            token0,
            token1,
            raw_swap_data,
            is_inverted,
        }
    }

    /// Determines swap direction from amount signs.
    ///
    /// Returns `true` if swapping token0 for token1 (zero_for_one).
    pub fn zero_for_one(&self) -> bool {
        self.raw_swap_data.amount0.is_positive()
    }

    /// Computes all trade information fields and returns a complete [`SwapTradeInfo`].
    ///
    /// Calculates order side, quantities, and prices from the raw swap data,
    /// applying token priority rules and decimal adjustments. If the price before
    /// the swap is provided, also computes price impact and slippage metrics.
    ///
    /// # Arguments
    ///
    /// * `sqrt_price_x96_before` - Optional square root price before the swap (Q96 format).
    ///   When provided, enables calculation of `spot_price_before`, price impact, and slippage.
    ///
    /// # Errors
    ///
    /// Returns an error if quantity or price calculations fail.
    pub fn compute(&self, sqrt_price_x96_before: Option<U160>) -> anyhow::Result<SwapTradeInfo> {
        let spot_price_before = if let Some(sqrt_price_x96_before) = sqrt_price_x96_before {
            Some(decode_sqrt_price_x96_to_price_tokens_adjusted(
                sqrt_price_x96_before,
                self.token0.decimals,
                self.token1.decimals,
                self.is_inverted,
            )?)
        } else {
            None
        };

        Ok(SwapTradeInfo {
            order_side: self.order_side(),
            quantity_base: self.quantity_base()?,
            quantity_quote: self.quantity_quote()?,
            spot_price: self.spot_price()?,
            execution_price: self.execution_price()?,
            is_inverted: self.is_inverted,
            spot_price_before,
        })
    }

    /// Determines the order side from the perspective of the determined base/quote tokens.
    ///
    /// Uses market convention where base is the asset being traded and quote is the pricing currency.
    ///
    /// # Returns
    /// - `OrderSide::Buy` when buying base token (selling quote for base)
    /// - `OrderSide::Sell` when selling base token (buying quote with base)
    ///
    /// # Logic
    ///
    /// The order side depends on:
    /// 1. Which token is being bought/sold (from amount signs)
    /// 2. Which token is base vs quote (from priority determination)
    pub fn order_side(&self) -> OrderSide {
        let zero_for_one = self.zero_for_one();
        if self.is_inverted {
            // When inverted: token0=quote, token1=base
            // - zero_for_one (sell token0/quote, buy token1/base) -> BUY base
            // - one_for_zero (sell token1/base, buy token0/quote -> SELL base
            if zero_for_one {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            }
        } else {
            // When NOT inverted: token0=base, token1=quote
            // - zero_for_one (sell token0/base, buy token1/quote) → SELL base
            // - one_for_zero (sell token1/quote, buy token0/base) → BUY base
            if zero_for_one {
                OrderSide::Sell
            } else {
                OrderSide::Buy
            }
        }
    }

    /// Returns the quantity of the base token involved in the swap.
    ///
    /// This is always the amount of the base asset being traded,
    /// regardless of whether it's token0 or token1 in the pool.
    ///
    /// # Returns
    /// Absolute value of base token amount with proper decimals
    ///
    /// # Errors
    ///
    /// Returns an error if the amount cannot be converted to a valid `Quantity`.
    pub fn quantity_base(&self) -> anyhow::Result<Quantity> {
        let (amount, precision) = if self.is_inverted {
            (
                self.raw_swap_data.amount1.unsigned_abs(),
                self.token1.decimals,
            )
        } else {
            (
                self.raw_swap_data.amount0.unsigned_abs(),
                self.token0.decimals,
            )
        };

        // Quantity expects raw values scaled to at least FIXED_PRECISION or higher(WEI)
        let scaled_amount = if precision < FIXED_PRECISION {
            amount
                .checked_mul(U256::from(10u128.pow((FIXED_PRECISION - precision) as u32)))
                .ok_or_else(|| anyhow::anyhow!("Base quantity overflow during scaling"))?
        } else {
            amount
        };

        let raw = QuantityRaw::try_from(scaled_amount).map_err(|_| {
            anyhow::anyhow!("Base quantity {} exceeds QuantityRaw range", scaled_amount)
        })?;

        Ok(Quantity::from_raw(raw, precision))
    }

    /// Returns the quantity of the quote token involved in the swap.
    ///
    /// This is always the amount of the quote (pricing) currency,
    /// regardless of whether it's token0 or token1 in the pool.
    ///
    /// # Returns
    /// Absolute value of quote token amount with proper decimals
    ///
    /// # Errors
    ///
    /// Returns an error if the amount cannot be converted to a valid `Quantity`.
    pub fn quantity_quote(&self) -> anyhow::Result<Quantity> {
        let (amount, precision) = if self.is_inverted {
            (
                self.raw_swap_data.amount0.unsigned_abs(),
                self.token0.decimals,
            )
        } else {
            (
                self.raw_swap_data.amount1.unsigned_abs(),
                self.token1.decimals,
            )
        };

        // Quantity expects raw values scaled to at least FIXED_PRECISION or higher(WEI)
        let scaled_amount = if precision < FIXED_PRECISION {
            amount
                .checked_mul(U256::from(10u128.pow((FIXED_PRECISION - precision) as u32)))
                .ok_or_else(|| anyhow::anyhow!("Quote quantity overflow during scaling"))?
        } else {
            amount
        };

        let raw = QuantityRaw::try_from(scaled_amount).map_err(|_| {
            anyhow::anyhow!("Quote quantity {} exceeds QuantityRaw range", scaled_amount)
        })?;

        Ok(Quantity::from_raw(raw, precision))
    }

    /// Returns the human-readable spot price in base/quote (market) convention.
    ///
    /// This is the instantaneous market price after the swap, adjusted for token decimals
    /// to provide a human-readable value. This price does NOT include fees or slippage.
    ///
    /// # Returns
    /// Price adjusted for token decimals in quote/base direction (market convention).
    ///
    /// # Base/Quote Logic
    /// - When is_inverted=false: token0=base, token1=quote → returns token1/token0 (quote/base)
    /// - When is_inverted=true: token0=quote, token1=base → returns token0/token1 (quote/base)
    ///
    /// # Use Cases
    /// - Displaying current market price to users
    /// - Calculating price impact: `(spot_after - spot_before) / spot_before`
    /// - Comparing market rate vs execution rate
    /// - Real-time price feeds
    fn spot_price(&self) -> anyhow::Result<Price> {
        // Pool always stores token1/token0
        // When is_inverted=false: token0=base, token1=quote → want token1/token0 (quote/base) → don't invert
        // When is_inverted=true: token0=quote, token1=base → want token0/token1 (quote/base) → invert
        decode_sqrt_price_x96_to_price_tokens_adjusted(
            self.raw_swap_data.sqrt_price_x96,
            self.token0.decimals,
            self.token1.decimals,
            self.is_inverted, // invert when base/quote differs from token0/token1
        )
    }

    /// Calculates the average execution price for this swap (includes fees and slippage).
    ///
    /// This is the actual realized price paid/received in the swap, calculated from
    /// the input and output amounts. This represents the true cost of the trade.
    ///
    /// # Returns
    /// Price in quote/base direction (market convention), adjusted for token decimals.
    ///
    /// # Formula
    /// ```text
    /// price = (quote_amount / 10^quote_decimals) / (base_amount / 10^base_decimals)
    ///       = (quote_amount * 10^base_decimals) / (base_amount * 10^quote_decimals)
    /// ```
    ///
    /// To preserve precision in U256 arithmetic, we scale by 10^FIXED_PRECISION:
    /// ```text
    /// price_raw = (quote_amount * 10^base_decimals * 10^FIXED_PRECISION) / (base_amount * 10^quote_decimals)
    /// ```
    ///
    /// # Base/Quote Logic
    /// - When is_inverted=false: quote=token1, base=token0 → price = amount1/amount0
    /// - When is_inverted=true: quote=token0, base=token1 → price = amount0/amount1
    ///
    /// # Use Cases
    /// - Trade accounting and P&L calculation
    /// - Comparing quoted vs executed prices
    /// - Cost analysis (includes all fees and price impact)
    /// - Performance reporting
    fn execution_price(&self) -> anyhow::Result<Price> {
        let amount0 = self.raw_swap_data.amount0.unsigned_abs();
        let amount1 = self.raw_swap_data.amount1.unsigned_abs();

        if amount0.is_zero() || amount1.is_zero() {
            anyhow::bail!("Cannot calculate execution price with zero amounts");
        }

        // Determine base and quote amounts/decimals based on inversion
        let (quote_amount, base_amount, quote_decimals, base_decimals) = if self.is_inverted {
            // inverted: token0=quote, token1=base
            (amount0, amount1, self.token0.decimals, self.token1.decimals)
        } else {
            // not inverted: token0=base, token1=quote
            (amount1, amount0, self.token1.decimals, self.token0.decimals)
        };

        // Create decimal scalars
        let base_decimals_scalar = U256::from(10u128.pow(base_decimals as u32));
        let quote_decimals_scalar = U256::from(10u128.pow(quote_decimals as u32));
        let fixed_scalar = U256::from(10u128.pow(FIXED_PRECISION as u32));

        // Calculate: (quote_amount * 10^base_decimals * 10^FIXED_PRECISION) / (base_amount * 10^quote_decimals)
        // Use FullMath::mul_div to handle large intermediate values safely

        // Step 1: numerator = quote_amount * 10^base_decimals
        let numerator_step1 = FullMath::mul_div(quote_amount, base_decimals_scalar, U256::from(1))?;

        // Step 2: numerator = (quote_amount * 10^base_decimals) * 10^FIXED_PRECISION
        let numerator_final = FullMath::mul_div(numerator_step1, fixed_scalar, U256::from(1))?;

        // Step 3: denominator = base_amount * 10^quote_decimals
        let denominator = FullMath::mul_div(base_amount, quote_decimals_scalar, U256::from(1))?;

        // Step 4: Final division
        let price_raw_u256 = FullMath::mul_div(numerator_final, U256::from(1), denominator)?;

        // Convert to PriceRaw (i128)
        anyhow::ensure!(
            price_raw_u256 <= U256::from(i128::MAX as u128),
            "Price overflow: {} exceeds i128::MAX",
            price_raw_u256
        );

        let price_raw = price_raw_u256.to::<i128>();

        // price_raw is at FIXED_PRECISION scale, which is what Price expects
        Ok(Price::from_raw(price_raw, FIXED_PRECISION))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy_primitives::{I256, U160};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::defi::stubs::{usdc, weth};

    #[rstest]
    fn test_swap_trade_info_calculator_calculations_buy(weth: Token, usdc: Token) {
        // Real Arbitrum transaction: https://arbiscan.io/tx/0xb9af1fd5eefe82650a5e0f8ff10b3a5e1c7f05f44f255e1335360df97bd1645a
        let raw_data = RawSwapData::new(
            I256::from_str("-466341596920355889").unwrap(),
            I256::from_str("1656236893").unwrap(),
            U160::from_str("4720799958938693700000000").unwrap(),
        );

        let calculator = SwapTradeInfoCalculator::new(&weth, &usdc, raw_data);
        let result = calculator.compute(None).unwrap();
        // Its not inverted first is WETH(base) and second USDC(quote) as stablecoin
        assert!(!calculator.is_inverted);
        // Its buy, as amount0(WETH) < 0 (we received WETH, pool outflow) and amount1 > 0 (USDC sent, pool inflow)
        assert_eq!(result.order_side, OrderSide::Buy);
        assert_eq!(
            result.quantity_base.as_decimal(),
            dec!(0.466341596920355889)
        );
        assert_eq!(result.quantity_quote.as_decimal(), dec!(1656.236893));
        assert_eq!(result.spot_price.as_decimal(), dec!(3550.3570265047994091));
        assert_eq!(
            result.execution_price.as_decimal(),
            dec!(3551.5529902061477063)
        );
    }

    #[rstest]
    fn test_swap_trade_info_calculator_calculations_sell(weth: Token, usdc: Token) {
        //Real Arbitrum transaction: https://arbiscan.io/tx/0x1fbedacf4a1cc7f76174d905c93d2f56d42335cadb4a782e2d74e3019107286b
        let raw_data = RawSwapData::new(
            I256::from_str("193450074461093702").unwrap(),
            I256::from_str("-691892530").unwrap(),
            U160::from_str("4739235524363817533004858").unwrap(),
        );

        let calculator = SwapTradeInfoCalculator::new(&weth, &usdc, raw_data);
        let result = calculator.compute(None).unwrap();
        // Its sell as amount0(WETH) > 0 (we send WETH, pool inflow) and amount1 <0 (USDC received, pool outflow)
        assert_eq!(result.order_side, OrderSide::Sell);
        assert_eq!(
            result.quantity_base.as_decimal(),
            dec!(0.193450074461093702)
        );
        assert_eq!(result.quantity_quote.as_decimal(), dec!(691.89253));
        assert_eq!(result.spot_price.as_decimal(), dec!(3578.1407251651610105));
        assert_eq!(
            result.execution_price.as_decimal(),
            dec!(3576.5947980503469024)
        );
    }
}
