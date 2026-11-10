#![no_main]

use nautilus_core::UnixNanos;
use nautilus_kalshi::{
    execution::{fill_report, order_report},
    http::models::{BookSide, Fill, FixedPoint, Order as VenueOrder, OrderStatus, OutcomeSide},
};
use nautilus_live::fuzz::fuzz_target;
use nautilus_model::{
    enums::{OrderSide, OrderStatus as ReportStatus},
    identifiers::AccountId,
};
use rust_decimal::Decimal;

#[path = "../common/mod.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let Some(data) = common::capped(data) else {
        return;
    };

    if data.len() < 8 {
        return;
    }
    let instrument = common::instrument(None);
    let source = venue_order(instrument.id.symbol.as_ref(), data);
    let fill = venue_fill(&source, data);
    let account = AccountId::from("KALSHI-007");
    let ts = UnixNanos::from(1);

    let first_order = order_report(&source, &instrument, account, None, ts);
    let second_order = order_report(&source, &instrument, account, None, ts);
    assert_eq!(
        first_order.is_ok(),
        second_order.is_ok(),
        "order report rejection changed"
    );
    let direction_ok = matches!(
        (source.book_side, source.outcome_side),
        (BookSide::Bid, OutcomeSide::Yes) | (BookSide::Ask, OutcomeSide::No)
    );
    let prices_ok = source.yes_price_dollars.0 >= Decimal::ZERO
        && source.yes_price_dollars.0 <= Decimal::ONE
        && source.yes_price_dollars.0 + source.no_price_dollars.0 == Decimal::ONE;
    let order_rejected = !direction_ok
        || !prices_ok
        || source.status == OrderStatus::Unknown
        || (source.status == OrderStatus::Resting && source.remaining_count_fp.0.is_zero())
        || (source.status == OrderStatus::Executed && !source.remaining_count_fp.0.is_zero());
    if order_rejected {
        assert!(
            first_order.is_err(),
            "invalid order became a report: {first_order:?}"
        );
    } else if source.status == OrderStatus::Canceled {
        assert!(
            matches!(first_order, Ok(None)),
            "canceled order without a cache became a report"
        );
    } else {
        let report = first_order
            .expect("valid order was rejected")
            .expect("valid order produced no report");
        assert_eq!(report.instrument_id, instrument.id);
        assert_eq!(
            report.order_side,
            Some(if source.book_side == BookSide::Bid {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            })
        );
        assert_eq!(
            report.order_status,
            if source.fill_count_fp.0.is_zero() {
                ReportStatus::Accepted
            } else if source.remaining_count_fp.0.is_zero() {
                ReportStatus::Filled
            } else {
                ReportStatus::PartiallyFilled
            }
        );
        assert_eq!(
            report.price.map(|price| price.as_decimal()),
            Some(source.yes_price_dollars.0)
        );
        assert_eq!(report.filled_qty.as_decimal(), source.fill_count_fp.0);
        assert_eq!(
            report.quantity.as_decimal(),
            source.fill_count_fp.0 + source.remaining_count_fp.0
        );
    }

    let first_fill = fill_report(&fill, &instrument, account, &source, ts);
    let second_fill = fill_report(&fill, &instrument, account, &source, ts);
    assert_eq!(
        first_fill.is_ok(),
        second_fill.is_ok(),
        "fill report rejection changed"
    );
    let fill_rejected = !direction_ok
        || !prices_ok
        || fill.fee_cost.0 < Decimal::ZERO
        || fill.count_fp.0 > source.fill_count_fp.0
        || fill.count_fp.0 <= Decimal::ZERO;
    if fill_rejected {
        assert!(
            first_fill.is_err(),
            "invalid fill became a report: {first_fill:?}"
        );
    } else {
        let report = first_fill.expect("valid fill was rejected");
        assert_eq!(
            report.order_side,
            if source.book_side == BookSide::Bid {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            }
        );
        assert_eq!(report.last_qty.as_decimal(), fill.count_fp.0);
        assert_eq!(report.last_px.as_decimal(), fill.yes_price_dollars.0);
        assert_eq!(report.commission.as_decimal(), fill.fee_cost.0);
        assert!(
            report.last_qty.is_positive(),
            "fill created a non-positive exposure"
        );
    }

    let mut zero = fill.clone();
    zero.count_fp = FixedPoint(Decimal::ZERO);
    assert!(
        fill_report(&zero, &instrument, account, &source, ts).is_err(),
        "zero fill count became a report"
    );
});

fn venue_order(ticker: &str, data: &[u8]) -> VenueOrder {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
    let mut source: VenueOrder =
        serde_json::from_value(fixtures["order"]["order"].clone()).unwrap();
    source.ticker = ticker.to_owned();

    source.book_side = if data[0].is_multiple_of(2) {
        BookSide::Bid
    } else {
        BookSide::Ask
    };

    source.outcome_side = if data[1].is_multiple_of(2) {
        OutcomeSide::Yes
    } else {
        OutcomeSide::No
    };

    let price = Decimal::new(i64::from(u16::from(data[2]) + 1), 4);
    source.yes_price_dollars = FixedPoint(price.min(Decimal::ONE));
    source.no_price_dollars = FixedPoint(Decimal::ONE - source.yes_price_dollars.0);

    match data[6] % 5 {
        0 => {
            source.yes_price_dollars = FixedPoint(Decimal::ZERO);
            source.no_price_dollars = FixedPoint(Decimal::ONE);
        }
        1 => source.no_price_dollars = FixedPoint(Decimal::ZERO),
        2 => source.yes_price_dollars = FixedPoint(Decimal::new(10_001, 4)),
        _ => {}
    }

    source.fill_count_fp = FixedPoint(Decimal::new(i64::from(data[3].max(1)), 2));
    source.remaining_count_fp = FixedPoint(Decimal::new(i64::from(data[4]), 2));
    source.initial_count_fp = FixedPoint(
        source
            .fill_count_fp
            .0
            .checked_add(source.remaining_count_fp.0)
            .unwrap_or(Decimal::MAX),
    );

    source.status = match data[5] % 4 {
        0 => OrderStatus::Resting,
        1 => OrderStatus::Executed,
        2 => OrderStatus::Canceled,
        _ => OrderStatus::Unknown,
    };

    if source.status == OrderStatus::Resting && data[6].is_multiple_of(11) {
        source.fill_count_fp = FixedPoint(Decimal::ZERO);
        if source.remaining_count_fp.0.is_zero() {
            source.remaining_count_fp = FixedPoint(Decimal::new(1, 2));
        }
    }

    source
}

fn venue_fill(source: &VenueOrder, data: &[u8]) -> Fill {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
    let mut fill: Fill = serde_json::from_value(fixtures["fills"]["fills"][0].clone()).unwrap();
    fill.order_id = source.order_id.clone();
    fill.ticker = source.ticker.clone();
    fill.market_ticker = source.ticker.clone();
    fill.book_side = source.book_side;
    fill.outcome_side = source.outcome_side;
    fill.yes_price_dollars = source.yes_price_dollars;
    fill.no_price_dollars = source.no_price_dollars;
    fill.count_fp = if data[7].is_multiple_of(5) {
        FixedPoint(source.fill_count_fp.0 + Decimal::ONE)
    } else {
        source.fill_count_fp
    };

    fill.fee_cost = if data[7].is_multiple_of(11) {
        FixedPoint(Decimal::new(-1, 4))
    } else {
        FixedPoint(Decimal::new(1, 6))
    };

    fill.created_time = source.created_time.clone();
    fill
}
