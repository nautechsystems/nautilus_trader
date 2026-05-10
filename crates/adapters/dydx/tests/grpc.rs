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

//! Unit tests for dYdX gRPC module components.

use std::collections::HashMap;

use chrono::{Duration, Utc};
use nautilus_dydx::{
    grpc::{
        ChainId, DEFAULT_RUST_CLIENT_METADATA, OrderBuilder, OrderGoodUntil, OrderMarketParams,
        SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
    },
    proto::dydxprotocol::{
        clob::{
            MsgBatchCancel, MsgCancelOrder, OrderBatch, OrderId, msg_cancel_order::GoodTilOneof,
        },
        subaccounts::SubaccountId,
    },
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
    assert_eq!(SHORT_TERM_ORDER_MAXIMUM_LIFETIME, 40);
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
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12345,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12346,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12347,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12348,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12349,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12350,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12351,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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

    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12352,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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
    let builder = OrderBuilder::new(
        market,
        "dydx1abc123".to_string(),
        3,
        12353,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        client_id,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12354,
        DEFAULT_RUST_CLIENT_METADATA,
    );

    let result = builder.until(OrderGoodUntil::Block(100)).build();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("size"));
}

#[rstest]
fn test_order_builder_missing_until_error() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        12355,
        DEFAULT_RUST_CLIENT_METADATA,
    );

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

    assert!(max_valid_block <= 1040);

    // Blocks within range
    assert!(current_block < max_valid_block);
    assert!(current_block + 10 <= max_valid_block);
    assert!(current_block + 40 <= max_valid_block);

    // Block beyond range would need long-term order
    assert!(current_block + 41 > max_valid_block);
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

fn sample_subaccount(owner: &str, number: u32) -> SubaccountId {
    SubaccountId {
        owner: owner.to_string(),
        number,
    }
}

#[rstest]
fn test_cancel_order_short_term_message_construction() {
    let client_id = 42u32;
    let clob_pair_id = 0u32;
    let block_height = 1000u32;
    let good_til_block = block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME;

    let msg = MsgCancelOrder {
        order_id: Some(OrderId {
            subaccount_id: Some(sample_subaccount("dydx1test", 0)),
            client_id,
            order_flags: 0, // short-term
            clob_pair_id,
        }),
        good_til_oneof: Some(GoodTilOneof::GoodTilBlock(good_til_block)),
    };

    let order_id = msg.order_id.as_ref().unwrap();
    assert_eq!(order_id.client_id, 42);
    assert_eq!(order_id.order_flags, 0);
    assert_eq!(order_id.clob_pair_id, 0);

    let subaccount = order_id.subaccount_id.as_ref().unwrap();
    assert_eq!(subaccount.owner, "dydx1test");
    assert_eq!(subaccount.number, 0);

    match msg.good_til_oneof.unwrap() {
        GoodTilOneof::GoodTilBlock(block) => {
            assert_eq!(block, 1040);
        }
        _ => panic!("Expected GoodTilBlock for short-term cancel"),
    }
}

#[rstest]
fn test_cancel_order_long_term_message_construction() {
    let client_id = 100u32;
    let clob_pair_id = 1u32;
    let cancel_good_til = (Utc::now() + Duration::days(90)).timestamp() as u32;

    let msg = MsgCancelOrder {
        order_id: Some(OrderId {
            subaccount_id: Some(sample_subaccount("dydx1wallet", 2)),
            client_id,
            order_flags: 64, // long-term
            clob_pair_id,
        }),
        good_til_oneof: Some(GoodTilOneof::GoodTilBlockTime(cancel_good_til)),
    };

    let order_id = msg.order_id.as_ref().unwrap();
    assert_eq!(order_id.client_id, 100);
    assert_eq!(order_id.order_flags, 64);
    assert_eq!(order_id.clob_pair_id, 1);

    let subaccount = order_id.subaccount_id.as_ref().unwrap();
    assert_eq!(subaccount.owner, "dydx1wallet");
    assert_eq!(subaccount.number, 2);

    match msg.good_til_oneof.unwrap() {
        GoodTilOneof::GoodTilBlockTime(ts) => {
            assert_eq!(ts, cancel_good_til);
            // Should be roughly 90 days from now
            let now = Utc::now().timestamp() as u32;
            let eighty_nine_days = 89 * 24 * 3600;
            assert!(ts > now + eighty_nine_days);
        }
        _ => panic!("Expected GoodTilBlockTime for long-term cancel"),
    }
}

#[rstest]
fn test_cancel_order_conditional_message_construction() {
    let client_id = 200u32;
    let clob_pair_id = 0u32;
    let cancel_good_til = (Utc::now() + Duration::days(90)).timestamp() as u32;

    let msg = MsgCancelOrder {
        order_id: Some(OrderId {
            subaccount_id: Some(sample_subaccount("dydx1test", 0)),
            client_id,
            order_flags: 32, // conditional
            clob_pair_id,
        }),
        good_til_oneof: Some(GoodTilOneof::GoodTilBlockTime(cancel_good_til)),
    };

    let order_id = msg.order_id.as_ref().unwrap();
    assert_eq!(order_id.order_flags, 32);

    match msg.good_til_oneof.unwrap() {
        GoodTilOneof::GoodTilBlockTime(_) => {}
        _ => panic!("Expected GoodTilBlockTime for conditional cancel"),
    }
}

#[rstest]
fn test_batch_cancel_short_term_single_market() {
    let block_height = 500u32;
    let good_til_block = block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME;

    let msg = MsgBatchCancel {
        subaccount_id: Some(sample_subaccount("dydx1test", 0)),
        short_term_cancels: vec![OrderBatch {
            clob_pair_id: 0,
            client_ids: vec![10, 20, 30],
        }],
        good_til_block,
    };

    let subaccount = msg.subaccount_id.as_ref().unwrap();
    assert_eq!(subaccount.owner, "dydx1test");
    assert_eq!(subaccount.number, 0);

    assert_eq!(msg.short_term_cancels.len(), 1);
    assert_eq!(msg.short_term_cancels[0].clob_pair_id, 0);
    assert_eq!(msg.short_term_cancels[0].client_ids, vec![10, 20, 30]);
    assert_eq!(msg.good_til_block, 540);
}

#[rstest]
fn test_batch_cancel_short_term_multiple_markets() {
    let block_height = 1000u32;
    let good_til_block = block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME;

    // Simulate grouping orders by clob_pair_id (the logic in build_batch_cancel_short_term)
    let orders: Vec<(u32, u32)> = vec![
        (0, 10), // BTC market, client_id=10
        (1, 20), // ETH market, client_id=20
        (0, 30), // BTC market, client_id=30
        (1, 40), // ETH market, client_id=40
        (0, 50), // BTC market, client_id=50
    ];

    let mut clob_groups: HashMap<u32, Vec<u32>> = HashMap::new();
    for (clob_pair_id, client_id) in &orders {
        clob_groups
            .entry(*clob_pair_id)
            .or_default()
            .push(*client_id);
    }

    let short_term_cancels: Vec<OrderBatch> = clob_groups
        .into_iter()
        .map(|(clob_pair_id, client_ids)| OrderBatch {
            clob_pair_id,
            client_ids,
        })
        .collect();

    let msg = MsgBatchCancel {
        subaccount_id: Some(sample_subaccount("dydx1test", 0)),
        short_term_cancels,
        good_til_block,
    };

    assert_eq!(msg.short_term_cancels.len(), 2);
    assert_eq!(msg.good_til_block, 1040);

    // Find BTC batch (clob_pair_id=0)
    let btc_batch = msg
        .short_term_cancels
        .iter()
        .find(|b| b.clob_pair_id == 0)
        .unwrap();
    assert_eq!(btc_batch.client_ids.len(), 3);
    assert!(btc_batch.client_ids.contains(&10));
    assert!(btc_batch.client_ids.contains(&30));
    assert!(btc_batch.client_ids.contains(&50));

    // Find ETH batch (clob_pair_id=1)
    let eth_batch = msg
        .short_term_cancels
        .iter()
        .find(|b| b.clob_pair_id == 1)
        .unwrap();
    assert_eq!(eth_batch.client_ids.len(), 2);
    assert!(eth_batch.client_ids.contains(&20));
    assert!(eth_batch.client_ids.contains(&40));
}

#[rstest]
fn test_batch_cancel_partitioning_by_order_lifetime() {
    use nautilus_dydx::execution::types::{
        ORDER_FLAG_CONDITIONAL, ORDER_FLAG_LONG_TERM, ORDER_FLAG_SHORT_TERM,
    };

    // Simulate the partitioning logic from batch_cancel_orders:
    // orders are (instrument_id, client_id, order_flags)
    let orders: Vec<(u32, u32)> = vec![
        (0, 10),  // short-term, clob_pair_id=0
        (64, 20), // long-term
        (0, 30),  // short-term, clob_pair_id=0
        (32, 40), // conditional
        (0, 50),  // short-term, clob_pair_id=1
    ];

    #[expect(clippy::type_complexity)]
    let (short_term, stateful): (Vec<&(u32, u32)>, Vec<&(u32, u32)>) = orders
        .iter()
        .partition(|(flags, _)| *flags == ORDER_FLAG_SHORT_TERM);

    assert_eq!(short_term.len(), 3, "Three short-term orders");
    assert_eq!(
        stateful.len(),
        2,
        "Two stateful orders (long-term + conditional)"
    );

    // Verify flag constants match protocol values
    assert_eq!(ORDER_FLAG_SHORT_TERM, 0);
    assert_eq!(ORDER_FLAG_LONG_TERM, 64);
    assert_eq!(ORDER_FLAG_CONDITIONAL, 32);
}

#[rstest]
fn test_take_profit_market_buy_order() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::{ConditionType, Side};

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        20001,
        DEFAULT_RUST_CLIENT_METADATA,
    );

    let until_time = Utc::now() + Duration::hours(1);
    let order = builder
        .take_profit_market(Side::Buy, dec!(48000), dec!(0.01))
        .until(OrderGoodUntil::Time(until_time))
        .build()
        .unwrap();

    assert_eq!(order.order_id.as_ref().unwrap().order_flags, 32);
    assert_eq!(order.condition_type, ConditionType::TakeProfit as i32);
    assert!(order.conditional_order_trigger_subticks > 0);
    assert_eq!(order.side, Side::Buy as i32);
}

#[rstest]
fn test_take_profit_market_sell_order() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::{ConditionType, Side};

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        20002,
        DEFAULT_RUST_CLIENT_METADATA,
    );

    let until_time = Utc::now() + Duration::hours(1);
    let order = builder
        .take_profit_market(Side::Sell, dec!(52000), dec!(0.01))
        .until(OrderGoodUntil::Time(until_time))
        .build()
        .unwrap();

    assert_eq!(order.order_id.as_ref().unwrap().order_flags, 32);
    assert_eq!(order.condition_type, ConditionType::TakeProfit as i32);
    assert!(order.conditional_order_trigger_subticks > 0);
    assert_eq!(order.side, Side::Sell as i32);
}

#[rstest]
fn test_take_profit_limit_buy_order() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::{ConditionType, Side};

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        20003,
        DEFAULT_RUST_CLIENT_METADATA,
    );

    let until_time = Utc::now() + Duration::hours(1);
    let order = builder
        .take_profit_limit(Side::Buy, dec!(47500), dec!(48000), dec!(0.01))
        .until(OrderGoodUntil::Time(until_time))
        .build()
        .unwrap();

    assert_eq!(order.order_id.as_ref().unwrap().order_flags, 32);
    assert_eq!(order.condition_type, ConditionType::TakeProfit as i32);
    assert!(order.conditional_order_trigger_subticks > 0);
    assert!(order.subticks > 0, "Limit price should produce subticks");
    assert_eq!(order.side, Side::Buy as i32);
}

#[rstest]
fn test_take_profit_limit_sell_order() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::{ConditionType, Side};

    let market = sample_btc_market_params();
    let builder = OrderBuilder::new(
        market,
        "dydx1test".to_string(),
        0,
        20004,
        DEFAULT_RUST_CLIENT_METADATA,
    );

    let until_time = Utc::now() + Duration::hours(1);
    let order = builder
        .take_profit_limit(Side::Sell, dec!(52500), dec!(52000), dec!(0.01))
        .until(OrderGoodUntil::Time(until_time))
        .build()
        .unwrap();

    assert_eq!(order.order_id.as_ref().unwrap().order_flags, 32);
    assert_eq!(order.condition_type, ConditionType::TakeProfit as i32);
    assert!(order.conditional_order_trigger_subticks > 0);
    assert!(order.subticks > 0, "Limit price should produce subticks");
    assert_eq!(order.side, Side::Sell as i32);
}

#[rstest]
fn test_take_profit_market_quantization() {
    use nautilus_dydx::proto::dydxprotocol::clob::order::Side;

    let btc_market = sample_btc_market_params();
    let eth_market = sample_eth_market_params();

    let until_time = Utc::now() + Duration::hours(1);

    let btc_order = OrderBuilder::new(
        btc_market,
        "dydx1test".to_string(),
        0,
        20005,
        DEFAULT_RUST_CLIENT_METADATA,
    )
    .take_profit_market(Side::Sell, dec!(55000), dec!(0.05))
    .until(OrderGoodUntil::Time(until_time))
    .build()
    .unwrap();

    let eth_order = OrderBuilder::new(
        eth_market,
        "dydx1test".to_string(),
        0,
        20006,
        DEFAULT_RUST_CLIENT_METADATA,
    )
    .take_profit_market(Side::Sell, dec!(3500), dec!(0.1))
    .until(OrderGoodUntil::Time(until_time))
    .build()
    .unwrap();

    assert!(btc_order.conditional_order_trigger_subticks > 0);
    assert!(eth_order.conditional_order_trigger_subticks > 0);
    assert_ne!(
        btc_order.conditional_order_trigger_subticks, eth_order.conditional_order_trigger_subticks,
        "Different markets should produce different subtick values"
    );
}
