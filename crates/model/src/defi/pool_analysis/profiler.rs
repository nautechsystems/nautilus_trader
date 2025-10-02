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
    data::{DexPoolData, PoolFeeCollect, PoolLiquidityUpdateType, block::BlockPosition},
    pool_analysis::{position::PoolPosition, swap_math::compute_swap_step},
    tick_map::{
        TickMap,
        full_math::{FullMath, Q128},
        liquidity_math::liquidity_math_add,
        sqrt_price_math::{get_amount0_delta, get_amount1_delta, get_amounts_for_liquidity},
        tick::Tick,
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
    /// Current tick position of the pool price.
    pub current_tick: Option<i32>,
    /// Current sqrt price ratio as Q64.96 fixed point number.
    pub price_sqrt_ratio_x96: Option<U160>,
    /// Total amount of token0 deposited through mints.
    pub total_amount0_deposited: U256,
    /// Total amount of token1 deposited through mints.
    pub total_amount1_deposited: U256,
    /// Total amount of token0 withdrawn through burns.
    pub total_amount0_withdrawn: U256,
    /// Total amount of token1 withdrawn through burns.
    pub total_amount1_withdrawn: U256,
    /// Accumulated protocol fees in token0 units.
    pub protocol_fees_token0: U256,
    /// Accumulated protocol fees in token1 units.
    pub protocol_fees_token1: U256,
    /// Protocol fee packed: lower 4 bits for token0, upper 4 bits for token1.
    pub fee_protocol: u8,
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
            current_tick: None,
            price_sqrt_ratio_x96: None,
            total_amount0_deposited: U256::ZERO,
            total_amount1_deposited: U256::ZERO,
            total_amount0_withdrawn: U256::ZERO,
            total_amount1_withdrawn: U256::ZERO,
            protocol_fees_token0: U256::ZERO,
            protocol_fees_token1: U256::ZERO,
            fee_protocol: 0,
        }
    }

    /// Initializes the pool with a starting price.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - Pool is already initialized.
    /// - Calculated tick does not match the pool's initial tick (if set).
    pub fn initialize(&mut self, price_sqrt_ratio_x96: U160) {
        if self.current_tick.is_some() || self.price_sqrt_ratio_x96.is_some() {
            panic!("Pool already initialized");
        }

        let calculated_tick = get_tick_at_sqrt_ratio(price_sqrt_ratio_x96);
        if let Some(initial_tick) = self.pool.initial_tick {
            assert_eq!(
                initial_tick, calculated_tick,
                "Calculated tick does not match pool initial tick"
            );
        }
        self.current_tick = Some(calculated_tick);
        self.price_sqrt_ratio_x96 = Some(price_sqrt_ratio_x96);
    }

    /// Verifies that the pool has been initialized.
    ///
    /// Internal helper method to ensure operations are only performed on initialized pools.
    ///
    /// # Panics
    ///
    /// Panics if the pool hasn't been initialized with a starting price.
    pub fn check_if_initialized(&self) {
        if self.current_tick.is_none() || self.price_sqrt_ratio_x96.is_none() {
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
        match event {
            DexPoolData::Swap(swap) => {
                self.process_swap(swap)?;
            }
            DexPoolData::LiquidityUpdate(update) => match update.kind {
                PoolLiquidityUpdateType::Mint => {
                    self.process_mint(update)?;
                }
                PoolLiquidityUpdateType::Burn => {
                    self.process_burn(update)?;
                }
            },
            DexPoolData::FeeCollect(collect) => {
                self.process_collect(collect)?;
            }
        }
        Ok(())
    }

    /// Processes a swap event.
    ///
    /// Updates the current tick and crosses any ticks in between.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Pool initialization checks fail.
    /// - Fee growth calculations overflow when scaled by liquidity.
    /// - Tick map updates fail because of inconsistent state.
    ///
    /// # Panics
    ///
    /// Panics if the pool has not been initialized (current_tick is None).
    pub fn process_swap(&mut self, swap: &PoolSwap) -> anyhow::Result<()> {
        self.check_if_initialized();

        let old_tick = self.current_tick.expect("Pool should be initialized");
        let new_tick = get_tick_at_sqrt_ratio(swap.sqrt_price_x96);

        // Approximate fees from the swap amounts (best effort)
        let (fee_amount0, fee_amount1) = self.approximate_swap_fees(swap.amount0, swap.amount1);

        // Update global fee growth if we have active liquidity
        if self.tick_map.liquidity > 0 {
            if fee_amount0 > U256::ZERO {
                let fee_growth_delta =
                    FullMath::mul_div(fee_amount0, Q128, U256::from(self.tick_map.liquidity))?;
                self.tick_map.fee_growth_global_0 += fee_growth_delta;
            }
            if fee_amount1 > U256::ZERO {
                let fee_growth_delta =
                    FullMath::mul_div(fee_amount1, Q128, U256::from(self.tick_map.liquidity))?;
                self.tick_map.fee_growth_global_1 += fee_growth_delta;
            }
        }

        // Cross ticks if price moved
        if new_tick != old_tick {
            self.cross_ticks_between(old_tick, new_tick);
        }

        // Update pool state with simulated values
        self.current_tick = Some(new_tick);
        self.price_sqrt_ratio_x96 = Some(swap.sqrt_price_x96);

        // Verify simulation against event data - correct with event values if mismatch detected
        if swap.tick != new_tick {
            tracing::error!(
                "Inconsistency in swap processing: Current tick mismatch: simulated {}, event {}",
                new_tick,
                swap.tick
            );
            self.current_tick = Some(swap.tick);
        }
        if swap.liquidity != self.tick_map.liquidity {
            tracing::error!(
                "Inconsistency in swap processing: Active liquidity mismatch: simulated {}, event {}",
                self.tick_map.liquidity,
                swap.liquidity
            );
            self.tick_map.liquidity = swap.liquidity;
        }

        Ok(())
    }

    /// Executes a simulated swap operation with precise AMM mathematics.
    ///
    /// Performs a complete swap simulation following UniswapV3 logic:
    /// - Validates price limits and swap direction.
    /// - Iteratively processes swap steps across liquidity ranges.
    /// - Handles tick crossing and liquidity updates.
    /// - Calculates protocol fees and updates global fee trackers.
    /// - Returns the resulting swap event.
    ///
    /// This is the core swap execution engine used for both exact input and output swaps.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Pool metadata is missing or invalid.
    /// - The provided sqrt price limit violates swap direction constraints.
    /// - Liquidity or fee calculations overflow the supported numeric range.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - Pool fee is not initialized (calls `.expect()` on fee).
    /// - Current tick or price is None after initialization check (calls `.unwrap()`).
    /// - Mathematical operations result in invalid state.
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
        let mut current_sqrt_price = self.price_sqrt_ratio_x96.unwrap();
        let mut current_tick = self.current_tick.unwrap();
        let exact_input = amount_specified.is_positive();

        // Validate sqrt price limit based on swap direction
        if zero_for_one {
            if sqrt_price_limit_x96 >= current_sqrt_price || sqrt_price_limit_x96 <= MIN_SQRT_RATIO
            {
                anyhow::bail!("SPL: Invalid sqrt price limit for zeroForOne swap");
            }
        } else if sqrt_price_limit_x96 <= current_sqrt_price
            || sqrt_price_limit_x96 >= MAX_SQRT_RATIO
        {
            anyhow::bail!("SPL: Invalid sqrt price limit for oneForZero swap");
        }

        // Swapping cache variables
        let fee_protocol = if zero_for_one {
            // Extract lower 4 bits for token0 protocol fee
            self.fee_protocol % 16
        } else {
            // Extract upper 4 bits for token1 protocol fee
            self.fee_protocol >> 4
        };
        let mut amount_specified_remaining = amount_specified;
        let mut amount_calculated = I256::ZERO;
        let mut protocol_fee = U256::ZERO;

        // Track current fee growth during swap (like state.feeGrowthGlobalX128 in Solidity)
        let original_fee_growth_global_0 = self.tick_map.fee_growth_global_0;
        let original_fee_growth_global_1 = self.tick_map.fee_growth_global_1;
        let mut current_fee_growth_global_0 = self.tick_map.fee_growth_global_0;
        let mut current_fee_growth_global_1 = self.tick_map.fee_growth_global_1;

        while amount_specified_remaining != I256::ZERO && sqrt_price_limit_x96 != current_sqrt_price
        {
            let sqrt_price_start_x96 = current_sqrt_price;

            let (mut tick_next, initialized) = self
                .tick_map
                .next_initialized_tick(current_tick, zero_for_one);

            // Make sure we do not overshoot MIN/MAX tick
            tick_next = tick_next.clamp(Tick::MIN_TICK, Tick::MAX_TICK);

            // Get the price for the next tick
            let sqrt_price_next = get_sqrt_ratio_at_tick(tick_next);

            // Compute values to swap to the target tick, price limit, or point where input/output amount is exhausted
            let sqrt_price_target = if zero_for_one {
                if sqrt_price_next < sqrt_price_limit_x96 {
                    sqrt_price_limit_x96
                } else {
                    sqrt_price_next
                }
            } else if sqrt_price_next > sqrt_price_limit_x96 {
                sqrt_price_limit_x96
            } else {
                sqrt_price_next
            };

            let fee_tier = self.pool.fee.expect("Pool fee should be initialized");

            let swap_step_result = compute_swap_step(
                current_sqrt_price,
                sqrt_price_target,
                self.get_active_liquidity(),
                amount_specified_remaining,
                fee_tier,
            )?;

            current_sqrt_price = swap_step_result.sqrt_ratio_next_x96;

            // Update amounts based on swap direction and type
            if exact_input {
                // For exact input swaps: subtract input amount and fees from remaining, subtract output from calculated
                amount_specified_remaining -=
                    I256::from(swap_step_result.amount_in + swap_step_result.fee_amount);
                amount_calculated -= I256::from(swap_step_result.amount_out);
            } else {
                // For exact output swaps: add output to remaining, add input and fees to calculated
                amount_specified_remaining += I256::from(swap_step_result.amount_out);
                amount_calculated +=
                    I256::from(swap_step_result.amount_in + swap_step_result.fee_amount);
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
                if zero_for_one {
                    current_fee_growth_global_0 += fee_growth_delta;
                    self.tick_map.fee_growth_global_0 = current_fee_growth_global_0;
                } else {
                    current_fee_growth_global_1 += fee_growth_delta;
                    self.tick_map.fee_growth_global_1 = current_fee_growth_global_1;
                }
            }

            // Shift tick if we reached the next price
            if current_sqrt_price == sqrt_price_next {
                // If the tick is initialized, run the tick transition
                if initialized {
                    let liquidity_net = self.tick_map.cross_tick(
                        tick_next,
                        if zero_for_one {
                            current_fee_growth_global_0
                        } else {
                            original_fee_growth_global_0
                        },
                        if zero_for_one {
                            original_fee_growth_global_1
                        } else {
                            current_fee_growth_global_1
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
            } else if sqrt_price_start_x96 != current_sqrt_price {
                // Recompute unless we're on a lower tick boundary (already transitioned ticks) and we haven't moved
                current_tick = get_tick_at_sqrt_ratio(current_sqrt_price);
            }
        }

        // Update pool state - match Solidity exactly
        if self.current_tick.unwrap() != current_tick {
            self.current_tick = Some(current_tick);
            self.price_sqrt_ratio_x96 = Some(current_sqrt_price);
        } else {
            // Otherwise just update the price
            self.price_sqrt_ratio_x96 = Some(current_sqrt_price);
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

        // Update protocol fees
        if protocol_fee > U256::ZERO {
            if zero_for_one {
                self.protocol_fees_token0 += protocol_fee;
            } else {
                self.protocol_fees_token1 += protocol_fee;
            }
        }

        let swap_event = PoolSwap::new(
            self.pool.chain.clone(),
            self.pool.dex.clone(),
            self.pool.address,
            block.number,
            block.hash,
            block.transaction_index,
            block.log_index,
            None,
            sender,
            recipient,
            amount0,
            amount1,
            current_sqrt_price,
            self.tick_map.liquidity,
            self.current_tick.unwrap(),
            None,
            None,
            None,
        );
        Ok(swap_event)
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
        self.validate_ticks(update.tick_lower, update.tick_upper)?;
        self.add_liquidity(
            &update.owner,
            update.tick_lower,
            update.tick_upper,
            update.position_liquidity,
            update.amount0,
            update.amount1,
        );
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
    ) {
        self.update_position(
            owner,
            tick_lower,
            tick_upper,
            liquidity as i128,
            amount0,
            amount1,
        );

        // Track deposited amounts
        self.total_amount0_deposited += amount0;
        self.total_amount1_deposited += amount1;
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
            self.price_sqrt_ratio_x96.unwrap(),
            tick_lower,
            tick_upper,
            liquidity,
            true,
        );
        self.add_liquidity(
            &recipient, tick_lower, tick_upper, liquidity, amount0, amount1,
        );

        let event = PoolLiquidityUpdate::new(
            self.pool.chain.clone(),
            self.pool.dex.clone(),
            self.pool.address,
            PoolLiquidityUpdateType::Mint,
            block.number,
            block.hash,
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
        self.validate_ticks(update.tick_lower, update.tick_upper)?;

        // Update the position with a negative liquidity delta for the burn.
        self.update_position(
            &update.owner,
            update.tick_lower,
            update.tick_upper,
            -(update.position_liquidity as i128),
            update.amount0,
            update.amount1,
        );

        // Track withdrawn amounts
        self.total_amount0_withdrawn += update.amount0;
        self.total_amount1_withdrawn += update.amount1;

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
            self.price_sqrt_ratio_x96.unwrap(),
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
        );

        // Track withdrawn amounts
        self.total_amount0_withdrawn += amount0;
        self.total_amount1_withdrawn += amount1;

        let event = PoolLiquidityUpdate::new(
            self.pool.chain.clone(),
            self.pool.dex.clone(),
            self.pool.address,
            PoolLiquidityUpdateType::Burn,
            block.number,
            block.hash,
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

        let position_key =
            PoolPosition::get_position_key(&collect.owner, collect.tick_lower, collect.tick_upper);
        if let Some(position) = self.positions.get_mut(&position_key) {
            position.collect_fees(collect.amount0, collect.amount1);
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
    ) {
        let current_tick = self.current_tick.expect("Pool should be initialized");
        let position_key = PoolPosition::get_position_key(owner, tick_lower, tick_upper);
        let position = self
            .positions
            .entry(position_key)
            .or_insert(PoolPosition::new(*owner, tick_lower, tick_upper, 0));

        // Update tickmaps.
        let flipped_lower = self
            .tick_map
            .update(tick_lower, current_tick, liquidity_delta, false);
        let flipped_upper = self
            .tick_map
            .update(tick_upper, current_tick, liquidity_delta, true);

        let (fee_growth_inside_0, fee_growth_inside_1) =
            self.tick_map
                .get_fee_growth_inside(tick_lower, tick_upper, current_tick);
        position.update_liquidity(liquidity_delta);
        position.update_fees(fee_growth_inside_0, fee_growth_inside_1);
        position.update_amounts(liquidity_delta, amount0, amount1);

        // Update active liquidity if this position spans the current tick
        if tick_lower <= current_tick && current_tick < tick_upper {
            self.tick_map.liquidity = ((self.tick_map.liquidity as i128) + liquidity_delta) as u128;
        }

        // Clear the ticks if they are flipped and burned
        if liquidity_delta < 0 && flipped_lower {
            self.tick_map.clear(tick_lower)
        }
        if liquidity_delta < 0 && flipped_upper {
            self.tick_map.clear(tick_upper)
        }
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

        if tick_lower < Tick::MIN_TICK || tick_upper > Tick::MAX_TICK {
            anyhow::bail!("Invalid tick bounds for {} and {}", tick_lower, tick_upper);
        }
        Ok(())
    }

    /// Crosses all ticks between old and new price positions.
    ///
    /// Updates active liquidity by crossing any initialized ticks that fall between
    /// the old and new tick positions. Handles both upward and downward price movements.
    /// Used by both historical event processing and swap simulations.
    fn cross_ticks_between(&mut self, old_tick: i32, new_tick: i32) {
        if new_tick > old_tick {
            // Price increased - cross ticks upward
            self.tick_map.cross_tick_up(old_tick, new_tick);
        } else if new_tick < old_tick {
            // Price decreased - cross ticks downward
            self.tick_map.cross_tick_down(old_tick, new_tick);
        }
        // If old_tick == new_tick, no ticks to cross
    }

    /// Approximates swap fees from token amounts and pool fee tier.
    ///
    /// Estimates fees charged during a swap based on input amounts and the pool's
    /// fee tier. This is a best-effort approximation since historical swap events
    /// don't contain detailed fee breakdowns.
    fn approximate_swap_fees(&self, amount0: I256, amount1: I256) -> (U256, U256) {
        let fee_tier = self.pool.fee.expect("Pool fee should be initialized");

        let mut fee_amount0 = U256::ZERO;
        let mut fee_amount1 = U256::ZERO;

        // Determine which token is the input (positive amount) and calculate fee
        if amount0.is_positive() {
            // Token0 is input - calculate fee on input amount
            let input_amount = amount0.unsigned_abs();
            fee_amount0 = (input_amount * U256::from(fee_tier)) / U256::from(1_000_000u32);
        }

        if amount1.is_positive() {
            // Token1 is input - calculate fee on input amount
            let input_amount = amount1.unsigned_abs();
            fee_amount1 = (input_amount * U256::from(fee_tier)) / U256::from(1_000_000u32);
        }

        (fee_amount0, fee_amount1)
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
    ///
    /// # Panics
    ///
    /// Panics if `current_tick` is `None` (pool not initialized).
    #[must_use]
    pub fn get_total_liquidity_from_active_positions(&self) -> u128 {
        let current_tick = self.current_tick.unwrap();

        self.positions
            .values()
            .filter(|position| {
                position.liquidity > 0
                    && position.tick_lower <= current_tick
                    && current_tick < position.tick_upper
            })
            .map(|position| position.liquidity)
            .sum()
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
    pub fn get_tick(&self, tick: i32) -> Option<&Tick> {
        self.tick_map.get_tick(tick)
    }

    /// Gets the current tick position of the pool.
    ///
    /// Returns the tick that corresponds to the current pool price.
    /// The pool must be initialized before calling this method.
    ///
    /// # Panics
    ///
    /// Panics if the pool has not been initialized
    pub fn get_current_tick(&self) -> i32 {
        self.current_tick.expect("Pool should be initialized")
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
        let current_sqrt_price = self.price_sqrt_ratio_x96.unwrap_or_default();
        let current_tick = self.current_tick.unwrap_or_default();
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
        let fee_growth_0 = self.tick_map.fee_growth_global_0;
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
        total_amount0 += self.protocol_fees_token0;

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
        let current_sqrt_price = self.price_sqrt_ratio_x96.unwrap_or_default();
        let current_tick = self.current_tick.unwrap_or_default();
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
        let fee_growth_1 = self.tick_map.fee_growth_global_1;
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
        total_amount1 += self.protocol_fees_token1;

        total_amount1 + total_fees_1_left
    }
}
