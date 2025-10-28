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

use std::{
    ops::{Div, Mul},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use alloy_primitives::{Address, I256, U160, U256, address};
use nautilus_core::UnixNanos;
use rstest::{fixture, rstest};
use rust_decimal::Decimal;

use crate::defi::{
    AmmType, Chain, Dex, DexType, Pool, PoolLiquidityUpdate, PoolLiquidityUpdateType, PoolSwap,
    SharedChain, SharedDex, Token,
    data::{DexPoolData, PoolFeeCollect, block::BlockPosition},
    pool_analysis::profiler::PoolProfiler,
    tick_map::{
        liquidity_math::tick_spacing_to_max_liquidity_per_tick,
        sqrt_price_math::{
            encode_sqrt_ratio_x96, expand_to_18_decimals, get_amounts_for_liquidity,
        },
        tick::PoolTick,
        tick_math::get_tick_at_sqrt_ratio,
    },
};

// Global counter for log indices to ensure each test event has a unique position
static LOG_INDEX_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Gets the next log index for test events
fn next_log_index() -> u32 {
    LOG_INDEX_COUNTER.fetch_add(1, Ordering::SeqCst)
}

fn arbitrum() -> SharedChain {
    Arc::new(Chain::from_chain_id(42161).unwrap().clone())
}

fn uniswap_v3(arbitrum: SharedChain) -> SharedDex {
    let dex = Dex::new(
        (*arbitrum).clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    );
    Arc::new(dex)
}

const TICK_SPACING: i32 = 60;

fn sqrt_price_x98() -> U160 {
    encode_sqrt_ratio_x96(1, 10)
}

/// Builds a test pool definition for Uniswap V3 scenarios.
///
/// # Panics
///
/// Panics if chain metadata or initial parameters are invalid for pool creation.
pub fn pool_definition(
    fee: Option<u32>,
    tick_spacing: Option<i32>,
    initial_sqrt_price_x96: Option<U160>,
) -> Pool {
    let arbitrum = arbitrum();
    let dex = uniswap_v3(arbitrum.clone());
    let weth = Token::new(
        arbitrum.clone(),
        address!("0x37a645648dF29205C6261289983FB04ECD70b4B3"),
        "Wrapped Ether".to_string(),
        "WETH".to_string(),
        18,
    );
    let coin_anime = Token::new(
        arbitrum,
        address!("0x37a645648dF29205C6261289983FB04ECD70b4B3"),
        "Animecoin".to_string(),
        "ANIME".to_string(),
        18,
    );
    let mut pool = Pool::new(
        Arc::new(Chain::from_chain_id(42161).unwrap().clone()), // Arbitrum,
        dex,
        address!("0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234"),
        0,
        coin_anime,
        weth,
        Some(fee.unwrap_or(3000)),
        Some(tick_spacing.unwrap_or(TICK_SPACING) as u32),
        UnixNanos::default(),
    );
    pool.initialize(initial_sqrt_price_x96.unwrap_or(sqrt_price_x98()));
    pool
}

fn create_mint_event(
    owner: Address,
    ticker_lower: i32,
    ticker_upper: i32,
    liquidity: u128,
) -> PoolLiquidityUpdate {
    let pool_definition = pool_definition(None, None, None);
    let (amount0, amount1) = get_amounts_for_liquidity(
        sqrt_price_x98(),
        ticker_lower,
        ticker_upper,
        liquidity,
        true,
    );
    PoolLiquidityUpdate::new(
        arbitrum(),
        uniswap_v3(arbitrum()),
        pool_definition.instrument_id,
        pool_definition.address,
        PoolLiquidityUpdateType::Mint,
        100000,
        "0x1aa3506e78dd6e7e53986fa310c7ef1b7825042e19693c04eb56b2404067407b".to_string(),
        0,
        next_log_index(),
        None,
        owner,
        liquidity,
        amount0,
        amount1,
        ticker_lower,
        ticker_upper,
        None,
    )
}

fn create_burn_event(
    owner: Address,
    ticker_lower: i32,
    ticker_upper: i32,
    liquidity: u128,
) -> PoolLiquidityUpdate {
    let pool_definition = pool_definition(None, None, None);
    let (amount0, amount1) = get_amounts_for_liquidity(
        sqrt_price_x98(),
        ticker_lower,
        ticker_upper,
        liquidity,
        false,
    );
    PoolLiquidityUpdate::new(
        arbitrum(),
        uniswap_v3(arbitrum()),
        pool_definition.instrument_id,
        pool_definition.address,
        PoolLiquidityUpdateType::Burn,
        100000,
        "0x1aa3506e78dd6e7e53986fa310c7ef1b7825042e19693c04eb56b2404067407b".to_string(),
        0,
        next_log_index(),
        None,
        owner,
        liquidity,
        amount0,
        amount1,
        ticker_lower,
        ticker_upper,
        None,
    )
}

fn create_collect_event(
    ticker_lower: i32,
    ticker_upper: i32,
    amount0: u128,
    amount1: u128,
) -> PoolFeeCollect {
    let pool_definition = pool_definition(None, None, None);
    PoolFeeCollect::new(
        arbitrum(),
        uniswap_v3(arbitrum()),
        pool_definition.instrument_id,
        pool_definition.address,
        100000,
        "0x1aa3506e78dd6e7e53986fa310c7ef1b7825042e19693c04eb56b2404067407b".to_string(),
        0,
        next_log_index(),
        lp_address(),
        amount0,
        amount1,
        ticker_lower,
        ticker_upper,
        None,
    )
}

fn create_block_position() -> BlockPosition {
    BlockPosition::new(
        100000,
        "0x1aa3506e78dd6e7e53986fa310c7ef1b7825042e19693c04eb56b2404067407b".to_string(),
        0,
        next_log_index(),
    )
}

fn lp_address() -> Address {
    address!("0x5E325eDA8064b456f4781070C0738d849c824258")
}

fn user_address() -> Address {
    address!("0x1aa3506e78dd6e7e53986fa310c7ef1b7825042e")
}

fn other_address() -> Address {
    address!("0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb0")
}

#[fixture]
fn profiler() -> PoolProfiler {
    let pool_definition = pool_definition(None, None, None);
    let mut profiler = PoolProfiler::new(Arc::new(pool_definition));
    profiler.initialize(sqrt_price_x98());
    profiler
}

#[rstest]
fn test_initial_state() {
    let pool_definition = pool_definition(None, None, None);
    let profiler = PoolProfiler::new(Arc::new(pool_definition));
    let max_liquidity = tick_spacing_to_max_liquidity_per_tick(60);
    assert_eq!(profiler.state.price_sqrt_ratio_x96, U160::ZERO);
    assert_eq!(profiler.state.current_tick, 0);
    assert_eq!(profiler.tick_map.active_tick_count(), 0);
    assert_eq!(profiler.pool.tick_spacing.unwrap(), 60);
    assert_eq!(profiler.tick_map.max_liquidity_per_tick, max_liquidity);
}

#[rstest]
fn test_initialize_success(profiler: PoolProfiler) {
    assert_eq!(profiler.state.price_sqrt_ratio_x96, sqrt_price_x98());
    assert_eq!(
        profiler.state.current_tick,
        get_tick_at_sqrt_ratio(sqrt_price_x98())
    );
}

#[rstest]
#[should_panic(expected = "Pool already initialized")]
fn test_initialize_already_initialized(mut profiler: PoolProfiler) {
    let price_sqrt_ratio = U160::from_str("511495728837967332084595714").unwrap();
    // Initialize again to panic
    profiler.initialize(price_sqrt_ratio);
}

#[rstest]
#[should_panic(expected = "Sqrt price out of bounds")]
fn test_if_starting_price_is_too_low() {
    let pool_definition = pool_definition(None, None, None);
    let mut profiler = PoolProfiler::new(Arc::new(pool_definition));
    let price_sqrt_ratio = U160::from_str("1").unwrap();
    profiler.initialize(price_sqrt_ratio);
}

#[rstest]
#[should_panic(expected = "Pool is not initialized")]
fn test_process_mint_with_fail_if_pool_not_initialized() {
    let pool_definition = pool_definition(None, None, None);
    let mut profiler = PoolProfiler::new(Arc::new(pool_definition));
    let tick_spacing = profiler.pool.tick_spacing.unwrap();
    let mint_event = create_mint_event(
        lp_address(),
        tick_spacing as i32,
        (tick_spacing * 2) as i32,
        1,
    );
    profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();
}

#[rstest]
fn test_if_pool_process_fails_if_tick_lower_is_greater_than_tick_upper(mut profiler: PoolProfiler) {
    let mint_event = create_mint_event(lp_address(), 2, 1, 1);
    let result = profiler.process(&DexPoolData::LiquidityUpdate(mint_event));
    assert!(result.is_err_and(|error| error.to_string() == "Invalid tick range: 2 >= 1"));
}

#[rstest]
fn test_if_pool_process_fails_if_tick_are_not_multiple_of_tick_spacing(mut profiler: PoolProfiler) {
    // Create mint event with tick 1 and 2 (which are not multiple of tick spacing which is 60)
    let mint_event = create_mint_event(lp_address(), 1, 2, 1);
    let result = profiler.process(&DexPoolData::LiquidityUpdate(mint_event));
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap().to_string(),
        "Ticks 1 and 2 must be multiples of the tick spacing"
    );
}

#[rstest]
fn test_if_pool_process_fails_if_outside_tick_bounds(mut profiler: PoolProfiler) {
    let tick_spacing = profiler.pool.tick_spacing.unwrap() as i32;

    // Find the first tick above MAX_TICK that's a multiple of tick_spacing
    let invalid_tick_lower = ((PoolTick::MAX_TICK / tick_spacing) + 1) * tick_spacing;
    let invalid_tick_upper = invalid_tick_lower + tick_spacing;

    // Create mint event manually to avoid calling get_amounts_for_liquidity with invalid ticks
    let pool_definition = pool_definition(None, None, None);
    let mint_event = PoolLiquidityUpdate::new(
        arbitrum(),
        uniswap_v3(arbitrum()),
        pool_definition.instrument_id,
        pool_definition.address,
        PoolLiquidityUpdateType::Mint,
        100000,
        "0x1aa3506e78dd6e7e53986fa310c7ef1b7825042e19693c04eb56b2404067407b".to_string(),
        0,
        1,
        None,
        lp_address(),
        10000,
        U256::from(1000),
        U256::from(1000),
        invalid_tick_lower,
        invalid_tick_upper,
        None,
    );
    let result = profiler.process(&DexPoolData::LiquidityUpdate(mint_event));
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap().to_string(),
        format!(
            "Invalid tick bounds for {} and {}",
            invalid_tick_lower, invalid_tick_upper
        )
        .as_str(),
    );
}

#[rstest]
fn test_execute_mint_equivalence() {
    let pool_definition = pool_definition(None, None, None);
    // Create two identical profilers
    let mut profiler1 = PoolProfiler::new(Arc::new(pool_definition.clone()));
    let mut profiler2 = PoolProfiler::new(Arc::new(pool_definition));

    profiler1.initialize(sqrt_price_x98());
    profiler2.initialize(sqrt_price_x98());

    let tick_lower = -240;
    let tick_upper = 0;
    let liquidity = 10000u128;
    let recipient = lp_address();
    let block = create_block_position();

    // Method 1: Use process_mint with a created event
    let mint_event = create_mint_event(lp_address(), tick_lower, tick_upper, liquidity);
    profiler1
        .process(&DexPoolData::LiquidityUpdate(mint_event.clone()))
        .unwrap();

    // Method 2: Use execute_mint to create and apply the event
    let executed_event = profiler2
        .execute_mint(recipient, block, tick_lower, tick_upper, liquidity)
        .unwrap();

    // Verify events are equivalent (amounts might differ due to calculation timing)
    assert_eq!(executed_event.kind, mint_event.kind);
    assert_eq!(executed_event.owner, mint_event.owner);
    assert_eq!(
        executed_event.position_liquidity,
        mint_event.position_liquidity
    );
    assert_eq!(executed_event.tick_lower, mint_event.tick_lower);
    assert_eq!(executed_event.tick_upper, mint_event.tick_upper);

    // Verify profiler states are identical
    assert_eq!(profiler1.state.current_tick, profiler2.state.current_tick);
    assert_eq!(
        profiler1.state.price_sqrt_ratio_x96,
        profiler2.state.price_sqrt_ratio_x96
    );
    assert_eq!(
        profiler1.get_active_tick_count(),
        profiler2.get_active_tick_count()
    );
    assert_eq!(
        profiler1.get_total_active_positions(),
        profiler2.get_total_active_positions()
    );
    assert_eq!(
        profiler1.get_total_inactive_positions(),
        profiler2.get_total_inactive_positions()
    );
    assert_eq!(
        profiler1.analytics.total_amount0_deposited,
        profiler2.analytics.total_amount0_deposited
    );
    assert_eq!(
        profiler1.analytics.total_amount1_deposited,
        profiler2.analytics.total_amount1_deposited
    );

    // Verify position states
    let pos1 = profiler1
        .get_position(&recipient, tick_lower, tick_upper)
        .expect("Position should exist");
    let pos2 = profiler2
        .get_position(&recipient, tick_lower, tick_upper)
        .expect("Position should exist");

    assert_eq!(pos1.liquidity, pos2.liquidity);
    assert_eq!(pos1.tick_lower, pos2.tick_lower);
    assert_eq!(pos1.tick_upper, pos2.tick_upper);
    assert_eq!(pos1.total_amount0_deposited, pos2.total_amount0_deposited);
    assert_eq!(pos1.total_amount1_deposited, pos2.total_amount1_deposited);
    assert_eq!(pos1.tokens_owed_0, pos2.tokens_owed_0);
    assert_eq!(pos1.tokens_owed_1, pos2.tokens_owed_1);

    // Verify tick states
    let mut tick_values1 = profiler1.get_active_tick_values();
    let mut tick_values2 = profiler2.get_active_tick_values();
    tick_values1.sort();
    tick_values2.sort();
    assert_eq!(tick_values1, tick_values2);

    // Verify individual tick states
    for tick_value in tick_values1 {
        let tick1 = profiler1.get_tick(tick_value).expect("Tick should exist");
        let tick2 = profiler2.get_tick(tick_value).expect("Tick should exist");
        assert_eq!(tick1.liquidity_gross, tick2.liquidity_gross);
        assert_eq!(tick1.liquidity_net, tick2.liquidity_net);
        assert_eq!(tick1.is_active(), tick2.is_active());
    }
}

#[rstest]
fn test_execute_burn_equivalence() {
    let pool_definition = pool_definition(None, None, None);
    // Create two identical profilers and set up initial mint
    let mut profiler1 = PoolProfiler::new(Arc::new(pool_definition.clone()));
    let mut profiler2 = PoolProfiler::new(Arc::new(pool_definition));

    profiler1.initialize(sqrt_price_x98());
    profiler2.initialize(sqrt_price_x98());

    let tick_lower = -240;
    let tick_upper = 0;
    let initial_liquidity = 20000u128;
    let burn_liquidity = 10000u128;
    let recipient = lp_address();
    let block = create_block_position();

    // Set up initial mint in both profilers
    let initial_mint = create_mint_event(lp_address(), tick_lower, tick_upper, initial_liquidity);
    profiler1
        .process(&DexPoolData::LiquidityUpdate(initial_mint.clone()))
        .unwrap();
    profiler2
        .process(&DexPoolData::LiquidityUpdate(initial_mint))
        .unwrap();

    // Method 1: Use process_burn with a created event
    let burn_event = create_burn_event(lp_address(), tick_lower, tick_upper, burn_liquidity);
    profiler1
        .process(&DexPoolData::LiquidityUpdate(burn_event.clone()))
        .unwrap();

    // Method 2: Use execute_burn to create and apply the event
    let executed_event = profiler2
        .execute_burn(recipient, block, tick_lower, tick_upper, burn_liquidity)
        .unwrap();

    // Verify events are equivalent
    assert_eq!(executed_event.kind, burn_event.kind);
    assert_eq!(executed_event.owner, burn_event.owner);
    assert_eq!(
        executed_event.position_liquidity,
        burn_event.position_liquidity
    );
    assert_eq!(executed_event.tick_lower, burn_event.tick_lower);
    assert_eq!(executed_event.tick_upper, burn_event.tick_upper);

    // Verify profiler states are identical
    assert_eq!(profiler1.state.current_tick, profiler2.state.current_tick);
    assert_eq!(
        profiler1.state.price_sqrt_ratio_x96,
        profiler2.state.price_sqrt_ratio_x96
    );
    assert_eq!(
        profiler1.get_active_tick_count(),
        profiler2.get_active_tick_count()
    );
    assert_eq!(
        profiler1.get_total_active_positions(),
        profiler2.get_total_active_positions()
    );
    assert_eq!(
        profiler1.get_total_inactive_positions(),
        profiler2.get_total_inactive_positions()
    );
    assert_eq!(
        profiler1.analytics.total_amount0_deposited,
        profiler2.analytics.total_amount0_deposited
    );
    assert_eq!(
        profiler1.analytics.total_amount1_deposited,
        profiler2.analytics.total_amount1_deposited
    );
    assert_eq!(
        profiler1.analytics.total_amount0_collected,
        profiler2.analytics.total_amount0_collected
    );
    assert_eq!(
        profiler1.analytics.total_amount1_collected,
        profiler2.analytics.total_amount1_collected
    );

    // Verify position states
    let pos1 = profiler1
        .get_position(&recipient, tick_lower, tick_upper)
        .expect("Position should exist");
    let pos2 = profiler2
        .get_position(&recipient, tick_lower, tick_upper)
        .expect("Position should exist");

    assert_eq!(pos1.liquidity, pos2.liquidity);
    assert_eq!(pos1.tick_lower, pos2.tick_lower);
    assert_eq!(pos1.tick_upper, pos2.tick_upper);
    assert_eq!(pos1.total_amount0_deposited, pos2.total_amount0_deposited);
    assert_eq!(pos1.total_amount1_deposited, pos2.total_amount1_deposited);
    assert_eq!(pos1.tokens_owed_0, pos2.tokens_owed_0);
    assert_eq!(pos1.tokens_owed_1, pos2.tokens_owed_1);

    // Verify tick states
    let mut tick_values1 = profiler1.get_active_tick_values();
    let mut tick_values2 = profiler2.get_active_tick_values();
    tick_values1.sort();
    tick_values2.sort();
    assert_eq!(tick_values1, tick_values2);

    // Verify individual tick states
    for tick_value in tick_values1 {
        if let (Some(tick1), Some(tick2)) = (
            profiler1.get_tick(tick_value),
            profiler2.get_tick(tick_value),
        ) {
            assert_eq!(tick1.liquidity_gross, tick2.liquidity_gross);
            assert_eq!(tick1.liquidity_net, tick2.liquidity_net);
            assert_eq!(tick1.is_active(), tick2.is_active());
        }
    }
}

#[rstest]
fn test_execute_swap_equivalence() {
    let pool_definition = pool_definition(None, None, None);
    // Create two identical profilers
    let mut profiler1 = PoolProfiler::new(Arc::new(pool_definition.clone()));
    let mut profiler2 = PoolProfiler::new(Arc::new(pool_definition));

    profiler1.initialize(sqrt_price_x98());
    profiler2.initialize(sqrt_price_x98());

    // Set up initial liquidity in both profilers
    let min_tick = PoolTick::get_min_tick(TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(TICK_SPACING);
    let initial_liquidity = 10000u128;
    let mint_event = create_mint_event(lp_address(), min_tick, max_tick, initial_liquidity);

    profiler1
        .process(&DexPoolData::LiquidityUpdate(mint_event.clone()))
        .unwrap();
    profiler2
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Parameters for the swap
    let sender = user_address();
    let recipient = user_address();
    let block = create_block_position();
    let amount0_in = U256::from(1000u32);

    // Method 1: Use execute_swap to create and apply the swap
    let swap_event = profiler1
        .swap_exact0_for_1(sender, recipient, block, amount0_in, None)
        .unwrap();

    // Method 2: Use process_swap with the created swap event
    profiler2.process(&DexPoolData::Swap(swap_event)).unwrap();

    // Verify profiler states are equivalent
    assert_eq!(profiler1.state.current_tick, profiler2.state.current_tick);
    assert_eq!(
        profiler1.state.price_sqrt_ratio_x96,
        profiler2.state.price_sqrt_ratio_x96
    );
    assert_eq!(
        profiler1.get_active_tick_count(),
        profiler2.get_active_tick_count()
    );
    assert_eq!(
        profiler1.get_active_liquidity(),
        profiler2.get_active_liquidity()
    );

    // Verify tick states match
    let mut tick_values1 = profiler1.get_active_tick_values();
    let mut tick_values2 = profiler2.get_active_tick_values();
    tick_values1.sort();
    tick_values2.sort();
    assert_eq!(tick_values1, tick_values2);

    // Verify individual tick states
    for tick_value in tick_values1 {
        let tick1 = profiler1.get_tick(tick_value).expect("Tick should exist");
        let tick2 = profiler2.get_tick(tick_value).expect("Tick should exist");
        assert_eq!(tick1.liquidity_gross, tick2.liquidity_gross);
        assert_eq!(tick1.liquidity_net, tick2.liquidity_net);
        assert_eq!(tick1.is_active(), tick2.is_active());
    }

    // Note: Fee growth tracking might differ slightly due to approximation in process_swap
    // but the core state (tick, price, liquidity) should be identical
}

// Follow Uniswapv3 official tests
// Initialize pool profiler here https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L194
#[fixture]
fn uni_pool_profiler() -> PoolProfiler {
    let pool_definition = pool_definition(None, None, None);
    let mut profiler = PoolProfiler::new(Arc::new(pool_definition));
    profiler.initialize(sqrt_price_x98());
    let min_tick = PoolTick::get_min_tick(TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(TICK_SPACING);
    let mint_event = create_mint_event(lp_address(), min_tick, max_tick, 3161);
    profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();
    profiler
}

// LOW fee tier pool (500 = 0.05%, tick spacing = 10) for matching Solidity tests
// Matches: https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L531
#[fixture]
fn low_fee_pool_profiler() -> PoolProfiler {
    let pool_definition = pool_definition(Some(500), Some(10), Some(encode_sqrt_ratio_x96(1, 1)));
    let mut profiler = PoolProfiler::new(Arc::new(pool_definition));
    profiler.initialize(encode_sqrt_ratio_x96(1, 1)); // Initialize at 1:1 price (tick 0)

    // Mint initial liquidity to match Solidity test setup (initializeLiquidityAmount = 2e18)
    const LOW_FEE_TICK_SPACING: i32 = 10;
    let min_tick = PoolTick::get_min_tick(LOW_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(LOW_FEE_TICK_SPACING);
    let initial_liquidity = expand_to_18_decimals(2);

    profiler
        .execute_mint(
            lp_address(),
            create_block_position(),
            min_tick,
            max_tick,
            initial_liquidity,
        )
        .unwrap();

    profiler
}

// MEDIUM fee tier pool (3000 = 0.3%, tick spacing = 60) for matching Solidity tests
// Matches: https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L564
#[fixture]
fn medium_fee_pool_profiler() -> PoolProfiler {
    let pool_definition = pool_definition(Some(3000), Some(60), Some(encode_sqrt_ratio_x96(1, 1)));
    let mut profiler = PoolProfiler::new(Arc::new(pool_definition));
    profiler.initialize(encode_sqrt_ratio_x96(1, 1)); // Initialize at 1:1 price (tick 0)

    // Mint initial liquidity to match Solidity test setup (initializeLiquidityAmount = 2e18)
    const MEDIUM_FEE_TICK_SPACING: i32 = 60;
    let min_tick = PoolTick::get_min_tick(MEDIUM_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(MEDIUM_FEE_TICK_SPACING);
    let initial_liquidity = expand_to_18_decimals(2);

    profiler
        .execute_mint(
            lp_address(),
            create_block_position(),
            min_tick,
            max_tick,
            initial_liquidity,
        )
        .unwrap();

    profiler
}

// Empty LOW fee tier pool (500 = 0.05%, tick spacing = 10) with no initial liquidity
// For tests that need to control liquidity from scratch
#[fixture]
fn empty_low_fee_pool_profiler() -> PoolProfiler {
    let pool_definition = pool_definition(Some(500), Some(10), Some(encode_sqrt_ratio_x96(1, 1)));
    let mut profiler = PoolProfiler::new(Arc::new(pool_definition));
    profiler.initialize(encode_sqrt_ratio_x96(1, 1)); // Initialize at 1:1 price (tick 0)
    profiler
}

#[rstest]
fn test_uni_pool_profiler_initial_state(uni_pool_profiler: PoolProfiler) {
    assert_eq!(uni_pool_profiler.state.current_tick, -23028);
    assert_eq!(uni_pool_profiler.get_active_tick_count(), 2);
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    let max_tick = PoolTick::get_max_tick(TICK_SPACING);
    let min_tick = PoolTick::get_min_tick(TICK_SPACING);
    let position = uni_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position should exist");
    assert_eq!(position.liquidity, 3161);
    assert_eq!(position.total_amount0_deposited, U256::from(9996u32));
    assert_eq!(position.total_amount1_deposited, U256::from(1000u32));
    assert_eq!(uni_pool_profiler.get_active_liquidity(), 3161);
    assert_eq!(
        uni_pool_profiler.get_active_liquidity(),
        uni_pool_profiler.get_total_liquidity_from_active_positions(),
        "Tick map liquidity should match sum of active positions"
    );
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 0);

    // Liquidity utilization should be 100% since all liquidity is in-range
    assert_eq!(uni_pool_profiler.get_total_liquidity(), 3161);
    assert_eq!(uni_pool_profiler.liquidity_utilization_rate(), 1.0);
}

// ---------- TEST MINTS ABOVE CURRENT PRICE ----------

#[rstest]
fn test_mint_above_current_price(mut uni_pool_profiler: PoolProfiler) {
    let lower_tick = -22980;
    let upper_tick = 0;
    let liquidity = 10000;
    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, liquidity);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();
    // We minted a position which doesn't contain active tick -23028 from initial univ3 setup
    // So active position count will stay 1, but inactive position count will be 1
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 1);
    let position = uni_pool_profiler
        .get_position(&lp_address(), lower_tick, upper_tick)
        .expect("Position should exist");
    assert_eq!(position.liquidity, liquidity);
    assert_eq!(position.tick_lower, lower_tick);
    assert_eq!(position.tick_upper, upper_tick);
    assert_eq!(position.total_amount0_deposited, 21549);
    assert_eq!(position.total_amount1_deposited, 0);
    // We have 4 active ticks (min and max from initial setup and new -22980 and 0)
    assert_eq!(uni_pool_profiler.get_active_tick_count(), 4);
    let mut active_tick_values = uni_pool_profiler.get_active_tick_values();
    active_tick_values.sort();
    assert_eq!(
        active_tick_values,
        vec![-887220, lower_tick, upper_tick, 887220]
    );
    assert!(
        uni_pool_profiler
            .get_tick(lower_tick)
            .is_some_and(|tick| tick.is_active())
    );
    assert!(
        uni_pool_profiler
            .get_tick(upper_tick)
            .is_some_and(|tick| tick.is_active())
    );
}

#[rstest]
fn test_max_tick_with_high_leverage(mut uni_pool_profiler: PoolProfiler) {
    let max_tick = PoolTick::get_max_tick(TICK_SPACING);
    let lower_tick = max_tick - (TICK_SPACING);
    let upper_tick = max_tick;
    let liquidity = U256::from(2u128).pow(U256::from(102u128)).to::<u128>();

    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, liquidity);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    let position = uni_pool_profiler
        .get_position(&lp_address(), lower_tick, upper_tick)
        .expect("Position should exist");
    assert_eq!(position.liquidity, liquidity);
    assert_eq!(position.tick_lower, lower_tick);
    assert_eq!(position.tick_upper, upper_tick);
    assert_eq!(position.total_amount0_deposited, U256::from(828011525u32));
    assert_eq!(position.total_amount1_deposited, U256::ZERO);
    // We have only three active ticks, and max_tick is updated two times (from init mint and this mint)
    assert_eq!(uni_pool_profiler.get_active_tick_count(), 3);
    assert!(
        uni_pool_profiler
            .tick_map
            .get_tick(max_tick)
            .is_some_and(|tick| tick.updates_count == 2)
    );
    let mut active_tick_values = uni_pool_profiler.get_active_tick_values();
    active_tick_values.sort();
    assert_eq!(active_tick_values, vec![-887220, lower_tick, max_tick]);
}

#[rstest]
fn test_minting_works_for_max_tick(mut uni_pool_profiler: PoolProfiler) {
    let max_tick = PoolTick::get_max_tick(TICK_SPACING);
    let lower_tick = -22980;
    let upper_tick = max_tick;
    let liquidity = 10000;

    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, liquidity);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    let position = uni_pool_profiler
        .get_position(&lp_address(), lower_tick, upper_tick)
        .expect("Position should exist");
    assert_eq!(position.liquidity, liquidity);
    assert_eq!(position.tick_lower, lower_tick);
    assert_eq!(position.tick_upper, upper_tick);
    assert_eq!(position.total_amount0_deposited, U256::from(31549u32));
    assert_eq!(position.total_amount1_deposited, U256::ZERO);
    // We touched max_tick once more, so it updated two times, but -22980 tick only once
    assert!(
        uni_pool_profiler
            .tick_map
            .get_tick(lower_tick)
            .is_some_and(|tick| tick.updates_count == 1)
    );
    assert!(
        uni_pool_profiler
            .tick_map
            .get_tick(upper_tick)
            .is_some_and(|tick| tick.updates_count == 2)
    );
    let mut active_tick_values = uni_pool_profiler.get_active_tick_values();
    active_tick_values.sort();
    assert_eq!(active_tick_values, vec![-887220, lower_tick, max_tick]);
}

#[rstest]
fn test_if_removing_of_liquidity_works_after_mint(mut uni_pool_profiler: PoolProfiler) {
    let lower_tick = -240;
    let upper_tick = 0;
    let liquidity = 10000;

    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, liquidity);
    let burn_event = create_burn_event(lp_address(), lower_tick, upper_tick, liquidity);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();

    // We will have one active position from init and this one which we
    // first minted then burned so its inactive and
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    // Active tick count stay the same from min and max_tick in init fixture
    assert_eq!(uni_pool_profiler.get_active_tick_count(), 2);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 1);
    assert_eq!(uni_pool_profiler.analytics.total_mints, 2);
    assert_eq!(uni_pool_profiler.analytics.total_burns, 1);
    assert_eq!(uni_pool_profiler.analytics.total_fee_collects, 0);
    assert_eq!(uni_pool_profiler.analytics.total_swaps, 0);
    // Lets inspect the state before fee collect
    if let Some(position) = uni_pool_profiler.get_position(&lp_address(), lower_tick, upper_tick) {
        let (amount0, amount1) =
            get_amounts_for_liquidity(sqrt_price_x98(), upper_tick, lower_tick, liquidity, true);
        assert_eq!(position.liquidity, 0);
        assert_eq!(position.total_amount0_deposited, amount0);
        assert_eq!(position.total_amount1_deposited, amount1);
        // With burn we didn't collect anything so and tokens stays in tokens_owned_* variables
        assert_eq!(position.total_amount0_collected, 0);
        assert_eq!(position.total_amount1_collected, 0);
        assert_eq!(position.tokens_owed_0, 120);
        assert_eq!(position.tokens_owed_1, 0);
    }

    // Run the collect and inspect the state
    let collect_event = create_collect_event(lower_tick, upper_tick, u128::MAX, u128::MAX);
    uni_pool_profiler
        .process(&DexPoolData::FeeCollect(collect_event))
        .unwrap();

    // After collect, position should be cleaned up since it's completely empty
    assert!(
        uni_pool_profiler
            .get_position(&lp_address(), lower_tick, upper_tick)
            .is_none(),
        "Position should be cleaned up after collecting all fees"
    );

    // Verify position is no longer counted
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1); // Only init position
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 0); // Cleaned up
}

#[rstest]
fn test_if_we_correctly_add_and_remove_liquidity_gross_after_every_updates(
    mut uni_pool_profiler: PoolProfiler,
) {
    let mint_event = create_mint_event(lp_address(), -240, 0, 100);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();
    // Target ticks have liquidity_gross correctly set
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(-240)
            .unwrap()
            .liquidity_gross,
        100
    );
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(0)
            .unwrap()
            .liquidity_gross,
        100
    );
    // Some other ticks have liquidity_gross zero
    assert!(uni_pool_profiler.tick_map.get_tick(TICK_SPACING).is_none());
    assert!(
        uni_pool_profiler
            .tick_map
            .get_tick(TICK_SPACING * 2)
            .is_none()
    );

    // Mint again at -240 and at TICK_SPACING
    let mint_event = create_mint_event(lp_address(), -240, TICK_SPACING, 150);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(-240)
            .unwrap()
            .liquidity_gross,
        250
    );
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(0)
            .unwrap()
            .liquidity_gross,
        100
    );
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(TICK_SPACING)
            .unwrap()
            .liquidity_gross,
        150
    );
    assert!(
        uni_pool_profiler
            .tick_map
            .get_tick(TICK_SPACING * 2)
            .is_none()
    );

    // Mint again at 0 and at TICK_SPACING * 2
    let mint_event = create_mint_event(lp_address(), 0, TICK_SPACING * 2, 60);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(-240)
            .unwrap()
            .liquidity_gross,
        250
    );
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(0)
            .unwrap()
            .liquidity_gross,
        160
    );
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(TICK_SPACING)
            .unwrap()
            .liquidity_gross,
        150
    );
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(TICK_SPACING * 2)
            .unwrap()
            .liquidity_gross,
        60
    );

    // Burn at tick -240 and 0
    let burn_event = create_burn_event(lp_address(), -240, 0, 90);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(-240)
            .unwrap()
            .liquidity_gross,
        160
    ); // 250 -90
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(0)
            .unwrap()
            .liquidity_gross,
        70
    ); // 160 -90
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(TICK_SPACING)
            .unwrap()
            .liquidity_gross,
        150
    ); // untouched

    // Burn again to clear the remaining 10 from position (-240, 0)
    let burn_event = create_burn_event(lp_address(), -240, 0, 10);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(-240)
            .unwrap()
            .liquidity_gross,
        150
    ); // 160 - 10
    assert_eq!(
        uni_pool_profiler
            .tick_map
            .get_tick(0)
            .unwrap()
            .liquidity_gross,
        60
    ); // 70 - 10
}

#[rstest]
fn test_burn_uninitialized_position(mut uni_pool_profiler: PoolProfiler) {
    // Try to burn a position that was never minted
    let burn_event = create_burn_event(lp_address(), -240, 0, 100);

    let result = uni_pool_profiler.process(&DexPoolData::LiquidityUpdate(burn_event));

    // Should fail because position doesn't exist (will be init with 0 liquidity) and trying to burn > 0
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "Position liquidity 0 is less than the requested burn amount of 100",
    );
}

#[rstest]
fn test_position_fee_growth_and_tokens_owed_after_swaps(mut uni_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L510
    let lower_tick = PoolTick::get_min_tick(TICK_SPACING) + TICK_SPACING;
    let upper_tick = PoolTick::get_max_tick(TICK_SPACING) - TICK_SPACING;

    // Mint position with 1e18 liquidity for "other" address
    let mint_event = create_mint_event(
        other_address(),
        lower_tick,
        upper_tick,
        expand_to_18_decimals(1),
    );
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Execute swaps to generate fees
    // Swap 0.1e18 token0 for token1
    uni_pool_profiler
        .swap_exact0_for_1(
            user_address(),
            user_address(),
            create_block_position(),
            U256::from(expand_to_18_decimals(1)) / U256::from(10),
            None,
        )
        .unwrap();

    // Swap 0.01e18 token1 for token0
    uni_pool_profiler
        .swap_exact1_for_0(
            user_address(),
            user_address(),
            create_block_position(),
            U256::from(expand_to_18_decimals(1)) / U256::from(100),
            None,
        )
        .unwrap();

    // Now mint 1 wei of liquidity for "lp_address" (wallet) - this creates a NEW position
    let mint_1_wei = create_mint_event(lp_address(), lower_tick, upper_tick, 1);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_1_wei))
        .unwrap();

    // Verify position state after minting 1 wei (this is a fresh position for lp_address)
    let position = uni_pool_profiler
        .get_position(&lp_address(), lower_tick, upper_tick)
        .expect("Position should exist");

    assert_eq!(position.liquidity, 1);
    assert_eq!(
        position.fee_growth_inside_0_last,
        U256::from_str("102084710076281216349243831104605583").unwrap()
    );
    assert_eq!(
        position.fee_growth_inside_1_last,
        U256::from_str("10208471007628121634924383110460558").unwrap()
    );
    assert_eq!(position.tokens_owed_0, 0, "tokens owed 0 before burn");
    assert_eq!(position.tokens_owed_1, 0, "tokens owed 1 before burn");

    // Burn the 1 wei of liquidity
    let burn_1_wei = create_burn_event(lp_address(), lower_tick, upper_tick, 1);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_1_wei))
        .unwrap();

    // Verify position state after burning
    let position_after = uni_pool_profiler
        .get_position(&lp_address(), lower_tick, upper_tick)
        .expect("Position should exist");

    assert_eq!(position_after.liquidity, 0);
    assert_eq!(
        position_after.fee_growth_inside_0_last,
        U256::from_str("102084710076281216349243831104605583").unwrap()
    );
    assert_eq!(
        position_after.fee_growth_inside_1_last,
        U256::from_str("10208471007628121634924383110460558").unwrap()
    );
    assert_eq!(position_after.tokens_owed_0, 3, "tokens owed 0 after burn");
    assert_eq!(position_after.tokens_owed_1, 0, "tokens owed 1 after burn");
}

#[rstest]
fn test_does_not_clear_position_fee_growth_snapshot_if_no_more_liquidity(
    mut medium_fee_pool_profiler: PoolProfiler,
) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L564
    const MEDIUM_FEE_TICK_SPACING: i32 = 60;
    let min_tick = PoolTick::get_min_tick(MEDIUM_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(MEDIUM_FEE_TICK_SPACING);

    // Mint 1e18 liquidity for "other" address
    let mint_event = create_mint_event(
        other_address(),
        min_tick,
        max_tick,
        expand_to_18_decimals(1),
    );
    medium_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Execute swaps to generate fees
    // Swap 1e18 token0 for token1
    medium_fee_pool_profiler
        .swap_exact0_for_1(
            user_address(),
            user_address(),
            create_block_position(),
            U256::from(expand_to_18_decimals(1)),
            None,
        )
        .unwrap();

    // Swap 1e18 token1 for token0
    medium_fee_pool_profiler
        .swap_exact1_for_0(
            user_address(),
            user_address(),
            create_block_position(),
            U256::from(expand_to_18_decimals(1)),
            None,
        )
        .unwrap();

    // Burn all liquidity
    let burn_event = create_burn_event(
        other_address(),
        min_tick,
        max_tick,
        expand_to_18_decimals(1),
    );
    medium_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();

    // Verify position state after burning all liquidity
    let position = medium_fee_pool_profiler
        .get_position(&other_address(), min_tick, max_tick)
        .expect("Position should exist");

    assert_eq!(position.liquidity, 0);
    assert_ne!(position.tokens_owed_0, 0, "tokens owed 0 should not be 0");
    assert_ne!(position.tokens_owed_1, 0, "tokens owed 1 should not be 0");
    // Fee growth snapshots should NOT be cleared when liquidity goes to 0
    assert_eq!(
        position.fee_growth_inside_0_last,
        U256::from_str("340282366920938463463374607431768211").unwrap()
    );
    assert_eq!(
        position.fee_growth_inside_1_last,
        U256::from_str("340282366920938576890830247744589365").unwrap()
    );
    assert_eq!(medium_fee_pool_profiler.analytics.total_burns, 1);
    assert_eq!(medium_fee_pool_profiler.analytics.total_fee_collects, 0);
    assert_eq!(medium_fee_pool_profiler.analytics.total_swaps, 2);
    assert_eq!(medium_fee_pool_profiler.analytics.total_mints, 2);
}

// ---------- TEST MINTS INCLUDING CURRENT PRICE ----------

#[rstest]
fn test_mint_if_range_includes_current_price(mut uni_pool_profiler: PoolProfiler) {
    let lower_tick = PoolTick::get_min_tick(TICK_SPACING) + TICK_SPACING;
    let upper_tick = PoolTick::get_max_tick(TICK_SPACING) - TICK_SPACING;

    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, 100);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // This becomes an active position, and with one at init we have 2
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 2);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 0);
    let position = uni_pool_profiler
        .get_position(&lp_address(), lower_tick, upper_tick)
        .expect("Position should exist");
    assert_eq!(position.liquidity, 100);
    assert_eq!(position.tick_lower, lower_tick);
    assert_eq!(position.tick_upper, upper_tick);
    assert_eq!(position.total_amount0_deposited, 317);
    assert_eq!(position.total_amount1_deposited, 32);
    // Both upper tick and lower ticks are initialized
    assert_eq!(
        uni_pool_profiler
            .get_tick(upper_tick)
            .unwrap()
            .liquidity_gross,
        100
    );
    assert_eq!(
        uni_pool_profiler
            .get_tick(lower_tick)
            .unwrap()
            .liquidity_gross,
        100
    );
}

#[rstest]
fn test_mint_for_min_and_max_ticks(mut uni_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L383
    let lower_tick = PoolTick::get_min_tick(TICK_SPACING);
    let upper_tick = PoolTick::get_max_tick(TICK_SPACING);
    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, 10000);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // We minted again at the same position
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 0);

    let position = uni_pool_profiler
        .get_position(&lp_address(), lower_tick, upper_tick)
        .expect("Position should exist");
    assert_eq!(position.liquidity, 10000 + 3161);
    assert_eq!(position.tick_lower, lower_tick);
    assert_eq!(position.tick_upper, upper_tick);
    assert_eq!(position.total_amount0_deposited, 9996 + 31623);
    assert_eq!(position.total_amount1_deposited, 1000 + 3163);
    assert_eq!(position.tokens_owed_0, 0);
    assert_eq!(position.tokens_owed_1, 0);
}

#[rstest]
fn test_mint_then_burning_and_collecting(mut uni_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L393
    let lower_tick = PoolTick::get_min_tick(TICK_SPACING) + TICK_SPACING;
    let upper_tick = PoolTick::get_max_tick(TICK_SPACING) - TICK_SPACING;

    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, 100);
    let burn_event = create_burn_event(lp_address(), lower_tick, upper_tick, 100);
    let collect_event = create_collect_event(lower_tick, upper_tick, u128::MAX, u128::MAX);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();
    uni_pool_profiler
        .process(&DexPoolData::FeeCollect(collect_event))
        .unwrap();

    // After collect, position should be cleaned up since it's completely empty
    assert!(
        uni_pool_profiler
            .get_position(&lp_address(), lower_tick, upper_tick)
            .is_none(),
        "Position should be cleaned up after collecting all fees"
    );

    // Verify position is no longer counted
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1); // Only init position
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 0); // Cleaned up
}

// ---------- TEST MINTS BELOW CURRENT PRICE ----------

#[rstest]
fn test_mint_below_current_price_when_token1_only_changed(mut uni_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L427
    let lower_tick = -46080;
    let upper_tick = -23040;
    let liquidity = 10000;
    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, liquidity);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // This position is not active right now because the current tick is -23028
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 1);
    let position = uni_pool_profiler
        .get_position(&lp_address(), lower_tick, upper_tick)
        .expect("Position should exist");
    assert_eq!(position.liquidity, liquidity);
    assert_eq!(position.tick_lower, lower_tick);
    assert_eq!(position.tick_upper, upper_tick);
    assert_eq!(position.total_amount0_deposited, 0);
    assert_eq!(position.total_amount1_deposited, 2162);
    assert_eq!(position.tokens_owed_0, 0);
    assert_eq!(position.tokens_owed_1, 0);
}

#[rstest]
fn test_mint_below_current_price_when_really_high_leverage(mut uni_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L435
    let lower_tick = PoolTick::get_min_tick(TICK_SPACING);
    let upper_tick = lower_tick + TICK_SPACING;
    let liquidity = U256::from(2u128).pow(U256::from(102u128)).to::<u128>();

    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, liquidity);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // This position is not active right now because the current tick is -23028
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 1);
    let position = uni_pool_profiler
        .get_position(&lp_address(), lower_tick, upper_tick)
        .expect("Position should exist");
    assert_eq!(position.liquidity, liquidity);
    assert_eq!(position.tick_lower, lower_tick);
    assert_eq!(position.tick_upper, upper_tick);
    assert_eq!(position.total_amount0_deposited, 0);
    assert_eq!(position.total_amount1_deposited, 828011520);
    assert_eq!(position.tokens_owed_0, 0);
    assert_eq!(position.tokens_owed_1, 0);
}

#[rstest]
fn test_if_mint_below_current_price_works_after_burn_and_fee_collect(
    mut uni_pool_profiler: PoolProfiler,
) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L450
    let lower_tick = -46080;
    let upper_tick = -46020;
    let liquidity = 10000;
    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, liquidity);
    let burn_event = create_burn_event(lp_address(), lower_tick, upper_tick, 10000);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();

    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 1);

    // Inspect the state before collecting
    if let Some(position) = uni_pool_profiler.get_position(&lp_address(), lower_tick, upper_tick) {
        assert_eq!(position.liquidity, 0);
        assert_eq!(position.total_amount0_deposited, 0);
        assert_eq!(position.total_amount1_deposited, 4);
        assert_eq!(position.tokens_owed_0, 0);
        assert_eq!(position.tokens_owed_1, 3);
        assert_eq!(position.total_amount0_collected, 0);
        assert_eq!(position.total_amount1_collected, 0);
    } else {
        panic!("Position should exist");
    }

    let collect_event = create_collect_event(lower_tick, upper_tick, u128::MAX, u128::MAX);
    uni_pool_profiler
        .process(&DexPoolData::FeeCollect(collect_event))
        .unwrap();

    // After collect, position should be cleaned up since it's completely empty
    assert!(
        uni_pool_profiler
            .get_position(&lp_address(), lower_tick, upper_tick)
            .is_none(),
        "Position should be cleaned up after collecting all fees"
    );

    // Verify position is no longer counted
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1); // Only init position
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 0); // Cleaned up
}

#[rstest]
fn test_collect_with_invalid_ticks_does_not_panic(mut uni_pool_profiler: PoolProfiler) {
    let min_tick = PoolTick::get_min_tick(TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(TICK_SPACING);

    let initial_position = uni_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position should exist");
    let initial_tokens_owed_0 = initial_position.tokens_owed_0;

    // Collect with invalid ticks (tick_lower > tick_upper) should not panic
    let invalid_collect = create_collect_event(100, 50, 1000, 1000);
    let result = uni_pool_profiler.process(&DexPoolData::FeeCollect(invalid_collect));
    assert!(result.is_ok());

    // Verify valid position remains unchanged
    let position_after = uni_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position should still exist");
    assert_eq!(position_after.tokens_owed_0, initial_tokens_owed_0);
}

#[rstest]
fn test_collect_works_with_multiple_lps(mut empty_low_fee_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L944
    const LOW_FEE_TICK_SPACING: i32 = 10;
    let min_tick = PoolTick::get_min_tick(LOW_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(LOW_FEE_TICK_SPACING);

    // Mint position 1 at full range (minTick, maxTick) with 1e18 liquidity
    let mint_event1 = create_mint_event(lp_address(), min_tick, max_tick, expand_to_18_decimals(1));
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event1))
        .unwrap();

    // Mint position 2 at narrower range (minTick + tickSpacing, maxTick - tickSpacing) with 2e18 liquidity
    let mint_event2 = create_mint_event(
        lp_address(),
        min_tick + LOW_FEE_TICK_SPACING,
        max_tick - LOW_FEE_TICK_SPACING,
        expand_to_18_decimals(2),
    );
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event2))
        .unwrap();

    // Execute swap: 1e18 token0 for token1
    empty_low_fee_pool_profiler
        .swap_exact0_for_1(
            user_address(),
            user_address(),
            create_block_position(),
            U256::from(expand_to_18_decimals(1)),
            None,
        )
        .unwrap();

    // "Poke" positions by burning 0 liquidity to update fee accounting
    let burn_event1 = create_burn_event(lp_address(), min_tick, max_tick, 0);
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event1))
        .unwrap();

    let burn_event2 = create_burn_event(
        lp_address(),
        min_tick + LOW_FEE_TICK_SPACING,
        max_tick - LOW_FEE_TICK_SPACING,
        0,
    );
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event2))
        .unwrap();

    // Check tokens owed for both positions
    let position0 = empty_low_fee_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position 0 should exist");

    let position1 = empty_low_fee_pool_profiler
        .get_position(
            &lp_address(),
            min_tick + LOW_FEE_TICK_SPACING,
            max_tick - LOW_FEE_TICK_SPACING,
        )
        .expect("Position 1 should exist");

    // Position 0 (full range, 1e18 liquidity) should get 1/3 of fees
    assert_eq!(position0.tokens_owed_0, 166666666666667);
    // Position 1 (narrower range, 2e18 liquidity) should get 2/3 of fees
    assert_eq!(position1.tokens_owed_0, 333333333333334);
}

// ---------- WORKS ACROSS LARGE FEE INCREASES ----------

#[rstest]
fn test_fee_growth_just_before_cap_binds(mut empty_low_fee_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L974
    const LOW_FEE_TICK_SPACING: i32 = 10;
    let min_tick = PoolTick::get_min_tick(LOW_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(LOW_FEE_TICK_SPACING);

    // Mint 1e18 liquidity
    let mint_event = create_mint_event(lp_address(), min_tick, max_tick, expand_to_18_decimals(1));
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Magic number: (2^128 - 1) * 2^128 / 1e18
    // This value will result in tokensOwed = MaxUint128 - 1
    let magic_number =
        U256::from_str("115792089237316195423570985008687907852929702298719625575994").unwrap();

    // Set fee growth to magic number
    empty_low_fee_pool_profiler.set_fee_growth_global(magic_number, U256::ZERO);

    // "Poke" position by burning 0 liquidity to update fee accounting
    let burn_event = create_burn_event(lp_address(), min_tick, max_tick, 0);
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();

    let position = empty_low_fee_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position should exist");

    // tokens_owed_0 should be MaxUint128 - 1
    assert_eq!(position.tokens_owed_0, u128::MAX - 1);
    assert_eq!(position.tokens_owed_1, 0);
}

#[rstest]
fn test_fee_growth_just_after_cap_binds(mut empty_low_fee_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L984
    const LOW_FEE_TICK_SPACING: i32 = 10;
    let min_tick = PoolTick::get_min_tick(LOW_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(LOW_FEE_TICK_SPACING);

    // Mint 1e18 liquidity
    let mint_event = create_mint_event(lp_address(), min_tick, max_tick, expand_to_18_decimals(1));
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Magic number + 1: This will cap at MaxUint128
    let magic_number =
        U256::from_str("115792089237316195423570985008687907852929702298719625575995").unwrap();

    // Set fee growth to magic number + 1
    empty_low_fee_pool_profiler.set_fee_growth_global(magic_number, U256::ZERO);

    // "Poke" position by burning 0 liquidity
    let burn_event = create_burn_event(lp_address(), min_tick, max_tick, 0);
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();

    let position = empty_low_fee_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position should exist");

    // tokens_owed_0 should be capped at MaxUint128
    assert_eq!(position.tokens_owed_0, u128::MAX);
    assert_eq!(position.tokens_owed_1, 0);
}

#[rstest]
fn test_fee_growth_well_after_cap_binds(mut empty_low_fee_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L994
    const LOW_FEE_TICK_SPACING: i32 = 10;
    let min_tick = PoolTick::get_min_tick(LOW_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(LOW_FEE_TICK_SPACING);

    // Mint 1e18 liquidity
    let mint_event = create_mint_event(lp_address(), min_tick, max_tick, expand_to_18_decimals(1));
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Set fee growth to MaxUint256
    empty_low_fee_pool_profiler.set_fee_growth_global(U256::MAX, U256::ZERO);

    // "Poke" position by burning 0 liquidity
    let burn_event = create_burn_event(lp_address(), min_tick, max_tick, 0);
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();

    let position = empty_low_fee_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position should exist");

    // tokens_owed_0 should still be capped at MaxUint128
    assert_eq!(position.tokens_owed_0, u128::MAX);
    assert_eq!(position.tokens_owed_1, 0);
}

// ---------- WORKS ACROSS OVERFLOW BOUNDARIES ----------

#[rstest]
fn test_overflow_boundary_token0(mut empty_low_fee_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L1012
    const LOW_FEE_TICK_SPACING: i32 = 10;
    let min_tick = PoolTick::get_min_tick(LOW_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(LOW_FEE_TICK_SPACING);

    // Set fee growth to MaxUint256 for both tokens (simulating overflow condition)
    empty_low_fee_pool_profiler.set_fee_growth_global(U256::MAX, U256::MAX);

    // Mint 10e18 liquidity
    let mint_event = create_mint_event(lp_address(), min_tick, max_tick, expand_to_18_decimals(10));
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Swap 1e18 token0 for token1
    empty_low_fee_pool_profiler
        .swap_exact0_for_1(
            user_address(),
            user_address(),
            create_block_position(),
            U256::from(expand_to_18_decimals(1)),
            None,
        )
        .unwrap();

    // "Poke" position by burning 0 liquidity to update fee accounting
    let burn_event = create_burn_event(lp_address(), min_tick, max_tick, 0);
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();

    let position = empty_low_fee_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position should exist");

    // When fee_growth wraps around from MaxUint256, the underflow-safe calculation
    // should still correctly compute fees
    assert_eq!(position.tokens_owed_0, 499999999999999);
    assert_eq!(position.tokens_owed_1, 0);
}

#[rstest]
fn test_overflow_boundary_token1(mut empty_low_fee_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L1024
    const LOW_FEE_TICK_SPACING: i32 = 10;
    let min_tick = PoolTick::get_min_tick(LOW_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(LOW_FEE_TICK_SPACING);

    // Set fee growth to MaxUint256 for both tokens
    empty_low_fee_pool_profiler.set_fee_growth_global(U256::MAX, U256::MAX);

    // Mint 10e18 liquidity
    let mint_event = create_mint_event(lp_address(), min_tick, max_tick, expand_to_18_decimals(10));
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Swap 1e18 token1 for token0
    empty_low_fee_pool_profiler
        .swap_exact1_for_0(
            user_address(),
            user_address(),
            create_block_position(),
            U256::from(expand_to_18_decimals(1)),
            None,
        )
        .unwrap();

    // "Poke" position by burning 0 liquidity
    let burn_event = create_burn_event(lp_address(), min_tick, max_tick, 0);
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();

    let position = empty_low_fee_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position should exist");

    assert_eq!(position.tokens_owed_0, 0);
    assert_eq!(position.tokens_owed_1, 499999999999999);
}

#[rstest]
fn test_overflow_boundary_token0_and_token1(mut empty_low_fee_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L1036
    const LOW_FEE_TICK_SPACING: i32 = 10;
    let min_tick = PoolTick::get_min_tick(LOW_FEE_TICK_SPACING);
    let max_tick = PoolTick::get_max_tick(LOW_FEE_TICK_SPACING);

    // Set fee growth to MaxUint256 for both tokens
    empty_low_fee_pool_profiler.set_fee_growth_global(U256::MAX, U256::MAX);

    // Mint 10e18 liquidity
    let mint_event = create_mint_event(lp_address(), min_tick, max_tick, expand_to_18_decimals(10));
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Swap 1e18 token0 for token1
    empty_low_fee_pool_profiler
        .swap_exact0_for_1(
            user_address(),
            user_address(),
            create_block_position(),
            U256::from(expand_to_18_decimals(1)),
            None,
        )
        .unwrap();

    // Swap 1e18 token1 for token0
    empty_low_fee_pool_profiler
        .swap_exact1_for_0(
            user_address(),
            user_address(),
            create_block_position(),
            U256::from(expand_to_18_decimals(1)),
            None,
        )
        .unwrap();

    // "Poke" position by burning 0 liquidity
    let burn_event = create_burn_event(lp_address(), min_tick, max_tick, 0);
    empty_low_fee_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(burn_event))
        .unwrap();

    let position = empty_low_fee_pool_profiler
        .get_position(&lp_address(), min_tick, max_tick)
        .expect("Position should exist");

    // Both tokens should have fees from their respective swaps
    assert_eq!(position.tokens_owed_0, 499999999999999);
    assert_eq!(position.tokens_owed_1, 500000000000000);
}

#[rstest]
fn test_flash_increases_fee_growth_by_expected_amount(mut medium_fee_pool_profiler: PoolProfiler) {
    // https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.spec.ts#L1463
    let _flash_event = medium_fee_pool_profiler
        .execute_flash(
            user_address(),
            other_address(),
            create_block_position(),
            U256::from(1001u32),
            U256::from(2002u32),
        )
        .unwrap();

    // BigNumber.from(4).mul(BigNumber.from(2).pow(128)).div(expandTo18Decimals(2))
    let expected_fee_growth_0 = (U256::from(4) * U256::from(2).pow(U256::from(128)))
        .div(U256::from(expand_to_18_decimals(2)));
    let expected_fee_growth_1 = (U256::from(7) * U256::from(2).pow(U256::from(128)))
        .div(U256::from(expand_to_18_decimals(2)));

    assert_eq!(
        medium_fee_pool_profiler.state.fee_growth_global_0, expected_fee_growth_0,
        "Fee growth global 0 mismatch"
    );
    assert_eq!(
        medium_fee_pool_profiler.state.fee_growth_global_1, expected_fee_growth_1,
        "Fee growth global 1 mismatch"
    );
    assert_eq!(medium_fee_pool_profiler.analytics.total_flashes, 1);
}

// ---------- ACTIVE LIQUIDITY AND TICK CROSSING TESTS WHEN SWAPPING ----------

#[rstest]
fn test_swap_crossing_tick_down_activates_position(mut uni_pool_profiler: PoolProfiler) {
    // Initial state: current tick is -23028, liquidity is 3161
    let initial_liquidity = uni_pool_profiler.get_active_liquidity();
    let initial_tick = uni_pool_profiler.state.current_tick;
    assert_eq!(initial_liquidity, 3161);
    assert_eq!(initial_tick, -23028);

    // Mint a position that extends from below current price to MAX_TICK
    // Position range: [-23040, MAX_TICK] - upper tick at max ensures swap won't exit the position
    // Tick spacing is 60, so ticks must be multiples of 60
    let upper_tick = -23040; // Just below current tick -23028
    let lower_tick = PoolTick::get_min_tick(TICK_SPACING);
    let position_liquidity = 50000u128;

    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, position_liquidity);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Position is inactive (current tick -23028 is below upper tick -23040), so active liquidity unchanged
    assert_eq!(uni_pool_profiler.get_active_liquidity(), initial_liquidity);
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 1);

    // Liquidity utilization before swap: active (3161) / total (3161 + 50000) = ~5.95%
    assert_eq!(uni_pool_profiler.get_total_liquidity(), U256::from(53161));
    let utilization_before = 3161.0 / 53161.0;
    assert!((uni_pool_profiler.liquidity_utilization_rate() - utilization_before).abs() < 1e-6);

    // Execute swap: token0 for token1 to move price down into the position range
    let amount0_in = U256::from(expand_to_18_decimals(1));
    let _ = uni_pool_profiler
        .swap_exact0_for_1(
            user_address(),
            user_address(),
            create_block_position(),
            amount0_in,
            None,
        )
        .unwrap();

    // Verify price moved down past the upper tick of the position
    let new_tick = uni_pool_profiler.state.current_tick;
    assert!(
        new_tick <= upper_tick,
        "Price should have crossed to or below tick {}, got {}",
        upper_tick,
        new_tick
    );

    // When crossing tick -23040 downward (cross_tick_down):
    // - liquidity_net at upper_tick is -50000 (negative, it's upper bound)
    // - We negate it: -(-50000) = +50000
    // - Active liquidity should increase by position_liquidity
    let final_liquidity = uni_pool_profiler.get_active_liquidity();
    assert_eq!(
        final_liquidity,
        initial_liquidity + position_liquidity,
        "Liquidity should increase when crossing into position range downward"
    );
    assert_eq!(
        uni_pool_profiler.get_active_liquidity(),
        uni_pool_profiler.get_total_liquidity_from_active_positions(),
        "Tick map liquidity should match sum of active positions after crossing"
    );

    // Verify the position is now active
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 2);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 0);
}

#[rstest]
fn test_swap_crossing_tick_up_activates_position(mut uni_pool_profiler: PoolProfiler) {
    // Initial state: current tick is -23028, liquidity is 3161
    let initial_liquidity = uni_pool_profiler.get_active_liquidity();
    let initial_tick = uni_pool_profiler.state.current_tick;
    assert_eq!(initial_liquidity, 3161);
    assert_eq!(initial_tick, -23028);

    // Mint a position above current price that will become active when price moves up
    // Position range: [-22980, MAX_TICK] - upper tick at max ensures swap won't exit the position
    // Tick spacing is 60, so ticks must be multiples of 60
    let lower_tick = -22980;
    let upper_tick = PoolTick::get_max_tick(TICK_SPACING);
    let position_liquidity = 40000u128;

    let mint_event = create_mint_event(lp_address(), lower_tick, upper_tick, position_liquidity);
    uni_pool_profiler
        .process(&DexPoolData::LiquidityUpdate(mint_event))
        .unwrap();

    // Position is inactive (below current price), so active liquidity unchanged
    assert_eq!(uni_pool_profiler.get_active_liquidity(), initial_liquidity);
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 1);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 1);

    // Liquidity utilization before swap: active (3161) / total (3161 + 40000) = ~7.32%
    assert_eq!(uni_pool_profiler.get_total_liquidity(), U256::from(43161));
    let utilization_before = 3161.0 / 43161.0;
    assert!((uni_pool_profiler.liquidity_utilization_rate() - utilization_before).abs() < 1e-6);

    // Execute large swap: token1 for token0 to move price up, crossing tick -22980
    let amount1_in = U256::from(expand_to_18_decimals(1000));
    let _ = uni_pool_profiler
        .swap_exact1_for_0(
            user_address(),
            user_address(),
            create_block_position(),
            amount1_in,
            None,
        )
        .unwrap();

    // Verify price moved up past the lower tick of the position
    let new_tick = uni_pool_profiler.state.current_tick;
    assert!(
        new_tick >= lower_tick,
        "Price should have crossed above or at tick {}, got {}",
        lower_tick,
        new_tick
    );

    // When crossing tick -22980 upward (cross_tick_up):
    // - liquidity_net at lower_tick is +40000 (positive, it's lower bound)
    // - We add it directly: +40000
    // - Active liquidity should increase by position_liquidity
    let final_liquidity = uni_pool_profiler.get_active_liquidity();
    assert_eq!(
        final_liquidity,
        initial_liquidity + position_liquidity,
        "Liquidity should increase when crossing into position range upward"
    );
    assert_eq!(
        uni_pool_profiler.get_active_liquidity(),
        uni_pool_profiler.get_total_liquidity_from_active_positions(),
        "Tick map liquidity should match sum of active positions after crossing"
    );

    // Verify the position is now active
    assert_eq!(uni_pool_profiler.get_total_active_positions(), 2);
    assert_eq!(uni_pool_profiler.get_total_inactive_positions(), 0);
    assert_eq!(uni_pool_profiler.analytics.total_mints, 2);
    assert_eq!(uni_pool_profiler.analytics.total_burns, 0);
    assert_eq!(uni_pool_profiler.analytics.total_swaps, 1);
    assert_eq!(uni_pool_profiler.analytics.total_fee_collects, 0);
}

// ----------- SWAP TESTING ----------
// https://github.com/Uniswap/v3-core/blob/main/test/UniswapV3Pool.swaps.spec.ts

#[derive(Debug, Clone)]
struct Position {
    tick_lower: i32,
    tick_upper: i32,
    liquidity: u128,
}

#[derive(Debug)]
struct PoolTestCase {
    tick_spacing: i32,
    fee_amount: u32,
    starting_price: U160,
    positions: Vec<Position>,
    tests: Vec<(SwapTestCase, ExpectedSwapResult)>,
}

#[derive(Debug)]
pub enum SwapTestCase {
    SwapExact0For1 {
        amount0: U256,
        sqrt_price_limit: Option<U160>,
    },
    SwapExact1For0 {
        amount1: U256,
        sqrt_price_limit: Option<U160>,
    },
    Swap0ForExact1 {
        amount1: U256,
        sqrt_price_limit: Option<U160>,
    },
    Swap1ForExact0 {
        amount0: U256,
        sqrt_price_limit: Option<U160>,
    },
    SwapToLowerPrice {
        sqrt_price_limit: U160,
    },
    SwapToHigherPrice {
        sqrt_price_limit: U160,
    },
}

#[derive(Debug)]
struct ExpectedSwapResult {
    amount0_before: U256,
    amount0_delta: I256,
    amount1_before: U256,
    amount1_delta: I256,
    pool_price_before: String,
    pool_price_after: String,
    tick_after: i32,
    tick_before: i32,
    fee_growth_global_0: U256,
    fee_growth_global_1: U256,
    execution_price: String,
}

fn execute_swap(pool_profiler: &mut PoolProfiler, test: SwapTestCase) -> anyhow::Result<PoolSwap> {
    match test {
        SwapTestCase::SwapExact0For1 {
            amount0,
            sqrt_price_limit,
        } => pool_profiler.swap_exact0_for_1(
            user_address(),
            user_address(),
            create_block_position(),
            amount0,
            sqrt_price_limit,
        ),
        SwapTestCase::SwapExact1For0 {
            amount1,
            sqrt_price_limit,
        } => pool_profiler.swap_exact1_for_0(
            user_address(),
            user_address(),
            create_block_position(),
            amount1,
            sqrt_price_limit,
        ),
        SwapTestCase::Swap0ForExact1 {
            amount1,
            sqrt_price_limit,
        } => pool_profiler.swap_0_for_exact1(
            user_address(),
            user_address(),
            create_block_position(),
            amount1,
            sqrt_price_limit,
        ),
        SwapTestCase::Swap1ForExact0 {
            amount0,
            sqrt_price_limit,
        } => pool_profiler.swap_1_for_exact0(
            user_address(),
            user_address(),
            create_block_position(),
            amount0,
            sqrt_price_limit,
        ),
        SwapTestCase::SwapToLowerPrice { sqrt_price_limit } => pool_profiler
            .swap_to_lower_sqrt_price(
                user_address(),
                user_address(),
                create_block_position(),
                sqrt_price_limit,
            ),
        SwapTestCase::SwapToHigherPrice { sqrt_price_limit } => pool_profiler
            .swap_to_higher_sqrt_price(
                user_address(),
                user_address(),
                create_block_position(),
                sqrt_price_limit,
            ),
    }
}

// Fee amount constants matching Uniswap V3
const FEE_HIGH: u32 = 10000;

// Tick spacing constants
const TICK_SPACING_HIGH: i32 = 200;

// Define test pool configurations

fn pool_high_fee_1on1_price_2e18_max_liquidity() -> PoolTestCase {
    PoolTestCase {
        tick_spacing: TICK_SPACING_HIGH,
        fee_amount: FEE_HIGH,
        starting_price: encode_sqrt_ratio_x96(1, 1),
        positions: vec![Position {
            tick_lower: PoolTick::get_min_tick(TICK_SPACING_HIGH),
            tick_upper: PoolTick::get_max_tick(TICK_SPACING_HIGH),
            liquidity: expand_to_18_decimals(2),
        }],
        tests: vec![
            (
                swap_exact_0_for_1_small_amount(),
                ExpectedSwapResult {
                    amount0_before: U256::from_str("2000000000000000000").unwrap(),
                    amount1_before: U256::from_str("2000000000000000000").unwrap(),
                    amount0_delta: I256::from_str("1000").unwrap(),
                    amount1_delta: I256::from_str("-989").unwrap(),
                    execution_price: "0.989".to_string(),
                    fee_growth_global_0: U256::from_str("1701411834604692317316").unwrap(),
                    fee_growth_global_1: U256::ZERO,
                    pool_price_before: "1.0000".to_string(),
                    pool_price_after: "1.0000".to_string(),
                    tick_before: 0,
                    tick_after: -1,
                },
            ),
            (
                swap_exact_1_for_0_small_amount(),
                ExpectedSwapResult {
                    amount0_before: U256::from_str("2000000000000000000").unwrap(),
                    amount1_before: U256::from_str("2000000000000000000").unwrap(),
                    amount0_delta: I256::from_str("-989").unwrap(),
                    amount1_delta: I256::from_str("1000").unwrap(),
                    execution_price: "1.01112".to_string(),
                    fee_growth_global_0: U256::ZERO,
                    fee_growth_global_1: U256::from_str("1701411834604692317316").unwrap(),
                    pool_price_before: "1.0000".to_string(),
                    pool_price_after: "1.0000".to_string(),
                    tick_before: 0,
                    tick_after: 0,
                },
            ),
            (
                swap_exact_0_for_1_1e18(),
                ExpectedSwapResult {
                    amount0_before: U256::from_str("2000000000000000000").unwrap(),
                    amount0_delta: I256::from_str("1000000000000000000").unwrap(),
                    amount1_before: U256::from_str("2000000000000000000").unwrap(),
                    amount1_delta: I256::from_str("-662207357859531772").unwrap(),
                    execution_price: "0.6622".to_string(),
                    fee_growth_global_0: U256::from_str("1701411834604692317316873037158841057")
                        .unwrap(),
                    fee_growth_global_1: U256::ZERO,
                    pool_price_before: "1.0000".to_string(),
                    pool_price_after: "0.4474".to_string(),
                    tick_before: 0,
                    tick_after: -8043,
                },
            ),
        ],
    }
}

// Swap test case helper functions

/// Swap exactly 1.0000 token0 for token1
fn swap_exact_0_for_1_1e18() -> SwapTestCase {
    SwapTestCase::SwapExact0For1 {
        amount0: U256::from(expand_to_18_decimals(1)),
        sqrt_price_limit: None,
    }
}

/// Swap exactly 0.0000000000000010000 token0 for token1
fn swap_exact_0_for_1_small_amount() -> SwapTestCase {
    SwapTestCase::SwapExact0For1 {
        amount0: U256::from(1000),
        sqrt_price_limit: None,
    }
}

/// Swap exactly 0.0000000000000010000 token1 for token0
fn swap_exact_1_for_0_small_amount() -> SwapTestCase {
    SwapTestCase::SwapExact1For0 {
        amount1: U256::from(1000),
        sqrt_price_limit: None,
    }
}

fn get_execution_price_string(amount0: I256, amount1: I256) -> String {
    // Convert to Decimal for precise division, mimicking JavaScript Decimal behavior
    let amount1_decimal = Decimal::from_str(&amount1.to_string()).unwrap();
    let amount0_decimal = Decimal::from_str(&amount0.to_string()).unwrap();
    let execution_price = amount1_decimal.div(amount0_decimal).mul(Decimal::from(-1));

    // Format to 5 significant digits to mimic toPrecision(5)
    format!("{:.5}", execution_price)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn format_price(sqrt_price_x96: U160) -> String {
    // Convert to U256 for calculations
    let sqrt_price = U256::from(sqrt_price_x96);

    // Square the price and divide by 2^192
    // (sqrtPrice / 2^96)^2 = sqrtPrice^2 / 2^192
    let price_squared = sqrt_price * sqrt_price;
    let divisor = U256::from(1u128) << 192;

    // Get integer and fractional parts
    let integer_part = price_squared / divisor;
    let remainder = price_squared % divisor;

    // Calculate 5 decimal places for rounding to 4
    let decimal_part = (remainder * U256::from(100000u64) + divisor / U256::from(2u64)) / divisor;

    // Round to 4 decimal places
    let rounded_decimal = decimal_part / U256::from(10u64);

    // Handle carry from rounding
    if rounded_decimal >= U256::from(10000u64) {
        format!("{}.0000", integer_part + U256::from(1u64))
    } else {
        format!("{}.{:04}", integer_part, rounded_decimal)
    }
}

fn test_pool_swaps(pool_test_case: PoolTestCase) {
    // Initialize the profiler
    let pool_definition = pool_definition(
        Some(pool_test_case.fee_amount),
        Some(pool_test_case.tick_spacing),
        Some(pool_test_case.starting_price),
    );
    let mut initial_profiler = PoolProfiler::new(Arc::new(pool_definition));
    initial_profiler.initialize(pool_test_case.starting_price);
    for mint in &pool_test_case.positions {
        initial_profiler
            .execute_mint(
                lp_address(),
                create_block_position(),
                mint.tick_lower,
                mint.tick_upper,
                mint.liquidity,
            )
            .unwrap();
    }

    for (swap, expected_result) in pool_test_case.tests {
        let mut profiler = initial_profiler.clone();

        let pool_balance0 = profiler.estimate_balance_of_token0();
        let pool_balance1 = profiler.estimate_balance_of_token1();
        let tick_before = profiler.state.current_tick;
        assert_eq!(pool_balance0, expected_result.amount0_before);
        assert_eq!(pool_balance1, expected_result.amount1_before);
        assert_eq!(tick_before, expected_result.tick_before);
        assert_eq!(
            format_price(profiler.state.price_sqrt_ratio_x96),
            expected_result.pool_price_before
        );

        // Execute swap and test
        match execute_swap(&mut profiler, swap) {
            Ok(swap_event) => {
                assert_eq!(swap_event.amount0, expected_result.amount0_delta);
                assert_eq!(swap_event.amount1, expected_result.amount1_delta);
                assert_eq!(profiler.state.current_tick, expected_result.tick_after);
                assert_eq!(
                    format_price(profiler.state.price_sqrt_ratio_x96),
                    expected_result.pool_price_after
                );
                assert_eq!(
                    profiler.state.fee_growth_global_0,
                    expected_result.fee_growth_global_0
                );
                assert_eq!(
                    profiler.state.fee_growth_global_1,
                    expected_result.fee_growth_global_1
                );
                assert_eq!(
                    get_execution_price_string(
                        expected_result.amount0_delta,
                        expected_result.amount1_delta
                    ),
                    expected_result.execution_price
                );
            }
            Err(_) => {
                panic!("Add error testing for failed swap")
            }
        }
    }
}

#[rstest]
fn test_swaps_for_pool_high_fee_1on1_price_2e18_max_liquidity() {
    test_pool_swaps(pool_high_fee_1on1_price_2e18_max_liquidity());
}
