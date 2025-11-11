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

use std::cmp::max;

use alloy_primitives::U256;

use crate::{
    defi::{
        Token,
        data::swap::RawSwapData,
        tick_map::{
            full_math::FullMath, sqrt_price_math::decode_sqrt_price_x96_to_price_tokens_adjusted,
        },
    },
    enums::OrderSide,
    types::{Price, Quantity, fixed::FIXED_PRECISION, price::PriceRaw, quantity::QuantityRaw},
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
    /// applying token priority rules and decimal adjustments.
    ///
    /// # Errors
    ///
    /// Returns an error if quantity or price calculations fail.
    pub fn compute(&self) -> anyhow::Result<SwapTradeInfo> {
        Ok(SwapTradeInfo {
            order_side: self.order_side(),
            quantity_base: self.quantity_base()?,
            quantity_quote: self.quantity_quote()?,
            spot_price: self.spot_price()?,
            execution_price: self.execution_price()?,
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
        let (amount, token_decimals) = if self.is_inverted {
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

        // Cap precision at FIXED_PRECISION (16) for safe f64 conversion
        let precision = token_decimals.min(FIXED_PRECISION);

        // Scale directly to FIXED_PRECISION based on diff between token_decimals and FIXED_PRECISION
        let decimal_diff = i32::from(token_decimals) - i32::from(FIXED_PRECISION);
        let raw_value = if decimal_diff > 0 {
            // Token has >16 decimals: scale DOWN
            amount / U256::from(10u128.pow(decimal_diff as u32))
        } else if decimal_diff < 0 {
            // Token has <16 decimals: scale UP
            amount * U256::from(10u128.pow((-decimal_diff) as u32))
        } else {
            // Exactly 16 decimals: no scaling
            amount
        };

        let raw = QuantityRaw::try_from(raw_value).map_err(|_| {
            anyhow::anyhow!("Base quantity {} exceeds QuantityRaw range", raw_value)
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
        let (amount, token_decimals) = if self.is_inverted {
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

        // Cap precision at FIXED_PRECISION (16) for safe f64 conversion
        let precision = token_decimals.min(FIXED_PRECISION);

        // Scale directly to FIXED_PRECISION based on diff between token_decimals and FIXED_PRECISION
        let decimal_diff = i32::from(token_decimals) - i32::from(FIXED_PRECISION);
        let raw_value = if decimal_diff > 0 {
            // Token has >16 decimals: scale DOWN
            amount / U256::from(10u128.pow(decimal_diff as u32))
        } else if decimal_diff < 0 {
            // Token has <16 decimals: scale UP
            amount * U256::from(10u128.pow((-decimal_diff) as u32))
        } else {
            // Exactly 16 decimals: no scaling
            amount
        };

        let raw = QuantityRaw::try_from(raw_value).map_err(|_| {
            anyhow::anyhow!("Quote quantity {} exceeds QuantityRaw range", raw_value)
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

        // Ensure we have amounts to work with
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

        let calc_precision = max(max(base_decimals, quote_decimals), FIXED_PRECISION);

        // price = quote_amount / base_amount (quote per base)
        let price_scaled = FullMath::mul_div(
            quote_amount,
            U256::from(10u128.pow(calc_precision as u32)),
            base_amount,
        )?;

        // Adjust for token decimals: multiply by 10^(base_decimals - quote_decimals)
        let decimal_diff = i32::from(base_decimals) - i32::from(quote_decimals);
        let price_adjusted = if decimal_diff > 0 {
            price_scaled
                .checked_mul(U256::from(10u128.pow(decimal_diff as u32)))
                .ok_or_else(|| anyhow::anyhow!("Price overflow"))?
        } else if decimal_diff < 0 {
            price_scaled
                .checked_div(U256::from(10u128.pow((-decimal_diff) as u32)))
                .ok_or_else(|| anyhow::anyhow!("Price underflow"))?
        } else {
            price_scaled
        };

        // Scale down if precision exceeds FIXED_PRECISION
        let (scaled_price, final_precision) = if calc_precision > FIXED_PRECISION {
            let scale_factor = 10u128.pow((calc_precision - FIXED_PRECISION) as u32);
            let scaled = price_adjusted / U256::from(scale_factor);
            (scaled, FIXED_PRECISION)
        } else {
            (price_adjusted, calc_precision)
        };

        let raw = PriceRaw::try_from(scaled_price)?;
        Ok(Price::from_raw(raw, final_precision))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy_primitives::{I256, U160};
    use rstest::rstest;

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
        let result = calculator.compute().unwrap();
        // Its not inverted first is WETH(base) and second USDC(quote) as stablecoin
        assert!(!calculator.is_inverted);
        // Its buy, as amount0(WETH) < 0 (we received WETH, pool outflow) and amount1 > 0 (USDC sent, pool inflow)
        assert_eq!(result.order_side, OrderSide::Buy);
        assert_eq!(result.quantity_base.as_f64(), 0.4663415969203558);
        assert_eq!(result.quantity_quote.as_f64(), 1656.236893);
        assert_eq!(result.spot_price.as_f64(), 3550.3570265047993);
        assert_eq!(result.execution_price.as_f64(), 3551.55299);
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
        let result = calculator.compute().unwrap();
        // Its sell as amount0(WETH) > 0 (we send WETH, pool inflow) and amount1 <0 (USDC received, pool outflow)
        assert_eq!(result.order_side, OrderSide::Sell);
        assert_eq!(result.quantity_base.as_f64(), 0.1934500744610937);
        assert_eq!(result.quantity_quote.as_f64(), 691.89253);
        assert_eq!(result.spot_price.as_f64(), 3578.140725165161);
        assert_eq!(result.execution_price.as_f64(), 3576.594798);
    }
}
