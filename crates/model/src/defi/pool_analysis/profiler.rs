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

//! Pool profiling utilities for analyzing DeFi pool event data.

use std::collections::HashMap;

use alloy_primitives::{Address, I256, U160, U256};

use crate::defi::{
    PoolLiquidityUpdate, PoolSwap, SharedPool,
    data::{
        DexPoolData, PoolFeeCollect, PoolLiquidityUpdateType, block::BlockPosition,
        flash::PoolFlash,
    },
    pool_analysis::{
        position::PoolPosition,
        snapshot::{PoolAnalytics, PoolSnapshot, PoolState},
        swap_math::compute_swap_step,
    },
    reporting::{BlockchainSyncReportItems, BlockchainSyncReporter},
    tick_map::{
        TickMap,
        full_math::{FullMath, Q128},
        liquidity_math::liquidity_math_add,
        sqrt_price_math::{get_amount0_delta, get_amount1_delta, get_amounts_for_liquidity},
        tick::PoolTick,
        tick_math::{
            MAX_SQRT_RATIO, MIN_SQRT_RATIO, get_sqrt_ratio_at_tick, get_tick_at_sqrt_ratio,
        },
    },
};

/// A DeFi pool state tracker and event processor for UniswapV3-style AMM pools.
///
/// The `PoolProfiler` provides complete pool state management including:
/// - Liquidity position tracking and management.
/// - Tick crossing and price movement simulation.
/// - Fee accumulation and distribution tracking.
/// - Protocol fee calculation.
/// - Pool state validation and maintenance.
///
/// This profiler can both process historical events and execute new operations,
/// making it suitable for both backtesting and simulation scenarios.
///
/// # Usage
///
/// Create a new profiler with a pool definition, initialize it with a starting price,
/// then either process historical events or execute new pool operations to simulate
/// trading activity and analyze pool behavior.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct PoolProfiler {
    /// Pool definition.
    pub pool: SharedPool,
    /// Position tracking by position key (owner:tick_lower:tick_upper).
    positions: HashMap<String, PoolPosition>,
    /// Tick map managing liquidity distribution across price ranges.
    pub tick_map: TickMap,
    /// Global pool state including current price, tick, and cumulative flows with fees.
    pub state: PoolState,
    /// Analytics counters tracking pool operations and performance metrics.
    pub analytics: PoolAnalytics,
    /// The block position of the last processed event.
    pub last_processed_event: Option<BlockPosition>,
    /// Flag indicating whether the pool has been initialized with a starting price.
    pub is_initialized: bool,
    /// Optional progress reporter for tracking event processing.
    reporter: Option<BlockchainSyncReporter>,
    /// The last block number that was reported (used for progress tracking).
    last_reported_block: u64,
}

impl PoolProfiler {
    /// Creates a new [`PoolProfiler`] instance for tracking pool state and events.
    ///
    /// # Panics
    ///
    /// Panics if the pool's tick spacing is not set.
    #[must_use]
    pub fn new(pool: SharedPool) -> Self {
        let tick_spacing = pool.tick_spacing.expect("Pool tick spacing must be set");
        Self {
            pool,
            positions: HashMap::new(),
            tick_map: TickMap::new(tick_spacing),
            state: PoolState::default(),
            analytics: PoolAnalytics::default(),
            last_processed_event: None,
            is_initialized: false,
            reporter: None,
            last_reported_block: 0,
        }
    }

    /// Initializes the pool with a starting price and activates the profiler.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - Pool is already initialized (checked via `is_initialized` flag)
    /// - Calculated tick from price doesn't match pool's `initial_tick` (if set)
    pub fn initialize(&mut self, price_sqrt_ratio_x96: U160) {
        if self.is_initialized {
            panic!("Pool already initialized");
        }

        let calculated_tick = get_tick_at_sqrt_ratio(price_sqrt_ratio_x96);
        if let Some(initial_tick) = self.pool.initial_tick {
            assert_eq!(
                initial_tick, calculated_tick,
                "Calculated tick does not match pool initial tick"
            );
        }

        tracing::info!(
            "Initializing pool profiler with tick {} and price sqrt ratio {}",
            calculated_tick,
            price_sqrt_ratio_x96
        );

        self.state.current_tick = calculated_tick;
        self.state.price_sqrt_ratio_x96 = price_sqrt_ratio_x96;
        self.is_initialized = true;
    }

    /// Verifies that the pool has been initialized.
    ///
    /// # Panics
    ///
    /// Panics if the pool hasn't been initialized with a starting price via [`initialize()`](Self::initialize).
    pub fn check_if_initialized(&self) {
        if !self.is_initialized {
            panic!("Pool is not initialized");
        }
    }

    /// Processes a historical pool event and updates internal state.
    ///
    /// Handles all types of pool events (swaps, mints, burns, fee collections),
    /// and updates the profiler's internal state accordingly. This is the main
    /// entry point for processing historical blockchain events.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Pool is not initialized.
    /// - Event contains invalid data (tick ranges, amounts).
    /// - Mathematical operations overflow.
    pub fn process(&mut self, event: &DexPoolData) -> anyhow::Result<()> {
        if self.check_if_already_processed(
            event.block_number(),
            event.transaction_index(),
            event.log_index(),
        ) {
            return Ok(());
        }

        match event {
            DexPoolData::Swap(swap) => self.process_swap(swap)?,
            DexPoolData::LiquidityUpdate(update) => match update.kind {
                PoolLiquidityUpdateType::Mint => self.process_mint(update)?,
                PoolLiquidityUpdateType::Burn => self.process_burn(update)?,
            },
            DexPoolData::FeeCollect(collect) => self.process_collect(collect)?,
            DexPoolData::Flash(flash) => self.process_flash(flash)?,
        }
        self.update_reporter_if_enabled(event.block_number());

        Ok(())
    }

    // Checks if we need to skip events at or before the last processed event to prevent double-processing.
    fn check_if_already_processed(&self, block: u64, tx_idx: u32, log_idx: u32) -> bool {
        if let Some(last_event) = &self.last_processed_event {
            let should_skip = block < last_event.number
                || (block == last_event.number && tx_idx < last_event.transaction_index)
                || (block == last_event.number
                    && tx_idx == last_event.transaction_index
                    && log_idx <= last_event.log_index);

            if should_skip {
                tracing::debug!(
                    "Skipping already processed event at block {} tx {} log {}",
                    block,
                    tx_idx,
                    log_idx
                );
            }
            return should_skip;
        }

        false
    }

    /// Auto-updates reporter if it's enabled.
    fn update_reporter_if_enabled(&mut self, current_block: u64) {
        // Auto-update reporter if enabled
        if let Some(reporter) = &mut self.reporter {
            let blocks_processed = current_block.saturating_sub(self.last_reported_block);

            if blocks_processed > 0 {
                reporter.update(blocks_processed as usize);
                self.last_reported_block = current_block;

                if reporter.should_log_progress(current_block, current_block) {
                    reporter.log_progress(current_block);
                }
            }
        }
    }

    /// Processes a historical swap event from blockchain data.
    ///
    /// Replays the swap by simulating it through [`Self::simulate_swap_through_ticks`],
    /// then verifies the simulation results against the actual event data. If mismatches
    /// are detected (tick or liquidity), the pool state is corrected to match the event
    /// values and warnings are logged.
    ///
    /// This self-healing approach ensures pool state stays synchronized with on-chain
    /// reality even if simulation logic differs slightly from actual contract behavior.
    ///
    /// # Use Case
    ///
    /// Historical event processing when rebuilding pool state from blockchain events.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Pool initialization checks fail.
    /// - Swap simulation fails (see [`Self::simulate_swap_through_ticks`] errors).
    ///
    /// # Panics
    ///
    /// Panics if the pool has not been initialized.
    pub fn process_swap(&mut self, swap: &PoolSwap) -> anyhow::Result<()> {
        self.check_if_initialized();
        if self.check_if_already_processed(swap.block, swap.transaction_index, swap.log_index) {
            return Ok(());
        }

        let zero_for_one = swap.amount0.is_positive();
        let amount_specified = if zero_for_one {
            swap.amount0
        } else {
            swap.amount1
        };
        // For price limit use the final sqrt price from swap, which is a
        // good proxy to price limit
        let sqrt_price_limit_x96 = swap.sqrt_price_x96;
        let (_, _) =
            self.simulate_swap_through_ticks(amount_specified, zero_for_one, sqrt_price_limit_x96)?;

        // Verify simulation against event data - correct with event values if mismatch detected
        if swap.tick != self.state.current_tick {
            tracing::error!(
                "Inconsistency in swap processing: Current tick mismatch: simulated {}, event {} on block {}",
                self.state.current_tick,
                swap.tick,
                swap.block
            );
            self.state.current_tick = swap.tick;
        }
        if swap.liquidity != self.tick_map.liquidity {
            tracing::error!(
                "Inconsistency in swap processing: Active liquidity mismatch: simulated {}, event {} on block {}",
                self.tick_map.liquidity,
                swap.liquidity,
                swap.block
            );
            self.tick_map.liquidity = swap.liquidity;
        }

        self.analytics.total_swaps += 1;
        self.last_processed_event = Some(BlockPosition::new(
            swap.block,
            swap.transaction_hash.clone(),
            swap.transaction_index,
            swap.log_index,
        ));
        self.update_reporter_if_enabled(swap.block);
        self.update_liquidity_analytics();

        Ok(())
    }

    /// Executes a new simulated swap and returns the resulting event.
    ///
    /// This is the public API for forward simulation of swap operations. It delegates
    /// the core swap mathematics to [`Self::simulate_swap_through_ticks`], then wraps
    /// the results in a [`PoolSwap`] event structure with full metadata.
    ///
    /// # Errors
    ///
    /// Returns errors from [`Self::simulate_swap_through_ticks`]:
    /// - Pool metadata missing or invalid
    /// - Price limit violations
    /// - Arithmetic overflow in fee or liquidity calculations
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - Pool fee is not initialized
    /// - Pool is not initialized
    pub fn execute_swap(
        &mut self,
        sender: Address,
        recipient: Address,
        block: BlockPosition,
        zero_for_one: bool,
        amount_specified: I256,
        sqrt_price_limit_x96: U160,
    ) -> anyhow::Result<PoolSwap> {
        self.check_if_initialized();
        let (amount0, amount1) =
            self.simulate_swap_through_ticks(amount_specified, zero_for_one, sqrt_price_limit_x96)?;

        self.analytics.total_swaps += 1;
        let swap_event = PoolSwap::new(
            self.pool.chain.clone(),
            self.pool.dex.clone(),
            self.pool.instrument_id,
            self.pool.address,
            block.number,
            block.transaction_hash,
            block.transaction_index,
            block.log_index,
            None,
            sender,
            recipient,
            amount0,
            amount1,
            self.state.price_sqrt_ratio_x96,
            self.tick_map.liquidity,
            self.state.current_tick,
            None,
            None,
            None,
        );
        Ok(swap_event)
    }

    /// Core swap simulation engine implementing UniswapV3 mathematics.
    ///
    /// This private method contains the complete AMM swap algorithm and is the
    /// computational heart of both [`Self::execute_swap`] (forward simulation)
    /// and [`Self::process_swap`] (historical replay).
    ///
    /// # Algorithm Overview
    ///
    /// 1. **Iterative price curve traversal**: Walks through liquidity ranges until
    ///    the input/output amount is exhausted or the price limit is reached
    /// 2. **Tick crossing**: When reaching an initialized tick boundary, updates
    ///    active liquidity by applying the tick's `liquidity_net`
    /// 3. **Fee calculation**: Splits fees between LPs (via fee growth globals)
    ///    and protocol (via protocol fee percentage)
    /// 4. **State mutation**: Updates current tick, sqrt price, liquidity, and
    ///    fee growth accumulators
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Pool fee is not configured
    /// - Fee growth arithmetic overflows when scaling by liquidity
    /// - Invalid state encountered during tick crossing
    ///
    /// # Panics
    ///
    /// Panics if pool is not initialized
    pub fn simulate_swap_through_ticks(
        &mut self,
        amount_specified: I256,
        zero_for_one: bool,
        sqrt_price_limit_x96: U160,
    ) -> anyhow::Result<(I256, I256)> {
        let mut current_sqrt_price = self.state.price_sqrt_ratio_x96;
        let mut current_tick = self.state.current_tick;
        let exact_input = amount_specified.is_positive();
        let mut amount_specified_remaining = amount_specified;
        let mut amount_calculated = I256::ZERO;
        let mut protocol_fee = U256::ZERO;
        let fee_tier = self.pool.fee.expect("Pool fee should be initialized");
        // Swapping cache variables
        let fee_protocol = if zero_for_one {
            // Extract lower 4 bits for token0 protocol fee
            self.state.fee_protocol % 16
        } else {
            // Extract upper 4 bits for token1 protocol fee
            self.state.fee_protocol >> 4
        };

        // Track current fee growth during swap
        let mut current_fee_growth_global = if zero_for_one {
            self.state.fee_growth_global_0
        } else {
            self.state.fee_growth_global_1
        };

        // Continue swapping as long as we haven't used the entire input/output or haven't reached the price limit
        while amount_specified_remaining != I256::ZERO && sqrt_price_limit_x96 != current_sqrt_price
        {
            let sqrt_price_start_x96 = current_sqrt_price;

            let (mut tick_next, initialized) = self
                .tick_map
                .next_initialized_tick(current_tick, zero_for_one);

            // Make sure we do not overshoot MIN/MAX tick
            tick_next = tick_next.clamp(PoolTick::MIN_TICK, PoolTick::MAX_TICK);

            // Get the price for the next tick
            let sqrt_price_next = get_sqrt_ratio_at_tick(tick_next);

            // Compute values to swap to the target tick, price limit, or point where input/output amount is exhausted
            let sqrt_price_target = if (zero_for_one && sqrt_price_next < sqrt_price_limit_x96)
                || (!zero_for_one && sqrt_price_next > sqrt_price_limit_x96)
            {
                sqrt_price_limit_x96
            } else {
                sqrt_price_next
            };
            let swap_step_result = compute_swap_step(
                current_sqrt_price,
                sqrt_price_target,
                self.get_active_liquidity(),
                amount_specified_remaining,
                fee_tier,
            )?;

            // Update current price to the new price after this swap step (BEFORE amount updates, matching Solidity)
            current_sqrt_price = swap_step_result.sqrt_ratio_next_x96;

            // Update amounts based on swap direction and type
            if exact_input {
                // For exact input swaps: subtract input amount and fees from remaining, subtract output from calculated
                amount_specified_remaining -= FullMath::truncate_to_i256(
                    swap_step_result.amount_in + swap_step_result.fee_amount,
                );
                amount_calculated -= FullMath::truncate_to_i256(swap_step_result.amount_out);
            } else {
                // For exact output swaps: add output to remaining, add input and fees to calculated
                amount_specified_remaining +=
                    FullMath::truncate_to_i256(swap_step_result.amount_out);
                amount_calculated += FullMath::truncate_to_i256(
                    swap_step_result.amount_in + swap_step_result.fee_amount,
                );
            }

            // Calculate protocol fee if enabled
            let mut step_fee_amount = swap_step_result.fee_amount;
            if fee_protocol > 0 {
                let protocol_fee_delta = swap_step_result.fee_amount / U256::from(fee_protocol);
                step_fee_amount -= protocol_fee_delta;
                protocol_fee += protocol_fee_delta;
            }

            // Update global fee tracker
            if self.tick_map.liquidity > 0 {
                let fee_growth_delta =
                    FullMath::mul_div(step_fee_amount, Q128, U256::from(self.tick_map.liquidity))?;
                current_fee_growth_global += fee_growth_delta;
            }

            // Shift tick if we reached the next price
            if swap_step_result.sqrt_ratio_next_x96 == sqrt_price_next {
                // We have swapped all the way to the boundary of the next tick.
                // Time to handle crossing into the next tick, which may change liquidity.
                // If the tick is initialized, run the tick transition logic (liquidity changes, fee accumulators, etc.).
                if initialized {
                    let liquidity_net = self.tick_map.cross_tick(
                        tick_next,
                        if zero_for_one {
                            current_fee_growth_global
                        } else {
                            self.state.fee_growth_global_0
                        },
                        if zero_for_one {
                            self.state.fee_growth_global_1
                        } else {
                            current_fee_growth_global
                        },
                    );

                    // Apply liquidity change based on crossing direction
                    // When crossing down (zeroForOne = true), negate liquidity_net before adding
                    // When crossing up (zeroForOne = false), use liquidity_net as-is without negation
                    self.tick_map.liquidity = if zero_for_one {
                        liquidity_math_add(self.tick_map.liquidity, -liquidity_net)
                    } else {
                        liquidity_math_add(self.tick_map.liquidity, liquidity_net)
                    };
                }

                current_tick = if zero_for_one {
                    tick_next - 1
                } else {
                    tick_next
                };
            } else if swap_step_result.sqrt_ratio_next_x96 != sqrt_price_start_x96 {
                // The price moved during this swap step, but didn't reach a tick boundary.
                // So, update the tick to match the new price.
                current_tick = get_tick_at_sqrt_ratio(current_sqrt_price);
            }
        }

        // Update pool state - match Solidity exactly
        if self.state.current_tick != current_tick {
            self.state.current_tick = current_tick;
            self.state.price_sqrt_ratio_x96 = current_sqrt_price;
        } else {
            // Otherwise just update the price
            self.state.price_sqrt_ratio_x96 = current_sqrt_price;
        }

        // Update fee growth global and if necessary, protocol fees
        if zero_for_one {
            self.state.fee_growth_global_0 = current_fee_growth_global;
            self.state.protocol_fees_token0 += protocol_fee;
        } else {
            self.state.fee_growth_global_1 = current_fee_growth_global;
            self.state.protocol_fees_token1 += protocol_fee;
        }

        // Calculate final amounts
        let (amount0, amount1) = if zero_for_one == exact_input {
            (
                amount_specified - amount_specified_remaining,
                amount_calculated,
            )
        } else {
            (
                amount_calculated,
                amount_specified - amount_specified_remaining,
            )
        };

        Ok((amount0, amount1))
    }

    /// Swaps an exact amount of token0 for token1.
    ///
    /// Convenience method for executing exact input swaps from token0 to token1.
    /// Sets up parameters and delegates to `execute_swap`.
    ///
    /// # Errors
    ///
    /// Returns error from [`Self::execute_swap`] when swap execution fails.
    pub fn swap_exact0_for_1(
        &mut self,
        sender: Address,
        recipient: Address,
        block: BlockPosition,
        amount0_in: U256,
        sqrt_price_limit_x96: Option<U160>,
    ) -> anyhow::Result<PoolSwap> {
        let amount_specified = I256::from(amount0_in);
        let sqrt_price_limit_x96 = sqrt_price_limit_x96.unwrap_or(MIN_SQRT_RATIO + U160::from(1));
        self.execute_swap(
            sender,
            recipient,
            block,
            true,
            amount_specified,
            sqrt_price_limit_x96,
        )
    }

    /// Swaps token0 for an exact amount of token1.
    ///
    /// Convenience method for executing exact output swaps from token0 to token1.
    /// Uses negative amount to indicate exact output specification.
    ///
    /// # Errors
    ///
    /// Returns error from [`Self::execute_swap`] when swap execution fails.
    pub fn swap_0_for_exact1(
        &mut self,
        sender: Address,
        recipient: Address,
        block: BlockPosition,
        amount1_out: U256,
        sqrt_price_limit_x96: Option<U160>,
    ) -> anyhow::Result<PoolSwap> {
        let amount_specified = -I256::from(amount1_out);
        let sqrt_price_limit_x96 = sqrt_price_limit_x96.unwrap_or(MIN_SQRT_RATIO + U160::from(1));
        self.execute_swap(
            sender,
            recipient,
            block,
            true,
            amount_specified,
            sqrt_price_limit_x96,
        )
    }

    /// Swaps an exact amount of token1 for token0.
    ///
    /// Convenience method for executing exact input swaps from token1 to token0.
    /// Sets up parameters and delegates to `execute_swap`.
    ///
    /// # Errors
    ///
    /// Returns error from [`Self::execute_swap`] when swap execution fails.
    pub fn swap_exact1_for_0(
        &mut self,
        sender: Address,
        recipient: Address,
        block: BlockPosition,
        amount1_in: U256,
        sqrt_price_limit_x96: Option<U160>,
    ) -> anyhow::Result<PoolSwap> {
        let amount_specified = I256::from(amount1_in);
        let sqrt_price_limit_x96 = sqrt_price_limit_x96.unwrap_or(MAX_SQRT_RATIO - U160::from(1));
        self.execute_swap(
            sender,
            recipient,
            block,
            false,
            amount_specified,
            sqrt_price_limit_x96,
        )
    }

    /// Swaps token1 for an exact amount of token0.
    ///
    /// Convenience method for executing exact output swaps from token1 to token0.
    /// Uses negative amount to indicate the exact output specification.
    ///
    /// # Errors
    ///
    /// Returns error from [`Self::execute_swap`] when swap execution fails.
    pub fn swap_1_for_exact0(
        &mut self,
        sender: Address,
        recipient: Address,
        block: BlockPosition,
        amount0_out: U256,
        sqrt_price_limit_x96: Option<U160>,
    ) -> anyhow::Result<PoolSwap> {
        let amount_specified = -I256::from(amount0_out);
        let sqrt_price_limit_x96 = sqrt_price_limit_x96.unwrap_or(MAX_SQRT_RATIO - U160::from(1));
        self.execute_swap(
            sender,
            recipient,
            block,
            false,
            amount_specified,
            sqrt_price_limit_x96,
        )
    }

    /// Swaps to move the pool price down to a target price.
    ///
    /// Performs a token0-for-token1 swap with maximum input to reach the target price.
    ///
    /// # Errors
    ///
    /// Returns error from [`Self::execute_swap`] when swap execution fails.
    pub fn swap_to_lower_sqrt_price(
        &mut self,
        sender: Address,
        recipient: Address,
        block: BlockPosition,
        sqrt_price_limit_x96: U160,
    ) -> anyhow::Result<PoolSwap> {
        self.execute_swap(
            sender,
            recipient,
            block,
            true,
            I256::MAX,
            sqrt_price_limit_x96,
        )
    }

    /// Swaps to move the pool price up to a target price.
    ///
    /// Performs a token1-for-token0 swap with maximum input to reach the target price.
    ///
    /// # Errors
    ///
    /// Returns error from [`Self::execute_swap`] when swap execution fails.
    pub fn swap_to_higher_sqrt_price(
        &mut self,
        sender: Address,
        recipient: Address,
        block: BlockPosition,
        sqrt_price_limit_x96: U160,
    ) -> anyhow::Result<PoolSwap> {
        self.execute_swap(
            sender,
            recipient,
            block,
            false,
            I256::MAX,
            sqrt_price_limit_x96,
        )
    }

    /// Processes a mint (liquidity add) event from historical data.
    ///
    /// Updates pool state when liquidity is added to a position, validates ticks,
    /// and delegates to internal liquidity management methods.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Pool is not initialized.
    /// - Tick range is invalid or not properly spaced.
    /// - Position updates fail.
    pub fn process_mint(&mut self, update: &PoolLiquidityUpdate) -> anyhow::Result<()> {
        self.check_if_initialized();
        if self.check_if_already_processed(update.block, update.transaction_index, update.log_index)
        {
            return Ok(());
        }

        self.validate_ticks(update.tick_lower, update.tick_upper)?;
        self.add_liquidity(
            &update.owner,
            update.tick_lower,
            update.tick_upper,
            update.position_liquidity,
            update.amount0,
            update.amount1,
        )?;

        self.analytics.total_mints += 1;
        self.last_processed_event = Some(BlockPosition::new(
            update.block,
            update.transaction_hash.clone(),
            update.transaction_index,
            update.log_index,
        ));
        self.update_reporter_if_enabled(update.block);
        self.update_liquidity_analytics();

        Ok(())
    }

    /// Internal helper to add liquidity to a position.
    ///
    /// Updates position state, tracks deposited amounts, and manages tick maps.
    /// Called by both historical event processing and simulated operations.
    fn add_liquidity(
        &mut self,
        owner: &Address,
        tick_lower: i32,
        tick_upper: i32,
        liquidity: u128,
        amount0: U256,
        amount1: U256,
    ) -> anyhow::Result<()> {
        self.update_position(
            owner,
            tick_lower,
            tick_upper,
            liquidity as i128,
            amount0,
            amount1,
        )?;

        // Track deposited amounts
        self.analytics.total_amount0_deposited += amount0;
        self.analytics.total_amount1_deposited += amount1;

        Ok(())
    }

    /// Executes a simulated mint (liquidity addition) operation.
    ///
    /// Calculates required token amounts for the specified liquidity amount,
    /// updates pool state, and returns the resulting mint event.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Pool is not initialized.
    /// - Tick range is invalid.
    /// - Amount calculations fail.
    ///
    /// # Panics
    ///
    /// Panics if the current sqrt price has not been initialized.
    pub fn execute_mint(
        &mut self,
        recipient: Address,
        block: BlockPosition,
        tick_lower: i32,
        tick_upper: i32,
        liquidity: u128,
    ) -> anyhow::Result<PoolLiquidityUpdate> {
        self.check_if_initialized();
        self.validate_ticks(tick_lower, tick_upper)?;
        let (amount0, amount1) = get_amounts_for_liquidity(
            self.state.price_sqrt_ratio_x96,
            tick_lower,
            tick_upper,
            liquidity,
            true,
        );
        self.add_liquidity(
            &recipient, tick_lower, tick_upper, liquidity, amount0, amount1,
        )?;

        self.analytics.total_mints += 1;
        let event = PoolLiquidityUpdate::new(
            self.pool.chain.clone(),
            self.pool.dex.clone(),
            self.pool.instrument_id,
            self.pool.address,
            PoolLiquidityUpdateType::Mint,
            block.number,
            block.transaction_hash,
            block.transaction_index,
            block.log_index,
            None,
            recipient,
            liquidity,
            amount0,
            amount1,
            tick_lower,
            tick_upper,
            None,
        );

        Ok(event)
    }

    /// Processes a burn (liquidity removal) event from historical data.
    ///
    /// Updates pool state when liquidity is removed from a position. Uses negative
    /// liquidity delta to reduce the position size and tracks withdrawn amounts.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Pool is not initialized.
    /// - Tick range is invalid.
    /// - Position updates fail.
    pub fn process_burn(&mut self, update: &PoolLiquidityUpdate) -> anyhow::Result<()> {
        self.check_if_initialized();
        if self.check_if_already_processed(update.block, update.transaction_index, update.log_index)
        {
            return Ok(());
        }
        self.validate_ticks(update.tick_lower, update.tick_upper)?;

        // Update the position with a negative liquidity delta for the burn.
        self.update_position(
            &update.owner,
            update.tick_lower,
            update.tick_upper,
            -(update.position_liquidity as i128),
            update.amount0,
            update.amount1,
        )?;

        self.analytics.total_burns += 1;
        self.last_processed_event = Some(BlockPosition::new(
            update.block,
            update.transaction_hash.clone(),
            update.transaction_index,
            update.log_index,
        ));
        self.update_reporter_if_enabled(update.block);
        self.update_liquidity_analytics();

        Ok(())
    }

    /// Executes a simulated burn (liquidity removal) operation.
    ///
    /// Calculates token amounts that would be withdrawn for the specified liquidity,
    /// updates pool state, and returns the resulting burn event.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Pool is not initialized.
    /// - Tick range is invalid.
    /// - Amount calculations fail.
    /// - Insufficient liquidity in position.
    ///
    /// # Panics
    ///
    /// Panics if the current sqrt price has not been initialized.
    pub fn execute_burn(
        &mut self,
        recipient: Address,
        block: BlockPosition,
        tick_lower: i32,
        tick_upper: i32,
        liquidity: u128,
    ) -> anyhow::Result<PoolLiquidityUpdate> {
        self.check_if_initialized();
        self.validate_ticks(tick_lower, tick_upper)?;
        let (amount0, amount1) = get_amounts_for_liquidity(
            self.state.price_sqrt_ratio_x96,
            tick_lower,
            tick_upper,
            liquidity,
            false,
        );

        // Update the position with a negative liquidity delta for the burn
        self.update_position(
            &recipient,
            tick_lower,
            tick_upper,
            -(liquidity as i128),
            amount0,
            amount1,
        )?;

        self.analytics.total_burns += 1;
        let event = PoolLiquidityUpdate::new(
            self.pool.chain.clone(),
            self.pool.dex.clone(),
            self.pool.instrument_id,
            self.pool.address,
            PoolLiquidityUpdateType::Burn,
            block.number,
            block.transaction_hash,
            block.transaction_index,
            block.log_index,
            None,
            recipient,
            liquidity,
            amount0,
            amount1,
            tick_lower,
            tick_upper,
            None,
        );

        Ok(event)
    }

    /// Processes a fee collect event from historical data.
    ///
    /// Updates position state when accumulated fees are collected. Finds the
    /// position and delegates fee collection to the position object.
    ///
    /// Note: Tick validation is intentionally skipped to match Uniswap V3 behavior.
    /// Invalid positions have no fees to collect, so they're silently ignored.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Pool is not initialized.
    pub fn process_collect(&mut self, collect: &PoolFeeCollect) -> anyhow::Result<()> {
        self.check_if_initialized();
        if self.check_if_already_processed(
            collect.block,
            collect.transaction_index,
            collect.log_index,
        ) {
            return Ok(());
        }

        let position_key =
            PoolPosition::get_position_key(&collect.owner, collect.tick_lower, collect.tick_upper);
        if let Some(position) = self.positions.get_mut(&position_key) {
            position.collect_fees(collect.amount0, collect.amount1);
        }

        // Cleanup position if it became empty after collecting all fees
        self.cleanup_position_if_empty(&position_key);

        self.analytics.total_amount0_collected += U256::from(collect.amount0);
        self.analytics.total_amount1_collected += U256::from(collect.amount1);

        self.analytics.total_fee_collects += 1;
        self.last_processed_event = Some(BlockPosition::new(
            collect.block,
            collect.transaction_hash.clone(),
            collect.transaction_index,
            collect.log_index,
        ));
        self.update_reporter_if_enabled(collect.block);
        self.update_liquidity_analytics();

        Ok(())
    }

    /// Processes a flash loan event from historical data.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Pool has no active liquidity.
    /// - Fee growth arithmetic overflows.
    ///
    /// # Panics
    ///
    /// Panics if the pool has not been initialized.
    pub fn process_flash(&mut self, flash: &PoolFlash) -> anyhow::Result<()> {
        self.check_if_initialized();
        if self.check_if_already_processed(flash.block, flash.transaction_index, flash.log_index) {
            return Ok(());
        }

        self.update_flash_state(flash.paid0, flash.paid1)?;

        self.analytics.total_flashes += 1;
        self.last_processed_event = Some(BlockPosition::new(
            flash.block,
            flash.transaction_hash.clone(),
            flash.transaction_index,
            flash.log_index,
        ));
        self.update_reporter_if_enabled(flash.block);
        self.update_liquidity_analytics();

        Ok(())
    }

    /// Executes a simulated flash loan operation and returns the resulting event.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Mathematical operations overflow when calculating fees.
    /// - Pool has no active liquidity.
    /// - Fee growth arithmetic overflows.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - Pool is not initialized
    /// - Pool fee is not set
    pub fn execute_flash(
        &mut self,
        sender: Address,
        recipient: Address,
        block: BlockPosition,
        amount0: U256,
        amount1: U256,
    ) -> anyhow::Result<PoolFlash> {
        self.check_if_initialized();
        let fee_tier = self.pool.fee.expect("Pool fee should be initialized");

        // Calculate fees or paid0/paid1
        let paid0 = if amount0 > U256::ZERO {
            FullMath::mul_div_rounding_up(amount0, U256::from(fee_tier), U256::from(1_000_000))?
        } else {
            U256::ZERO
        };

        let paid1 = if amount1 > U256::ZERO {
            FullMath::mul_div_rounding_up(amount1, U256::from(fee_tier), U256::from(1_000_000))?
        } else {
            U256::ZERO
        };

        self.update_flash_state(paid0, paid1)?;
        self.analytics.total_flashes += 1;

        let flash_event = PoolFlash::new(
            self.pool.chain.clone(),
            self.pool.dex.clone(),
            self.pool.instrument_id,
            self.pool.address,
            block.number,
            block.transaction_hash,
            block.transaction_index,
            block.log_index,
            None,
            sender,
            recipient,
            amount0,
            amount1,
            paid0,
            paid1,
        );

        Ok(flash_event)
    }

    /// Core flash loan state update logic.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - No active liquidity in pool
    /// - Fee growth arithmetic overflows
    fn update_flash_state(&mut self, paid0: U256, paid1: U256) -> anyhow::Result<()> {
        let liquidity = self.tick_map.liquidity;
        if liquidity == 0 {
            anyhow::bail!("No liquidity")
        }

        let fee_protocol_0 = self.state.fee_protocol % 16;
        let fee_protocol_1 = self.state.fee_protocol >> 4;

        // Process token0 fees
        if paid0 > U256::ZERO {
            let protocol_fee_0 = if fee_protocol_0 > 0 {
                paid0 / U256::from(fee_protocol_0)
            } else {
                U256::ZERO
            };

            if protocol_fee_0 > U256::ZERO {
                self.state.protocol_fees_token0 += protocol_fee_0;
            }

            let lp_fee_0 = paid0 - protocol_fee_0;
            let delta = FullMath::mul_div(lp_fee_0, Q128, U256::from(liquidity))?;
            self.state.fee_growth_global_0 += delta;
        }

        // Process token1 fees
        if paid1 > U256::ZERO {
            let protocol_fee_1 = if fee_protocol_1 > 0 {
                paid1 / U256::from(fee_protocol_1)
            } else {
                U256::ZERO
            };

            if protocol_fee_1 > U256::ZERO {
                self.state.protocol_fees_token1 += protocol_fee_1;
            }

            let lp_fee_1 = paid1 - protocol_fee_1;
            let delta = FullMath::mul_div(lp_fee_1, Q128, U256::from(liquidity))?;
            self.state.fee_growth_global_1 += delta;
        }

        Ok(())
    }

    /// Updates position state and tick maps when liquidity changes.
    ///
    /// Core internal method that handles position updates for both mints and burns.
    /// Updates tick maps, position tracking, fee growth, and active liquidity.
    fn update_position(
        &mut self,
        owner: &Address,
        tick_lower: i32,
        tick_upper: i32,
        liquidity_delta: i128,
        amount0: U256,
        amount1: U256,
    ) -> anyhow::Result<()> {
        let current_tick = self.state.current_tick;
        let position_key = PoolPosition::get_position_key(owner, tick_lower, tick_upper);
        let position = self
            .positions
            .entry(position_key)
            .or_insert(PoolPosition::new(*owner, tick_lower, tick_upper, 0));

        // Only validate when burning (negative liquidity_delta)
        if liquidity_delta < 0 {
            let burn_amount = liquidity_delta.unsigned_abs();
            if position.liquidity < burn_amount {
                anyhow::bail!(
                    "Position liquidity {} is less than the requested burn amount of {}",
                    position.liquidity,
                    burn_amount
                );
            }
        }

        // Update tickmaps.
        let flipped_lower = self.tick_map.update(
            tick_lower,
            current_tick,
            liquidity_delta,
            false,
            self.state.fee_growth_global_0,
            self.state.fee_growth_global_1,
        );
        let flipped_upper = self.tick_map.update(
            tick_upper,
            current_tick,
            liquidity_delta,
            true,
            self.state.fee_growth_global_0,
            self.state.fee_growth_global_1,
        );

        let (fee_growth_inside_0, fee_growth_inside_1) = self.tick_map.get_fee_growth_inside(
            tick_lower,
            tick_upper,
            current_tick,
            self.state.fee_growth_global_0,
            self.state.fee_growth_global_1,
        );
        position.update_liquidity(liquidity_delta);
        position.update_fees(fee_growth_inside_0, fee_growth_inside_1);
        position.update_amounts(liquidity_delta, amount0, amount1);

        // Update active liquidity if this position spans the current tick
        if tick_lower <= current_tick && current_tick < tick_upper {
            self.tick_map.liquidity = ((self.tick_map.liquidity as i128) + liquidity_delta) as u128;
        }

        // Clear the ticks if they are flipped and burned
        if liquidity_delta < 0 && flipped_lower {
            self.tick_map.clear(tick_lower);
        }
        if liquidity_delta < 0 && flipped_upper {
            self.tick_map.clear(tick_upper);
        }

        Ok(())
    }

    /// Removes position from tracking if it's completely empty.
    ///
    /// This prevents accumulation of positions in the memory that are not used anymore.
    fn cleanup_position_if_empty(&mut self, position_key: &str) {
        if let Some(position) = self.positions.get(position_key)
            && position.is_empty()
        {
            tracing::debug!(
                "CLEANING UP EMPTY POSITION: owner={}, ticks=[{}, {}]",
                position.owner,
                position.tick_lower,
                position.tick_upper,
            );
            self.positions.remove(position_key);
        }
    }

    /// Calculates the liquidity utilization rate for the pool.
    ///
    /// The utilization rate measures what percentage of total deployed liquidity
    /// is currently active (in-range and earning fees) at the current price tick.
    pub fn liquidity_utilization_rate(&self) -> f64 {
        let total_liquidity = self.get_total_liquidity();
        let active_liquidity = self.get_active_liquidity();

        if total_liquidity == U256::ZERO {
            return 0.0;
        }

        // 6 decimal places
        const PRECISION: u32 = 1_000_000;
        let ratio = FullMath::mul_div(
            U256::from(active_liquidity),
            U256::from(PRECISION),
            total_liquidity,
        )
        .unwrap_or(U256::ZERO);

        // Safe to cast to u64: Since active_liquidity <= total_liquidity,
        // the ratio is guaranteed to be <= PRECISION (1_000_000), which fits in u64
        ratio.to::<u64>() as f64 / PRECISION as f64
    }

    /// Validates tick range for position operations.
    ///
    /// Ensures ticks are properly ordered, aligned to tick spacing, and within
    /// valid bounds. Used by all position-related operations.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - `tick_lower >= tick_upper` (invalid range).
    /// - Ticks are not multiples of pool's tick spacing.
    /// - Ticks are outside MIN_TICK/MAX_TICK bounds.
    fn validate_ticks(&self, tick_lower: i32, tick_upper: i32) -> anyhow::Result<()> {
        if tick_lower >= tick_upper {
            anyhow::bail!("Invalid tick range: {} >= {}", tick_lower, tick_upper)
        }

        if tick_lower % self.pool.tick_spacing.unwrap() as i32 != 0
            || tick_upper % self.pool.tick_spacing.unwrap() as i32 != 0
        {
            anyhow::bail!(
                "Ticks {} and {} must be multiples of the tick spacing",
                tick_lower,
                tick_upper
            )
        }

        if tick_lower < PoolTick::MIN_TICK || tick_upper > PoolTick::MAX_TICK {
            anyhow::bail!("Invalid tick bounds for {} and {}", tick_lower, tick_upper);
        }
        Ok(())
    }

    /// Updates all liquidity analytics.
    fn update_liquidity_analytics(&mut self) {
        self.analytics.liquidity_utilization_rate = self.liquidity_utilization_rate();
    }

    /// Returns the pool's active liquidity tracked by the tick map.
    ///
    /// This represents the effective liquidity available for trading at the current price.
    /// The tick map maintains this value efficiently by updating it during tick crossings
    /// as the price moves through different ranges.
    ///
    /// # Returns
    /// The active liquidity (u128) at the current tick from the tick map
    #[must_use]
    pub fn get_active_liquidity(&self) -> u128 {
        self.tick_map.liquidity
    }

    /// Calculates total liquidity by summing all individual positions at the current tick.
    ///
    /// This computes liquidity by iterating through all positions and summing those that
    /// span the current tick. Unlike [`Self::get_active_liquidity`], which returns the maintained
    /// tick map value, this method performs a fresh calculation from position data.
    #[must_use]
    pub fn get_total_liquidity_from_active_positions(&self) -> u128 {
        self.positions
            .values()
            .filter(|position| {
                position.liquidity > 0
                    && position.tick_lower <= self.state.current_tick
                    && self.state.current_tick < position.tick_upper
            })
            .map(|position| position.liquidity)
            .sum()
    }

    /// Calculates total liquidity across all positions, regardless of range status.
    #[must_use]
    pub fn get_total_liquidity(&self) -> U256 {
        self.positions
            .values()
            .map(|position| U256::from(position.liquidity))
            .fold(U256::ZERO, |acc, liq| acc + liq)
    }

    /// Restores the profiler state from a saved snapshot.
    ///
    /// This method allows resuming profiling from a previously saved state,
    /// enabling incremental processing without reprocessing all historical events.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Tick insertion into the tick map fails.
    ///
    /// # Panics
    ///
    /// Panics if the pool's tick spacing is not set.
    pub fn restore_from_snapshot(&mut self, snapshot: PoolSnapshot) -> anyhow::Result<()> {
        let liquidity = snapshot.state.liquidity;

        // Restore state
        self.state = snapshot.state;

        // Restore analytics (skip duration fields as they're debug-only)
        self.analytics.total_amount0_deposited = snapshot.analytics.total_amount0_deposited;
        self.analytics.total_amount1_deposited = snapshot.analytics.total_amount1_deposited;
        self.analytics.total_amount0_collected = snapshot.analytics.total_amount0_collected;
        self.analytics.total_amount1_collected = snapshot.analytics.total_amount1_collected;
        self.analytics.total_swaps = snapshot.analytics.total_swaps;
        self.analytics.total_mints = snapshot.analytics.total_mints;
        self.analytics.total_burns = snapshot.analytics.total_burns;
        self.analytics.total_fee_collects = snapshot.analytics.total_fee_collects;
        self.analytics.total_flashes = snapshot.analytics.total_flashes;

        // Rebuild positions HashMap
        self.positions.clear();
        for position in snapshot.positions {
            let key = PoolPosition::get_position_key(
                &position.owner,
                position.tick_lower,
                position.tick_upper,
            );
            self.positions.insert(key, position);
        }

        // Rebuild tick_map
        self.tick_map = TickMap::new(
            self.pool
                .tick_spacing
                .expect("Pool tick spacing must be set"),
        );
        for tick in snapshot.ticks {
            self.tick_map.restore_tick(tick);
        }

        // Restore active liquidity
        self.tick_map.liquidity = liquidity;

        // Set last processed event
        self.last_processed_event = Some(snapshot.block_position);

        // Mark as initialized
        self.is_initialized = true;

        // Recalculate analytics
        self.update_liquidity_analytics();

        Ok(())
    }

    /// Gets a list of all initialized tick values.
    ///
    /// Returns tick values that have been initialized (have liquidity positions).
    /// Useful for understanding the liquidity distribution across price ranges.
    pub fn get_active_tick_values(&self) -> Vec<i32> {
        self.tick_map
            .get_all_ticks()
            .iter()
            .filter(|(_, tick)| self.tick_map.is_tick_initialized(tick.value))
            .map(|(tick_value, _)| *tick_value)
            .collect()
    }

    /// Gets the number of active ticks.
    #[must_use]
    pub fn get_active_tick_count(&self) -> usize {
        self.tick_map.active_tick_count()
    }

    /// Gets tick information for a specific tick value.
    ///
    /// Returns the tick data structure containing liquidity and fee information
    /// for the specified tick, if it exists.
    pub fn get_tick(&self, tick: i32) -> Option<&PoolTick> {
        self.tick_map.get_tick(tick)
    }

    /// Gets the current tick position of the pool.
    ///
    /// Returns the tick that corresponds to the current pool price.
    /// The pool must be initialized before calling this method.
    pub fn get_current_tick(&self) -> i32 {
        self.state.current_tick
    }

    /// Gets the total number of ticks tracked by the tick map.
    ///
    /// Returns count of all ticks that have ever been initialized,
    /// including those that may no longer have active liquidity.
    ///
    /// # Returns
    /// Total tick count in the tick map
    pub fn get_total_tick_count(&self) -> usize {
        self.tick_map.total_tick_count()
    }

    /// Gets position information for a specific owner and tick range.
    ///
    /// Looks up a position by its unique key (owner + tick range) and returns
    /// the position data if it exists.
    pub fn get_position(
        &self,
        owner: &Address,
        tick_lower: i32,
        tick_upper: i32,
    ) -> Option<&PoolPosition> {
        let position_key = PoolPosition::get_position_key(owner, tick_lower, tick_upper);
        self.positions.get(&position_key)
    }

    /// Returns a list of all currently active positions.
    ///
    /// Active positions are those with liquidity > 0 whose tick range includes
    /// the current pool tick, meaning they have tokens actively deployed in the pool
    /// and are earning fees from trades at the current price.
    ///
    /// # Returns
    ///
    /// A vector of references to active [`PoolPosition`] objects.
    pub fn get_active_positions(&self) -> Vec<&PoolPosition> {
        self.positions
            .values()
            .filter(|position| {
                let current_tick = self.get_current_tick();
                position.liquidity > 0
                    && position.tick_lower <= current_tick
                    && current_tick < position.tick_upper
            })
            .collect()
    }

    /// Returns a list of all positions tracked by the profiler.
    ///
    /// This includes both active and inactive positions, regardless of their
    /// liquidity or tick range relative to the current pool tick.
    ///
    /// # Returns
    ///
    /// A vector of references to all [`PoolPosition`] objects.
    pub fn get_all_positions(&self) -> Vec<&PoolPosition> {
        self.positions.values().collect()
    }

    /// Returns position keys for all tracked positions.
    pub fn get_all_position_keys(&self) -> Vec<(Address, i32, i32)> {
        self.get_all_positions()
            .iter()
            .map(|position| (position.owner, position.tick_lower, position.tick_upper))
            .collect()
    }

    /// Extracts a complete snapshot of the current pool state.
    ///
    /// Extracts and bundles the complete pool state including global variables,
    /// all liquidity positions, and the full tick distribution into a portable
    /// [`PoolSnapshot`] structure. This snapshot can be serialized, persisted
    /// to database, or used to restore pool state later.
    ///
    /// # Panics
    ///
    /// Panics if no events have been processed yet.
    pub fn extract_snapshot(&self) -> PoolSnapshot {
        let positions: Vec<_> = self.positions.values().cloned().collect();
        let ticks: Vec<_> = self.tick_map.get_all_ticks().values().copied().collect();

        let mut state = self.state.clone();
        state.liquidity = self.tick_map.liquidity;

        PoolSnapshot::new(
            self.pool.instrument_id,
            state,
            positions,
            ticks,
            self.analytics.clone(),
            self.last_processed_event
                .clone()
                .expect("No events processed yet"),
        )
    }

    /// Gets the count of positions that are currently active.
    ///
    /// Active positions are those with liquidity > 0 and whose tick range
    /// includes the current pool tick (meaning they have tokens in the pool).
    pub fn get_total_active_positions(&self) -> usize {
        self.positions
            .iter()
            .filter(|(_, position)| {
                let current_tick = self.get_current_tick();
                position.liquidity > 0
                    && position.tick_lower <= current_tick
                    && current_tick < position.tick_upper
            })
            .count()
    }

    /// Gets the count of positions that are currently inactive.
    ///
    /// Inactive positions are those that exist but don't span the current tick,
    /// meaning their liquidity is entirely in one token or the other.
    pub fn get_total_inactive_positions(&self) -> usize {
        self.positions.len() - self.get_total_active_positions()
    }

    /// Estimates the total amount of token0 in the pool.
    ///
    /// Calculates token0 balance by summing:
    /// - Token0 amounts from all active liquidity positions
    /// - Accumulated trading fees (approximated from fee growth)
    /// - Protocol fees collected
    pub fn estimate_balance_of_token0(&self) -> U256 {
        let mut total_amount0 = U256::ZERO;
        let current_sqrt_price = self.state.price_sqrt_ratio_x96;
        let current_tick = self.state.current_tick;
        let mut total_fees_0_collected: u128 = 0;

        // 1. Calculate token0 from active liquidity positions
        for position in self.positions.values() {
            if position.liquidity > 0 {
                if position.tick_upper <= current_tick {
                    // Position is below current price - no token0
                    continue;
                } else if position.tick_lower > current_tick {
                    // Position is above current price - all token0
                    let sqrt_ratio_a = get_sqrt_ratio_at_tick(position.tick_lower);
                    let sqrt_ratio_b = get_sqrt_ratio_at_tick(position.tick_upper);
                    let amount0 =
                        get_amount0_delta(sqrt_ratio_a, sqrt_ratio_b, position.liquidity, true);
                    total_amount0 += amount0;
                } else {
                    // Position is active - token0 from current price to upper tick
                    let sqrt_ratio_upper = get_sqrt_ratio_at_tick(position.tick_upper);
                    let amount0 = get_amount0_delta(
                        current_sqrt_price,
                        sqrt_ratio_upper,
                        position.liquidity,
                        true,
                    );
                    total_amount0 += amount0;
                }
            }

            total_fees_0_collected += position.total_amount0_collected;
        }

        // 2. Add accumulated swap fees (fee_growth_global represents total fees accumulated)
        // Note: In a real pool, fees are distributed as liquidity, but for balance estimation
        // we can use a simplified approach by converting fee growth to token amounts
        let fee_growth_0 = self.state.fee_growth_global_0;
        if fee_growth_0 > U256::ZERO {
            // Convert fee growth to actual token amount using FullMath for precision
            // Fee growth is in Q128.128 format, so we need to scale it properly
            let active_liquidity = self.get_active_liquidity();
            if active_liquidity > 0 {
                // fee_growth_global is fees per unit of liquidity in Q128.128
                // To get total fees: mul_div(fee_growth, liquidity, 2^128)
                if let Ok(total_fees_0) =
                    FullMath::mul_div(fee_growth_0, U256::from(active_liquidity), Q128)
                {
                    total_amount0 += total_fees_0;
                }
            }
        }

        let total_fees_0_left = fee_growth_0 - U256::from(total_fees_0_collected);

        // 4. Add protocol fees
        total_amount0 += self.state.protocol_fees_token0;

        total_amount0 + total_fees_0_left
    }

    /// Estimates the total amount of token1 in the pool.
    ///
    /// Calculates token1 balance by summing:
    /// - Token1 amounts from all active liquidity positions
    /// - Accumulated trading fees (approximated from fee growth)
    /// - Protocol fees collected
    pub fn estimate_balance_of_token1(&self) -> U256 {
        let mut total_amount1 = U256::ZERO;
        let current_sqrt_price = self.state.price_sqrt_ratio_x96;
        let current_tick = self.state.current_tick;
        let mut total_fees_1_collected: u128 = 0;

        // 1. Calculate token1 from active liquidity positions
        for position in self.positions.values() {
            if position.liquidity > 0 {
                if position.tick_lower > current_tick {
                    // Position is above current price - no token1
                    continue;
                } else if position.tick_upper <= current_tick {
                    // Position is below current price - all token1
                    let sqrt_ratio_a = get_sqrt_ratio_at_tick(position.tick_lower);
                    let sqrt_ratio_b = get_sqrt_ratio_at_tick(position.tick_upper);
                    let amount1 =
                        get_amount1_delta(sqrt_ratio_a, sqrt_ratio_b, position.liquidity, true);
                    total_amount1 += amount1;
                } else {
                    // Position is active - token1 from lower tick to current price
                    let sqrt_ratio_lower = get_sqrt_ratio_at_tick(position.tick_lower);
                    let amount1 = get_amount1_delta(
                        sqrt_ratio_lower,
                        current_sqrt_price,
                        position.liquidity,
                        true,
                    );
                    total_amount1 += amount1;
                }
            }

            // Sum collected fees
            total_fees_1_collected += position.total_amount1_collected;
        }

        // 2. Add accumulated swap fees for token1
        let fee_growth_1 = self.state.fee_growth_global_1;
        if fee_growth_1 > U256::ZERO {
            let active_liquidity = self.get_active_liquidity();
            if active_liquidity > 0 {
                // Convert fee growth to actual token amount using FullMath
                if let Ok(total_fees_1) =
                    FullMath::mul_div(fee_growth_1, U256::from(active_liquidity), Q128)
                {
                    total_amount1 += total_fees_1;
                }
            }
        }

        let total_fees_1_left = fee_growth_1 - U256::from(total_fees_1_collected);

        // 4. Add protocol fees
        total_amount1 += self.state.protocol_fees_token1;

        total_amount1 + total_fees_1_left
    }

    /// Sets the global fee growth for both tokens.
    ///
    /// This is primarily used for testing to simulate specific fee growth scenarios.
    /// In production, fee growth is updated through swap operations.
    ///
    /// # Arguments
    /// * `fee_growth_global_0` - New global fee growth for token0
    /// * `fee_growth_global_1` - New global fee growth for token1
    pub fn set_fee_growth_global(&mut self, fee_growth_global_0: U256, fee_growth_global_1: U256) {
        self.state.fee_growth_global_0 = fee_growth_global_0;
        self.state.fee_growth_global_1 = fee_growth_global_1;
    }

    /// Returns the total number of events processed.
    pub fn get_total_events(&self) -> u64 {
        self.analytics.total_swaps
            + self.analytics.total_mints
            + self.analytics.total_burns
            + self.analytics.total_fee_collects
            + self.analytics.total_flashes
    }

    /// Enables progress reporting for pool profiler event processing.
    ///
    /// When enabled, the profiler will automatically track and log progress
    /// as events are processed through the `process()` method.
    pub fn enable_reporting(&mut self, from_block: u64, total_blocks: u64, update_interval: u64) {
        self.reporter = Some(BlockchainSyncReporter::new(
            BlockchainSyncReportItems::PoolProfiling,
            from_block,
            total_blocks,
            update_interval,
        ));
        self.last_reported_block = from_block;
    }

    /// Finalizes reporting and logs final statistics.
    ///
    /// Should be called after all events have been processed to output
    /// the final summary of the profiler bootstrap operation.
    pub fn finalize_reporting(&mut self) {
        if let Some(reporter) = &self.reporter {
            reporter.log_final_stats();
        }
        self.reporter = None;
    }
}
