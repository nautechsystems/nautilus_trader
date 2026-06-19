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

use std::{
    ffi::c_char,
    path::{Path, PathBuf},
};

use databento::dbn::{
    self,
    decode::{DecodeStream, dbn::Decoder},
};
use fallible_streaming_iterator::FallibleStreamingIterator;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BarType, BookOrder, DEPTH10_LEN, Data},
    enums::{
        AggressorSide, AssetClass, BookAction, InstrumentClass, MarketStatusAction, OptionKind,
        OrderSide,
    },
    identifiers::{InstrumentId, TradeId},
    instruments::Instrument,
    types::{
        Currency, Price, Quantity,
        price::{PRICE_UNDEF, decode_raw_price_i64},
    },
};
use rstest::*;
use ustr::Ustr;

use super::{
    market_data::{
        BAR_CLOSE_ADJUSTMENT_1D, BAR_CLOSE_ADJUSTMENT_1H, BAR_CLOSE_ADJUSTMENT_1M,
        BAR_CLOSE_ADJUSTMENT_1S, derive_cmbp_trade_id, is_trade_msg,
    },
    primitives::parse_currency_or_usd_default,
    *,
};
use crate::enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction};

fn test_data_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

#[rstest]
#[case('Y' as c_char, Some(true))]
#[case('N' as c_char, Some(false))]
#[case('X' as c_char, None)]
fn test_parse_optional_bool(#[case] input: c_char, #[case] expected: Option<bool>) {
    assert_eq!(parse_optional_bool(input), expected);
}

#[rstest]
#[case('A' as c_char, OrderSide::Sell)]
#[case('B' as c_char, OrderSide::Buy)]
#[case('X' as c_char, OrderSide::NoOrderSide)]
fn test_parse_order_side(#[case] input: c_char, #[case] expected: OrderSide) {
    assert_eq!(parse_order_side(input), expected);
}

#[rstest]
#[case('A' as c_char, AggressorSide::Seller)]
#[case('B' as c_char, AggressorSide::Buyer)]
#[case('X' as c_char, AggressorSide::NoAggressor)]
fn test_parse_aggressor_side(#[case] input: c_char, #[case] expected: AggressorSide) {
    assert_eq!(parse_aggressor_side(input), expected);
}

#[rstest]
#[case('T' as c_char, true)]
#[case('A' as c_char, false)]
#[case('C' as c_char, false)]
#[case('F' as c_char, false)]
#[case('M' as c_char, false)]
#[case('R' as c_char, false)]
fn test_is_trade_msg(#[case] action: c_char, #[case] expected: bool) {
    assert_eq!(is_trade_msg(action), expected);
}

#[rstest]
fn test_derive_cmbp_trade_id_is_deterministic() {
    let instrument_id = InstrumentId::from("ES.c.0.GLBX");
    let first = derive_cmbp_trade_id(instrument_id, 1, 2, 100, 5, 'B' as c_char);
    let second = derive_cmbp_trade_id(instrument_id, 1, 2, 100, 5, 'B' as c_char);
    assert_eq!(first, second);
}

#[rstest]
fn test_derive_cmbp_trade_id_format_is_16_hex_chars() {
    let instrument_id = InstrumentId::from("ES.c.0.GLBX");
    let trade_id = derive_cmbp_trade_id(instrument_id, 0, 0, 0, 0, 'B' as c_char);
    let value = trade_id.as_str();
    assert_eq!(value.len(), 16);
    assert!(
        value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    );
}

#[rstest]
#[case::ts_event_changed(
    derive_cmbp_trade_id(InstrumentId::from("ES.c.0.GLBX"), 2, 2, 100, 5, 'B' as c_char),
)]
#[case::ts_recv_changed(
    derive_cmbp_trade_id(InstrumentId::from("ES.c.0.GLBX"), 1, 3, 100, 5, 'B' as c_char),
)]
#[case::price_changed(
    derive_cmbp_trade_id(InstrumentId::from("ES.c.0.GLBX"), 1, 2, 101, 5, 'B' as c_char),
)]
#[case::size_changed(
    derive_cmbp_trade_id(InstrumentId::from("ES.c.0.GLBX"), 1, 2, 100, 6, 'B' as c_char),
)]
#[case::side_changed(
    derive_cmbp_trade_id(InstrumentId::from("ES.c.0.GLBX"), 1, 2, 100, 5, 'A' as c_char),
)]
#[case::instrument_changed(
    derive_cmbp_trade_id(InstrumentId::from("NQ.c.0.GLBX"), 1, 2, 100, 5, 'B' as c_char),
)]
fn test_derive_cmbp_trade_id_each_field_affects_output(#[case] altered: TradeId) {
    let baseline = derive_cmbp_trade_id(
        InstrumentId::from("ES.c.0.GLBX"),
        1,
        2,
        100,
        5,
        'B' as c_char,
    );
    assert_ne!(baseline, altered);
}

#[rstest]
fn test_derive_cmbp_trade_id_field_delimiter_prevents_collision() {
    let instrument_id = InstrumentId::from("ES.c.0.GLBX");
    // If fields were concatenated without delimiters, these two triples
    // would produce the same input stream.
    let a = derive_cmbp_trade_id(instrument_id, 0x100, 0, 0, 0, 'B' as c_char);
    let b = derive_cmbp_trade_id(instrument_id, 0, 0x100, 0, 0, 'B' as c_char);
    assert_ne!(a, b);
}

mod cmbp_trade_id_property_tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    proptest! {
        #[rstest]
        fn prop_derive_cmbp_trade_id_is_stable_for_same_inputs(
            ts_event in any::<u64>(),
            ts_recv in any::<u64>(),
            price in any::<i64>(),
            size in any::<u32>(),
            side_byte in 0u8..128,
        ) {
            let instrument_id = InstrumentId::from("ES.c.0.GLBX");
            let side = side_byte as c_char;

            let first = derive_cmbp_trade_id(
                instrument_id, ts_event, ts_recv, price, size, side,
            );
            let second = derive_cmbp_trade_id(
                instrument_id, ts_event, ts_recv, price, size, side,
            );
            prop_assert_eq!(first, second);
        }

        #[rstest]
        fn prop_derive_cmbp_trade_id_output_is_16_hex_chars(
            ts_event in any::<u64>(),
            ts_recv in any::<u64>(),
            price in any::<i64>(),
            size in any::<u32>(),
            side_byte in 0u8..128,
        ) {
            let instrument_id = InstrumentId::from("ES.c.0.GLBX");
            let side = side_byte as c_char;
            let id = derive_cmbp_trade_id(
                instrument_id, ts_event, ts_recv, price, size, side,
            );
            let value = id.as_str();
            prop_assert_eq!(value.len(), 16);
            prop_assert!(value.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
        }
    }
}

#[rstest]
#[case('A' as c_char, Ok(BookAction::Add))]
#[case('C' as c_char, Ok(BookAction::Delete))]
#[case('F' as c_char, Ok(BookAction::Update))]
#[case('M' as c_char, Ok(BookAction::Update))]
#[case('R' as c_char, Ok(BookAction::Clear))]
#[case('X' as c_char, Err("Invalid `BookAction`, was 'X'"))]
fn test_parse_book_action(#[case] input: c_char, #[case] expected: Result<BookAction, &str>) {
    match parse_book_action(input) {
        Ok(action) => assert_eq!(Ok(action), expected),
        Err(e) => assert_eq!(Err(e.to_string().as_str()), expected),
    }
}

#[rstest]
#[case('C' as c_char, Ok(OptionKind::Call))]
#[case('P' as c_char, Ok(OptionKind::Put))]
#[case('X' as c_char, Err("Invalid `OptionKind`, was 'X'"))]
fn test_parse_option_kind(#[case] input: c_char, #[case] expected: Result<OptionKind, &str>) {
    match parse_option_kind(input) {
        Ok(kind) => assert_eq!(Ok(kind), expected),
        Err(e) => assert_eq!(Err(e.to_string().as_str()), expected),
    }
}

#[rstest]
#[case(Ok("USD"), Currency::USD())]
#[case(Ok("EUR"), Currency::try_from_str("EUR").unwrap())]
#[case(Ok(""), Currency::USD())]
#[case(Err("Error"), Currency::USD())]
fn test_parse_currency_or_usd_default(
    #[case] input: Result<&str, &'static str>, // Using `&'static str` for errors
    #[case] expected: Currency,
) {
    let actual = parse_currency_or_usd_default(input.map_err(std::io::Error::other));
    assert_eq!(actual, expected);
}

#[rstest]
#[case("DII", (Some(AssetClass::Index), Some(InstrumentClass::Future)))]
#[case("EII", (Some(AssetClass::Index), Some(InstrumentClass::Future)))]
#[case("EIA", (Some(AssetClass::Equity), Some(InstrumentClass::Future)))]
#[case("XXX", (None, None))]
#[case("D", (None, None))]
#[case("", (None, None))]
fn test_parse_cfi_iso10926(
    #[case] input: &str,
    #[case] expected: (Option<AssetClass>, Option<InstrumentClass>),
) {
    let result = parse_cfi_iso10926(input);
    assert_eq!(result, expected);
}

#[rstest]
#[case(0, 2, Price::from_raw(0, 2))]
#[case(
    1_000_000_000,
    2,
    Price::from_raw(decode_raw_price_i64(1_000_000_000), 2)
)]
fn test_decode_price(#[case] value: i64, #[case] precision: u8, #[case] expected: Price) {
    let actual = decode_price(value, precision, "test_field").unwrap();
    assert_eq!(actual, expected);
}

#[rstest]
fn test_decode_price_undefined_errors() {
    let result = decode_price(i64::MAX, 2, "strike_price");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("strike_price"));
}

#[rstest]
#[case(0, 0)]
#[case(1, 9)] // 0.000000001 needs 9 decimal places
#[case(10, 8)] // 0.00000001 needs 8
#[case(3_906_250, 8)] // ZT: 1/256 = 0.00390625
#[case(7_812_500, 7)] // ZF: 1/128 = 0.0078125
#[case(15_625_000, 6)] // ZN: 1/64 = 0.015625
#[case(31_250_000, 5)] // ZB: 1/32 = 0.03125
#[case(250_000_000, 2)] // ES: 0.25
#[case(1_000_000_000, 0)] // 1.0
#[case(10_000_000_000, 0)] // 10.0
fn test_precision_from_raw(#[case] value: i64, #[case] expected: u8) {
    assert_eq!(precision_from_raw(value), expected);
}

#[rstest]
#[case(0, 2, Price::new(0.01, 2))] // Default for 0
#[case(i64::MAX, 2, Price::new(0.01, 2))] // Default for i64::MAX
#[case(
    10_000_000_000,
    2,
    Price::from_raw(decode_raw_price_i64(10_000_000_000), 2)
)] // 10.0: derived=0, max(0,2)=2
#[case(3_906_250, 2, Price::from_raw(decode_raw_price_i64(3_906_250), 8))] // ZT 1/256: derived=8, max(8,2)=8
#[case(7_812_500, 2, Price::from_raw(decode_raw_price_i64(7_812_500), 7))] // ZF 1/128: derived=7, max(7,2)=7
#[case(15_625_000, 2, Price::from_raw(decode_raw_price_i64(15_625_000), 6))] // ZN 1/64: derived=6, max(6,2)=6
#[case(31_250_000, 2, Price::from_raw(decode_raw_price_i64(31_250_000), 5))] // ZB 1/32: derived=5, max(5,2)=5
#[case(250_000_000, 2, Price::from_raw(decode_raw_price_i64(250_000_000), 2))] // ES 0.25: derived=2, max(2,2)=2
fn test_decode_price_increment(#[case] value: i64, #[case] precision: u8, #[case] expected: Price) {
    let actual = decode_price_increment(value, precision);
    assert_eq!(actual, expected);
}

#[rstest]
#[case(i64::MAX, 2, None)] // None for i64::MAX
#[case(0, 2, Some(Price::from_raw(0, 2)))] // 0 is valid here
#[case(
    10_000_000_000,
    2,
    Some(Price::from_raw(decode_raw_price_i64(10_000_000_000), 2))
)]
fn test_decode_optional_price(
    #[case] value: i64,
    #[case] precision: u8,
    #[case] expected: Option<Price>,
) {
    let actual = decode_optional_price(value, precision);
    assert_eq!(actual, expected);
}

#[rstest]
#[case(0, 2, Price::from_raw(0, 2))]
#[case(
    1_000_000_000,
    2,
    Price::from_raw(decode_raw_price_i64(1_000_000_000), 2)
)]
#[case(i64::MAX, 2, Price::from_raw(PRICE_UNDEF, 0))] // Sentinel becomes PRICE_UNDEF
fn test_decode_price_or_undef(#[case] value: i64, #[case] precision: u8, #[case] expected: Price) {
    let actual = decode_price_or_undef(value, precision);
    assert_eq!(actual, expected);
}

#[rstest]
#[case(i64::MAX, None)] // None for i32::MAX
#[case(0, Some(Quantity::new(0.0, 0)))] // 0 is valid quantity
#[case(10, Some(Quantity::new(10.0, 0)))] // Arbitrary valid quantity
fn test_decode_optional_quantity(#[case] value: i64, #[case] expected: Option<Quantity>) {
    let actual = decode_optional_quantity(value);
    assert_eq!(actual, expected);
}

#[rstest]
#[case(0, UnixNanos::from(0))]
#[case(1_000_000_000, UnixNanos::from(1_000_000_000))]
fn test_decode_timestamp(#[case] value: u64, #[case] expected: UnixNanos) {
    let actual = decode_timestamp(value, "test_field").unwrap();
    assert_eq!(actual, expected);
}

#[rstest]
fn test_decode_timestamp_undefined_errors() {
    let result = decode_timestamp(dbn::UNDEF_TIMESTAMP, "expiration");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("expiration"));
}

#[rstest]
#[case(0, Some(UnixNanos::from(0)))]
#[case(1_000_000_000, Some(UnixNanos::from(1_000_000_000)))]
#[case(dbn::UNDEF_TIMESTAMP, None)]
fn test_decode_optional_timestamp(#[case] value: u64, #[case] expected: Option<UnixNanos>) {
    let actual = decode_optional_timestamp(value);
    assert_eq!(actual, expected);
}

#[rstest]
#[case(0, Quantity::from(1))] // Default fallback for 0
#[case(i64::MAX, Quantity::from(1))] // Default fallback for i64::MAX
#[case(50_000_000_000, Quantity::from("50"))] // 50.0 exactly
#[case(12_500_000_000, Quantity::from("12.5"))] // 12.5 exactly
#[case(1_000_000_000, Quantity::from("1"))] // 1.0 exactly
#[case(1, Quantity::from("0.000000001"))] // Smallest positive value
#[case(1_000_000_001, Quantity::from("1.000000001"))] // Just over 1.0
#[case(999_999_999, Quantity::from("0.999999999"))] // Just under 1.0
#[case(123_456_789_000, Quantity::from("123.456789"))] // Trailing zeros trimmed
fn test_decode_multiplier_precise(#[case] raw: i64, #[case] expected: Quantity) {
    assert_eq!(decode_multiplier(raw).unwrap(), expected);
}

#[rstest]
#[case(-1_500_000_000)] // Large negative value
#[case(-1)] // Small negative value
#[case(-999_999_999)] // Another negative value
fn test_decode_multiplier_negative_error(#[case] raw: i64) {
    let result = decode_multiplier(raw);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid negative multiplier")
    );
}

#[rstest]
#[case(100, Quantity::from(100))]
#[case(1000, Quantity::from(1000))]
#[case(5, Quantity::from(5))]
fn test_decode_quantity(#[case] value: u64, #[case] expected: Quantity) {
    assert_eq!(decode_quantity(value), expected);
}

#[rstest]
#[case(0, Quantity::from(1))] // Default for 0
#[case(i32::MAX, Quantity::from(1))] // Default for MAX
#[case(100, Quantity::from(100))]
#[case(1, Quantity::from(1))]
#[case(1000, Quantity::from(1000))]
fn test_decode_lot_size(#[case] value: i32, #[case] expected: Quantity) {
    assert_eq!(decode_lot_size(value), expected);
}

#[rstest]
#[case(0, None)] // None for 0
#[case(1, Some(Ustr::from("Scheduled")))]
#[case(2, Some(Ustr::from("Surveillance intervention")))]
#[case(3, Some(Ustr::from("Market event")))]
#[case(10, Some(Ustr::from("Regulatory")))]
#[case(30, Some(Ustr::from("News pending")))]
#[case(40, Some(Ustr::from("Order imbalance")))]
#[case(50, Some(Ustr::from("LULD pause")))]
#[case(60, Some(Ustr::from("Operational")))]
#[case(100, Some(Ustr::from("Corporate action")))]
#[case(120, Some(Ustr::from("Market wide halt level 1")))]
fn test_parse_status_reason(#[case] value: u16, #[case] expected: Option<Ustr>) {
    assert_eq!(parse_status_reason(value).unwrap(), expected);
}

#[rstest]
#[case(999)] // Invalid code
fn test_parse_status_reason_invalid(#[case] value: u16) {
    assert!(parse_status_reason(value).is_err());
}

#[rstest]
#[case(0, None)] // None for 0
#[case(1, Some(Ustr::from("No cancel")))]
#[case(2, Some(Ustr::from("Change trading session")))]
#[case(3, Some(Ustr::from("Implied matching on")))]
#[case(4, Some(Ustr::from("Implied matching off")))]
fn test_parse_status_trading_event(#[case] value: u16, #[case] expected: Option<Ustr>) {
    assert_eq!(parse_status_trading_event(value).unwrap(), expected);
}

#[rstest]
#[case(5)] // Invalid code
#[case(100)] // Invalid code
fn test_parse_status_trading_event_invalid(#[case] value: u16) {
    assert!(parse_status_trading_event(value).is_err());
}

#[rstest]
fn test_decode_mbo_msg() {
    let path = test_data_path().join("test_data.mbo.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::MboMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (delta, _) = decode_mbo_msg(msg, instrument_id, 2, Some(0.into()), false).unwrap();
    let delta = delta.unwrap();

    assert_eq!(delta.instrument_id, instrument_id);
    assert_eq!(delta.action, BookAction::Delete);
    assert_eq!(delta.order.side, OrderSide::Sell);
    assert_eq!(delta.order.price, Price::from("3722.75"));
    assert_eq!(delta.order.size, Quantity::from("1"));
    assert_eq!(delta.order.order_id, 647_784_973_705);
    assert_eq!(delta.flags, 128);
    assert_eq!(delta.sequence, 1_170_352);
    assert_eq!(delta.ts_event, msg.ts_recv);
    assert_eq!(delta.ts_event, 1_609_160_400_000_704_060);
    assert_eq!(delta.ts_init, 0);
}

#[rstest]
fn test_decode_mbo_msg_clear_action() {
    // Create an MBO message with Clear action (action='R', side='N')
    let ts_recv = 1_609_160_400_000_000_000;
    let msg = dbn::MboMsg {
        hd: dbn::RecordHeader::new::<dbn::MboMsg>(1, 1, ts_recv as u32, 0),
        order_id: 0,
        price: i64::MAX,
        size: 0,
        flags: dbn::FlagSet::empty(),
        channel_id: 0,
        action: 'R' as c_char,
        side: 'N' as c_char, // NoOrderSide for Clear
        ts_recv,
        ts_in_delta: 0,
        sequence: 1_000_000,
    };

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (delta, trade) = decode_mbo_msg(&msg, instrument_id, 2, Some(0.into()), false).unwrap();

    // Clear messages should produce OrderBookDelta, not TradeTick
    assert!(trade.is_none());
    let delta = delta.expect("Clear action should produce OrderBookDelta");

    assert_eq!(delta.instrument_id, instrument_id);
    assert_eq!(delta.action, BookAction::Clear);
    assert_eq!(delta.order.side, OrderSide::NoOrderSide);
    assert_eq!(delta.order.size, Quantity::from("0"));
    assert_eq!(delta.order.order_id, 0);
    assert_eq!(delta.sequence, 1_000_000);
    assert_eq!(delta.ts_event, ts_recv);
    assert_eq!(delta.ts_init, 0);
    assert!(delta.order.price.is_undefined());
    assert_eq!(delta.order.price.precision, 0);
}

#[rstest]
fn test_decode_mbo_msg_price_undef_with_precision() {
    // Test that PRICE_UNDEF (i64::MAX) forces precision to 0 even when price_precision is non-zero
    let ts_recv = 1_609_160_400_000_000_000;
    let msg = dbn::MboMsg {
        hd: dbn::RecordHeader::new::<dbn::MboMsg>(1, 1, ts_recv as u32, 0),
        order_id: 0,
        price: i64::MAX, // PRICE_UNDEF
        size: 0,
        flags: dbn::FlagSet::empty(),
        channel_id: 0,
        action: 'R' as c_char, // Clear
        side: 'N' as c_char,   // NoOrderSide
        ts_recv,
        ts_in_delta: 0,
        sequence: 0,
    };

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (delta, _) = decode_mbo_msg(&msg, instrument_id, 2, Some(0.into()), false).unwrap();
    let delta = delta.unwrap();

    assert!(delta.order.price.is_undefined());
    assert_eq!(delta.order.price.precision, 0);
    assert_eq!(delta.order.price.raw, PRICE_UNDEF);
}

#[rstest]
fn test_decode_mbo_msg_no_order_side_update() {
    // MBO messages with NoOrderSide are now passed through to the book
    // The book will resolve the side from its cache using the order_id
    let ts_recv = 1_609_160_400_000_000_000;
    let msg = dbn::MboMsg {
        hd: dbn::RecordHeader::new::<dbn::MboMsg>(1, 1, ts_recv as u32, 0),
        order_id: 123_456_789,
        price: 4_800_250_000_000, // $4800.25 with precision 2
        size: 1,
        flags: dbn::FlagSet::empty(),
        channel_id: 1,
        action: 'M' as c_char, // Modify/Update action
        side: 'N' as c_char,   // NoOrderSide
        ts_recv,
        ts_in_delta: 0,
        sequence: 1_000_000,
    };

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (delta, trade) = decode_mbo_msg(&msg, instrument_id, 2, Some(0.into()), false).unwrap();

    // Delta should be created with NoOrderSide (book will resolve it)
    assert!(delta.is_some());
    assert!(trade.is_none());
    let delta = delta.unwrap();
    assert_eq!(delta.order.side, OrderSide::NoOrderSide);
    assert_eq!(delta.order.order_id, 123_456_789);
    assert_eq!(delta.action, BookAction::Update);
}

#[rstest]
fn test_decode_mbp1_msg() {
    let path = test_data_path().join("test_data.mbp-1.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::Mbp1Msg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (maybe_quote, _) = decode_mbp1_msg(msg, instrument_id, 2, Some(0.into()), false).unwrap();
    let quote = maybe_quote.expect("Expected valid quote");

    assert_eq!(quote.instrument_id, instrument_id);
    assert_eq!(quote.bid_price, Price::from("3720.25"));
    assert_eq!(quote.ask_price, Price::from("3720.50"));
    assert_eq!(quote.bid_size, Quantity::from("24"));
    assert_eq!(quote.ask_size, Quantity::from("11"));
    assert_eq!(quote.ts_event, msg.ts_recv);
    assert_eq!(quote.ts_event, 1_609_160_400_006_136_329);
    assert_eq!(quote.ts_init, 0);
}

#[rstest]
fn test_decode_mbp1_msg_undefined_ask_skips_quote() {
    let ts_recv = 1_609_160_400_000_000_000;
    let msg = dbn::Mbp1Msg {
        hd: dbn::RecordHeader::new::<dbn::Mbp1Msg>(1, 1, ts_recv as u32, 0),
        price: 3_720_250_000_000, // Valid trade price
        size: 5,
        action: 'A' as c_char,
        side: 'B' as c_char,
        flags: dbn::FlagSet::empty(),
        depth: 0,
        ts_recv,
        ts_in_delta: 0,
        sequence: 1_170_352,
        levels: [dbn::BidAskPair {
            bid_px: 3_720_250_000_000, // Valid bid price
            ask_px: i64::MAX,          // Undefined ask price
            bid_sz: 24,
            ask_sz: 0,
            bid_ct: 1,
            ask_ct: 0,
        }],
    };

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (maybe_quote, _) = decode_mbp1_msg(&msg, instrument_id, 2, Some(0.into()), false).unwrap();

    // Quote should be None because ask price is undefined
    assert!(maybe_quote.is_none());
}

#[rstest]
fn test_decode_mbp1_msg_undefined_bid_skips_quote() {
    let ts_recv = 1_609_160_400_000_000_000;
    let msg = dbn::Mbp1Msg {
        hd: dbn::RecordHeader::new::<dbn::Mbp1Msg>(1, 1, ts_recv as u32, 0),
        price: 3_720_500_000_000, // Valid trade price
        size: 5,
        action: 'A' as c_char,
        side: 'A' as c_char,
        flags: dbn::FlagSet::empty(),
        depth: 0,
        ts_recv,
        ts_in_delta: 0,
        sequence: 1_170_352,
        levels: [dbn::BidAskPair {
            bid_px: i64::MAX,          // Undefined bid price
            ask_px: 3_720_500_000_000, // Valid ask price
            bid_sz: 0,
            ask_sz: 11,
            bid_ct: 0,
            ask_ct: 1,
        }],
    };

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (maybe_quote, _) = decode_mbp1_msg(&msg, instrument_id, 2, Some(0.into()), false).unwrap();

    // Quote should be None because bid price is undefined
    assert!(maybe_quote.is_none());
}

#[rstest]
fn test_decode_mbp1_msg_trade_still_returned_with_undefined_prices() {
    let ts_recv = 1_609_160_400_000_000_000;
    let msg = dbn::Mbp1Msg {
        hd: dbn::RecordHeader::new::<dbn::Mbp1Msg>(1, 1, ts_recv as u32, 0),
        price: 3_720_250_000_000, // Valid trade price
        size: 5,
        action: 'T' as c_char, // Trade action
        side: 'A' as c_char,
        flags: dbn::FlagSet::empty(),
        depth: 0,
        ts_recv,
        ts_in_delta: 0,
        sequence: 1_170_352,
        levels: [dbn::BidAskPair {
            bid_px: i64::MAX, // Undefined bid
            ask_px: i64::MAX, // Undefined ask
            bid_sz: 0,
            ask_sz: 0,
            bid_ct: 0,
            ask_ct: 0,
        }],
    };

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (maybe_quote, maybe_trade) =
        decode_mbp1_msg(&msg, instrument_id, 2, Some(0.into()), true).unwrap();

    // Quote should be None because both prices are undefined
    assert!(maybe_quote.is_none());

    // Trade should still be present
    let trade = maybe_trade.expect("Expected trade");
    assert_eq!(trade.instrument_id, instrument_id);
    assert_eq!(trade.price, Price::from("3720.25"));
    assert_eq!(trade.size, Quantity::from("5"));
}

#[rstest]
fn test_decode_bbo_1s_msg() {
    let path = test_data_path().join("test_data.bbo-1s.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::BboMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let maybe_quote = decode_bbo_msg(msg, instrument_id, 2, Some(0.into())).unwrap();
    let quote = maybe_quote.expect("Expected valid quote");

    assert_eq!(quote.instrument_id, instrument_id);
    assert_eq!(quote.bid_price, Price::from("3702.25"));
    assert_eq!(quote.ask_price, Price::from("3702.75"));
    assert_eq!(quote.bid_size, Quantity::from("18"));
    assert_eq!(quote.ask_size, Quantity::from("13"));
    assert_eq!(quote.ts_event, msg.ts_recv);
    assert_eq!(quote.ts_event, 1609113600000000000);
    assert_eq!(quote.ts_init, 0);
}

#[rstest]
fn test_decode_bbo_1m_msg() {
    let path = test_data_path().join("test_data.bbo-1m.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::BboMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let maybe_quote = decode_bbo_msg(msg, instrument_id, 2, Some(0.into())).unwrap();
    let quote = maybe_quote.expect("Expected valid quote");

    assert_eq!(quote.instrument_id, instrument_id);
    assert_eq!(quote.bid_price, Price::from("3702.25"));
    assert_eq!(quote.ask_price, Price::from("3702.75"));
    assert_eq!(quote.bid_size, Quantity::from("18"));
    assert_eq!(quote.ask_size, Quantity::from("13"));
    assert_eq!(quote.ts_event, msg.ts_recv);
    assert_eq!(quote.ts_event, 1609113600000000000);
    assert_eq!(quote.ts_init, 0);
}

#[rstest]
fn test_decode_mbp10_msg() {
    let path = test_data_path().join("test_data.mbp-10.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::Mbp10Msg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let depth10 = decode_mbp10_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

    assert_eq!(depth10.instrument_id, instrument_id);
    assert_eq!(depth10.bids.len(), 10);
    assert_eq!(depth10.asks.len(), 10);
    assert_eq!(depth10.bid_counts.len(), 10);
    assert_eq!(depth10.ask_counts.len(), 10);
    assert_eq!(depth10.flags, 128);
    assert_eq!(depth10.sequence, 1_170_352);
    assert_eq!(depth10.ts_event, msg.ts_recv);
    assert_eq!(depth10.ts_event, 1_609_160_400_000_704_060);
    assert_eq!(depth10.ts_init, 0);
}

#[rstest]
fn test_decode_trade_msg() {
    let path = test_data_path().join("test_data.trades.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::TradeMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let trade = decode_trade_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

    assert_eq!(trade.instrument_id, instrument_id);
    assert_eq!(trade.price, Price::from("3720.25"));
    assert_eq!(trade.size, Quantity::from("5"));
    assert_eq!(trade.aggressor_side, AggressorSide::Seller);
    assert_eq!(trade.trade_id.to_string(), "1170380");
    assert_eq!(trade.ts_event, msg.ts_recv);
    assert_eq!(trade.ts_event, 1_609_160_400_099_150_057);
    assert_eq!(trade.ts_init, 0);
}

#[rstest]
fn test_decode_tbbo_msg() {
    let path = test_data_path().join("test_data.tbbo.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::Mbp1Msg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (maybe_quote, trade) = decode_tbbo_msg(msg, instrument_id, 2, Some(0.into())).unwrap();
    let quote = maybe_quote.expect("Expected valid quote");

    assert_eq!(quote.instrument_id, instrument_id);
    assert_eq!(quote.bid_price, Price::from("3720.25"));
    assert_eq!(quote.ask_price, Price::from("3720.50"));
    assert_eq!(quote.bid_size, Quantity::from("26"));
    assert_eq!(quote.ask_size, Quantity::from("7"));
    assert_eq!(quote.ts_event, msg.ts_recv);
    assert_eq!(quote.ts_event, 1_609_160_400_099_150_057);
    assert_eq!(quote.ts_init, 0);

    assert_eq!(trade.instrument_id, instrument_id);
    assert_eq!(trade.price, Price::from("3720.25"));
    assert_eq!(trade.size, Quantity::from("5"));
    assert_eq!(trade.aggressor_side, AggressorSide::Seller);
    assert_eq!(trade.trade_id.to_string(), "1170380");
    assert_eq!(trade.ts_event, msg.ts_recv);
    assert_eq!(trade.ts_event, 1_609_160_400_099_150_057);
    assert_eq!(trade.ts_init, 0);
}

#[rstest]
fn test_decode_ohlcv_msg() {
    let path = test_data_path().join("test_data.ohlcv-1s.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::OhlcvMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let bar = decode_ohlcv_msg(msg, instrument_id, 2, Some(0.into()), true).unwrap();

    assert_eq!(
        bar.bar_type,
        BarType::from("ESM4.GLBX-1-SECOND-LAST-EXTERNAL")
    );
    assert_eq!(bar.open, Price::from("372025.00"));
    assert_eq!(bar.high, Price::from("372050.00"));
    assert_eq!(bar.low, Price::from("372025.00"));
    assert_eq!(bar.close, Price::from("372050.00"));
    assert_eq!(bar.volume, Quantity::from("57"));
    assert_eq!(bar.ts_event, msg.hd.ts_event + BAR_CLOSE_ADJUSTMENT_1S); // timestamp_on_close=true
    assert_eq!(bar.ts_init, 0); // ts_init was Some(0)
}

#[rstest]
fn test_decode_definition_msg() {
    let path = test_data_path().join("test_data.definition.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::InstrumentDefMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let result = decode_instrument_def_msg(msg, instrument_id, Some(0.into()));

    let instrument = result
        .expect("decode failed")
        .expect("definition class should produce an instrument");
    assert_eq!(instrument.multiplier(), Quantity::from(1));
}

#[rstest]
fn test_decode_status_msg() {
    let path = test_data_path().join("test_data.status.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::StatusMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let status = decode_status_msg(msg, instrument_id, Some(0.into())).unwrap();

    assert_eq!(status.instrument_id, instrument_id);
    assert_eq!(status.action, MarketStatusAction::Trading);
    assert_eq!(status.ts_event, msg.hd.ts_event);
    assert_eq!(status.ts_init, 0);
    assert_eq!(status.reason, Some(Ustr::from("Scheduled")));
    assert_eq!(status.trading_event, None);
    assert_eq!(status.is_trading, Some(true));
    assert_eq!(status.is_quoting, Some(true));
    assert_eq!(status.is_short_sell_restricted, None);
}

#[rstest]
fn test_decode_imbalance_msg() {
    let path = test_data_path().join("test_data.imbalance.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::ImbalanceMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let imbalance = decode_imbalance_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

    assert_eq!(imbalance.instrument_id, instrument_id);
    assert_eq!(imbalance.ref_price, Price::from("229.43"));
    assert_eq!(imbalance.cont_book_clr_price, Price::from("0.00"));
    assert_eq!(imbalance.auct_interest_clr_price, Price::from("0.00"));
    assert_eq!(imbalance.paired_qty, Quantity::from("0"));
    assert_eq!(imbalance.total_imbalance_qty, Quantity::from("2000"));
    assert_eq!(imbalance.side, OrderSide::Buy);
    assert_eq!(imbalance.significant_imbalance, 126);
    assert_eq!(imbalance.ts_event, msg.hd.ts_event);
    assert_eq!(imbalance.ts_recv, msg.ts_recv);
    assert_eq!(imbalance.ts_init, 0);
}

#[rstest]
#[case::index('I' as c_char)]
#[case::bond('B' as c_char)]
#[case::fx_spot('X' as c_char)]
#[case::unknown('Z' as c_char)]
fn test_decode_instrument_def_msg_unsupported_class_returns_none(#[case] instrument_class: c_char) {
    // Regression: dbn 0.58 publishers (e.g. CGIF.TITANIUM = 110) emit class 'I'
    let msg = dbn::InstrumentDefMsg {
        hd: dbn::RecordHeader::new::<dbn::InstrumentDefMsg>(
            dbn::enums::rtype::INSTRUMENT_DEF,
            1,
            1,
            1_000_000_000,
        ),
        ts_recv: 1_000_000_000,
        instrument_class,
        ..Default::default()
    };

    let instrument_id = InstrumentId::from("SPX.XCBO");
    let result = decode_instrument_def_msg(&msg, instrument_id, Some(0.into()))
        .expect("decoder should not bail on unsupported class");
    assert!(result.is_none());
}

#[rstest]
#[case::volatility(14, DatabentoStatisticType::Volatility)]
#[case::delta(15, DatabentoStatisticType::Delta)]
#[case::uncrossing_price(16, DatabentoStatisticType::UncrossingPrice)]
#[case::upper_price_limit(17, DatabentoStatisticType::UpperPriceLimit)]
#[case::lower_price_limit(18, DatabentoStatisticType::LowerPriceLimit)]
#[case::block_volume(19, DatabentoStatisticType::BlockVolume)]
#[case::indicative_close(20, DatabentoStatisticType::IndicativeClosePrice)]
fn test_decode_statistics_msg_dbn_058_stat_types(
    #[case] stat_type_raw: u16,
    #[case] expected: DatabentoStatisticType,
) {
    // Regression: dbn 0.58 added stat types 14-20 (Volatility..IndicativeClosePrice)
    let msg = dbn::StatMsg {
        hd: dbn::RecordHeader::new::<dbn::StatMsg>(
            dbn::enums::rtype::STATISTICS,
            1,
            1,
            1_000_000_000,
        ),
        ts_recv: 1_000_000_000,
        ts_ref: 1_000_000_000,
        stat_type: stat_type_raw,
        update_action: 1, // Added
        price: 100_000_000_000,
        ..Default::default()
    };

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let statistics = decode_statistics_msg(&msg, instrument_id, 2, Some(0.into()))
        .expect("decoder should accept dbn 0.58 stat types")
        .expect("known stat type should produce a statistics record");
    assert_eq!(statistics.stat_type, expected);
    assert_eq!(
        statistics.update_action,
        DatabentoStatisticUpdateAction::Added
    );
}

#[rstest]
#[case::venue_specific_volume1(10_001)]
#[case::venue_specific_price1(10_002)]
#[case::unknown_future(12_345)]
fn test_decode_statistics_msg_unknown_stat_type_returns_none(#[case] stat_type_raw: u16) {
    // Wire values 10001/10002 exceed the u8 Arrow column width; must skip not bail
    let msg = dbn::StatMsg {
        hd: dbn::RecordHeader::new::<dbn::StatMsg>(
            dbn::enums::rtype::STATISTICS,
            1,
            1,
            1_000_000_000,
        ),
        ts_recv: 1_000_000_000,
        ts_ref: 1_000_000_000,
        stat_type: stat_type_raw,
        update_action: 1,
        price: 100_000_000_000,
        ..Default::default()
    };

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let result = decode_statistics_msg(&msg, instrument_id, 2, Some(0.into()))
        .expect("decoder should not bail on unknown stat type");
    assert!(result.is_none());
}

#[rstest]
fn test_decode_statistics_msg() {
    let path = test_data_path().join("test_data.statistics.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::StatMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let statistics = decode_statistics_msg(msg, instrument_id, 2, Some(0.into()))
        .unwrap()
        .expect("fixture stat type should map to a Nautilus variant");

    assert_eq!(statistics.instrument_id, instrument_id);
    assert_eq!(statistics.stat_type, DatabentoStatisticType::LowestOffer);
    assert_eq!(
        statistics.update_action,
        DatabentoStatisticUpdateAction::Added
    );
    assert_eq!(statistics.price, Some(Price::from("100.00")));
    assert_eq!(statistics.quantity, None);
    assert_eq!(statistics.channel_id, 13);
    assert_eq!(statistics.stat_flags, 255);
    assert_eq!(statistics.sequence, 2);
    assert_eq!(statistics.ts_ref, 18_446_744_073_709_551_615);
    assert_eq!(statistics.ts_in_delta, 26961);
    assert_eq!(statistics.ts_event, msg.hd.ts_event);
    assert_eq!(statistics.ts_recv, msg.ts_recv);
    assert_eq!(statistics.ts_init, 0);
}

#[rstest]
fn test_decode_cmbp1_msg() {
    let path = test_data_path().join("test_data.cmbp-1.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::Cmbp1Msg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (maybe_quote, trade) =
        decode_cmbp1_msg(msg, instrument_id, 2, Some(0.into()), true).unwrap();
    let quote = maybe_quote.expect("Expected valid quote");

    assert_eq!(quote.instrument_id, instrument_id);
    assert!(quote.bid_price.raw > 0);
    assert!(quote.ask_price.raw > 0);
    assert!(quote.bid_size.raw > 0);
    assert!(quote.ask_size.raw > 0);
    assert_eq!(quote.ts_event, msg.ts_recv);
    assert_eq!(quote.ts_init, 0);

    // Check if trade is present based on action
    if is_trade_msg(msg.action) {
        assert!(trade.is_some());
        let trade = trade.unwrap();
        assert_eq!(trade.instrument_id, instrument_id);
    } else {
        assert!(trade.is_none());
    }
}

#[rstest]
fn test_decode_cbbo_1s_msg() {
    let path = test_data_path().join("test_data.cbbo-1s.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::CbboMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let maybe_quote = decode_cbbo_msg(msg, instrument_id, 2, Some(0.into())).unwrap();
    let quote = maybe_quote.expect("Expected valid quote");

    assert_eq!(quote.instrument_id, instrument_id);
    assert!(quote.bid_price.raw > 0);
    assert!(quote.ask_price.raw > 0);
    assert!(quote.bid_size.raw > 0);
    assert!(quote.ask_size.raw > 0);
    assert_eq!(quote.ts_event, msg.ts_recv);
    assert_eq!(quote.ts_init, 0);
}

#[rstest]
fn test_decode_mbp10_msg_with_all_levels() {
    let mut msg = dbn::Mbp10Msg::default();
    for i in 0..10 {
        msg.levels[i].bid_px = 100_000_000_000 - i as i64 * 10_000_000;
        msg.levels[i].ask_px = 100_010_000_000 + i as i64 * 10_000_000;
        msg.levels[i].bid_sz = 10 + i as u32;
        msg.levels[i].ask_sz = 10 + i as u32;
        msg.levels[i].bid_ct = 1 + i as u32;
        msg.levels[i].ask_ct = 1 + i as u32;
    }
    msg.ts_recv = 1_609_160_400_000_704_060;

    let instrument_id = InstrumentId::from("TEST.VENUE");
    let result = decode_mbp10_msg(&msg, instrument_id, 2, None);

    assert!(result.is_ok());
    let depth = result.unwrap();
    assert_eq!(depth.bids.len(), 10);
    assert_eq!(depth.asks.len(), 10);
    assert_eq!(depth.bid_counts.len(), 10);
    assert_eq!(depth.ask_counts.len(), 10);
}

#[rstest]
fn test_decode_mbp10_msg_with_undefined_levels() {
    let mut msg = dbn::Mbp10Msg::default();
    for i in 0..10 {
        msg.levels[i].bid_px = 100_000_000_000 - i as i64 * 10_000_000;
        msg.levels[i].ask_px = 100_010_000_000 + i as i64 * 10_000_000;
        msg.levels[i].bid_sz = 10 + i as u32;
        msg.levels[i].ask_sz = 10 + i as u32;
        msg.levels[i].bid_ct = 1 + i as u32;
        msg.levels[i].ask_ct = 1 + i as u32;
    }
    // Levels 5 (bid) and 7 (ask) are undefined per Databento sentinel.
    msg.levels[5].bid_px = i64::MAX;
    msg.levels[5].bid_sz = 0;
    msg.levels[5].bid_ct = 0;
    msg.levels[7].ask_px = i64::MAX;
    msg.levels[7].ask_sz = 0;
    msg.levels[7].ask_ct = 0;
    msg.ts_recv = 1_609_160_400_000_704_060;

    let instrument_id = InstrumentId::from("TEST.VENUE");
    let depth = decode_mbp10_msg(&msg, instrument_id, 2, None).unwrap();

    assert_eq!(depth.bids[5].side, OrderSide::NoOrderSide);
    assert_eq!(depth.bids[5].price.raw, 0);
    assert_eq!(depth.bids[5].price.precision, 0);
    assert_eq!(depth.bids[5].size.raw, 0);
    assert_eq!(depth.asks[7].side, OrderSide::NoOrderSide);
    assert_eq!(depth.asks[7].price.raw, 0);
    assert_eq!(depth.asks[7].price.precision, 0);
    assert_eq!(depth.asks[7].size.raw, 0);

    // Defined neighbours keep their normal side and instrument precision
    assert_eq!(depth.bids[0].side, OrderSide::Buy);
    assert_eq!(depth.bids[0].price.precision, 2);
    assert_eq!(depth.asks[0].side, OrderSide::Sell);
    assert_eq!(depth.asks[0].price.precision, 2);
}

#[rstest]
fn test_array_conversion_error_handling() {
    let mut bids = Vec::new();
    let mut asks = Vec::new();

    // Intentionally create fewer than DEPTH10_LEN elements
    for i in 0..5 {
        bids.push(BookOrder::new(
            OrderSide::Buy,
            Price::from(format!("{}.00", 100 - i)),
            Quantity::from(10),
            i as u64,
        ));
        asks.push(BookOrder::new(
            OrderSide::Sell,
            Price::from(format!("{}.00", 101 + i)),
            Quantity::from(10),
            i as u64,
        ));
    }

    let result: Result<[BookOrder; DEPTH10_LEN], _> =
        bids.try_into().map_err(|v: Vec<BookOrder>| {
            anyhow::anyhow!(
                "Expected exactly {DEPTH10_LEN} bid levels, received {}",
                v.len()
            )
        });
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Expected exactly 10 bid levels, received 5")
    );
}

#[rstest]
fn test_decode_tcbbo_msg() {
    // Use cbbo-1s as base since cbbo.dbn.zst was invalid
    let path = test_data_path().join("test_data.cbbo-1s.dbn.zst");
    let mut dbn_stream = Decoder::from_zstd_file(path)
        .unwrap()
        .decode_stream::<dbn::CbboMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    // Simulate TCBBO by adding trade data
    let mut tcbbo_msg = msg.clone();
    tcbbo_msg.price = 3702500000000;
    tcbbo_msg.size = 10;

    let instrument_id = InstrumentId::from("ESM4.GLBX");
    let (maybe_quote, trade) =
        decode_tcbbo_msg(&tcbbo_msg, instrument_id, 2, Some(0.into())).unwrap();
    let quote = maybe_quote.expect("Expected valid quote");

    assert_eq!(quote.instrument_id, instrument_id);
    assert!(quote.bid_price.raw > 0);
    assert!(quote.ask_price.raw > 0);
    assert!(quote.bid_size.raw > 0);
    assert!(quote.ask_size.raw > 0);
    assert_eq!(quote.ts_event, tcbbo_msg.ts_recv);
    assert_eq!(quote.ts_init, 0);

    assert_eq!(trade.instrument_id, instrument_id);
    assert_eq!(trade.price, Price::from("3702.50"));
    assert_eq!(trade.size, Quantity::from(10));
    assert_eq!(trade.ts_event, tcbbo_msg.ts_recv);
    assert_eq!(trade.ts_init, 0);
}

#[rstest]
fn test_decode_bar_type() {
    let mut msg = dbn::OhlcvMsg::default_for_schema(dbn::Schema::Ohlcv1S);
    let instrument_id = InstrumentId::from("ESM4.GLBX");

    // Test 1-second bar
    msg.hd.rtype = 32;
    let bar_type = decode_bar_type(&msg, instrument_id).unwrap();
    assert_eq!(bar_type, BarType::from("ESM4.GLBX-1-SECOND-LAST-EXTERNAL"));

    // Test 1-minute bar
    msg.hd.rtype = 33;
    let bar_type = decode_bar_type(&msg, instrument_id).unwrap();
    assert_eq!(bar_type, BarType::from("ESM4.GLBX-1-MINUTE-LAST-EXTERNAL"));

    // Test 1-hour bar
    msg.hd.rtype = 34;
    let bar_type = decode_bar_type(&msg, instrument_id).unwrap();
    assert_eq!(bar_type, BarType::from("ESM4.GLBX-1-HOUR-LAST-EXTERNAL"));

    // Test 1-day bar
    msg.hd.rtype = 35;
    let bar_type = decode_bar_type(&msg, instrument_id).unwrap();
    assert_eq!(bar_type, BarType::from("ESM4.GLBX-1-DAY-LAST-EXTERNAL"));

    // Test unsupported rtype
    msg.hd.rtype = 99;
    let result = decode_bar_type(&msg, instrument_id);
    assert!(result.is_err());
}

#[rstest]
fn test_decode_ts_event_adjustment() {
    let mut msg = dbn::OhlcvMsg::default_for_schema(dbn::Schema::Ohlcv1S);

    // Test 1-second bar adjustment
    msg.hd.rtype = 32;
    let adjustment = decode_ts_event_adjustment(&msg).unwrap();
    assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1S);

    // Test 1-minute bar adjustment
    msg.hd.rtype = 33;
    let adjustment = decode_ts_event_adjustment(&msg).unwrap();
    assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1M);

    // Test 1-hour bar adjustment
    msg.hd.rtype = 34;
    let adjustment = decode_ts_event_adjustment(&msg).unwrap();
    assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1H);

    // Test 1-day bar adjustment
    msg.hd.rtype = 35;
    let adjustment = decode_ts_event_adjustment(&msg).unwrap();
    assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1D);

    // Test eod bar adjustment (same as 1d)
    msg.hd.rtype = 36;
    let adjustment = decode_ts_event_adjustment(&msg).unwrap();
    assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1D);

    // Test unsupported rtype
    msg.hd.rtype = 99;
    let result = decode_ts_event_adjustment(&msg);
    assert!(result.is_err());
}

#[rstest]
fn test_decode_record() {
    // Test with MBO message
    let path = test_data_path().join("test_data.mbo.dbn.zst");
    let decoder = Decoder::from_zstd_file(path).unwrap();
    let mut dbn_stream = decoder.decode_stream::<dbn::MboMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let record_ref = dbn::RecordRef::from(msg);
    let instrument_id = InstrumentId::from("ESM4.GLBX");

    let (data1, data2) =
        decode_record(&record_ref, instrument_id, 2, Some(0.into()), true, false).unwrap();

    assert!(data1.is_some());
    assert!(data2.is_none());

    // Test with Trade message
    let path = test_data_path().join("test_data.trades.dbn.zst");
    let decoder = Decoder::from_zstd_file(path).unwrap();
    let mut dbn_stream = decoder.decode_stream::<dbn::TradeMsg>();
    let msg = dbn_stream.next().unwrap().unwrap();

    let record_ref = dbn::RecordRef::from(msg);

    let (data1, data2) =
        decode_record(&record_ref, instrument_id, 2, Some(0.into()), true, false).unwrap();

    assert!(data1.is_some());
    assert!(data2.is_none());
    assert!(matches!(data1.unwrap(), Data::Trade(_)));
}
