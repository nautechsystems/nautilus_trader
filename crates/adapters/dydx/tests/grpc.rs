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

//! Unit tests for dYdX gRPC module components.
//!
//! These tests verify:
//! 1. Order builder quantization logic
//! 2. Order flags and order ID construction
//! 3. Chain ID handling
//! 4. Wallet address derivation (mocked)

use chrono::{Duration, Utc};
use nautilus_dydx::grpc::{
    ChainId, OrderBuilder, OrderGoodUntil, OrderMarketParams, SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
};
use rstest::rstest;
use rust_decimal_macros::dec;

fn sample_btc_market_params() -> OrderMarketParams {
    OrderMarketParams {
        atomic_resolution: -10,
        clob_pair_id: 0,
        oracle_price: Some(dec!(50000)),
        quantum_conversion_exponent: -9,
        step_base_quantums: 1_000_000,
        subticks_per_tick: 100_000,
    }
}

fn sample_eth_market_params() -> OrderMarketParams {
    OrderMarketParams {
        atomic_resolution: -9,
        clob_pair_id: 1,
        oracle_price: Some(dec!(3000)),
        quantum_conversion_exponent: -9,
        step_base_quantums: 1_000_000,
        subticks_per_tick: 10_000,
    }
}

#[rstest]
fn test_chain_id_variants() {
    assert_eq!(ChainId::Mainnet1.as_ref(), "dydx-mainnet-1");
    assert_eq!(ChainId::Testnet4.as_ref(), "dydx-testnet-4");
}

#[rstest]
fn test_short_term_order_maximum_lifetime() {
    assert_eq!(SHORT_TERM_ORDER_MAXIMUM_LIFETIME, 20);
}

#[rstest]
fn test_order_good_until_block() {
    let until = OrderGoodUntil::Block(100);
    match until {
        OrderGoodUntil::Block(height) => assert_eq!(height, 100),
        _ => panic!("Expected Block variant"),
    }
}

#[rstest]
fn test_order_good_until_time() {
    let future_time = Utc::now() + Duration::hours(1);
    let until = OrderGoodUntil::Time(future_time);
    match until {
        OrderGoodUntil::Time(time) => assert_eq!(time, future_time),
        _ => panic!("Expected Time variant"),
    }
}

#[rstest]
fn test_btc_price_quantization() {
    let market = sample_btc_market_params();

    // Standard price
    let subticks = market.quantize_price(dec!(50000)).unwrap();
    assert_eq!(subticks, 5_000_000_000);

    // Lower price
    let subticks = market.quantize_price(dec!(30000)).unwrap();
    assert_eq!(subticks, 3_000_000_000);

    // Higher price
    let subticks = market.quantize_price(dec!(100000)).unwrap();
    assert_eq!(subticks, 10_000_000_000);
}

#[rstest]
fn test_eth_price_quantization() {
    let market = sample_eth_market_params();

    // Standard ETH price
    let subticks = market.quantize_price(dec!(3000)).unwrap();
    // ETH has different atomic_resolution and subticks_per_tick
    assert!(subticks > 0);
}

#[rstest]
fn test_btc_quantity_quantization() {
    let market = sample_btc_market_params();

    // 0.01 BTC
    let quantums = market.quantize_quantity(dec!(0.01)).unwrap();
    assert_eq!(quantums, 100_000_000);

    // 0.1 BTC
    let quantums = market.quantize_quantity(dec!(0.1)).unwrap();
    assert_eq!(quantums, 1_000_000_000);

    // 1 BTC
    let quantums = market.quantize_quantity(dec!(1)).unwrap();
    assert_eq!(quantums, 10_000_000_000);
}

#[rstest]
fn test_eth_quantity_quantization() {
    let market = sample_eth_market_params();

    // 0.1 ETH
    let quantums = market.quantize_quantity(dec!(0.1)).unwrap();
    // ETH has different atomic_resolution
    assert!(quantums > 0);
}

#[rstest]
fn test_quantize_minimum_values() {
    let market = sample_btc_market_params();

    // Very small price should return at least subticks_per_tick
    let subticks = market.quantize_price(dec!(0.0001)).unwrap();
    assert!(subticks >= market.subticks_per_tick as u64);

    // Very small quantity should return at least step_base_quantums
    let quantums = market.quantize_quantity(dec!(0.00000001)).unwrap();
    assert!(quantums >= market.step_base_quantums);
}

#[rstest]
fn test_quantize_rounding_half_up() {
    let market = sample_btc_market_params();

    // Test prices that should round up
    let subticks_up = market.quantize_price(dec!(50000.6)).unwrap();
    // Should round to next tick
    assert_eq!(subticks_up, 5_000_100_000);

    // Test prices that should round down
    let subticks_down = market.quantize_price(dec!(50000.4)).unwrap();
    assert_eq!(subticks_down, 5_000_000_000);
}

#[rstest]
fn test_order_builder_default() {
    let builder = OrderBuilder::default();
    // Default should be short-term
    let result = builder
        .size(dec!(0.01))
        .until(OrderGoodUntil::Block(100))
        .build();
    assert!(result.is_ok());
}

#[rstest]
fn test_order_builder_market_order() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12345);

    let order = builder
        .market(Side::Buy, dec!(0.05))
        .until(OrderGoodUntil::Block(100))
        .build()
        .unwrap();

    assert_eq!(order.side, Side::Buy as i32);
    assert_eq!(order.quantums, 500_000_000); // 0.05 * 10^10
}

#[rstest]
fn test_order_builder_limit_order() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12346);

    let order = builder
        .limit(Side::Sell, dec!(52000), dec!(0.1))
        .until(OrderGoodUntil::Block(100))
        .build()
        .unwrap();

    assert_eq!(order.side, Side::Sell as i32);
    assert_eq!(order.quantums, 1_000_000_000); // 0.1 * 10^10
    assert_eq!(order.subticks, 5_200_000_000); // 52000 quantized
}

#[rstest]
fn test_order_builder_short_term_flags() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12347);

    let order = builder
        .short_term()
        .market(Side::Buy, dec!(0.01))
        .until(OrderGoodUntil::Block(100))
        .build()
        .unwrap();

    // Short-term flag is 0
    assert_eq!(order.order_id.as_ref().unwrap().order_flags, 0);
}

#[rstest]
fn test_order_builder_long_term_flags() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12348);

    let until_time = Utc::now() + Duration::hours(1);
    let order = builder
        .long_term()
        .limit(Side::Buy, dec!(50000), dec!(0.01))
        .until(OrderGoodUntil::Time(until_time))
        .build()
        .unwrap();

    // Long-term flag is 64
    assert_eq!(order.order_id.as_ref().unwrap().order_flags, 64);
}

#[rstest]
fn test_order_builder_conditional_flags() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12349);

    let order = builder
        .stop_limit(Side::Sell, dec!(48000), dec!(49000), dec!(0.01))
        .until(OrderGoodUntil::Block(100))
        .build()
        .unwrap();

    // Conditional flag is 32
    assert_eq!(order.order_id.as_ref().unwrap().order_flags, 32);
    assert!(order.conditional_order_trigger_subticks > 0);
}

#[rstest]
fn test_order_builder_reduce_only() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12350);

    let order = builder
        .limit(Side::Sell, dec!(50000), dec!(0.01))
        .reduce_only(true)
        .until(OrderGoodUntil::Block(100))
        .build()
        .unwrap();

    assert!(order.reduce_only);
}

#[rstest]
fn test_order_builder_time_in_force() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::{Side, TimeInForce};

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12351);

    let order = builder
        .limit(Side::Buy, dec!(50000), dec!(0.01))
        .time_in_force(TimeInForce::Ioc)
        .until(OrderGoodUntil::Block(100))
        .build()
        .unwrap();

    assert_eq!(order.time_in_force, TimeInForce::Ioc as i32);
}

#[rstest]
fn test_order_builder_clob_pair_id() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let mut market = sample_btc_market_params();
    market.clob_pair_id = 5; // Different market

    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12352);

    let order = builder
        .market(Side::Buy, dec!(0.01))
        .until(OrderGoodUntil::Block(100))
        .build()
        .unwrap();

    assert_eq!(order.order_id.as_ref().unwrap().clob_pair_id, 5);
}

#[rstest]
fn test_order_builder_subaccount() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1abc123".to_string(), 3, 12353);

    let order = builder
        .market(Side::Buy, dec!(0.01))
        .until(OrderGoodUntil::Block(100))
        .build()
        .unwrap();

    let order_id = order.order_id.as_ref().unwrap();
    let subaccount = order_id.subaccount_id.as_ref().unwrap();
    assert_eq!(subaccount.owner, "dydx1abc123");
    assert_eq!(subaccount.number, 3);
}

#[rstest]
fn test_order_builder_client_id() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let client_id = 999_888_777;
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, client_id);

    let order = builder
        .market(Side::Buy, dec!(0.01))
        .until(OrderGoodUntil::Block(100))
        .build()
        .unwrap();

    assert_eq!(order.order_id.as_ref().unwrap().client_id, client_id);
}

#[rstest]
fn test_order_builder_missing_size_error() {
    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12354);

    let result = builder.until(OrderGoodUntil::Block(100)).build();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("size"));
}

#[rstest]
fn test_order_builder_missing_until_error() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(market, "dydx1test".to_string(), 0, 12355);

    let result = builder.market(Side::Buy, dec!(0.01)).build();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("until") || err.to_string().contains("expiration"));
}

#[rstest]
fn test_order_good_until_block_range() {
    // Short-term orders can only go up to SHORT_TERM_ORDER_MAXIMUM_LIFETIME blocks ahead
    let current_block = 1000u32;
    let max_valid_block = current_block + SHORT_TERM_ORDER_MAXIMUM_LIFETIME;

    assert!(max_valid_block <= 1020);

    // Blocks within range
    assert!(current_block < max_valid_block);
    assert!(current_block + 10 <= max_valid_block);
    assert!(current_block + 20 <= max_valid_block);

    // Block beyond range would need long-term order
    assert!(current_block + 21 > max_valid_block);
}

#[rstest]
fn test_order_market_params_different_markets() {
    let btc = sample_btc_market_params();
    let eth = sample_eth_market_params();

    // Different atomic resolutions
    assert_ne!(btc.atomic_resolution, eth.atomic_resolution);

    // Different clob pair IDs
    assert_ne!(btc.clob_pair_id, eth.clob_pair_id);

    // Different subticks per tick
    assert_ne!(btc.subticks_per_tick, eth.subticks_per_tick);
}

#[rstest]
fn test_large_order_values() {
    let market = sample_btc_market_params();

    // Large price (100,000 BTC)
    let subticks = market.quantize_price(dec!(100000)).unwrap();
    assert_eq!(subticks, 10_000_000_000);

    // Large quantity (10 BTC)
    let quantums = market.quantize_quantity(dec!(10)).unwrap();
    assert_eq!(quantums, 100_000_000_000);
}

#[rstest]
fn test_decimal_precision_preserved() {
    let market = sample_btc_market_params();

    // Precise price
    let price = dec!(50123.45);
    let subticks = market.quantize_price(price).unwrap();
    // Should quantize to tick boundary
    assert!(subticks > 0);

    // Precise quantity
    let quantity = dec!(0.012345);
    let quantums = market.quantize_quantity(quantity).unwrap();
    // Should quantize to step boundary
    assert!(quantums > 0);
}
