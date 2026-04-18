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

#![cfg(test)]
#![expect(clippy::too_many_arguments)]

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSideSpecified, TimeInForce},
    events::{OrderAccepted, OrderEventAny, OrderFilled, OrderSubmitted},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
    instruments::{
        Instrument, InstrumentAny,
        stubs::{audusd_sim, crypto_perpetual_ethusdt},
    },
    orders::{
        Order, OrderAny, OrderTestBuilder,
        stubs::{TestOrderEventStubs, TestOrderStubs},
    },
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rstest::{fixture, rstest};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use super::*;

#[fixture]
fn instrument() -> InstrumentAny {
    InstrumentAny::CurrencyPair(audusd_sim())
}

fn create_test_venue_order_id(value: &str) -> VenueOrderId {
    VenueOrderId::new(value)
}

fn submit_accept(order: &mut OrderAny, account_id: AccountId, venue_order_id: VenueOrderId) {
    let submitted = TestOrderEventStubs::submitted(order, account_id);
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(order, account_id, venue_order_id);
    order.apply(accepted).unwrap();
}

fn apply_fill(
    order: &mut OrderAny,
    instrument: &InstrumentAny,
    trade_id: TradeId,
    last_qty: Quantity,
    last_px: Price,
) {
    let fill = TestOrderEventStubs::filled(
        order,
        instrument,
        Some(trade_id),
        None,
        Some(last_px),
        Some(last_qty),
        Some(LiquiditySide::Taker),
        None,
        None,
        None,
    );
    order.apply(fill).unwrap();
}

#[rstest]
fn test_fill_snapshot_direction() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let buy_fill = FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id);
    assert_eq!(buy_fill.direction(), 1);

    let sell_fill = FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id);
    assert_eq!(sell_fill.direction(), -1);
}

#[rstest]
fn test_simulate_position_accumulate_long() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Buy, dec!(5), dec!(102), venue_order_id),
    ];

    let (qty, value) = simulate_position(&fills);
    assert_eq!(qty, dec!(15));
    assert_eq!(value, dec!(1510)); // 10*100 + 5*102
}

#[rstest]
fn test_simulate_position_close_and_flip() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(15), dec!(102), venue_order_id),
    ];

    let (qty, value) = simulate_position(&fills);
    assert_eq!(qty, dec!(-5)); // Flipped from +10 to -5
    assert_eq!(value, dec!(510)); // Remaining 5 @ 102
}

#[rstest]
fn test_simulate_position_partial_close() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(5), dec!(102), venue_order_id),
    ];

    let (qty, value) = simulate_position(&fills);
    assert_eq!(qty, dec!(5));
    assert_eq!(value, dec!(500)); // Reduced proportionally: 1000 * (1 - 5/10) = 500

    // Verify average price is maintained
    let avg_px = value / qty;
    assert_eq!(avg_px, dec!(100));
}

#[rstest]
fn test_simulate_position_multiple_partial_closes() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(10.0), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(25), dec!(11.0), venue_order_id), // Close 25%
        FillSnapshot::new(3000, OrderSide::Sell, dec!(25), dec!(12.0), venue_order_id), // Close another 25%
    ];

    let (qty, value) = simulate_position(&fills);
    assert_eq!(qty, dec!(50));
    // After first close: value = 1000 * (1 - 25/100) = 1000 * 0.75 = 750
    // After second close: value = 750 * (1 - 25/75) = 750 * (50/75) = 500
    // Due to decimal precision, we check it's close to 500
    assert!((value - dec!(500)).abs() < dec!(0.01));

    // Verify average price is maintained at 10.0
    let avg_px = value / qty;
    assert!((avg_px - dec!(10.0)).abs() < dec!(0.01));
}

#[rstest]
fn test_simulate_position_short_partial_close() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Buy, dec!(5), dec!(98), venue_order_id), // Partial close
    ];

    let (qty, value) = simulate_position(&fills);
    assert_eq!(qty, dec!(-5));
    assert_eq!(value, dec!(500)); // Reduced proportionally: 1000 * (1 - 5/10) = 500

    // Verify average price is maintained
    let avg_px = value / qty.abs();
    assert_eq!(avg_px, dec!(100));
}

#[rstest]
fn test_detect_zero_crossings() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), venue_order_id), // Close to zero
        FillSnapshot::new(3000, OrderSide::Buy, dec!(5), dec!(103), venue_order_id),
        FillSnapshot::new(4000, OrderSide::Sell, dec!(5), dec!(104), venue_order_id), // Close to zero again
    ];

    let crossings = detect_zero_crossings(&fills);
    assert_eq!(crossings.len(), 2);
    assert_eq!(crossings[0], 2000);
    assert_eq!(crossings[1], 4000);
}

#[rstest]
fn test_check_position_match_exact() {
    let result = check_position_match(dec!(10), dec!(1000), dec!(10), dec!(100), dec!(0.0001));
    assert!(result);
}

#[rstest]
fn test_check_position_match_within_tolerance() {
    // Simulated avg px = 1000/10 = 100, venue = 100.005
    // Relative diff = 0.005 / 100.005 = 0.00004999 < 0.0001
    let result = check_position_match(dec!(10), dec!(1000), dec!(10), dec!(100.005), dec!(0.0001));
    assert!(result);
}

#[rstest]
fn test_check_position_match_qty_mismatch() {
    let result = check_position_match(dec!(10), dec!(1000), dec!(11), dec!(100), dec!(0.0001));
    assert!(!result);
}

#[rstest]
fn test_check_position_match_both_flat() {
    let result = check_position_match(dec!(0), dec!(0), dec!(0), dec!(0), dec!(0.0001));
    assert!(result);
}

#[rstest]
fn test_reconciliation_price_flat_to_long(_instrument: InstrumentAny) {
    let result = calculate_reconciliation_price(dec!(0), None, dec!(10), Some(dec!(100)));
    assert!(result.is_some());
    assert_eq!(result.unwrap(), dec!(100));
}

#[rstest]
fn test_reconciliation_price_no_target_avg_px(_instrument: InstrumentAny) {
    let result = calculate_reconciliation_price(dec!(5), Some(dec!(100)), dec!(10), None);
    assert!(result.is_none());
}

#[rstest]
fn test_reconciliation_price_no_quantity_change(_instrument: InstrumentAny) {
    let result =
        calculate_reconciliation_price(dec!(10), Some(dec!(100)), dec!(10), Some(dec!(105)));
    assert!(result.is_none());
}

#[rstest]
fn test_reconciliation_price_long_position_increase(_instrument: InstrumentAny) {
    let result =
        calculate_reconciliation_price(dec!(10), Some(dec!(100)), dec!(15), Some(dec!(102)));
    assert!(result.is_some());
    // Expected: (15 * 102 - 10 * 100) / 5 = (1530 - 1000) / 5 = 106
    assert_eq!(result.unwrap(), dec!(106));
}

#[rstest]
fn test_reconciliation_price_flat_to_short(_instrument: InstrumentAny) {
    let result = calculate_reconciliation_price(dec!(0), None, dec!(-10), Some(dec!(100)));
    assert!(result.is_some());
    assert_eq!(result.unwrap(), dec!(100));
}

#[rstest]
fn test_reconciliation_price_long_to_flat(_instrument: InstrumentAny) {
    // Close long position to flat: 100 @ 1.20 to 0
    // When closing to flat, reconciliation price equals current average price
    let result =
        calculate_reconciliation_price(dec!(100), Some(dec!(1.20)), dec!(0), Some(dec!(0)));
    assert!(result.is_some());
    assert_eq!(result.unwrap(), dec!(1.20));
}

#[rstest]
fn test_reconciliation_price_short_to_flat(_instrument: InstrumentAny) {
    // Close short position to flat: -50 @ 2.50 to 0
    // When closing to flat, reconciliation price equals current average price
    let result = calculate_reconciliation_price(dec!(-50), Some(dec!(2.50)), dec!(0), None);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), dec!(2.50));
}

#[rstest]
fn test_reconciliation_price_short_position_increase(_instrument: InstrumentAny) {
    // Short position increase: -100 @ 1.30 to -200 @ 1.28
    // (−200 × 1.28) = (−100 × 1.30) + (−100 × reconciliation_px)
    // −256 = −130 + (−100 × reconciliation_px)
    // reconciliation_px = 1.26
    let result =
        calculate_reconciliation_price(dec!(-100), Some(dec!(1.30)), dec!(-200), Some(dec!(1.28)));
    assert!(result.is_some());
    assert_eq!(result.unwrap(), dec!(1.26));
}

#[rstest]
fn test_reconciliation_price_long_position_decrease(_instrument: InstrumentAny) {
    // Long position decrease: 200 @ 1.20 to 100 @ 1.20
    let result =
        calculate_reconciliation_price(dec!(200), Some(dec!(1.20)), dec!(100), Some(dec!(1.20)));
    assert!(result.is_some());
    assert_eq!(result.unwrap(), dec!(1.20));
}

#[rstest]
fn test_reconciliation_price_long_to_short_flip(_instrument: InstrumentAny) {
    // Long to short flip: 100 @ 1.20 to -100 @ 1.25
    // Due to netting simulation resetting value on flip, reconciliation_px = target_avg_px
    let result =
        calculate_reconciliation_price(dec!(100), Some(dec!(1.20)), dec!(-100), Some(dec!(1.25)));
    assert!(result.is_some());
    assert_eq!(result.unwrap(), dec!(1.25));
}

#[rstest]
fn test_reconciliation_price_short_to_long_flip(_instrument: InstrumentAny) {
    // Short to long flip: -100 @ 1.30 to 100 @ 1.25
    // Due to netting simulation resetting value on flip, reconciliation_px = target_avg_px
    let result =
        calculate_reconciliation_price(dec!(-100), Some(dec!(1.30)), dec!(100), Some(dec!(1.25)));
    assert!(result.is_some());
    assert_eq!(result.unwrap(), dec!(1.25));
}

#[rstest]
fn test_reconciliation_price_complex_scenario(_instrument: InstrumentAny) {
    // Complex: 150 @ 1.23456 to 250 @ 1.24567
    // (250 × 1.24567) = (150 × 1.23456) + (100 × reconciliation_px)
    // 311.4175 = 185.184 + (100 × reconciliation_px)
    // reconciliation_px = 1.262335
    let result = calculate_reconciliation_price(
        dec!(150),
        Some(dec!(1.23456)),
        dec!(250),
        Some(dec!(1.24567)),
    );
    assert!(result.is_some());
    assert_eq!(result.unwrap(), dec!(1.262335));
}

#[rstest]
fn test_reconciliation_price_zero_target_avg_px(_instrument: InstrumentAny) {
    let result =
        calculate_reconciliation_price(dec!(100), Some(dec!(1.20)), dec!(200), Some(dec!(0)));
    assert!(result.is_none());
}

#[rstest]
fn test_reconciliation_price_negative_price(_instrument: InstrumentAny) {
    // Negative price calculation: 100 @ 2.00 to 200 @ 1.00
    // (200 × 1.00) = (100 × 2.00) + (100 × reconciliation_px)
    // 200 = 200 + (100 × reconciliation_px)
    // reconciliation_px = 0 (should return None as price must be positive)
    let result =
        calculate_reconciliation_price(dec!(100), Some(dec!(2.00)), dec!(200), Some(dec!(1.00)));
    assert!(result.is_none());
}

#[rstest]
fn test_reconciliation_price_flip_simulation_compatibility() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    // Start with long position: 100 @ 1.20
    // Target: -100 @ 1.25
    // Calculate reconciliation price
    let recon_px =
        calculate_reconciliation_price(dec!(100), Some(dec!(1.20)), dec!(-100), Some(dec!(1.25)))
            .expect("reconciliation price");

    assert_eq!(recon_px, dec!(1.25));

    // Simulate the flip with reconciliation fill (sell 200 to go from +100 to -100)
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(1.20), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(200), recon_px, venue_order_id),
    ];

    let (final_qty, final_value) = simulate_position(&fills);
    assert_eq!(final_qty, dec!(-100));
    let final_avg = final_value / final_qty.abs();
    assert_eq!(final_avg, dec!(1.25), "Final average should match target");
}

#[rstest]
fn test_reconciliation_price_accumulation_simulation_compatibility() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    // Start with long position: 100 @ 1.20
    // Target: 200 @ 1.22
    let recon_px =
        calculate_reconciliation_price(dec!(100), Some(dec!(1.20)), dec!(200), Some(dec!(1.22)))
            .expect("reconciliation price");

    // Simulate accumulation with reconciliation fill
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(1.20), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Buy, dec!(100), recon_px, venue_order_id),
    ];

    let (final_qty, final_value) = simulate_position(&fills);
    assert_eq!(final_qty, dec!(200));
    let final_avg = final_value / final_qty.abs();
    assert_eq!(final_avg, dec!(1.22), "Final average should match target");
}

#[rstest]
fn test_simulate_position_accumulate_short() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(5), dec!(98), venue_order_id),
    ];

    let (qty, value) = simulate_position(&fills);
    assert_eq!(qty, dec!(-15));
    assert_eq!(value, dec!(1490)); // 10*100 + 5*98
}

#[rstest]
fn test_simulate_position_short_to_long_flip() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Buy, dec!(15), dec!(102), venue_order_id),
    ];

    let (qty, value) = simulate_position(&fills);
    assert_eq!(qty, dec!(5)); // Flipped from -10 to +5
    assert_eq!(value, dec!(510)); // Remaining 5 @ 102
}

#[rstest]
fn test_simulate_position_multiple_flips() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(15), dec!(105), venue_order_id), // Flip to -5
        FillSnapshot::new(3000, OrderSide::Buy, dec!(10), dec!(110), venue_order_id),  // Flip to +5
    ];

    let (qty, value) = simulate_position(&fills);
    assert_eq!(qty, dec!(5)); // Final position: +5
    assert_eq!(value, dec!(550)); // 5 @ 110
}

#[rstest]
fn test_simulate_position_empty_fills() {
    let fills: Vec<FillSnapshot> = vec![];
    let (qty, value) = simulate_position(&fills);
    assert_eq!(qty, dec!(0));
    assert_eq!(value, dec!(0));
}

#[rstest]
fn test_detect_zero_crossings_no_crossings() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Buy, dec!(5), dec!(102), venue_order_id),
    ];

    let crossings = detect_zero_crossings(&fills);
    assert_eq!(crossings.len(), 0);
}

#[rstest]
fn test_detect_zero_crossings_single_crossing() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), venue_order_id), // Close to zero
    ];

    let crossings = detect_zero_crossings(&fills);
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0], 2000);
}

#[rstest]
fn test_detect_zero_crossings_empty_fills() {
    let fills: Vec<FillSnapshot> = vec![];
    let crossings = detect_zero_crossings(&fills);
    assert_eq!(crossings.len(), 0);
}

#[rstest]
fn test_detect_zero_crossings_long_to_short_flip() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    // Buy 10, then Sell 15 -> flip from +10 to -5
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(15), dec!(102), venue_order_id), // Flip
    ];

    let crossings = detect_zero_crossings(&fills);
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0], 2000); // Detected the flip
}

#[rstest]
fn test_detect_zero_crossings_short_to_long_flip() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    // Sell 10, then Buy 20 -> flip from -10 to +10
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Buy, dec!(20), dec!(102), venue_order_id), // Flip
    ];

    let crossings = detect_zero_crossings(&fills);
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0], 2000);
}

#[rstest]
fn test_detect_zero_crossings_multiple_flips() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), venue_order_id), // Land on zero
        FillSnapshot::new(3000, OrderSide::Sell, dec!(5), dec!(103), venue_order_id),  // Go short
        FillSnapshot::new(4000, OrderSide::Buy, dec!(15), dec!(104), venue_order_id), // Flip to long
    ];

    let crossings = detect_zero_crossings(&fills);
    assert_eq!(crossings.len(), 2);
    assert_eq!(crossings[0], 2000); // First zero-crossing (land on zero)
    assert_eq!(crossings[1], 4000); // Second zero-crossing (flip)
}

#[rstest]
fn test_check_position_match_outside_tolerance() {
    // Simulated avg px = 1000/10 = 100, venue = 101
    // Relative diff = 1 / 101 = 0.0099 > 0.0001
    let result = check_position_match(dec!(10), dec!(1000), dec!(10), dec!(101), dec!(0.0001));
    assert!(!result);
}

#[rstest]
fn test_check_position_match_edge_of_tolerance() {
    // Simulated avg px = 1000/10 = 100, venue = 100.01
    // Relative diff = 0.01 / 100.01 = 0.00009999 < 0.0001
    let result = check_position_match(dec!(10), dec!(1000), dec!(10), dec!(100.01), dec!(0.0001));
    assert!(result);
}

#[rstest]
fn test_check_position_match_zero_venue_avg_px() {
    let result = check_position_match(dec!(10), dec!(1000), dec!(10), dec!(0), dec!(0.0001));
    assert!(!result); // Should fail because relative diff calculation with zero denominator
}

#[rstest]
fn test_adjust_fills_no_fills() {
    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(0.02),
        avg_px: dec!(4100.00),
    };
    let instrument = instrument();
    let result = adjust_fills_for_partial_window(&[], &venue_position, &instrument, dec!(0.0001));
    assert!(matches!(result, FillAdjustmentResult::NoAdjustment));
}

#[rstest]
fn test_adjust_fills_flat_position() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fills = vec![FillSnapshot::new(
        1000,
        OrderSide::Buy,
        dec!(0.01),
        dec!(4100.00),
        venue_order_id,
    )];
    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(0),
        avg_px: dec!(0),
    };
    let instrument = instrument();
    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));
    assert!(matches!(result, FillAdjustmentResult::NoAdjustment));
}

#[rstest]
fn test_adjust_fills_complete_lifecycle_no_adjustment() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let venue_order_id2 = create_test_venue_order_id("ORDER2");
    let fills = vec![
        FillSnapshot::new(
            1000,
            OrderSide::Buy,
            dec!(0.01),
            dec!(4100.00),
            venue_order_id,
        ),
        FillSnapshot::new(
            2000,
            OrderSide::Buy,
            dec!(0.01),
            dec!(4100.00),
            venue_order_id2,
        ),
    ];
    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(0.02),
        avg_px: dec!(4100.00),
    };
    let instrument = instrument();
    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));
    assert!(matches!(result, FillAdjustmentResult::NoAdjustment));
}

#[rstest]
fn test_adjust_fills_incomplete_lifecycle_adds_synthetic() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    // Window only sees +0.02 @ 4200, but venue has 0.04 @ 4100
    let fills = vec![FillSnapshot::new(
        2000,
        OrderSide::Buy,
        dec!(0.02),
        dec!(4200.00),
        venue_order_id,
    )];
    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(0.04),
        avg_px: dec!(4100.00),
    };
    let instrument = instrument();
    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    match result {
        FillAdjustmentResult::AddSyntheticOpening {
            synthetic_fill,
            existing_fills,
        } => {
            assert_eq!(synthetic_fill.side, OrderSide::Buy);
            assert_eq!(synthetic_fill.qty, dec!(0.02)); // Missing 0.02
            assert_eq!(existing_fills.len(), 1);
        }
        _ => panic!("Expected AddSyntheticOpening"),
    }
}

#[rstest]
fn test_adjust_fills_with_zero_crossings() {
    let venue_order_id1 = create_test_venue_order_id("ORDER1");
    let venue_order_id2 = create_test_venue_order_id("ORDER2");
    let venue_order_id3 = create_test_venue_order_id("ORDER3");

    // Lifecycle 1: LONG 0.02 -> FLAT (zero-crossing at 2000)
    // Lifecycle 2: LONG 0.03 (current)
    let fills = vec![
        FillSnapshot::new(
            1000,
            OrderSide::Buy,
            dec!(0.02),
            dec!(4100.00),
            venue_order_id1,
        ),
        FillSnapshot::new(
            2000,
            OrderSide::Sell,
            dec!(0.02),
            dec!(4150.00),
            venue_order_id2,
        ), // Zero-crossing
        FillSnapshot::new(
            3000,
            OrderSide::Buy,
            dec!(0.03),
            dec!(4200.00),
            venue_order_id3,
        ), // Current lifecycle
    ];

    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(0.03),
        avg_px: dec!(4200.00),
    };

    let instrument = instrument();
    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    // Should filter to current lifecycle only
    match result {
        FillAdjustmentResult::FilterToCurrentLifecycle {
            last_zero_crossing_ts,
            current_lifecycle_fills,
        } => {
            assert_eq!(last_zero_crossing_ts, 2000);
            assert_eq!(current_lifecycle_fills.len(), 1);
            assert_eq!(current_lifecycle_fills[0].venue_order_id, venue_order_id3);
        }
        _ => panic!("Expected FilterToCurrentLifecycle, was {result:?}"),
    }
}

#[rstest]
fn test_adjust_fills_multiple_zero_crossings_mismatch() {
    let venue_order_id1 = create_test_venue_order_id("ORDER1");
    let venue_order_id2 = create_test_venue_order_id("ORDER2");
    let _venue_order_id3 = create_test_venue_order_id("ORDER3");
    let venue_order_id4 = create_test_venue_order_id("ORDER4");
    let venue_order_id5 = create_test_venue_order_id("ORDER5");

    // Lifecycle 1: LONG 0.05 -> FLAT
    // Lifecycle 2: Current fills produce 0.10 @ 4050, but venue has 0.05 @ 4142.04
    let fills = vec![
        FillSnapshot::new(
            1000,
            OrderSide::Buy,
            dec!(0.05),
            dec!(4000.00),
            venue_order_id1,
        ),
        FillSnapshot::new(
            2000,
            OrderSide::Sell,
            dec!(0.05),
            dec!(4050.00),
            venue_order_id2,
        ), // Zero-crossing
        FillSnapshot::new(
            3000,
            OrderSide::Buy,
            dec!(0.05),
            dec!(4000.00),
            venue_order_id4,
        ), // Current lifecycle
        FillSnapshot::new(
            4000,
            OrderSide::Buy,
            dec!(0.05),
            dec!(4100.00),
            venue_order_id5,
        ), // Current lifecycle
    ];

    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(0.05),
        avg_px: dec!(4142.04),
    };

    let instrument = instrument();
    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    // Should replace current lifecycle with synthetic
    match result {
        FillAdjustmentResult::ReplaceCurrentLifecycle {
            synthetic_fill,
            first_venue_order_id,
        } => {
            assert_eq!(synthetic_fill.qty, dec!(0.05));
            assert_eq!(synthetic_fill.px, dec!(4142.04));
            assert_eq!(synthetic_fill.side, OrderSide::Buy);
            assert_eq!(first_venue_order_id, venue_order_id4);
        }
        _ => panic!("Expected ReplaceCurrentLifecycle, was {result:?}"),
    }
}

#[rstest]
fn test_adjust_fills_short_position() {
    let venue_order_id = create_test_venue_order_id("ORDER1");

    // Window only sees SELL 0.02 @ 4120, but venue has -0.05 @ 4100
    let fills = vec![FillSnapshot::new(
        1000,
        OrderSide::Sell,
        dec!(0.02),
        dec!(4120.00),
        venue_order_id,
    )];

    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Sell,
        qty: dec!(0.05),
        avg_px: dec!(4100.00),
    };

    let instrument = instrument();
    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    // Should add synthetic opening SHORT fill
    match result {
        FillAdjustmentResult::AddSyntheticOpening {
            synthetic_fill,
            existing_fills,
        } => {
            assert_eq!(synthetic_fill.side, OrderSide::Sell);
            assert_eq!(synthetic_fill.qty, dec!(0.03)); // Missing 0.03
            assert_eq!(existing_fills.len(), 1);
        }
        _ => panic!("Expected AddSyntheticOpening, was {result:?}"),
    }
}

#[rstest]
fn test_adjust_fills_timestamp_underflow_protection() {
    let venue_order_id = create_test_venue_order_id("ORDER1");

    // First fill at timestamp 0 - saturating_sub should prevent underflow
    let fills = vec![FillSnapshot::new(
        0,
        OrderSide::Buy,
        dec!(0.01),
        dec!(4100.00),
        venue_order_id,
    )];

    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(0.02),
        avg_px: dec!(4100.00),
    };

    let instrument = instrument();
    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    // Should add synthetic fill with timestamp 0 (not u64::MAX)
    match result {
        FillAdjustmentResult::AddSyntheticOpening { synthetic_fill, .. } => {
            assert_eq!(synthetic_fill.ts_event, 0); // saturating_sub(1) from 0 = 0
        }
        _ => panic!("Expected AddSyntheticOpening, was {result:?}"),
    }
}

#[rstest]
fn test_adjust_fills_with_flip_scenario() {
    let venue_order_id1 = create_test_venue_order_id("ORDER1");
    let venue_order_id2 = create_test_venue_order_id("ORDER2");

    // Long 10 @ 100, then Sell 20 @ 105 -> flip to Short 10 @ 105
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id1),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(20), dec!(105), venue_order_id2), // Flip
    ];

    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Sell,
        qty: dec!(10),
        avg_px: dec!(105),
    };

    let instrument = instrument();
    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    // Should recognize the flip and match correctly
    match result {
        FillAdjustmentResult::NoAdjustment => {
            // Verify simulation matches
            let (qty, value) = simulate_position(&fills);
            assert_eq!(qty, dec!(-10));
            let avg = value / qty.abs();
            assert_eq!(avg, dec!(105));
        }
        _ => panic!("Expected NoAdjustment for matching flip, was {result:?}"),
    }
}

#[rstest]
fn test_detect_zero_crossings_complex_lifecycle() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    // Complex scenario with multiple lifecycles
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(1.20), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(50), dec!(1.25), venue_order_id), // Reduce
        FillSnapshot::new(3000, OrderSide::Sell, dec!(100), dec!(1.30), venue_order_id), // Flip to -50
        FillSnapshot::new(4000, OrderSide::Buy, dec!(50), dec!(1.28), venue_order_id), // Close to zero
        FillSnapshot::new(5000, OrderSide::Buy, dec!(75), dec!(1.22), venue_order_id), // Open long
        FillSnapshot::new(6000, OrderSide::Sell, dec!(150), dec!(1.24), venue_order_id), // Flip to -75
    ];

    let crossings = detect_zero_crossings(&fills);
    assert_eq!(crossings.len(), 3);
    assert_eq!(crossings[0], 3000); // First flip
    assert_eq!(crossings[1], 4000); // Close to zero
    assert_eq!(crossings[2], 6000); // Second flip
}

#[rstest]
fn test_reconciliation_price_partial_close() {
    let venue_order_id = create_test_venue_order_id("ORDER1");
    // Partial close scenario: 100 @ 1.20 to 50 @ 1.20
    let recon_px =
        calculate_reconciliation_price(dec!(100), Some(dec!(1.20)), dec!(50), Some(dec!(1.20)))
            .expect("reconciliation price");

    // Simulate partial close
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(1.20), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(50), recon_px, venue_order_id),
    ];

    let (final_qty, final_value) = simulate_position(&fills);
    assert_eq!(final_qty, dec!(50));
    let final_avg = final_value / final_qty.abs();
    assert_eq!(final_avg, dec!(1.20), "Average should be maintained");
}

#[rstest]
fn test_detect_zero_crossings_identical_timestamps() {
    let venue_order_id1 = create_test_venue_order_id("ORDER1");
    let venue_order_id2 = create_test_venue_order_id("ORDER2");

    // Two fills with identical timestamps - should process deterministically
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id1),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(5), dec!(102), venue_order_id1),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(5), dec!(103), venue_order_id2), // Same ts
    ];

    let crossings = detect_zero_crossings(&fills);

    // Position: +10 -> +5 -> 0 (zero crossing at last fill)
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0], 2000);

    // Verify final position is flat
    let (qty, _) = simulate_position(&fills);
    assert_eq!(qty, dec!(0));
}

#[rstest]
fn test_detect_zero_crossings_five_lifecycles() {
    let venue_order_id = create_test_venue_order_id("ORDER1");

    // Five complete position lifecycles: open->close repeated 5 times
    let fills = vec![
        // Lifecycle 1: Long
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(101), venue_order_id),
        // Lifecycle 2: Short
        FillSnapshot::new(3000, OrderSide::Sell, dec!(20), dec!(102), venue_order_id),
        FillSnapshot::new(4000, OrderSide::Buy, dec!(20), dec!(101), venue_order_id),
        // Lifecycle 3: Long
        FillSnapshot::new(5000, OrderSide::Buy, dec!(15), dec!(103), venue_order_id),
        FillSnapshot::new(6000, OrderSide::Sell, dec!(15), dec!(104), venue_order_id),
        // Lifecycle 4: Short
        FillSnapshot::new(7000, OrderSide::Sell, dec!(25), dec!(105), venue_order_id),
        FillSnapshot::new(8000, OrderSide::Buy, dec!(25), dec!(104), venue_order_id),
        // Lifecycle 5: Long (still open)
        FillSnapshot::new(9000, OrderSide::Buy, dec!(30), dec!(106), venue_order_id),
    ];

    let crossings = detect_zero_crossings(&fills);

    // Should detect 4 zero-crossings (positions closing to flat)
    assert_eq!(crossings.len(), 4);
    assert_eq!(crossings[0], 2000);
    assert_eq!(crossings[1], 4000);
    assert_eq!(crossings[2], 6000);
    assert_eq!(crossings[3], 8000);

    // Final position should be +30
    let (qty, _) = simulate_position(&fills);
    assert_eq!(qty, dec!(30));
}

#[rstest]
fn test_adjust_fills_five_zero_crossings(instrument: InstrumentAny) {
    let venue_order_id = create_test_venue_order_id("ORDER1");

    // Complex scenario: 4 complete lifecycles + current open position
    let fills = vec![
        // Old lifecycles (should be filtered out)
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(101), venue_order_id),
        FillSnapshot::new(3000, OrderSide::Sell, dec!(20), dec!(102), venue_order_id),
        FillSnapshot::new(4000, OrderSide::Buy, dec!(20), dec!(101), venue_order_id),
        FillSnapshot::new(5000, OrderSide::Buy, dec!(15), dec!(103), venue_order_id),
        FillSnapshot::new(6000, OrderSide::Sell, dec!(15), dec!(104), venue_order_id),
        FillSnapshot::new(7000, OrderSide::Sell, dec!(25), dec!(105), venue_order_id),
        FillSnapshot::new(8000, OrderSide::Buy, dec!(25), dec!(104), venue_order_id),
        // Current lifecycle (should be kept)
        FillSnapshot::new(9000, OrderSide::Buy, dec!(30), dec!(106), venue_order_id),
    ];

    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(30),
        avg_px: dec!(106),
    };

    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    // Should filter to current lifecycle only (after last zero-crossing at 8000)
    match result {
        FillAdjustmentResult::FilterToCurrentLifecycle {
            last_zero_crossing_ts,
            current_lifecycle_fills,
        } => {
            assert_eq!(last_zero_crossing_ts, 8000);
            assert_eq!(current_lifecycle_fills.len(), 1);
            assert_eq!(current_lifecycle_fills[0].ts_event, 9000);
            assert_eq!(current_lifecycle_fills[0].qty, dec!(30));
        }
        _ => panic!("Expected FilterToCurrentLifecycle, was {result:?}"),
    }
}

#[rstest]
fn test_adjust_fills_alternating_long_short_positions(instrument: InstrumentAny) {
    let venue_order_id = create_test_venue_order_id("ORDER1");

    // Alternating: Long -> Short -> Long -> Short -> Long
    // These are flips (sign changes) but never go to exactly zero
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(20), dec!(102), venue_order_id), // Flip to -10
        FillSnapshot::new(3000, OrderSide::Buy, dec!(20), dec!(101), venue_order_id), // Flip to +10
        FillSnapshot::new(4000, OrderSide::Sell, dec!(20), dec!(103), venue_order_id), // Flip to -10
        FillSnapshot::new(5000, OrderSide::Buy, dec!(20), dec!(102), venue_order_id), // Flip to +10
    ];

    // Current position: +10 @ 102
    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(10),
        avg_px: dec!(102),
    };

    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    // Position never went flat (0), just flipped sides. This is treated as one
    // continuous lifecycle since no explicit close occurred. The final position
    // matches so no adjustment needed.
    assert!(
        matches!(result, FillAdjustmentResult::NoAdjustment),
        "Expected NoAdjustment (continuous lifecycle with matching position), was {result:?}"
    );
}

#[rstest]
fn test_adjust_fills_with_flat_crossings(instrument: InstrumentAny) {
    let venue_order_id = create_test_venue_order_id("ORDER1");

    // Proper lifecycle boundaries with flat crossings (position goes to exactly 0)
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), venue_order_id), // Close to 0
        FillSnapshot::new(3000, OrderSide::Sell, dec!(10), dec!(101), venue_order_id), // New short
        FillSnapshot::new(4000, OrderSide::Buy, dec!(10), dec!(99), venue_order_id),   // Close to 0
        FillSnapshot::new(5000, OrderSide::Buy, dec!(10), dec!(98), venue_order_id),   // New long
    ];

    // Current position: +10 @ 98
    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(10),
        avg_px: dec!(98),
    };

    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    // Position went flat at ts=2000 and ts=4000
    // Current lifecycle starts after last flat (4000)
    match result {
        FillAdjustmentResult::FilterToCurrentLifecycle {
            last_zero_crossing_ts,
            current_lifecycle_fills,
        } => {
            assert_eq!(last_zero_crossing_ts, 4000);
            assert_eq!(current_lifecycle_fills.len(), 1);
            assert_eq!(current_lifecycle_fills[0].ts_event, 5000);
            assert_eq!(current_lifecycle_fills[0].qty, dec!(10));
        }
        _ => panic!("Expected FilterToCurrentLifecycle, was {result:?}"),
    }
}

#[rstest]
fn test_replace_current_lifecycle_uses_first_venue_order_id(instrument: InstrumentAny) {
    let order_id_1 = create_test_venue_order_id("ORDER1");
    let order_id_2 = create_test_venue_order_id("ORDER2");
    let order_id_3 = create_test_venue_order_id("ORDER3");

    // Previous lifecycle closes, then current lifecycle has fills from multiple orders
    let fills = vec![
        FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), order_id_1),
        FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), order_id_1), // Close to 0
        // Current lifecycle: fills from different venue order IDs
        FillSnapshot::new(3000, OrderSide::Buy, dec!(5), dec!(103), order_id_2),
        FillSnapshot::new(4000, OrderSide::Buy, dec!(5), dec!(104), order_id_3),
    ];

    // Venue position differs from simulated (+10 @ 103.5) to trigger replacement
    let venue_position = VenuePositionSnapshot {
        side: OrderSide::Buy,
        qty: dec!(15),
        avg_px: dec!(105),
    };

    let result =
        adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

    // Should replace with synthetic fill using first fill's venue_order_id (order_id_2)
    match result {
        FillAdjustmentResult::ReplaceCurrentLifecycle {
            synthetic_fill,
            first_venue_order_id,
        } => {
            assert_eq!(first_venue_order_id, order_id_2);
            assert_eq!(synthetic_fill.venue_order_id, order_id_2);
            assert_eq!(synthetic_fill.qty, dec!(15));
            assert_eq!(synthetic_fill.px, dec!(105));
        }
        _ => panic!("Expected ReplaceCurrentLifecycle, was {result:?}"),
    }
}

fn make_test_report(
    instrument_id: InstrumentId,
    order_type: OrderType,
    status: OrderStatus,
    filled_qty: &str,
    post_only: bool,
) -> OrderStatusReport {
    let account_id = AccountId::from("TEST-001");
    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None,
        VenueOrderId::from("V-001"),
        OrderSide::Buy,
        order_type,
        TimeInForce::Gtc,
        status,
        Quantity::from("1.0"),
        Quantity::from(filled_qty),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_price(Price::from("100.00"))
    .with_avg_px(100.0)
    .unwrap();
    report.post_only = post_only;
    report
}

#[rstest]
#[case::accepted(OrderStatus::Accepted, "0", 1, "Accepted")]
#[case::triggered(OrderStatus::Triggered, "0", 1, "Accepted")]
#[case::canceled(OrderStatus::Canceled, "0", 2, "Canceled")]
#[case::expired(OrderStatus::Expired, "0", 2, "Expired")]
#[case::filled(OrderStatus::Filled, "1.0", 2, "Filled")]
#[case::partially_filled(OrderStatus::PartiallyFilled, "0.5", 2, "Filled")]
#[case::rejected(OrderStatus::Rejected, "0", 1, "Rejected")]
fn test_external_order_status_event_generation(
    #[case] status: OrderStatus,
    #[case] filled_qty: &str,
    #[case] expected_events: usize,
    #[case] last_event_type: &str,
) {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.00"))
        .build();
    let report = make_test_report(instrument.id(), OrderType::Limit, status, filled_qty, false);

    let events = generate_external_order_status_events(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        UnixNanos::from(2_000_000),
    );

    assert_eq!(events.len(), expected_events, "status={status}");
    let last = events.last().unwrap();
    let actual_type = match last {
        OrderEventAny::Accepted(_) => "Accepted",
        OrderEventAny::Canceled(_) => "Canceled",
        OrderEventAny::Expired(_) => "Expired",
        OrderEventAny::Filled(_) => "Filled",
        OrderEventAny::Rejected(_) => "Rejected",
        _ => "Other",
    };
    assert_eq!(actual_type, last_event_type, "status={status}");
}

#[rstest]
#[case::market(OrderType::Market, false, LiquiditySide::Taker)]
#[case::stop_market(OrderType::StopMarket, false, LiquiditySide::Taker)]
#[case::trailing_stop_market(OrderType::TrailingStopMarket, false, LiquiditySide::Taker)]
#[case::limit_post_only(OrderType::Limit, true, LiquiditySide::Maker)]
#[case::limit_default(OrderType::Limit, false, LiquiditySide::NoLiquiditySide)]
fn test_inferred_fill_liquidity_side(
    #[case] order_type: OrderType,
    #[case] post_only: bool,
    #[case] expected: LiquiditySide,
) {
    let instrument = crypto_perpetual_ethusdt();
    let order = match order_type {
        OrderType::Limit => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .price(Price::from("100.00"))
            .build(),
        OrderType::StopMarket => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .trigger_price(Price::from("100.00"))
            .build(),
        OrderType::TrailingStopMarket => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .trigger_price(Price::from("100.00"))
            .trailing_offset(dec!(1.0))
            .build(),
        _ => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build(),
    };
    let report = make_test_report(
        instrument.id(),
        order_type,
        OrderStatus::Filled,
        "1.0",
        post_only,
    );

    let fill = create_inferred_fill(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        UnixNanos::from(2_000_000),
        None,
    );

    let filled = match fill.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };
    assert_eq!(
        filled.liquidity_side, expected,
        "order_type={order_type}, post_only={post_only}"
    );
}

#[rstest]
fn test_inferred_fill_no_price_returns_none() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .build();

    let report = OrderStatusReport::new(
        AccountId::from("TEST-001"),
        instrument.id(),
        None,
        VenueOrderId::from("V-001"),
        OrderSide::Buy,
        OrderType::Market,
        TimeInForce::Ioc,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let fill = create_inferred_fill(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        UnixNanos::from(2_000_000),
        None,
    );

    assert!(fill.is_none());
}

// Tests for reconcile_fill_report

fn create_test_fill_report(
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    trade_id: TradeId,
    last_qty: Quantity,
    last_px: Price,
) -> FillReport {
    FillReport::new(
        AccountId::from("TEST-001"),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        last_qty,
        last_px,
        Money::new(0.10, Currency::USD()),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
}

#[rstest]
fn test_reconcile_fill_report_success(instrument: InstrumentAny) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();

    let fill_report = create_test_fill_report(
        instrument.id(),
        VenueOrderId::from("V-001"),
        TradeId::from("T-001"),
        Quantity::from("50"),
        Price::from("1.00000"),
    );

    let result = reconcile_fill_report(
        &order,
        &fill_report,
        &instrument,
        UnixNanos::from(2_000_000),
        false,
    );

    assert!(result.is_some());
    if let Some(OrderEventAny::Filled(filled)) = result {
        assert_eq!(filled.last_qty, Quantity::from("50"));
        assert_eq!(filled.last_px, Price::from("1.00000"));
        assert_eq!(filled.trade_id, TradeId::from("T-001"));
        assert!(filled.reconciliation);
    } else {
        panic!("Expected OrderFilled event");
    }
}

#[rstest]
fn test_reconcile_fill_report_duplicate_detected(instrument: InstrumentAny) {
    // Create an order
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();

    let account_id = AccountId::from("TEST-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let trade_id = TradeId::from("T-001");

    // Submit the order first
    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        account_id,
        UUID4::new(),
        UnixNanos::from(500_000),
        UnixNanos::from(500_000),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    // Accept the order
    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        account_id,
        UUID4::new(),
        UnixNanos::from(600_000),
        UnixNanos::from(600_000),
        false,
    );
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    // Now apply a fill to the order
    let filled_event = OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        account_id,
        trade_id,
        OrderSide::Buy,
        order.order_type(),
        Quantity::from("50"),
        Price::from("1.00000"),
        Currency::USD(),
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        false,
        None,
        None,
    );
    order.apply(OrderEventAny::Filled(filled_event)).unwrap();

    // Now try to reconcile the same fill - should be rejected as duplicate
    let fill_report = create_test_fill_report(
        instrument.id(),
        venue_order_id,
        trade_id, // Same trade_id
        Quantity::from("50"),
        Price::from("1.00000"),
    );

    let result = reconcile_fill_report(
        &order,
        &fill_report,
        &instrument,
        UnixNanos::from(2_000_000),
        false,
    );

    assert!(result.is_none());
}

#[rstest]
fn test_reconcile_fill_report_overfill_rejected(instrument: InstrumentAny) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();

    // Fill for 150 would overfill a 100 qty order
    let fill_report = create_test_fill_report(
        instrument.id(),
        VenueOrderId::from("V-001"),
        TradeId::from("T-001"),
        Quantity::from("150"),
        Price::from("1.00000"),
    );

    let result = reconcile_fill_report(
        &order,
        &fill_report,
        &instrument,
        UnixNanos::from(2_000_000),
        false, // Don't allow overfills
    );

    assert!(result.is_none());
}

#[rstest]
fn test_reconcile_fill_report_overfill_allowed(instrument: InstrumentAny) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();

    let fill_report = create_test_fill_report(
        instrument.id(),
        VenueOrderId::from("V-001"),
        TradeId::from("T-001"),
        Quantity::from("150"),
        Price::from("1.00000"),
    );

    let result = reconcile_fill_report(
        &order,
        &fill_report,
        &instrument,
        UnixNanos::from(2_000_000),
        true, // Allow overfills
    );

    // Should produce a fill event when overfills are allowed
    assert!(result.is_some());
}

// Tests for check_position_reconciliation

#[rstest]
fn test_check_position_reconciliation_both_flat() {
    let report = PositionStatusReport::new(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUDUSD.SIM"),
        PositionSideSpecified::Flat,
        Quantity::from("0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None,
    );

    let result = check_position_reconciliation(&report, dec!(0), Some(5));
    assert!(result);
}

#[rstest]
fn test_check_position_reconciliation_exact_match_long() {
    let report = PositionStatusReport::new(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUDUSD.SIM"),
        PositionSideSpecified::Long,
        Quantity::from("100"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None,
    );

    let result = check_position_reconciliation(&report, dec!(100), Some(0));
    assert!(result);
}

#[rstest]
fn test_check_position_reconciliation_exact_match_short() {
    let report = PositionStatusReport::new(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUDUSD.SIM"),
        PositionSideSpecified::Short,
        Quantity::from("50"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None,
    );

    let result = check_position_reconciliation(&report, dec!(-50), Some(0));
    assert!(result);
}

#[rstest]
fn test_check_position_reconciliation_within_tolerance() {
    let report = PositionStatusReport::new(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUDUSD.SIM"),
        PositionSideSpecified::Long,
        Quantity::from("100.00001"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None,
    );

    // Cached qty is slightly different but within tolerance
    let result = check_position_reconciliation(&report, dec!(100.00000), Some(5));
    assert!(result);
}

#[rstest]
fn test_check_position_reconciliation_discrepancy() {
    let report = PositionStatusReport::new(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUDUSD.SIM"),
        PositionSideSpecified::Long,
        Quantity::from("100"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None,
    );

    // Cached qty is significantly different
    let result = check_position_reconciliation(&report, dec!(50), Some(0));
    assert!(!result);
}

// Tests for is_within_single_unit_tolerance

#[rstest]
fn test_is_within_single_unit_tolerance_exact_match() {
    assert!(is_within_single_unit_tolerance(dec!(100), dec!(100), 0));
    assert!(is_within_single_unit_tolerance(
        dec!(100.12345),
        dec!(100.12345),
        5
    ));
}

#[rstest]
fn test_is_within_single_unit_tolerance_integer_precision() {
    // Integer precision requires exact match
    assert!(is_within_single_unit_tolerance(dec!(100), dec!(100), 0));
    assert!(!is_within_single_unit_tolerance(dec!(100), dec!(101), 0));
}

#[rstest]
fn test_is_within_single_unit_tolerance_fractional_precision() {
    // With precision 2, tolerance is 0.01
    assert!(is_within_single_unit_tolerance(dec!(100), dec!(100.01), 2));
    assert!(is_within_single_unit_tolerance(dec!(100), dec!(99.99), 2));
    assert!(!is_within_single_unit_tolerance(dec!(100), dec!(100.02), 2));
}

#[rstest]
fn test_is_within_single_unit_tolerance_high_precision() {
    // With precision 5, tolerance is 0.00001
    assert!(is_within_single_unit_tolerance(
        dec!(100),
        dec!(100.00001),
        5
    ));
    assert!(is_within_single_unit_tolerance(
        dec!(100),
        dec!(99.99999),
        5
    ));
    assert!(!is_within_single_unit_tolerance(
        dec!(100),
        dec!(100.00002),
        5
    ));
}

fn create_test_order_status_report(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    order_type: OrderType,
    order_status: OrderStatus,
    quantity: Quantity,
    filled_qty: Quantity,
) -> OrderStatusReport {
    OrderStatusReport::new(
        AccountId::from("SIM-001"),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy,
        order_type,
        TimeInForce::Gtc,
        order_status,
        quantity,
        filled_qty,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
}

#[rstest]
#[case::identical_limit_order(
    OrderType::Limit,
    Quantity::from(100),
    Some(Price::from("1.00000")),
    None,
    Quantity::from(100),
    Some(Price::from("1.00000")),
    None,
    false
)]
#[case::quantity_changed(
    OrderType::Limit,
    Quantity::from(100),
    Some(Price::from("1.00000")),
    None,
    Quantity::from(150),
    Some(Price::from("1.00000")),
    None,
    true
)]
#[case::limit_price_changed(
    OrderType::Limit,
    Quantity::from(100),
    Some(Price::from("1.00000")),
    None,
    Quantity::from(100),
    Some(Price::from("1.00100")),
    None,
    true
)]
#[case::stop_trigger_changed(
    OrderType::StopMarket,
    Quantity::from(100),
    None,
    Some(Price::from("0.99000")),
    Quantity::from(100),
    None,
    Some(Price::from("0.98000")),
    true
)]
#[case::stop_limit_trigger_changed(
    OrderType::StopLimit,
    Quantity::from(100),
    Some(Price::from("1.00000")),
    Some(Price::from("0.99000")),
    Quantity::from(100),
    Some(Price::from("1.00000")),
    Some(Price::from("0.98000")),
    true
)]
#[case::stop_limit_price_changed(
    OrderType::StopLimit,
    Quantity::from(100),
    Some(Price::from("1.00000")),
    Some(Price::from("0.99000")),
    Quantity::from(100),
    Some(Price::from("1.00100")),
    Some(Price::from("0.99000")),
    true
)]
#[case::market_order_no_update(
    OrderType::Market,
    Quantity::from(100),
    None,
    None,
    Quantity::from(100),
    None,
    None,
    false
)]
fn test_should_reconciliation_update(
    instrument: InstrumentAny,
    #[case] order_type: OrderType,
    #[case] order_qty: Quantity,
    #[case] order_price: Option<Price>,
    #[case] order_trigger: Option<Price>,
    #[case] report_qty: Quantity,
    #[case] report_price: Option<Price>,
    #[case] report_trigger: Option<Price>,
    #[case] expected: bool,
) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = match (order_price, order_trigger) {
        (Some(price), Some(trigger)) => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(order_qty)
            .price(price)
            .trigger_price(trigger)
            .build(),
        (Some(price), None) => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(order_qty)
            .price(price)
            .build(),
        (None, Some(trigger)) => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(order_qty)
            .trigger_price(trigger)
            .build(),
        (None, None) => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(order_qty)
            .build(),
    };

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        order_type,
        OrderStatus::Accepted,
        report_qty,
        Quantity::from(0),
    );
    report.price = report_price;
    report.trigger_price = report_trigger;

    assert_eq!(should_reconciliation_update(&order, &report), expected);
}

#[rstest]
fn test_reconcile_order_report_already_in_sync(instrument: InstrumentAny) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Accepted,
        Quantity::from(100),
        Quantity::from(0),
    );
    report.price = Some(Price::from("1.00000"));

    let result = reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
    assert!(result.is_none());
}

#[rstest]
fn test_reconcile_order_report_generates_canceled(instrument: InstrumentAny) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    let report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Canceled,
        Quantity::from(100),
        Quantity::from(0),
    );

    let result = reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), OrderEventAny::Canceled(_)));
}

#[rstest]
fn test_generate_reconciliation_order_events_accepts_before_cancel(instrument: InstrumentAny) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Canceled,
        Quantity::from(100),
        Quantity::from(0),
    );

    let events = generate_reconciliation_order_events(
        &order,
        &report,
        Some(&instrument),
        UnixNanos::default(),
    );

    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(events[1], OrderEventAny::Canceled(_)));
}

#[rstest]
fn test_generate_reconciliation_order_events_accepts_before_fill(instrument: InstrumentAny) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Filled,
        Quantity::from(100),
        Quantity::from(100),
    );
    report.avg_px = Some(dec!(1.0));

    let events = generate_reconciliation_order_events(
        &order,
        &report,
        Some(&instrument),
        UnixNanos::default(),
    );

    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(events[1], OrderEventAny::Filled(_)));
}

#[rstest]
fn test_generate_reconciliation_order_events_does_not_accept_before_reject(
    instrument: InstrumentAny,
) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Rejected,
        Quantity::from(100),
        Quantity::from(0),
    );
    report.cancel_reason = Some("INSUFFICIENT_MARGIN".to_string());

    let events = generate_reconciliation_order_events(
        &order,
        &report,
        Some(&instrument),
        UnixNanos::default(),
    );

    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], OrderEventAny::Rejected(_)));
}

#[rstest]
fn test_reconcile_order_report_generates_expired(instrument: InstrumentAny) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    let report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Expired,
        Quantity::from(100),
        Quantity::from(0),
    );

    let result = reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), OrderEventAny::Expired(_)));
}

#[rstest]
fn test_reconcile_order_report_generates_rejected(instrument: InstrumentAny) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Rejected,
        Quantity::from(100),
        Quantity::from(0),
    );
    report.cancel_reason = Some("INSUFFICIENT_MARGIN".to_string());

    let result = reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
    assert!(result.is_some());
    if let OrderEventAny::Rejected(rejected) = result.unwrap() {
        assert_eq!(rejected.reason.as_str(), "INSUFFICIENT_MARGIN");
        assert_eq!(rejected.reconciliation, 1);
    } else {
        panic!("Expected Rejected event");
    }
}

#[rstest]
fn test_reconcile_order_report_generates_updated(instrument: InstrumentAny) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    // Report with changed price - same status, same filled_qty
    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Accepted,
        Quantity::from(100),
        Quantity::from(0),
    );
    report.price = Some(Price::from("1.00100"));

    let result = reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), OrderEventAny::Updated(_)));
}

#[rstest]
fn test_reconcile_order_report_generates_fill_for_qty_mismatch(instrument: InstrumentAny) {
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    // Report shows 50 filled but order has 0
    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::PartiallyFilled,
        Quantity::from(100),
        Quantity::from(50),
    );
    report.avg_px = Some(dec!(1.0));

    let result = reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), OrderEventAny::Filled(_)));
}

#[rstest]
fn test_create_reconciliation_rejected_with_reason() {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let client_order_id = ClientOrderId::from("O-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let result =
        create_reconciliation_rejected(&order, Some("MARGIN_CALL"), UnixNanos::from(1_000));
    assert!(result.is_some());
    if let OrderEventAny::Rejected(rejected) = result.unwrap() {
        assert_eq!(rejected.reason.as_str(), "MARGIN_CALL");
        assert_eq!(rejected.reconciliation, 1);
        assert_eq!(rejected.due_post_only, 0);
    } else {
        panic!("Expected Rejected event");
    }
}

#[rstest]
fn test_create_reconciliation_rejected_without_reason() {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let client_order_id = ClientOrderId::from("O-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        AccountId::from("SIM-001"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let result = create_reconciliation_rejected(&order, None, UnixNanos::from(1_000));
    assert!(result.is_some());
    if let OrderEventAny::Rejected(rejected) = result.unwrap() {
        assert_eq!(rejected.reason.as_str(), "UNKNOWN");
    } else {
        panic!("Expected Rejected event");
    }
}

#[rstest]
fn test_create_reconciliation_rejected_no_account_id() {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let client_order_id = ClientOrderId::from("O-001");

    // Order without account_id (not yet submitted)
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    let result = create_reconciliation_rejected(&order, Some("TEST"), UnixNanos::from(1_000));
    assert!(result.is_none());
}

#[rstest]
fn test_create_synthetic_venue_order_id_format() {
    let fill = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );
    let id = create_synthetic_venue_order_id(&fill, InstrumentId::from("AUD/USD.SIM"));

    // Format: S-{hex_timestamp}-{hash_suffix}
    assert!(id.as_str().starts_with("S-"));
    let parts: Vec<&str> = id.as_str().split('-').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "S");
    assert!(!parts[1].is_empty());
}

#[rstest]
fn test_create_synthetic_trade_id_format() {
    let fill = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );

    let id = create_synthetic_trade_id(&fill);

    // Format: S-{hex_timestamp}-{hash_suffix}
    assert!(id.as_str().starts_with("S-"));
    let parts: Vec<&str> = id.as_str().split('-').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "S");
    assert!(!parts[1].is_empty());
}

#[rstest]
fn test_create_synthetic_venue_order_id_is_deterministic() {
    let fill = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );
    let instrument_id = InstrumentId::from("AUD/USD.SIM");

    let first = create_synthetic_venue_order_id(&fill, instrument_id);
    let second = create_synthetic_venue_order_id(&fill, instrument_id);

    assert_eq!(first, second);
}

#[rstest]
fn test_create_synthetic_venue_order_id_differs_across_instruments() {
    let fill = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );

    let first = create_synthetic_venue_order_id(&fill, InstrumentId::from("AUD/USD.SIM"));
    let second = create_synthetic_venue_order_id(&fill, InstrumentId::from("EUR/USD.SIM"));

    assert_ne!(first, second);
}

#[rstest]
fn test_create_synthetic_trade_id_is_deterministic() {
    let fill = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );

    let first = create_synthetic_trade_id(&fill);
    let second = create_synthetic_trade_id(&fill);

    assert_eq!(first, second);
}

/// Regression guard: `create_synthetic_order_report` must propagate `fill.px` to
/// `avg_px` on the resulting report. Without this, downstream
/// `create_inferred_fill` calls on the synthetic order see both `avg_px` and
/// `price` as `None` and emit "no avg_px or price available" warnings, producing
/// no reconciliation fill for the position gap.
#[rstest]
fn test_create_synthetic_order_report_populates_avg_px() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let venue_order_id = create_test_venue_order_id("ORDER1");
    let fill = FillSnapshot::new(
        1_000_000,
        OrderSide::Sell,
        dec!(0.042),
        dec!(2355.8),
        venue_order_id,
    );

    let report = create_synthetic_order_report(
        &fill,
        AccountId::from("ETHUSDT-PERP-001"),
        instrument.id(),
        &instrument,
        venue_order_id,
    )
    .expect("synthetic report creation should succeed");

    assert_eq!(report.avg_px, Some(dec!(2355.8)));
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty.as_decimal(), dec!(0.042));
}

#[rstest]
fn test_create_inferred_reconciliation_trade_id_differs_across_instruments() {
    let first = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("V-1")),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Quantity::from("100000"),
        Price::from("1.00000"),
        PositionId::from("AUD/USD.SIM-EXTERNAL"),
        UnixNanos::from(1),
    );
    let second = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("EUR/USD.SIM"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("V-1")),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Quantity::from("100000"),
        Price::from("1.00000"),
        PositionId::from("AUD/USD.SIM-EXTERNAL"),
        UnixNanos::from(1),
    );

    assert_ne!(first, second);
}

#[rstest]
fn test_create_inferred_reconciliation_trade_id_is_deterministic() {
    let first = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("V-1")),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Quantity::from("100000"),
        Price::from("1.00000"),
        PositionId::from("AUD/USD.SIM-EXTERNAL"),
        UnixNanos::from(1),
    );
    let second = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("V-1")),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Quantity::from("100000"),
        Price::from("1.00000"),
        PositionId::from("AUD/USD.SIM-EXTERNAL"),
        UnixNanos::from(1),
    );

    assert_eq!(first, second);
}

#[rstest]
fn test_create_position_reconciliation_venue_order_id_is_deterministic() {
    let first = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(Price::from("1.00010")),
        None,
        None,
        UnixNanos::from(1),
    );
    let second = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(Price::from("1.00010")),
        None,
        None,
        UnixNanos::from(1),
    );

    assert_eq!(first, second);
}

#[rstest]
fn test_create_position_reconciliation_venue_order_id_differs_across_instruments() {
    let first = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(Price::from("1.00010")),
        None,
        None,
        UnixNanos::from(1),
    );
    let second = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("EUR/USD.SIM"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(Price::from("1.00010")),
        None,
        None,
        UnixNanos::from(1),
    );

    assert_ne!(first, second);
}

#[rstest]
fn test_create_position_reconciliation_venue_order_id_differs_across_accounts() {
    let first = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(Price::from("1.00010")),
        None,
        None,
        UnixNanos::from(1),
    );
    let second = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-002"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(Price::from("1.00010")),
        None,
        None,
        UnixNanos::from(1),
    );

    assert_ne!(first, second);
}

#[rstest]
fn test_create_position_reconciliation_venue_order_id_differs_across_ts_last() {
    let first = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(Price::from("1.00010")),
        None,
        None,
        UnixNanos::from(1_000_000),
    );
    let second = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(Price::from("1.00010")),
        None,
        None,
        UnixNanos::from(2_000_000),
    );

    assert_ne!(first, second);
}

#[rstest]
fn test_create_inferred_reconciliation_trade_id_differs_across_accounts() {
    let first = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("V-1")),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Quantity::from("100000"),
        Price::from("1.00000"),
        PositionId::from("AUD/USD.SIM-EXTERNAL"),
        UnixNanos::from(1),
    );
    let second = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-002"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("V-1")),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Quantity::from("100000"),
        Price::from("1.00000"),
        PositionId::from("AUD/USD.SIM-EXTERNAL"),
        UnixNanos::from(1),
    );

    assert_ne!(first, second);
}

#[rstest]
fn test_create_inferred_reconciliation_trade_id_differs_across_ts_last() {
    let first = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("V-1")),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Quantity::from("100000"),
        Price::from("1.00000"),
        PositionId::from("AUD/USD.SIM-EXTERNAL"),
        UnixNanos::from(1_000_000),
    );
    let second = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("V-1")),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Quantity::from("100000"),
        Price::from("1.00000"),
        PositionId::from("AUD/USD.SIM-EXTERNAL"),
        UnixNanos::from(2_000_000),
    );

    assert_ne!(first, second);
}

#[rstest]
fn test_create_position_reconciliation_venue_order_id_differs_across_tags() {
    let close = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Sell,
        OrderType::Market,
        Quantity::from("100000"),
        Some(Price::from("1.00000")),
        None,
        Some("CLOSE"),
        UnixNanos::from(1),
    );
    let open = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Sell,
        OrderType::Market,
        Quantity::from("100000"),
        Some(Price::from("1.00000")),
        None,
        Some("OPEN"),
        UnixNanos::from(1),
    );

    assert_ne!(close, open);
}

#[rstest]
fn test_create_position_reconciliation_venue_order_id_varies_with_each_field() {
    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let position_id = PositionId::from("P-1");
    let price = Price::from("1.00010");

    let baseline = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        instrument_id,
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(price),
        Some(position_id),
        Some("CLOSE"),
        UnixNanos::from(1),
    );

    assert_ne!(
        baseline,
        create_position_reconciliation_venue_order_id(
            AccountId::from("TEST-001"),
            instrument_id,
            OrderSide::Sell,
            OrderType::Limit,
            Quantity::from("100000"),
            Some(price),
            Some(position_id),
            Some("CLOSE"),
            UnixNanos::from(1),
        ),
        "side must discriminate",
    );
    assert_ne!(
        baseline,
        create_position_reconciliation_venue_order_id(
            AccountId::from("TEST-001"),
            instrument_id,
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("100000"),
            Some(price),
            Some(position_id),
            Some("CLOSE"),
            UnixNanos::from(1),
        ),
        "order type must discriminate",
    );
    assert_ne!(
        baseline,
        create_position_reconciliation_venue_order_id(
            AccountId::from("TEST-001"),
            instrument_id,
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("50000"),
            Some(price),
            Some(position_id),
            Some("CLOSE"),
            UnixNanos::from(1),
        ),
        "quantity must discriminate",
    );
    assert_ne!(
        baseline,
        create_position_reconciliation_venue_order_id(
            AccountId::from("TEST-001"),
            instrument_id,
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("100000"),
            Some(Price::from("1.00020")),
            Some(position_id),
            Some("CLOSE"),
            UnixNanos::from(1),
        ),
        "price must discriminate",
    );
    assert_ne!(
        baseline,
        create_position_reconciliation_venue_order_id(
            AccountId::from("TEST-001"),
            instrument_id,
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("100000"),
            None,
            Some(position_id),
            Some("CLOSE"),
            UnixNanos::from(1),
        ),
        "Some(price) vs None must discriminate",
    );
    assert_ne!(
        baseline,
        create_position_reconciliation_venue_order_id(
            AccountId::from("TEST-001"),
            instrument_id,
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("100000"),
            Some(price),
            Some(PositionId::from("P-2")),
            Some("CLOSE"),
            UnixNanos::from(1),
        ),
        "venue position id must discriminate",
    );
    assert_ne!(
        baseline,
        create_position_reconciliation_venue_order_id(
            AccountId::from("TEST-001"),
            instrument_id,
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("100000"),
            Some(price),
            None,
            Some("CLOSE"),
            UnixNanos::from(1),
        ),
        "Some(position) vs None must discriminate",
    );
    assert_ne!(
        baseline,
        create_position_reconciliation_venue_order_id(
            AccountId::from("TEST-001"),
            instrument_id,
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("100000"),
            Some(price),
            Some(position_id),
            None,
            UnixNanos::from(1),
        ),
        "Some(tag) vs None must discriminate",
    );
}

#[rstest]
fn test_create_inferred_reconciliation_trade_id_varies_with_each_field() {
    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let client_order_id = ClientOrderId::from("O-1");
    let venue_order_id = VenueOrderId::from("V-1");
    let position_id = PositionId::from("AUD/USD.SIM-EXTERNAL");
    let qty = Quantity::from("100000");
    let px = Price::from("1.00000");

    let baseline = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-001"),
        instrument_id,
        client_order_id,
        Some(venue_order_id),
        OrderSide::Buy,
        OrderType::Limit,
        qty,
        qty,
        px,
        position_id,
        UnixNanos::from(1),
    );

    assert_ne!(
        baseline,
        create_inferred_reconciliation_trade_id(
            AccountId::from("TEST-001"),
            instrument_id,
            ClientOrderId::from("O-2"),
            Some(venue_order_id),
            OrderSide::Buy,
            OrderType::Limit,
            qty,
            qty,
            px,
            position_id,
            UnixNanos::from(1),
        ),
        "client order id must discriminate",
    );
    assert_ne!(
        baseline,
        create_inferred_reconciliation_trade_id(
            AccountId::from("TEST-001"),
            instrument_id,
            client_order_id,
            Some(VenueOrderId::from("V-2")),
            OrderSide::Buy,
            OrderType::Limit,
            qty,
            qty,
            px,
            position_id,
            UnixNanos::from(1),
        ),
        "venue order id must discriminate",
    );
    assert_ne!(
        baseline,
        create_inferred_reconciliation_trade_id(
            AccountId::from("TEST-001"),
            instrument_id,
            client_order_id,
            None,
            OrderSide::Buy,
            OrderType::Limit,
            qty,
            qty,
            px,
            position_id,
            UnixNanos::from(1),
        ),
        "Some(venue order id) vs None must discriminate",
    );
    assert_ne!(
        baseline,
        create_inferred_reconciliation_trade_id(
            AccountId::from("TEST-001"),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            OrderSide::Sell,
            OrderType::Limit,
            qty,
            qty,
            px,
            position_id,
            UnixNanos::from(1),
        ),
        "order side must discriminate",
    );
    assert_ne!(
        baseline,
        create_inferred_reconciliation_trade_id(
            AccountId::from("TEST-001"),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            OrderSide::Buy,
            OrderType::Market,
            qty,
            qty,
            px,
            position_id,
            UnixNanos::from(1),
        ),
        "order type must discriminate",
    );
    assert_ne!(
        baseline,
        create_inferred_reconciliation_trade_id(
            AccountId::from("TEST-001"),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("50000"),
            qty,
            px,
            position_id,
            UnixNanos::from(1),
        ),
        "filled qty must discriminate",
    );
    assert_ne!(
        baseline,
        create_inferred_reconciliation_trade_id(
            AccountId::from("TEST-001"),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            OrderSide::Buy,
            OrderType::Limit,
            qty,
            Quantity::from("50000"),
            px,
            position_id,
            UnixNanos::from(1),
        ),
        "last qty must discriminate",
    );
    assert_ne!(
        baseline,
        create_inferred_reconciliation_trade_id(
            AccountId::from("TEST-001"),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            OrderSide::Buy,
            OrderType::Limit,
            qty,
            qty,
            Price::from("1.00010"),
            position_id,
            UnixNanos::from(1),
        ),
        "last px must discriminate",
    );
    assert_ne!(
        baseline,
        create_inferred_reconciliation_trade_id(
            AccountId::from("TEST-001"),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            OrderSide::Buy,
            OrderType::Limit,
            qty,
            qty,
            px,
            PositionId::from("P-OTHER"),
            UnixNanos::from(1),
        ),
        "position id must discriminate",
    );
}

#[rstest]
fn test_create_synthetic_venue_order_id_varies_with_each_field() {
    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let baseline_fill = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );

    let baseline = create_synthetic_venue_order_id(&baseline_fill, instrument_id);

    let ts_changed = FillSnapshot::new(
        2_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );
    assert_ne!(
        baseline,
        create_synthetic_venue_order_id(&ts_changed, instrument_id),
        "ts_event must discriminate",
    );

    let side_changed = FillSnapshot::new(
        1_000_000,
        OrderSide::Sell,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );
    assert_ne!(
        baseline,
        create_synthetic_venue_order_id(&side_changed, instrument_id),
        "side must discriminate",
    );

    let qty_changed = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(2.50),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );
    assert_ne!(
        baseline,
        create_synthetic_venue_order_id(&qty_changed, instrument_id),
        "qty must discriminate",
    );

    let px_changed = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(200.00),
        create_test_venue_order_id("ORDER1"),
    );
    assert_ne!(
        baseline,
        create_synthetic_venue_order_id(&px_changed, instrument_id),
        "px must discriminate",
    );

    let venue_order_changed = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER2"),
    );
    assert_ne!(
        baseline,
        create_synthetic_venue_order_id(&venue_order_changed, instrument_id),
        "source venue_order_id must discriminate",
    );
}

#[rstest]
fn test_create_synthetic_trade_id_varies_with_each_field() {
    let baseline_fill = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );
    let baseline = create_synthetic_trade_id(&baseline_fill);

    let ts_changed = FillSnapshot::new(
        2_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );
    assert_ne!(
        baseline,
        create_synthetic_trade_id(&ts_changed),
        "ts_event must discriminate",
    );

    let side_changed = FillSnapshot::new(
        1_000_000,
        OrderSide::Sell,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );
    assert_ne!(
        baseline,
        create_synthetic_trade_id(&side_changed),
        "side must discriminate",
    );

    let qty_changed = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(2.50),
        dec!(100.50),
        create_test_venue_order_id("ORDER1"),
    );
    assert_ne!(
        baseline,
        create_synthetic_trade_id(&qty_changed),
        "qty must discriminate",
    );

    let px_changed = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(200.00),
        create_test_venue_order_id("ORDER1"),
    );
    assert_ne!(
        baseline,
        create_synthetic_trade_id(&px_changed),
        "px must discriminate",
    );

    let venue_order_changed = FillSnapshot::new(
        1_000_000,
        OrderSide::Buy,
        dec!(1.25),
        dec!(100.50),
        create_test_venue_order_id("ORDER2"),
    );
    assert_ne!(
        baseline,
        create_synthetic_trade_id(&venue_order_changed),
        "source venue_order_id must discriminate",
    );
}

#[rstest]
fn test_position_reconciliation_venue_order_id_parses_as_uuid_v5() {
    let id = create_position_reconciliation_venue_order_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Some(Price::from("1.00010")),
        None,
        None,
        UnixNanos::from(1),
    );

    let uuid = Uuid::parse_str(id.as_str()).expect("id must be a valid uuid");
    assert_eq!(uuid.get_version_num(), 5, "uuid version nibble must be 5");
}

#[rstest]
fn test_inferred_reconciliation_trade_id_parses_as_uuid_v5() {
    let id = create_inferred_reconciliation_trade_id(
        AccountId::from("TEST-001"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("V-1")),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("100000"),
        Quantity::from("100000"),
        Price::from("1.00000"),
        PositionId::from("AUD/USD.SIM-EXTERNAL"),
        UnixNanos::from(1),
    );

    let uuid = Uuid::parse_str(id.as_str()).expect("id must be a valid uuid");
    assert_eq!(uuid.get_version_num(), 5, "uuid version nibble must be 5");
}

#[rstest]
fn test_create_inferred_fill_for_qty_zero_quantity_returns_none() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.00"))
        .build();

    let report = make_test_report(
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Filled,
        "10.0",
        false,
    );

    let result = create_inferred_fill_for_qty(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        Quantity::zero(0),
        UnixNanos::from(1_000_000),
        None,
    );

    assert!(result.is_none());
}

#[rstest]
fn test_create_inferred_fill_for_qty_uses_report_avg_px() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.00"))
        .build();

    // Report with avg_px different from order price
    let report = OrderStatusReport::new(
        AccountId::from("TEST-001"),
        instrument.id(),
        Some(order.client_order_id()),
        VenueOrderId::from("V-001"),
        OrderSide::Buy,
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_avg_px(105.50)
    .unwrap();

    let result = create_inferred_fill_for_qty(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        Quantity::from("5.0"),
        UnixNanos::from(2_000_000),
        None,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    // Should use avg_px from report (105.50), not order price (100.00)
    assert_eq!(filled.last_px, Price::from("105.50"));
    assert_eq!(filled.last_qty, Quantity::from("5.0"));
}

#[rstest]
fn test_create_inferred_fill_for_qty_uses_report_price_when_no_avg_px() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.00"))
        .build();

    // Report with price but no avg_px
    let report = OrderStatusReport::new(
        AccountId::from("TEST-001"),
        instrument.id(),
        Some(order.client_order_id()),
        VenueOrderId::from("V-001"),
        OrderSide::Buy,
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_price(Price::from("102.00"));

    let result = create_inferred_fill_for_qty(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        Quantity::from("5.0"),
        UnixNanos::from(2_000_000),
        None,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    // Should use price from report (102.00)
    assert_eq!(filled.last_px, Price::from("102.00"));
}

#[rstest]
fn test_create_inferred_fill_for_qty_uses_order_price_as_fallback() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.00"))
        .build();

    // Report with no price information
    let report = OrderStatusReport::new(
        AccountId::from("TEST-001"),
        instrument.id(),
        Some(order.client_order_id()),
        VenueOrderId::from("V-001"),
        OrderSide::Buy,
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let result = create_inferred_fill_for_qty(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        Quantity::from("5.0"),
        UnixNanos::from(2_000_000),
        None,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    // Should fall back to order price (100.00)
    assert_eq!(filled.last_px, Price::from("100.00"));
}

#[rstest]
fn test_create_inferred_fill_for_qty_no_price_returns_none() {
    let instrument = crypto_perpetual_ethusdt();

    // Market order has no price
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .build();

    // Report with no price information
    let report = OrderStatusReport::new(
        AccountId::from("TEST-001"),
        instrument.id(),
        Some(order.client_order_id()),
        VenueOrderId::from("V-001"),
        OrderSide::Buy,
        OrderType::Market,
        TimeInForce::Ioc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let result = create_inferred_fill_for_qty(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        Quantity::from("5.0"),
        UnixNanos::from(2_000_000),
        None,
    );

    assert!(result.is_none());
}

#[rstest]
#[case::market_order(OrderType::Market, false, LiquiditySide::Taker)]
#[case::stop_market(OrderType::StopMarket, false, LiquiditySide::Taker)]
#[case::trailing_stop_market(OrderType::TrailingStopMarket, false, LiquiditySide::Taker)]
#[case::limit_post_only(OrderType::Limit, true, LiquiditySide::Maker)]
#[case::limit_default(OrderType::Limit, false, LiquiditySide::NoLiquiditySide)]
fn test_create_inferred_fill_for_qty_liquidity_side(
    #[case] order_type: OrderType,
    #[case] post_only: bool,
    #[case] expected: LiquiditySide,
) {
    let instrument = crypto_perpetual_ethusdt();
    let order = match order_type {
        OrderType::Limit => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.00"))
            .post_only(post_only)
            .build(),
        OrderType::StopMarket => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .trigger_price(Price::from("100.00"))
            .build(),
        OrderType::TrailingStopMarket => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .trigger_price(Price::from("100.00"))
            .trailing_offset(Decimal::from(1))
            .build(),
        _ => OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .build(),
    };

    let report = make_test_report(
        instrument.id(),
        order_type,
        OrderStatus::Filled,
        "10.0",
        post_only,
    );

    let result = create_inferred_fill_for_qty(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        Quantity::from("5.0"),
        UnixNanos::from(2_000_000),
        None,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    assert_eq!(
        filled.liquidity_side, expected,
        "order_type={order_type}, post_only={post_only}"
    );
}

#[rstest]
fn test_create_inferred_fill_for_qty_trade_id_format() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.00"))
        .build();

    let report = make_test_report(
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Filled,
        "10.0",
        false,
    );

    let ts_now = UnixNanos::from(2_000_000);
    let result = create_inferred_fill_for_qty(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        Quantity::from("5.0"),
        ts_now,
        None,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    // Trade ID should be a valid UUID (36 characters with dashes)
    assert_eq!(filled.trade_id.as_str().len(), 36);
    assert!(filled.trade_id.as_str().contains('-'));
}

#[rstest]
fn test_create_inferred_fill_for_qty_reconciliation_flag() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.00"))
        .build();

    let report = make_test_report(
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Filled,
        "10.0",
        false,
    );

    let result = create_inferred_fill_for_qty(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        Quantity::from("5.0"),
        UnixNanos::from(2_000_000),
        None,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    assert!(filled.reconciliation, "reconciliation flag should be true");
}

#[rstest]
fn test_create_incremental_inferred_fill_with_commission() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.00"))
        .build();

    let instrument_any = InstrumentAny::CryptoPerpetual(instrument.clone());
    let mut accepted_order = TestOrderStubs::make_accepted_order(&order);
    let partial_fill = TestOrderEventStubs::filled(
        &accepted_order,
        &instrument_any,
        None,
        None,
        None,
        Some(Quantity::from("3.0")),
        None,
        None,
        None,
        None,
    );
    accepted_order.apply(partial_fill).unwrap();

    let report = OrderStatusReport::new(
        AccountId::from("TEST-001"),
        instrument.id(),
        Some(accepted_order.client_order_id()),
        VenueOrderId::from("V-001"),
        OrderSide::Buy,
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_avg_px(100.0)
    .unwrap();

    let commission = Some(Money::new(2.50, Currency::USDT()));

    let result = create_incremental_inferred_fill(
        &accepted_order,
        &report,
        &AccountId::from("TEST-001"),
        &instrument_any,
        UnixNanos::from(2_000_000),
        commission,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    assert_eq!(filled.last_qty, Quantity::from("7.0"));
    assert_eq!(filled.commission, Some(Money::new(2.50, Currency::USDT())));
}

#[rstest]
fn test_create_inferred_fill_with_commission() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .build();

    let report = make_test_report(
        instrument.id(),
        OrderType::Market,
        OrderStatus::Filled,
        "1.0",
        false,
    );

    let commission = Some(Money::new(5.0, Currency::USDT()));

    let fill = create_inferred_fill(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        UnixNanos::from(2_000_000),
        commission,
    );

    let filled = match fill.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    assert_eq!(filled.commission, Some(Money::new(5.0, Currency::USDT())));
}

#[rstest]
fn test_create_inferred_fill_none_commission() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .build();

    let report = make_test_report(
        instrument.id(),
        OrderType::Market,
        OrderStatus::Filled,
        "1.0",
        false,
    );

    let fill = create_inferred_fill(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        UnixNanos::from(2_000_000),
        None,
    );

    let filled = match fill.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    assert_eq!(filled.commission, None);
}

#[rstest]
fn test_create_inferred_fill_for_qty_with_commission() {
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.00"))
        .build();

    let report = make_test_report(
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Filled,
        "10.0",
        false,
    );

    let commission = Some(Money::new(1.23, Currency::USDT()));

    let result = create_inferred_fill_for_qty(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        Quantity::from("5.0"),
        UnixNanos::from(2_000_000),
        commission,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };

    assert_eq!(filled.commission, Some(Money::new(1.23, Currency::USDT())));
}

// Phase 1 edge-case tests (reconciliation_testing_strategy.md)

#[rstest]
fn test_incremental_fill_zero_cost_first_fill_no_panic() {
    // Airdrop-style execution where venue reports a first fill with avg_px = 0;
    // ensures create_incremental_inferred_fill handles a zero price without
    // panicking and produces a zero-price fill rather than rejecting it.
    let instrument = crypto_perpetual_ethusdt();
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .build();

    let mut report = make_test_report(
        instrument.id(),
        OrderType::Market,
        OrderStatus::Filled,
        "10.0",
        false,
    );
    report.avg_px = Some(dec!(0));
    report.price = None;

    let result = create_incremental_inferred_fill(
        &order,
        &report,
        &AccountId::from("TEST-001"),
        &InstrumentAny::CryptoPerpetual(instrument),
        UnixNanos::from(2_000_000),
        None,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };
    assert_eq!(filled.last_qty, Quantity::from("10.0"));
    assert_eq!(filled.last_px.as_decimal(), dec!(0));
}

#[rstest]
fn test_incremental_fill_zero_cost_incremental_no_panic(instrument: InstrumentAny) {
    // Airdrop-style execution where the order already has a prior zero-price
    // fill; exercises the weighted-average branch of
    // calculate_incremental_fill_price with avg_px = 0 and confirms it does
    // not panic or emit a negative price.
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let account_id = AccountId::from("TEST-001");

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10"))
        .build();

    submit_accept(&mut order, account_id, venue_order_id);
    apply_fill(
        &mut order,
        &instrument,
        TradeId::from("T-001"),
        Quantity::from("3"),
        Price::from("0.00000"),
    );

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Market,
        OrderStatus::Filled,
        Quantity::from("10"),
        Quantity::from("10"),
    );
    report.avg_px = Some(dec!(0));

    let result = create_incremental_inferred_fill(
        &order,
        &report,
        &account_id,
        &instrument,
        UnixNanos::from(2_000_000),
        None,
    );

    let filled = match result.unwrap() {
        OrderEventAny::Filled(f) => f,
        _ => panic!("Expected Filled event"),
    };
    assert_eq!(filled.last_qty, Quantity::from("7"));
    assert!(
        filled.last_px.as_decimal() >= dec!(0),
        "incremental zero-cost fill must not emit a negative price, was {}",
        filled.last_px,
    );
}

#[rstest]
#[case::one_ulp_above(dec!(1.000000001), dec!(1.0), 9, true)]
#[case::one_ulp_below(dec!(0.999999999), dec!(1.0), 9, true)]
#[case::float_bleed_within(dec!(1.0000000000000000001), dec!(1.0), 9, true)]
#[case::just_outside(dec!(1.000000011), dec!(1.0), 9, false)]
fn test_is_within_single_unit_tolerance_float_bleed(
    #[case] value1: Decimal,
    #[case] value2: Decimal,
    #[case] precision: u8,
    #[case] expected: bool,
) {
    // Guards against WebSocket-level float bleed generating a ghost fill:
    // a venue-reported quantity with sub-ulp noise must still read as equal
    // to the locally cached quantity at the instrument's precision.
    assert_eq!(
        is_within_single_unit_tolerance(value1, value2, precision),
        expected,
        "value1={value1}, value2={value2}, precision={precision}",
    );
}

#[rstest]
fn test_status_vs_qty_mismatch_emits_updated(instrument: InstrumentAny) {
    // Venue has reduced the order quantity (partial cancel) so it reports
    // Filled with qty=10 while the local cache holds PartiallyFilled with 10
    // of 20 filled; reconciliation must emit OrderUpdated to shrink the local
    // quantity to 10 and must not synthesize a duplicate fill since filled_qty
    // already matches.
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let account_id = AccountId::from("SIM-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(20))
        .price(Price::from("1.00000"))
        .build();

    submit_accept(&mut order, account_id, venue_order_id);
    apply_fill(
        &mut order,
        &instrument,
        TradeId::from("T-001"),
        Quantity::from(10),
        Price::from("1.00000"),
    );
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Filled,
        Quantity::from(10),
        Quantity::from(10),
    );
    report.price = Some(Price::from("1.00000"));

    let result = reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());

    let event = result.expect("expected OrderUpdated for reduced-qty Filled report");
    let updated = match event.clone() {
        OrderEventAny::Updated(u) => u,
        other => panic!("expected OrderUpdated, was {other:?}"),
    };
    assert_eq!(updated.quantity, Quantity::from(10));
    assert_eq!(updated.reconciliation, 1);

    order.apply(event).unwrap();
    assert_eq!(order.quantity(), Quantity::from(10));
    assert_eq!(order.filled_qty(), Quantity::from(10));
    // Documented limitation shared with Python reference: OrderUpdated alone
    // does not transition PartiallyFilled -> Filled; status persists here
    // even though filled_qty now equals quantity.
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
}

#[rstest]
fn test_status_vs_qty_mismatch_no_qty_change_returns_none(instrument: InstrumentAny) {
    // Status differs between local PartiallyFilled and venue Filled with
    // matching filled_qty AND matching total quantity; without a quantity
    // change there is nothing safe to emit, so reconciliation must return
    // None (state is logged but no spurious events are produced).
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let account_id = AccountId::from("SIM-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(20))
        .price(Price::from("1.00000"))
        .build();

    submit_accept(&mut order, account_id, venue_order_id);
    apply_fill(
        &mut order,
        &instrument,
        TradeId::from("T-001"),
        Quantity::from(10),
        Price::from("1.00000"),
    );

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Filled,
        Quantity::from(20),
        Quantity::from(10),
    );
    report.price = Some(Price::from("1.00000"));

    let result = reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
    assert!(result.is_none());
}

#[rstest]
fn test_reconcile_fill_report_overfill_after_partial_rejected(instrument: InstrumentAny) {
    // Previously unfilled order absorbs a partial fill and must then reject
    // a second fill that together with the partial would exceed its total
    // quantity; guards against reconciliation stacking fills past the
    // order's size when the venue redelivers stale events.
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let account_id = AccountId::from("SIM-001");

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();

    submit_accept(&mut order, account_id, venue_order_id);
    apply_fill(
        &mut order,
        &instrument,
        TradeId::from("T-001"),
        Quantity::from("60"),
        Price::from("1.00000"),
    );

    let fill_report = create_test_fill_report(
        instrument.id(),
        venue_order_id,
        TradeId::from("T-002"),
        Quantity::from("50"),
        Price::from("1.00000"),
    );

    let result = reconcile_fill_report(
        &order,
        &fill_report,
        &instrument,
        UnixNanos::from(3_000_000),
        false,
    );
    assert!(result.is_none(), "expected overfill rejection");
}

#[rstest]
fn test_should_reconciliation_update_rejects_shrink_below_filled(instrument: InstrumentAny) {
    // Venue must never downsize an order below what the engine has already
    // filled; should_reconciliation_update returns false to prevent emitting
    // an OrderUpdated that would violate filled_qty <= quantity.
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let account_id = AccountId::from("SIM-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(20))
        .price(Price::from("1.00000"))
        .build();

    submit_accept(&mut order, account_id, venue_order_id);
    apply_fill(
        &mut order,
        &instrument,
        TradeId::from("T-001"),
        Quantity::from(15),
        Price::from("1.00000"),
    );

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::PartiallyFilled,
        Quantity::from(10),
        Quantity::from(15),
    );
    report.price = Some(Price::from("1.00000"));

    assert!(!should_reconciliation_update(&order, &report));
}

#[rstest]
fn test_reconciliation_updated_strips_trigger_price_for_limit(instrument: InstrumentAny) {
    // Guards against venues that report trigger_price on non-triggerable
    // orders (e.g. Bybit sends "0.00" for Limit orders); a naive
    // pass-through panics inside LimitOrder::update, so
    // create_reconciliation_updated must force trigger_price to None and
    // the resulting event must apply cleanly.
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let account_id = AccountId::from("SIM-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();

    submit_accept(&mut order, account_id, venue_order_id);

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Limit,
        OrderStatus::Accepted,
        Quantity::from(100),
        Quantity::from(0),
    );
    report.price = Some(Price::from("1.00100"));
    report.trigger_price = Some(Price::from("0.00000"));

    let event = create_reconciliation_updated(&order, &report, UnixNanos::default());
    let updated = match event.clone() {
        OrderEventAny::Updated(u) => u,
        other => panic!("expected OrderUpdated, was {other:?}"),
    };
    assert_eq!(updated.trigger_price, None);
    assert_eq!(updated.price, Some(Price::from("1.00100")));

    order.apply(event).unwrap();
}

#[rstest]
fn test_reconcile_closed_order_within_tolerance_is_noop(instrument: InstrumentAny) {
    // Venue can redeliver a filled order with sub-precision jitter on
    // filled_qty after it closed locally; the mismatch is within single-unit
    // tolerance so reconciliation must not synthesize an inferred fill that
    // would overfill the already closed order.
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let account_id = AccountId::from("SIM-001");

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10"))
        .build();

    submit_accept(&mut order, account_id, venue_order_id);
    apply_fill(
        &mut order,
        &instrument,
        TradeId::from("T-001"),
        Quantity::from("10"),
        Price::from("1.00000"),
    );
    assert!(order.is_closed());

    let mut report = create_test_order_status_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        OrderType::Market,
        OrderStatus::Filled,
        Quantity::from("10"),
        Quantity::from("10.000001"),
    );
    report.avg_px = Some(dec!(1.0));

    let result = reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
    assert!(
        result.is_none(),
        "closed order with sub-tolerance jitter must not emit a new fill",
    );
}
