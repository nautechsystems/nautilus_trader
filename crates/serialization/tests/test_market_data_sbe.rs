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

//! SBE serialization integration tests for market data types.

#![cfg(feature = "sbe")]
#![allow(
    clippy::unreadable_literal,
    reason = "wire-format fixture timestamps and IDs are easier to compare in raw form"
)]

use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, FundingRateUpdate, IndexPriceUpdate,
        InstrumentClose, InstrumentStatus, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas,
        OrderBookDepth10, QuoteTick, TradeTick,
        stubs::{
            stub_bar, stub_delta, stub_deltas, stub_depth10, stub_instrument_close,
            stub_instrument_status, stub_trade_ethusdt_buyer,
        },
    },
    enums::{
        AggregationSource, BarAggregation, BookAction, MarketStatusAction, OrderSide, PriceType,
    },
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_serialization::sbe::{DataAny, FromSbe, FromSbeReuse, SbeEncodeError, ToSbe};
use rstest::rstest;
use rust_decimal_macros::dec;
use ustr::Ustr;

macro_rules! sbe_roundtrip_test {
    ($name:ident, $value:expr, $ty:ty) => {
        #[rstest]
        fn $name() {
            let value: $ty = $value;
            let bytes = value.to_sbe().unwrap();
            let decoded = <$ty>::from_sbe(&bytes).unwrap();
            assert_eq!(value, decoded);
        }
    };
}

sbe_roundtrip_test!(test_quote_tick_roundtrip, QuoteTick::default(), QuoteTick);
sbe_roundtrip_test!(
    test_trade_tick_roundtrip,
    stub_trade_ethusdt_buyer(),
    TradeTick
);
sbe_roundtrip_test!(
    test_bar_type_roundtrip,
    BarType::new(
        InstrumentId::from("AAPL.XNAS"),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::Internal,
    ),
    BarType
);
sbe_roundtrip_test!(test_bar_roundtrip, stub_bar(), Bar);
sbe_roundtrip_test!(
    test_mark_price_update_roundtrip,
    sample_mark_price_update(),
    MarkPriceUpdate
);
sbe_roundtrip_test!(
    test_index_price_update_roundtrip,
    sample_index_price_update(),
    IndexPriceUpdate
);
sbe_roundtrip_test!(
    test_instrument_close_roundtrip,
    stub_instrument_close(),
    InstrumentClose
);

#[rstest]
fn test_book_order_roundtrip() {
    let value = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.50"),
        Quantity::from("10"),
        123_456,
    );

    let bytes = value.to_sbe().unwrap();
    let decoded = BookOrder::from_sbe(&bytes).unwrap();

    assert_book_order_fields(&value, &decoded);
}

#[rstest]
fn test_order_book_delta_roundtrip() {
    let value = stub_delta();

    let bytes = value.to_sbe().unwrap();
    let decoded = OrderBookDelta::from_sbe(&bytes).unwrap();

    assert_order_book_delta_fields(&value, &decoded);
}

#[rstest]
fn test_order_book_deltas_roundtrip() {
    let value = stub_deltas();

    let bytes = value.to_sbe().unwrap();
    let decoded = OrderBookDeltas::from_sbe(&bytes).unwrap();

    assert_order_book_deltas_fields(&value, &decoded);
}

#[rstest]
fn test_order_book_deltas_preserve_delta_instrument_ids() {
    let value = OrderBookDeltas::new(
        InstrumentId::from("AAPL.XNAS"),
        vec![
            OrderBookDelta::new(
                InstrumentId::from("AAPL.XNAS"),
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("100.00"),
                    Quantity::from("10"),
                    1,
                ),
                0,
                1,
                10.into(),
                11.into(),
            ),
            OrderBookDelta::new(
                InstrumentId::from("MSFT.XNAS"),
                BookAction::Update,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("101.00"),
                    Quantity::from("5"),
                    2,
                ),
                1,
                2,
                12.into(),
                13.into(),
            ),
        ],
    );

    let bytes = value.to_sbe().unwrap();
    let decoded = OrderBookDeltas::from_sbe(&bytes).unwrap();

    assert_order_book_deltas_fields(&value, &decoded);
}

#[rstest]
fn test_order_book_deltas_from_sbe_reuse_matches_from_sbe() {
    let value = stub_deltas();
    let bytes = value.to_sbe().unwrap();

    let mut scratch: Vec<OrderBookDelta> = Vec::new();
    let reused = OrderBookDeltas::from_sbe_reuse(&bytes, &mut scratch).unwrap();
    let plain = OrderBookDeltas::from_sbe(&bytes).unwrap();

    assert_order_book_deltas_fields(&reused, &plain);
    assert!(
        scratch.is_empty(),
        "scratch must be left empty after decode"
    );
}

#[rstest]
fn test_order_book_deltas_from_sbe_reuse_preserves_allocation() {
    let value = stub_deltas();
    let bytes = value.to_sbe().unwrap();

    let mut scratch: Vec<OrderBookDelta> = Vec::new();
    let first = OrderBookDeltas::from_sbe_reuse(&bytes, &mut scratch).unwrap();

    // Move the allocation back for the second decode; capacity should be reused.
    scratch = first.deltas;
    let cap_before = scratch.capacity();
    assert!(cap_before >= value.deltas.len());

    let second = OrderBookDeltas::from_sbe_reuse(&bytes, &mut scratch).unwrap();
    assert_eq!(second.deltas.capacity(), cap_before);
    assert_order_book_deltas_fields(&value, &second);
}

#[rstest]
fn test_order_book_depth10_roundtrip() {
    let value = stub_depth10();

    let bytes = value.to_sbe().unwrap();
    let decoded = OrderBookDepth10::from_sbe(&bytes).unwrap();

    assert_order_book_depth10_matches_capnp_parity(&value, &decoded);
}

#[rstest]
fn test_funding_rate_update_roundtrip() {
    let value = sample_funding_rate_update();

    let bytes = value.to_sbe().unwrap();
    let decoded = FundingRateUpdate::from_sbe(&bytes).unwrap();

    assert_funding_rate_update_fields(&value, &decoded);
}

#[rstest]
fn test_funding_rate_update_roundtrip_without_optional_fields() {
    let value = FundingRateUpdate::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        dec!(-0.00025),
        None,
        None,
        9876543210.into(),
        9876543211.into(),
    );

    let bytes = value.to_sbe().unwrap();
    let decoded = FundingRateUpdate::from_sbe(&bytes).unwrap();

    assert_funding_rate_update_fields(&value, &decoded);
}

#[rstest]
fn test_funding_rate_update_zero_values_roundtrip() {
    let value = FundingRateUpdate::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        dec!(0.0001),
        Some(0),
        Some(0.into()),
        9876543210.into(),
        9876543211.into(),
    );

    let bytes = value.to_sbe().unwrap();
    let decoded = FundingRateUpdate::from_sbe(&bytes).unwrap();

    assert_eq!(decoded.interval, Some(0));
    assert_eq!(decoded.next_funding_ns, Some(0.into()));
}

#[rstest]
fn test_instrument_status_roundtrip() {
    let value = stub_instrument_status();

    let bytes = value.to_sbe().unwrap();
    let decoded = InstrumentStatus::from_sbe(&bytes).unwrap();

    assert_instrument_status_matches_capnp_parity(&value, &decoded);
}

#[rstest]
fn test_instrument_status_with_no_optional_fields() {
    let value = InstrumentStatus {
        instrument_id: InstrumentId::from("TSLA.NASDAQ"),
        action: MarketStatusAction::Trading,
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
        reason: None,
        trading_event: None,
        is_trading: None,
        is_quoting: None,
        is_short_sell_restricted: None,
    };

    let bytes = value.to_sbe().unwrap();
    let decoded = InstrumentStatus::from_sbe(&bytes).unwrap();

    assert_instrument_status_matches_capnp_parity(&value, &decoded);
}

#[rstest]
fn test_instrument_status_with_empty_strings() {
    let value = InstrumentStatus {
        instrument_id: InstrumentId::from("MSFT.NASDAQ"),
        action: MarketStatusAction::PreOpen,
        ts_event: 5555555555.into(),
        ts_init: 5555555556.into(),
        reason: Some(Ustr::from("")),
        trading_event: Some(Ustr::from("")),
        is_trading: Some(true),
        is_quoting: Some(false),
        is_short_sell_restricted: Some(false),
    };

    let bytes = value.to_sbe().unwrap();
    let decoded = InstrumentStatus::from_sbe(&bytes).unwrap();

    assert_instrument_status_matches_capnp_parity(&value, &decoded);
}

#[rstest]
#[case(None, None, None)]
#[case(Some(true), Some(true), Some(true))]
#[case(Some(false), Some(false), Some(false))]
#[case(Some(true), None, Some(false))]
#[case(None, Some(false), None)]
fn test_instrument_status_optional_bool_roundtrip(
    #[case] is_trading: Option<bool>,
    #[case] is_quoting: Option<bool>,
    #[case] is_short_sell_restricted: Option<bool>,
) {
    let value = InstrumentStatus {
        instrument_id: InstrumentId::from("AAPL.NASDAQ"),
        action: MarketStatusAction::Trading,
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
        reason: None,
        trading_event: None,
        is_trading,
        is_quoting,
        is_short_sell_restricted,
    };

    let bytes = value.to_sbe().unwrap();
    let decoded = InstrumentStatus::from_sbe(&bytes).unwrap();

    assert_eq!(value, decoded);
}

#[rstest]
fn test_bar_type_composite_roundtrip_matches_capnp_parity() {
    let value = BarType::new_composite(
        InstrumentId::from("AAPL.XNAS"),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::Internal,
        5,
        BarAggregation::Minute,
        AggregationSource::External,
    );

    let bytes = value.to_sbe().unwrap();
    let decoded = BarType::from_sbe(&bytes).unwrap();

    assert_eq!(normalize_bar_type_capnp_parity(value), decoded);
}

#[rstest]
fn test_bar_with_composite_type_roundtrip_matches_capnp_parity() {
    let value = Bar::new(
        BarType::new_composite(
            InstrumentId::from("AAPL.XNAS"),
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::Internal,
            5,
            BarAggregation::Minute,
            AggregationSource::External,
        ),
        Price::from("150.00"),
        Price::from("151.00"),
        Price::from("149.00"),
        Price::from("150.50"),
        Quantity::from("1000"),
        123.into(),
        124.into(),
    );

    let bytes = value.to_sbe().unwrap();
    let decoded = Bar::from_sbe(&bytes).unwrap();

    assert_bar_matches_capnp_parity(&value, &decoded);
}

#[cfg(target_pointer_width = "64")]
#[rstest]
fn test_bar_type_step_overflow_returns_encode_error() {
    let value = BarType::new(
        InstrumentId::from("AAPL.XNAS"),
        BarSpecification::new(
            (u32::MAX as usize) + 1,
            BarAggregation::Minute,
            PriceType::Last,
        ),
        AggregationSource::Internal,
    );

    let err = value.to_sbe().unwrap_err();

    assert_eq!(
        err,
        SbeEncodeError::NumericOverflow {
            field: "BarSpecification.step",
        }
    );
}

#[rstest]
fn test_order_book_depth10_header_block_length_matches_fixed_body() {
    let value = stub_depth10();

    let bytes = value.to_sbe().unwrap();
    let block_length = u16::from_le_bytes([bytes[0], bytes[1]]);

    assert_eq!(block_length, 785);
}

#[rstest]
fn test_data_any_quote_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(QuoteTick::default()));
}

#[rstest]
fn test_data_any_trade_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(stub_trade_ethusdt_buyer()));
}

#[rstest]
fn test_data_any_bar_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(stub_bar()));
}

#[rstest]
fn test_data_any_mark_price_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(sample_mark_price_update()));
}

#[rstest]
fn test_data_any_index_price_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(sample_index_price_update()));
}

#[rstest]
fn test_data_any_instrument_close_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(stub_instrument_close()));
}

#[rstest]
fn test_data_any_instrument_status_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(stub_instrument_status()));
}

#[rstest]
fn test_data_any_funding_rate_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(sample_funding_rate_update()));
}

#[rstest]
fn test_data_any_order_book_delta_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(stub_delta()));
}

#[rstest]
fn test_data_any_order_book_deltas_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(stub_deltas()));
}

#[rstest]
fn test_data_any_order_book_depth10_roundtrip() {
    assert_data_any_roundtrip_matches_capnp_parity(DataAny::from(stub_depth10()));
}

#[rstest]
fn test_to_sbe_into_reuses_buffer_and_clears_previous_bytes() {
    let larger = DataAny::from(stub_deltas());
    let smaller = DataAny::from(QuoteTick::default());
    let mut buf = Vec::new();

    larger.to_sbe_into(&mut buf).unwrap();
    let reused_capacity = buf.capacity();

    smaller.to_sbe_into(&mut buf).unwrap();

    assert_eq!(buf, smaller.to_sbe().unwrap());
    assert_eq!(buf.capacity(), reused_capacity);
}

fn sample_mark_price_update() -> MarkPriceUpdate {
    MarkPriceUpdate::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        Price::from("50000.50"),
        1234567890.into(),
        1234567891.into(),
    )
}

fn sample_index_price_update() -> IndexPriceUpdate {
    IndexPriceUpdate::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        Price::from("50125.75"),
        1234567890.into(),
        1234567891.into(),
    )
}

fn sample_funding_rate_update() -> FundingRateUpdate {
    FundingRateUpdate::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        dec!(0.0001),
        Some(60),
        Some(1234567890.into()),
        1234567890.into(),
        1234567891.into(),
    )
}

fn assert_book_order_fields(expected: &BookOrder, actual: &BookOrder) {
    assert_eq!(expected.side, actual.side);
    assert_eq!(expected.price, actual.price);
    assert_eq!(expected.size, actual.size);
    assert_eq!(expected.order_id, actual.order_id);
}

fn assert_order_book_delta_fields(expected: &OrderBookDelta, actual: &OrderBookDelta) {
    assert_eq!(expected.instrument_id, actual.instrument_id);
    assert_eq!(expected.action, actual.action);
    assert_book_order_fields(&expected.order, &actual.order);
    assert_eq!(expected.flags, actual.flags);
    assert_eq!(expected.sequence, actual.sequence);
    assert_eq!(expected.ts_event, actual.ts_event);
    assert_eq!(expected.ts_init, actual.ts_init);
}

fn assert_order_book_deltas_fields(expected: &OrderBookDeltas, actual: &OrderBookDeltas) {
    assert_eq!(expected.instrument_id, actual.instrument_id);
    assert_eq!(expected.flags, actual.flags);
    assert_eq!(expected.sequence, actual.sequence);
    assert_eq!(expected.ts_event, actual.ts_event);
    assert_eq!(expected.ts_init, actual.ts_init);
    assert_eq!(expected.deltas.len(), actual.deltas.len());

    for (expected_delta, actual_delta) in expected.deltas.iter().zip(&actual.deltas) {
        assert_order_book_delta_fields(expected_delta, actual_delta);
    }
}

fn assert_order_book_depth10_matches_capnp_parity(
    expected: &OrderBookDepth10,
    actual: &OrderBookDepth10,
) {
    let expected = normalize_depth10_capnp_parity(*expected);

    assert_eq!(expected.instrument_id, actual.instrument_id);
    assert_eq!(expected.bid_counts, actual.bid_counts);
    assert_eq!(expected.ask_counts, actual.ask_counts);
    assert_eq!(expected.flags, actual.flags);
    assert_eq!(expected.sequence, actual.sequence);
    assert_eq!(expected.ts_event, actual.ts_event);
    assert_eq!(expected.ts_init, actual.ts_init);

    for (expected_bid, actual_bid) in expected.bids.iter().zip(&actual.bids) {
        assert_book_order_fields(expected_bid, actual_bid);
    }

    for (expected_ask, actual_ask) in expected.asks.iter().zip(&actual.asks) {
        assert_book_order_fields(expected_ask, actual_ask);
    }
}

fn assert_funding_rate_update_fields(expected: &FundingRateUpdate, actual: &FundingRateUpdate) {
    assert_eq!(expected.instrument_id, actual.instrument_id);
    assert_eq!(expected.rate, actual.rate);
    assert_eq!(expected.interval, actual.interval);
    assert_eq!(expected.next_funding_ns, actual.next_funding_ns);
    assert_eq!(expected.ts_event, actual.ts_event);
    assert_eq!(expected.ts_init, actual.ts_init);
}

fn assert_instrument_status_matches_capnp_parity(
    expected: &InstrumentStatus,
    actual: &InstrumentStatus,
) {
    assert_eq!(normalize_instrument_status_capnp_parity(*expected), *actual);
}

fn assert_bar_matches_capnp_parity(expected: &Bar, actual: &Bar) {
    let expected = normalize_bar_capnp_parity(*expected);

    assert_eq!(expected.bar_type, actual.bar_type);
    assert_eq!(expected.open, actual.open);
    assert_eq!(expected.high, actual.high);
    assert_eq!(expected.low, actual.low);
    assert_eq!(expected.close, actual.close);
    assert_eq!(expected.volume, actual.volume);
    assert_eq!(expected.ts_event, actual.ts_event);
    assert_eq!(expected.ts_init, actual.ts_init);
}

fn assert_data_any_roundtrip_matches_capnp_parity(value: DataAny) {
    let bytes = value.to_sbe().unwrap();
    let decoded = DataAny::from_sbe(&bytes).unwrap();

    match (value, decoded) {
        (DataAny::Quote(expected), DataAny::Quote(actual)) => assert_eq!(expected, actual),
        (DataAny::Trade(expected), DataAny::Trade(actual)) => assert_eq!(expected, actual),
        (DataAny::Bar(expected), DataAny::Bar(actual)) => {
            assert_bar_matches_capnp_parity(&expected, &actual);
        }
        (DataAny::MarkPrice(expected), DataAny::MarkPrice(actual)) => {
            assert_eq!(expected, actual);
        }
        (DataAny::IndexPrice(expected), DataAny::IndexPrice(actual)) => {
            assert_eq!(expected, actual);
        }
        (DataAny::InstrumentClose(expected), DataAny::InstrumentClose(actual)) => {
            assert_eq!(expected, actual);
        }
        (DataAny::InstrumentStatus(expected), DataAny::InstrumentStatus(actual)) => {
            assert_instrument_status_matches_capnp_parity(&expected, &actual);
        }
        (DataAny::FundingRate(expected), DataAny::FundingRate(actual)) => {
            assert_funding_rate_update_fields(&expected, &actual);
        }
        (DataAny::OrderBookDelta(expected), DataAny::OrderBookDelta(actual)) => {
            assert_order_book_delta_fields(&expected, &actual);
        }
        (DataAny::OrderBookDeltas(expected), DataAny::OrderBookDeltas(actual)) => {
            assert_order_book_deltas_fields(&expected, &actual);
        }
        (DataAny::OrderBookDepth10(expected), DataAny::OrderBookDepth10(actual)) => {
            assert_order_book_depth10_matches_capnp_parity(&expected, &actual);
        }
        (expected, actual) => {
            panic!("DataAny variant mismatch: expected {expected:?}, was {actual:?}");
        }
    }
}

fn normalize_instrument_status_capnp_parity(status: InstrumentStatus) -> InstrumentStatus {
    status
}

fn normalize_bar_type_capnp_parity(bar_type: BarType) -> BarType {
    BarType::new(
        bar_type.instrument_id(),
        bar_type.spec(),
        bar_type.aggregation_source(),
    )
}

fn normalize_bar_capnp_parity(mut bar: Bar) -> Bar {
    bar.bar_type = normalize_bar_type_capnp_parity(bar.bar_type);
    bar
}

fn normalize_depth10_capnp_parity(mut depth: OrderBookDepth10) -> OrderBookDepth10 {
    for bid in &mut depth.bids {
        bid.order_id = 0;
    }

    for ask in &mut depth.asks {
        ask.order_id = 0;
    }

    depth
}
