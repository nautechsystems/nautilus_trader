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

use std::fmt::Display;

use alloy_primitives::{Address, I256, U160};
use nautilus_core::UnixNanos;

use crate::{
    defi::{
        PoolIdentifier, SharedChain, SharedDex, Token,
        data::swap_trade_info::{SwapTradeInfo, SwapTradeInfoCalculator},
    },
    identifiers::InstrumentId,
};

/// Raw swap data directly from the blockchain event log.
#[derive(Debug, Clone)]
pub struct RawSwapData {
    /// Amount of token0 involved in the swap (positive = in, negative = out).
    pub amount0: I256,
    /// Amount of token1 involved in the swap (positive = in, negative = out).
    pub amount1: I256,
    /// Square root price of the pool AFTER the swap (Q64.96 fixed-point format).
    pub sqrt_price_x96: U160,
}

impl RawSwapData {
    /// Creates a new [`RawSwapData`] instance with the specified values.
    pub fn new(amount0: I256, amount1: I256, sqrt_price_x96: U160) -> Self {
        Self {
            amount0,
            amount1,
            sqrt_price_x96,
        }
    }
}

/// Represents a token swap transaction on a decentralized exchange (DEX).
///
/// This structure captures both the raw blockchain data from a swap event and
/// optionally includes computed market-oriented trade information. It serves as
/// the primary data structure for tracking and analyzing DEX swap activity.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct PoolSwap {
    /// The blockchain network where the swap occurred.
    pub chain: SharedChain,
    /// The decentralized exchange where the swap was executed.
    pub dex: SharedDex,
    /// The instrument ID for this pool's trading pair.
    pub instrument_id: InstrumentId,
    /// The unique identifier for this pool (could be an address or other protocol-specific hex string).
    pub pool_identifier: PoolIdentifier,
    /// The blockchain block number at which the swap was executed.
    pub block: u64,
    /// The unique hash identifier of the blockchain transaction containing the swap.
    pub transaction_hash: String,
    /// The index position of the transaction within the block.
    pub transaction_index: u32,
    /// The index position of the swap event log within the transaction.
    pub log_index: u32,
    /// The blockchain address of the user or contract that initiated the swap.
    pub sender: Address,
    /// The blockchain address that received the swapped tokens.
    pub recipient: Address,
    /// The sqrt price after the swap (Q64.96 format).
    pub sqrt_price_x96: U160,
    /// The amount of token0 involved in the swap.
    pub amount0: I256,
    /// The amount of token1 involved in the swap.
    pub amount1: I256,
    /// The liquidity of the pool after the swap occurred.
    pub liquidity: u128,
    /// The current tick of the pool after the swap occurred.
    pub tick: i32,
    /// UNIX timestamp (nanoseconds) when the swap occurred.
    pub timestamp: Option<UnixNanos>,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: Option<UnixNanos>,
    /// Optional computed trade information in market-oriented format.
    /// This translates raw blockchain data into standard trading terminology.
    pub trade_info: Option<SwapTradeInfo>,
}

impl PoolSwap {
    /// Creates a new [`PoolSwap`] instance with the specified properties.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain: SharedChain,
        dex: SharedDex,
        instrument_id: InstrumentId,
        pool_identifier: PoolIdentifier,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        timestamp: Option<UnixNanos>,
        sender: Address,
        recipient: Address,
        amount0: I256,
        amount1: I256,
        sqrt_price_x96: U160,
        liquidity: u128,
        tick: i32,
    ) -> Self {
        Self {
            chain,
            dex,
            instrument_id,
            pool_identifier,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            timestamp,
            sender,
            recipient,
            amount0,
            amount1,
            sqrt_price_x96,
            liquidity,
            tick,
            ts_init: timestamp, // TODO: Use swap timestamp as init timestamp for now
            trade_info: None,
        }
    }

    /// Calculates and populates the `trade_info` field with market-oriented trade data.
    ///
    /// This method transforms the raw blockchain swap data (token0/token1 amounts) into
    /// standard trading terminology (base/quote, buy/sell, execution price). The computation
    /// determines token roles based on priority and handles decimal adjustments.
    ///
    /// # Arguments
    ///
    /// * `token0` - Reference to token0 in the pool
    /// * `token1` - Reference to token1 in the pool
    /// * `sqrt_price_x96` - Optional square root price before the swap (Q96 format) for calculating price impact and slippage
    ///
    /// # Errors
    ///
    /// Returns an error if the trade info computation or price calculations fail.
    ///
    pub fn calculate_trade_info(
        &mut self,
        token0: &Token,
        token1: &Token,
        sqrt_price_x96: Option<U160>,
    ) -> anyhow::Result<()> {
        let trade_info_calculator = SwapTradeInfoCalculator::new(
            token0,
            token1,
            RawSwapData::new(self.amount0, self.amount1, self.sqrt_price_x96),
        );
        self.trade_info = Some(trade_info_calculator.compute(sqrt_price_x96)?);

        Ok(())
    }
}

impl Display for PoolSwap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={})",
            stringify!(PoolSwap),
            self.instrument_id,
        )
    }
}
