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

use alloy_primitives::{Address, I256, U160, U256};

use crate::{
    defi::{
        Pool, PoolSwap, SharedChain, SharedDex, Token,
        data::{
            block::BlockPosition,
            swap::RawSwapData,
            swap_trade_info::{SwapTradeInfo, SwapTradeInfoCalculator},
        },
        tick_map::{full_math::FullMath, tick::CrossedTick},
    },
    identifiers::InstrumentId,
};

/// Comprehensive swap quote containing profiling metrics for a hypothetical swap.
///
/// This structure provides detailed analysis of what would happen if a swap were executed,
/// including price impact, fees, slippage, and execution details, without actually
/// modifying the pool state.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct SwapQuote {
    /// Instrument identifier ......
    pub instrument_id: InstrumentId,
    /// Amount of token0 that would be swapped (positive = in, negative = out).
    pub amount0: I256,
    /// Amount of token1 that would be swapped (positive = in, negative = out).
    pub amount1: I256,
    /// Square root price before the swap (Q96 format).
    pub sqrt_price_before_x96: U160,
    /// Square root price after the swap (Q96 format).
    pub sqrt_price_after_x96: U160,
    /// Tick position before the swap.
    pub tick_before: i32,
    /// Tick position after the swap.
    pub tick_after: i32,
    /// Active liquidity after the swap.
    pub liquidity_after: u128,
    /// Fee growth global for target token after the swap (Q128.128 format).
    pub fee_growth_global_after: U256,
    /// Total fees paid to liquidity providers.
    pub lp_fee: U256,
    /// Total fees paid to the protocol.
    pub protocol_fee: U256,
    /// List of tick boundaries crossed during the swap, in order of crossing.
    pub crossed_ticks: Vec<CrossedTick>,
    /// Computed swap trade information in market-oriented format.
    pub trade_info: Option<SwapTradeInfo>,
}

impl SwapQuote {
    #[allow(clippy::too_many_arguments)]
    /// Creates a [`SwapQuote`] instance with comprehensive swap simulation results.
    ///
    /// The `trade_info` field is initialized to `None` and must be populated by calling
    /// [`calculate_trade_info()`](Self::calculate_trade_info) or will be lazily computed
    /// when accessing price impact or slippage methods.
    pub fn new(
        instrument_id: InstrumentId,
        amount0: I256,
        amount1: I256,
        sqrt_price_before_x96: U160,
        sqrt_price_after_x96: U160,
        tick_before: i32,
        tick_after: i32,
        liquidity_after: u128,
        fee_growth_global_after: U256,
        lp_fee: U256,
        protocol_fee: U256,
        crossed_ticks: Vec<CrossedTick>,
    ) -> Self {
        Self {
            instrument_id,
            amount0,
            amount1,
            sqrt_price_before_x96,
            sqrt_price_after_x96,
            tick_before,
            tick_after,
            liquidity_after,
            fee_growth_global_after,
            lp_fee,
            protocol_fee,
            crossed_ticks,
            trade_info: None,
        }
    }

    fn check_if_trade_info_initialized(&mut self) -> anyhow::Result<&SwapTradeInfo> {
        if self.trade_info.is_none() {
            anyhow::bail!(
                "Trade info is not initialized. Please call calculate_trade_info() first."
            );
        }

        Ok(self.trade_info.as_ref().unwrap())
    }

    /// Calculates and populates the `trade_info` field with market-oriented trade data.
    ///
    /// This method transforms the raw swap quote data (token0/token1 amounts, sqrt prices)
    /// into standard trading terminology (base/quote, order side, execution price).
    /// The computation uses the `sqrt_price_before_x96` to calculate price impact and slippage.
    ///
    /// # Errors
    ///
    /// Returns an error if trade info computation or price calculations fail.
    pub fn calculate_trade_info(&mut self, token0: &Token, token1: &Token) -> anyhow::Result<()> {
        let trade_info_calculator = SwapTradeInfoCalculator::new(
            token0,
            token1,
            RawSwapData::new(self.amount0, self.amount1, self.sqrt_price_after_x96),
        );
        let trade_info = trade_info_calculator.compute(Some(self.sqrt_price_before_x96))?;
        self.trade_info = Some(trade_info);

        Ok(())
    }

    /// Determines swap direction from amount signs.
    ///
    /// Returns `true` if swapping token0 for token1 (zero_for_one).
    pub fn zero_for_one(&self) -> bool {
        self.amount0.is_positive()
    }

    /// Returns the total fees paid in input token(LP fees + protocol fees).
    pub fn total_fee(&self) -> U256 {
        self.lp_fee + self.protocol_fee
    }

    /// Gets the effective fee rate in basis points based on actual fees charged
    pub fn get_effective_fee_bps(&self) -> u32 {
        let input_amount = self.get_input_amount();
        if input_amount.is_zero() {
            return 0;
        }

        let total_fees = self.lp_fee + self.protocol_fee;

        // fee_bps = (total_fees / input_amount) × 10000
        let fee_bps =
            FullMath::mul_div(total_fees, U256::from(10_000), input_amount).unwrap_or(U256::ZERO);

        fee_bps.to::<u32>()
    }

    /// Returns the number of tick boundaries crossed during this swap.
    ///
    /// This equals the length of the `crossed_ticks` vector and indicates
    /// how much liquidity the swap traversed.
    pub fn total_crossed_ticks(&self) -> u32 {
        self.crossed_ticks.len() as u32
    }

    /// Gets the output amount for the given swap direction.
    pub fn get_output_amount(&self) -> U256 {
        if self.zero_for_one() {
            self.amount1.unsigned_abs()
        } else {
            self.amount0.unsigned_abs()
        }
    }

    /// Gets the input amount for the given swap direction.
    pub fn get_input_amount(&self) -> U256 {
        if self.zero_for_one() {
            self.amount0.unsigned_abs()
        } else {
            self.amount1.unsigned_abs()
        }
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
    pub fn get_price_impact_bps(&mut self) -> anyhow::Result<u32> {
        match self.check_if_trade_info_initialized() {
            Ok(trade_info) => trade_info.get_price_impact_bps(),
            Err(e) => anyhow::bail!("Failed to calculate price impact: {}", e),
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
    pub fn get_slippage_bps(&mut self) -> anyhow::Result<u32> {
        match self.check_if_trade_info_initialized() {
            Ok(trade_info) => trade_info.get_slippage_bps(),
            Err(e) => anyhow::bail!("Failed to calculate slippage: {}", e),
        }
    }

    /// # Errors
    ///
    /// Returns an error if the actual slippage exceeds the maximum slippage tolerance.
    pub fn validate_slippage_tolerance(&mut self, max_slippage_bps: u32) -> anyhow::Result<()> {
        let actual_slippage = self.get_slippage_bps()?;
        if actual_slippage > max_slippage_bps {
            anyhow::bail!(
                "Slippage {} bps exceeds tolerance {} bps",
                actual_slippage,
                max_slippage_bps
            );
        }
        Ok(())
    }

    /// Validates that the quote satisfied an exact output request.
    ///
    /// # Errors
    /// Returns error if the actual output is less than the requested amount.
    pub fn validate_exact_output(&self, amount_out_requested: U256) -> anyhow::Result<()> {
        let actual_out = self.get_output_amount();
        if actual_out < amount_out_requested {
            anyhow::bail!(
                "Insufficient liquidity: requested {}, available {}",
                amount_out_requested,
                actual_out
            );
        }
        Ok(())
    }

    /// Converts this quote into a [`PoolSwap`] event with the provided metadata.
    ///
    /// # Returns
    /// A [`PoolSwap`] event containing both the quote data and provided metadata
    #[allow(clippy::too_many_arguments)]
    pub fn to_swap_event(
        &self,
        chain: SharedChain,
        dex: SharedDex,
        pool_address: &Address,
        block: BlockPosition,
        sender: Address,
        recipient: Address,
    ) -> PoolSwap {
        let instrument_id = Pool::create_instrument_id(chain.name, &dex, pool_address);
        PoolSwap::new(
            chain,
            dex,
            instrument_id,
            *pool_address,
            block.number,
            block.transaction_hash,
            block.transaction_index,
            block.log_index,
            None, // timestamp
            sender,
            recipient,
            self.amount0,
            self.amount1,
            self.sqrt_price_after_x96,
            self.liquidity_after,
            self.tick_after,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        defi::{SharedPool, stubs::rain_pool},
        enums::OrderSide,
    };

    #[rstest]
    fn test_swap_quote_sell(rain_pool: SharedPool) {
        // https://arbiscan.io/tx/0x3d03debc9f4becac1817c462b80ceae3705887a57b2b07b0d3ae4979d7aed519
        let sqrt_x96_price_before = U160::from_str("76951769738874829996307631").unwrap();
        let amount0 = I256::from_str("287175356684998201516914").unwrap();
        let amount1 = I256::from_str("-270157537808188649").unwrap();

        let mut swap_quote = SwapQuote::new(
            rain_pool.instrument_id,
            amount0,
            amount1,
            sqrt_x96_price_before,
            U160::from_str("76812046714213096298497129").unwrap(),
            -138746,
            -138782,
            292285495328044734302670,
            U256::ZERO,
            U256::ZERO,
            U256::ZERO,
            vec![],
        );
        swap_quote
            .calculate_trade_info(&rain_pool.token0, &rain_pool.token1)
            .unwrap();

        if let Some(swap_trade_info) = &swap_quote.trade_info {
            assert_eq!(swap_trade_info.order_side, OrderSide::Sell);
            assert_eq!(swap_quote.get_input_amount(), amount0.unsigned_abs());
            assert_eq!(swap_quote.get_output_amount(), amount1.unsigned_abs());
            // Check with DexScreener to get their trade data calculations
            assert_eq!(
                swap_trade_info.quantity_base.as_decimal(),
                dec!(287175.356684998201516914)
            );
            assert_eq!(
                swap_trade_info.quantity_quote.as_decimal(),
                dec!(0.270157537808188649)
            );
            assert_eq!(
                swap_trade_info.spot_price.as_decimal(),
                dec!(0.0000009399386483)
            );
            assert_eq!(swap_trade_info.get_price_impact_bps().unwrap(), 36);
            assert_eq!(swap_trade_info.get_slippage_bps().unwrap(), 28);
        } else {
            panic!("Trade info is None");
        }
    }

    #[rstest]
    fn test_swap_quote_buy(rain_pool: SharedPool) {
        // https://arbiscan.io/tx/0x50b5adaf482558f84539e3234dd01b3a29fc43a1e2ab997960efd219d6e81ffe
        let sqrt_x96_price_before = U160::from_str("76827576486429933391429745").unwrap();
        let amount0 = I256::from_str("-117180628248242869089291").unwrap();
        let amount1 = I256::from_str("110241020399788696").unwrap();

        let mut swap_quote = SwapQuote::new(
            rain_pool.instrument_id,
            amount0,
            amount1,
            sqrt_x96_price_before,
            U160::from_str("76857455902960072891859299").unwrap(),
            -138778,
            -138770,
            292285495328044734302670,
            U256::ZERO,
            U256::ZERO,
            U256::ZERO,
            vec![],
        );
        swap_quote
            .calculate_trade_info(&rain_pool.token0, &rain_pool.token1)
            .unwrap();

        if let Some(swap_trade_info) = &swap_quote.trade_info {
            assert_eq!(swap_trade_info.order_side, OrderSide::Buy);
            assert_eq!(swap_quote.get_input_amount(), amount1.unsigned_abs());
            assert_eq!(swap_quote.get_output_amount(), amount0.unsigned_abs());
            // Check with DexScreener to get their trade data calculations
            assert_eq!(
                swap_trade_info.quantity_base.as_decimal(),
                dec!(117180.628248242869089291)
            );
            assert_eq!(
                swap_trade_info.quantity_quote.as_decimal(),
                dec!(0.110241020399788696)
            );
            assert_eq!(
                swap_trade_info.spot_price.as_decimal(),
                dec!(0.000000941050309)
            );
            assert_eq!(
                swap_trade_info.execution_price.as_decimal(),
                dec!(0.0000009407785403)
            );
            assert_eq!(swap_trade_info.get_price_impact_bps().unwrap(), 8);
            assert_eq!(swap_trade_info.get_slippage_bps().unwrap(), 5);
        } else {
            panic!("Trade info is None");
        }
    }
}
