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

use anyhow::Context;
use nautilus_core::{UnixNanos, datetime::try_datetime_to_unix_nanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce},
    events::OrderEventAny,
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    instruments::BinaryOption,
    orders::{Order, OrderAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Money, Quantity},
};
use rust_decimal::Decimal;

use super::timestamp;
use crate::{
    common::{enums::OrderType as VenueOrderType, parse::parse_instrument_id},
    data::parse::{price, size},
    http::models::{
        BookSide, Field, Fill, FixedPoint, MarketPosition, Order as VenueOrder,
        OrderStatus as VenueOrderStatus, OutcomeSide,
    },
};

/// Builds an order-status report from one venue order.
///
/// Returns `Ok(None)` when a canceled order has no cached quantity to preserve, or the cached order
/// is not closed with the same last time and filled count.
///
/// # Errors
///
/// Returns an error if identity, direction, type, status, counts, prices, or timestamps are invalid.
pub fn order(
    value: &VenueOrder,
    instrument: &BinaryOption,
    account_id: AccountId,
    cached: Option<&OrderAny>,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderStatusReport>> {
    order_inner(value, instrument, account_id, cached, ts_init, false)
}

pub(super) fn order_aggregate(
    value: &VenueOrder,
    instrument: &BinaryOption,
    account_id: AccountId,
    cached: &OrderAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderStatusReport>> {
    order_inner(value, instrument, account_id, Some(cached), ts_init, true)
}

fn order_inner(
    value: &VenueOrder,
    instrument: &BinaryOption,
    account_id: AccountId,
    cached: Option<&OrderAny>,
    ts_init: UnixNanos,
    allow_post_only_rejection: bool,
) -> anyhow::Result<Option<OrderStatusReport>> {
    anyhow::ensure!(
        parse_instrument_id(&value.ticker)? == instrument.id,
        "order instrument mismatch"
    );
    anyhow::ensure!(
        value.order_type == VenueOrderType::Limit,
        "unsupported Kalshi order type"
    );
    let side = direction(value.book_side, value.outcome_side)?;
    let px = price(value.yes_price_dollars, instrument.price_precision)?;
    complementary(value.yes_price_dollars, value.no_price_dollars)?;
    let filled = quantity(value.fill_count_fp, instrument.size_precision)?;
    let remaining = quantity(value.remaining_count_fp, instrument.size_precision)?;
    let ts_accepted = timestamp(&value.created_time)?;
    let ts_last = timestamp(&value.last_update_time)?;
    anyhow::ensure!(ts_last >= ts_accepted, "order timestamps out of sequence");
    let venue_order_id = VenueOrderId::new_checked(&value.order_id)?;
    let client_order_id = client_id(&value.client_order_id)?;
    let total = value
        .fill_count_fp
        .0
        .checked_add(value.remaining_count_fp.0)
        .context("order total overflow")?;

    let status = match value.status {
        VenueOrderStatus::Resting => {
            anyhow::ensure!(
                remaining.is_positive(),
                "resting order has no remaining quantity"
            );

            if filled.is_zero() {
                OrderStatus::Accepted
            } else {
                OrderStatus::PartiallyFilled
            }
        }
        VenueOrderStatus::Executed => {
            anyhow::ensure!(
                remaining.is_zero() && !filled.is_zero(),
                "invalid executed order counts"
            );
            OrderStatus::Filled
        }
        VenueOrderStatus::Canceled => OrderStatus::Canceled,
        VenueOrderStatus::Unknown => anyhow::bail!("unknown Kalshi order status"),
    };

    let expire_time = match value.expiration_time {
        Field::Value(value) => {
            Some(try_datetime_to_unix_nanos(value)?).filter(|value| *value != UnixNanos::default())
        }
        Field::Missing | Field::Null => None,
    };

    if let Some(cached) = cached {
        anyhow::ensure!(
            cached.instrument_id() == instrument.id
                && cached.order_type() == OrderType::Limit
                && cached.order_side() == side,
            "cached order identity or direction mismatch"
        );
        anyhow::ensure!(
            cached.account_id().is_none_or(|id| id == account_id),
            "cached order account mismatch"
        );
        anyhow::ensure!(
            cached
                .venue_order_id()
                .is_none_or(|id| id == venue_order_id),
            "cached venue order identity mismatch"
        );
        anyhow::ensure!(
            client_order_id.is_none_or(|id| id == cached.client_order_id()),
            "cached client order identity mismatch"
        );
    }

    let (quantity, status) = if value.status == VenueOrderStatus::Canceled {
        let Some(state) = canceled_order_state(cached, filled, ts_last, allow_post_only_rejection)
        else {
            return Ok(None);
        };

        state
    } else {
        (
            quantity(FixedPoint(total), instrument.size_precision)?,
            status,
        )
    };

    anyhow::ensure!(
        quantity.is_positive() && filled <= quantity,
        "invalid order total or filled count"
    );

    let default_tif = if expire_time.is_some() {
        TimeInForce::Gtd
    } else {
        TimeInForce::Gtc
    };

    let time_in_force = cached.map_or(default_tif, Order::time_in_force);
    anyhow::ensure!(
        (time_in_force == TimeInForce::Gtd) == expire_time.is_some(),
        "order expiration and command time in force disagree"
    );

    let mut report = OrderStatusReport::new(
        account_id,
        instrument.id,
        cached.map(Order::client_order_id).or(client_order_id),
        venue_order_id,
        Some(side),
        OrderType::Limit,
        time_in_force,
        status,
        quantity,
        filled,
        ts_accepted,
        ts_last,
        ts_init,
        None,
    );

    report.price = Some(px);
    report.expire_time = expire_time;
    if let Some(cached) = cached {
        report.post_only = cached.is_post_only();
        report.reduce_only = cached.is_reduce_only();
        report.order_list_id = cached.order_list_id();
    }

    Ok(Some(report))
}

fn canceled_order_state(
    cached: Option<&OrderAny>,
    filled: Quantity,
    ts_last: UnixNanos,
    allow_post_only_rejection: bool,
) -> Option<(Quantity, OrderStatus)> {
    let cached = cached?;
    let post_only_rejection = allow_post_only_rejection
        && cached.status() == OrderStatus::Rejected
        && cached.is_post_only()
        && filled.is_zero()
        && cached
            .events()
            .iter()
            .any(|event| matches!(event, OrderEventAny::Rejected(event) if event.due_post_only));

    if !cached.is_closed()
        || cached.ts_last() != ts_last && !post_only_rejection
        || cached.filled_qty() != filled
    {
        return None;
    }

    let status = if post_only_rejection {
        OrderStatus::Rejected
    } else {
        OrderStatus::Canceled
    };

    Some((cached.quantity(), status))
}

/// Builds a fill report from one venue fill and its order.
///
/// The report preserves the venue price, count, and fee. It does not apply the fill to an order.
///
/// # Errors
///
/// Returns an error if the fill and order identities or directions disagree, prices are not
/// complementary, or the count or fee is invalid.
pub fn fill(
    value: &Fill,
    instrument: &BinaryOption,
    account_id: AccountId,
    order: &VenueOrder,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    anyhow::ensure!(
        parse_instrument_id(&value.ticker)? == instrument.id
            && value.market_ticker == value.ticker
            && order.ticker == value.ticker
            && order.order_id == value.order_id,
        "fill order or instrument identity mismatch"
    );
    let side = direction(value.book_side, value.outcome_side)?;
    anyhow::ensure!(
        side == direction(order.book_side, order.outcome_side)?,
        "fill and order direction mismatch"
    );
    complementary(value.yes_price_dollars, value.no_price_dollars)?;
    let qty = size(value.count_fp, instrument.size_precision)?;
    let px = price(value.yes_price_dollars, instrument.price_precision)?;
    anyhow::ensure!(
        value.count_fp.0 <= order.fill_count_fp.0,
        "fill exceeds reported cumulative count"
    );
    anyhow::ensure!(value.fee_cost.0 >= Decimal::ZERO, "negative fill fee");
    let commission = Money::from_decimal_exact(value.fee_cost.0, Currency::USD())?;
    let ts_event = timestamp(&value.created_time)?;
    let venue_order_id = VenueOrderId::new_checked(&value.order_id)?;
    let trade_id = TradeId::new_checked(&value.fill_id)?;
    let client_order_id = client_id(&order.client_order_id)?;

    let liquidity = if value.is_taker {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    let report = FillReport::new(
        account_id,
        instrument.id,
        venue_order_id,
        trade_id,
        side,
        qty,
        px,
        commission,
        liquidity,
        client_order_id,
        None,
        ts_event,
        ts_init,
        None,
    );

    Ok(report)
}

pub(super) fn position(
    value: &MarketPosition,
    instrument: &BinaryOption,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    anyhow::ensure!(
        parse_instrument_id(&value.ticker)? == instrument.id,
        "position instrument mismatch"
    );
    let signed = value.position_fp.0;

    let side = if signed > Decimal::ZERO {
        PositionSide::Long
    } else if signed < Decimal::ZERO {
        PositionSide::Short
    } else {
        PositionSide::Flat
    };

    let magnitude = if signed < Decimal::ZERO {
        signed.abs()
    } else {
        signed
    };

    let qty = quantity(FixedPoint(magnitude), instrument.size_precision)?;
    let ts_last = timestamp(&Field::Value(value.last_updated_ts))?;

    let report = PositionStatusReport::new(
        account_id,
        instrument.id,
        side,
        qty,
        ts_last,
        ts_init,
        None,
        None,
        None,
    );

    Ok(report)
}

pub(super) fn quantity(value: FixedPoint, precision: u8) -> anyhow::Result<Quantity> {
    anyhow::ensure!(
        value.0 >= Decimal::ZERO,
        "negative order or position magnitude"
    );

    if value.0.is_zero() {
        return Ok(Quantity::zero(precision));
    }

    size(value, precision)
}

pub(super) fn direction(book: BookSide, outcome: OutcomeSide) -> anyhow::Result<OrderSide> {
    match (book, outcome) {
        (BookSide::Bid, OutcomeSide::Yes) => Ok(OrderSide::Buy),
        (BookSide::Ask, OutcomeSide::No) => Ok(OrderSide::Sell),
        _ => anyhow::bail!("inconsistent canonical execution direction"),
    }
}

pub(super) fn complementary(yes: FixedPoint, no: FixedPoint) -> anyhow::Result<()> {
    anyhow::ensure!(
        yes.0.checked_add(no.0) == Some(Decimal::ONE),
        "inconsistent complementary execution prices"
    );
    Ok(())
}

fn client_id(value: &str) -> anyhow::Result<Option<ClientOrderId>> {
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ClientOrderId::new_checked(value)?))
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UUID4;
    use nautilus_model::{
        events::{OrderAccepted, OrderCanceled, OrderEventAny, OrderRejected, OrderSubmitted},
        orders::builder::OrderTestBuilder,
        types::Price,
    };
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::http::{
        models::{Event, Market},
        parse::parse_instrument,
    };

    #[rstest]
    #[case("0.00", PositionSide::Flat)]
    #[case("7.25", PositionSide::Long)]
    #[case("-3.50", PositionSide::Short)]
    fn position_preserves_sign_flat_and_exact_fields(
        #[case] signed: &str,
        #[case] side: PositionSide,
    ) {
        let instrument = instrument();
        let mut value: MarketPosition =
            serde_json::from_value(fixture("positions")["market_positions"][0].clone()).unwrap();
        value.ticker = instrument.id.symbol.to_string();
        value.position_fp = FixedPoint(signed.parse().unwrap());
        let report = position(
            &value,
            &instrument,
            AccountId::from("KALSHI-007"),
            UnixNanos::from(543),
        )
        .unwrap();
        assert_eq!(report.account_id, AccountId::from("KALSHI-007"));
        assert_eq!(report.instrument_id, instrument.id);
        assert_eq!(report.position_side, side);
        assert_eq!(
            report.quantity.as_decimal(),
            signed.parse::<Decimal>().unwrap().abs()
        );
        assert_eq!(
            report.signed_decimal_qty,
            signed.parse::<Decimal>().unwrap()
        );
        assert_eq!(
            report.ts_last,
            try_datetime_to_unix_nanos(value.last_updated_ts).unwrap()
        );
        assert_eq!(report.ts_init, UnixNanos::from(543));
        assert_eq!(report.venue_position_id, None);
        assert_eq!(report.avg_px_open, None);
        assert_eq!(report.avg_px_open_reconciliation, Default::default());
        assert_eq!(report.avg_px_open_precision, None);
    }

    #[rstest]
    #[case(BookSide::Bid, OutcomeSide::Yes, OrderSide::Buy, true)]
    #[case(BookSide::Ask, OutcomeSide::No, OrderSide::Sell, false)]
    fn fill_preserves_every_report_field_and_authoritative_fee(
        #[case] book: BookSide,
        #[case] outcome: OutcomeSide,
        #[case] side: OrderSide,
        #[case] taker: bool,
    ) {
        let instrument = instrument();
        let mut raw: Fill = serde_json::from_value(fixture("fills")["fills"][0].clone()).unwrap();
        let mut context = venue_order(&instrument);
        raw.ticker = instrument.id.symbol.to_string();
        raw.market_ticker = raw.ticker.clone();
        raw.book_side = book;
        raw.outcome_side = outcome;
        raw.is_taker = taker;
        context.book_side = book;
        context.outcome_side = outcome;
        context.fill_count_fp = raw.count_fp;
        let report = fill(
            &raw,
            &instrument,
            AccountId::from("KALSHI-007"),
            &context,
            UnixNanos::from(543),
        )
        .unwrap();
        assert_eq!(report.account_id, AccountId::from("KALSHI-007"));
        assert_eq!(report.instrument_id, instrument.id);
        assert_eq!(report.venue_order_id, VenueOrderId::from("order-a"));
        assert_eq!(report.trade_id, TradeId::from("fill-a"));
        assert_eq!(report.order_side, side);
        assert_eq!(
            report.last_qty.as_decimal(),
            "2.75".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            report.last_px.as_decimal(),
            "0.123456".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            report.commission,
            Money::from_decimal_exact("0.000001".parse().unwrap(), Currency::USD()).unwrap()
        );
        assert_eq!(
            report.liquidity_side,
            if taker {
                LiquiditySide::Taker
            } else {
                LiquiditySide::Maker
            }
        );
        assert_eq!(report.avg_px, None);
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("client-a"))
        );
        assert_eq!(report.venue_position_id, None);
        assert_eq!(report.ts_event, timestamp(&raw.created_time).unwrap());
        assert_eq!(report.ts_init, UnixNanos::from(543));
    }

    #[rstest]
    #[case("direction")]
    #[case("price")]
    #[case("fee")]
    #[case("commission_bounds")]
    #[case("qty")]
    #[case("timestamp")]
    #[case("identity")]
    fn invalid_fill_economics_or_identity_fail(#[case] mode: &str) {
        let instrument = instrument();
        let mut value: Fill = serde_json::from_value(fixture("fills")["fills"][0].clone()).unwrap();
        value.ticker = instrument.id.symbol.to_string();
        value.market_ticker = value.ticker.clone();
        value.outcome_side = OutcomeSide::Yes;
        let mut context = venue_order(&instrument);
        context.book_side = BookSide::Bid;
        context.outcome_side = OutcomeSide::Yes;
        context.fill_count_fp = value.count_fp;
        match mode {
            "direction" => value.outcome_side = OutcomeSide::No,
            "price" => value.no_price_dollars = FixedPoint("0.10".parse().unwrap()),
            "fee" => value.fee_cost = FixedPoint("-0.000001".parse().unwrap()),
            "commission_bounds" => value.fee_cost = FixedPoint(Decimal::MAX),
            "qty" => value.count_fp = FixedPoint("2.751".parse().unwrap()),
            "timestamp" => value.created_time = Field::Missing,
            "identity" => value.order_id = "wrong-order".into(),
            _ => unreachable!(),
        }

        assert!(
            fill(
                &value,
                &instrument,
                AccountId::from("KALSHI-007"),
                &context,
                UnixNanos::from(543)
            )
            .is_err()
        );
    }

    #[rstest]
    #[case("0.0001")]
    #[case("-2.751")]
    fn unsupported_position_precision_fails(#[case] signed: &str) {
        let instrument = instrument();
        let mut value: MarketPosition =
            serde_json::from_value(fixture("positions")["market_positions"][0].clone()).unwrap();
        value.ticker = instrument.id.symbol.to_string();
        value.position_fp = FixedPoint(signed.parse().unwrap());
        assert!(
            position(
                &value,
                &instrument,
                AccountId::from("KALSHI-007"),
                UnixNanos::from(543)
            )
            .is_err()
        );
    }

    #[rstest]
    fn resting_order_uses_exact_totals_and_proven_command_metadata() {
        let instrument = instrument();
        let value = venue_order(&instrument);
        let cached = cached_order(&instrument);
        let report = order(
            &value,
            &instrument,
            AccountId::from("KALSHI-007"),
            Some(&cached),
            UnixNanos::from(543),
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.account_id, AccountId::from("KALSHI-007"));
        assert_eq!(report.instrument_id, instrument.id);
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("client-a"))
        );
        assert_eq!(report.venue_order_id, VenueOrderId::from("order-a"));
        assert_eq!(report.order_side, Some(OrderSide::Sell));
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.time_in_force, TimeInForce::Gtc);
        assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(
            report.quantity.as_decimal(),
            "5.75".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            report.filled_qty.as_decimal(),
            "2.25".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            report.price.unwrap().as_decimal(),
            "0.123401".parse::<Decimal>().unwrap()
        );
        assert_eq!(report.ts_accepted, timestamp(&value.created_time).unwrap());
        assert_eq!(report.ts_last, timestamp(&value.last_update_time).unwrap());
        assert_eq!(report.ts_init, UnixNanos::from(543));
        assert!(report.post_only);
        assert!(!report.reduce_only);
        assert_eq!(report.expire_time, None);
        assert_eq!(report.avg_px, None);
    }

    #[rstest]
    #[case(false, TimeInForce::Gtc)]
    #[case(true, TimeInForce::Gtd)]
    fn external_metadata_uses_recovery_defaults(#[case] expires: bool, #[case] tif: TimeInForce) {
        let instrument = instrument();
        let mut value = venue_order(&instrument);
        let expiry: jiff::Timestamp = "2026-10-17T12:34:56.123456789Z".parse().unwrap();
        value.expiration_time = if expires {
            Field::Value(expiry)
        } else {
            Field::Null
        };

        let report = order(
            &value,
            &instrument,
            AccountId::from("KALSHI-007"),
            None,
            UnixNanos::from(543),
        )
        .unwrap()
        .unwrap();
        let mut cached = cached_order(&instrument);

        // Compare the full report against the existing converter with equivalent known instructions
        if expires {
            cached = OrderTestBuilder::new(OrderType::Limit)
                .instrument_id(instrument.id)
                .client_order_id(ClientOrderId::from("client-a"))
                .side(OrderSide::Sell)
                .quantity(Quantity::from("5.75"))
                .price(Price::from("0.123401"))
                .time_in_force(TimeInForce::Gtd)
                .expire_time(try_datetime_to_unix_nanos(expiry).unwrap())
                .build();
        }

        let mut expected = order(
            &value,
            &instrument,
            AccountId::from("KALSHI-007"),
            Some(&cached),
            UnixNanos::from(543),
        )
        .unwrap()
        .unwrap();
        expected.report_id = report.report_id;
        expected.post_only = false;
        assert_eq!(report, expected);
        assert_eq!(report.time_in_force, tif);
        assert!(!report.post_only);
        assert!(!report.reduce_only);
    }

    #[rstest]
    fn external_canceled_quantity_is_not_inferred_from_original_count() {
        let instrument = instrument();
        let mut value = venue_order(&instrument);
        value.status = VenueOrderStatus::Canceled;
        value.remaining_count_fp = FixedPoint(Decimal::ZERO);
        assert_eq!(
            order(
                &value,
                &instrument,
                AccountId::from("KALSHI-007"),
                None,
                UnixNanos::from(543)
            )
            .unwrap(),
            None
        );
    }

    #[rstest]
    fn canceled_original_count_does_not_establish_final_total() {
        let instrument = instrument();
        let mut value = venue_order(&instrument);
        value.status = VenueOrderStatus::Canceled;
        value.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let cached = cached_order(&instrument);
        assert_eq!(
            order(
                &value,
                &instrument,
                AccountId::from("KALSHI-007"),
                Some(&cached),
                UnixNanos::from(543)
            )
            .unwrap(),
            None
        );
    }

    #[rstest]
    fn mismatched_cached_identity_is_an_error() {
        let instrument = instrument();
        let mut value = venue_order(&instrument);
        value.client_order_id = "another-client".into();
        assert!(
            order(
                &value,
                &instrument,
                AccountId::from("KALSHI-007"),
                Some(&cached_order(&instrument)),
                UnixNanos::from(543)
            )
            .is_err()
        );
    }

    #[rstest]
    fn executed_order_preserves_its_final_filled_total() {
        let instrument = instrument();
        let mut value = venue_order(&instrument);
        value.status = VenueOrderStatus::Executed;
        value.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let report = order(
            &value,
            &instrument,
            AccountId::from("KALSHI-007"),
            Some(&cached_order(&instrument)),
            UnixNanos::from(543),
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.order_status, OrderStatus::Filled);
        assert_eq!(
            report.quantity.as_decimal(),
            "2.25".parse::<Decimal>().unwrap()
        );
        assert_eq!(report.filled_qty, report.quantity);
    }

    #[rstest]
    fn unknown_order_status_is_an_error() {
        let instrument = instrument();
        let mut value = venue_order(&instrument);
        value.status = VenueOrderStatus::Unknown;
        assert!(
            order(
                &value,
                &instrument,
                AccountId::from("KALSHI-007"),
                Some(&cached_order(&instrument)),
                UnixNanos::from(543)
            )
            .unwrap_err()
            .to_string()
            .contains("unknown Kalshi order status")
        );
    }

    #[rstest]
    fn gtd_order_preserves_authoritative_expiration() {
        let instrument = instrument();
        let mut value = venue_order(&instrument);
        let expiry: jiff::Timestamp = "2026-10-17T12:34:56.123456789Z".parse().unwrap();
        value.expiration_time = Field::Value(expiry);
        let expiry = try_datetime_to_unix_nanos(expiry).unwrap();
        let cached = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id(ClientOrderId::from("client-a"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from_decimal_dp("5.75".parse().unwrap(), 2).unwrap())
            .price(Price::from_decimal_dp("0.123401".parse().unwrap(), 6).unwrap())
            .time_in_force(TimeInForce::Gtd)
            .expire_time(expiry)
            .build();
        let report = order(
            &value,
            &instrument,
            AccountId::from("KALSHI-007"),
            Some(&cached),
            UnixNanos::from(543),
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.time_in_force, TimeInForce::Gtd);
        assert_eq!(report.expire_time, Some(expiry));
    }

    #[rstest]
    fn canceled_order_uses_proven_total_instead_of_initial_count() {
        let instrument = instrument();
        let mut value = venue_order(&instrument);
        value.status = VenueOrderStatus::Canceled;
        value.fill_count_fp = FixedPoint(Decimal::ZERO);
        value.remaining_count_fp = FixedPoint(Decimal::ZERO);
        value.initial_count_fp = FixedPoint("10.00".parse().unwrap());
        let mut cached = cached_order(&instrument);
        let account = AccountId::from("KALSHI-007");
        let venue = VenueOrderId::from("order-a");
        cached
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                cached.trader_id(),
                cached.strategy_id(),
                instrument.id,
                cached.client_order_id(),
                venue,
                account,
                UUID4::new(),
                timestamp(&value.created_time).unwrap(),
                UnixNanos::from(123),
                false,
            )))
            .unwrap();
        cached
            .apply(OrderEventAny::Canceled(OrderCanceled::new(
                cached.trader_id(),
                cached.strategy_id(),
                instrument.id,
                cached.client_order_id(),
                UUID4::new(),
                timestamp(&value.last_update_time).unwrap(),
                UnixNanos::from(234),
                false,
                Some(venue),
                Some(account),
                None,
            )))
            .unwrap();
        let report = order(
            &value,
            &instrument,
            account,
            Some(&cached),
            UnixNanos::from(543),
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(
            report.quantity.as_decimal(),
            "5.75".parse::<Decimal>().unwrap()
        );
        assert_eq!(report.filled_qty, Quantity::zero(2));
    }

    #[rstest]
    fn post_only_rejection_relaxes_timestamp_only_for_aggregate() {
        let instrument = instrument();
        let mut value = venue_order(&instrument);
        value.status = VenueOrderStatus::Canceled;
        value.fill_count_fp = FixedPoint(Decimal::ZERO);
        value.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let account = AccountId::from("KALSHI-007");
        let submitted_at = timestamp(&value.created_time).unwrap();
        let rejected_at = UnixNanos::from(timestamp(&value.last_update_time).unwrap().as_u64() + 1);
        let mut cached = cached_order(&instrument);
        cached
            .apply(OrderEventAny::Submitted(OrderSubmitted::new(
                cached.trader_id(),
                cached.strategy_id(),
                instrument.id,
                cached.client_order_id(),
                account,
                UUID4::new(),
                submitted_at,
                submitted_at,
            )))
            .unwrap();
        cached
            .apply(OrderEventAny::Rejected(OrderRejected::new(
                cached.trader_id(),
                cached.strategy_id(),
                instrument.id,
                cached.client_order_id(),
                account,
                "Post-only order would cross".into(),
                UUID4::new(),
                rejected_at,
                rejected_at,
                false,
                true,
            )))
            .unwrap();

        let ordinary = order(
            &value,
            &instrument,
            account,
            Some(&cached),
            UnixNanos::from(543),
        )
        .unwrap();
        let aggregate =
            order_aggregate(&value, &instrument, account, &cached, UnixNanos::from(543))
                .unwrap()
                .unwrap();

        assert_eq!(ordinary, None);
        assert_eq!(aggregate.order_status, OrderStatus::Rejected);
        assert_eq!(aggregate.quantity, Quantity::from("5.75"));
        assert_eq!(aggregate.filled_qty, Quantity::zero(2));
        assert_eq!(
            aggregate.ts_last,
            timestamp(&value.last_update_time).unwrap()
        );
        assert_ne!(aggregate.ts_last, rejected_at);
        assert!(aggregate.post_only);
    }

    #[rstest]
    fn missing_position_update_time_is_an_error() {
        let instrument = instrument();
        let mut value: MarketPosition =
            serde_json::from_value(fixture("positions")["market_positions"][0].clone()).unwrap();
        value.ticker = instrument.id.symbol.to_string();
        value.last_updated_ts = jiff::Timestamp::UNIX_EPOCH;
        assert!(
            position(
                &value,
                &instrument,
                AccountId::from("KALSHI-007"),
                UnixNanos::from(543)
            )
            .unwrap_err()
            .to_string()
            .contains("invalid execution timestamp")
        );
    }

    fn fixture(name: &str) -> Value {
        let f: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        f[name].clone()
    }

    fn instrument() -> BinaryOption {
        let f: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let market: Market =
            serde_json::from_value(f["definitions"]["live"]["market"].clone()).unwrap();
        let event: Event =
            serde_json::from_value(f["definitions"]["live"]["event"].clone()).unwrap();
        parse_instrument(&market, &event, UnixNanos::from(1)).unwrap()
    }

    fn venue_order(instrument: &BinaryOption) -> VenueOrder {
        let mut value: VenueOrder =
            serde_json::from_value(fixture("order")["order"].clone()).unwrap();
        value.ticker = instrument.id.symbol.to_string();
        value.outcome_side = OutcomeSide::No;
        value
    }

    fn cached_order(instrument: &BinaryOption) -> OrderAny {
        OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id(ClientOrderId::from("client-a"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from_decimal_dp("5.75".parse().unwrap(), 2).unwrap())
            .price(Price::from_decimal_dp("0.123401".parse().unwrap(), 6).unwrap())
            .time_in_force(TimeInForce::Gtc)
            .post_only(true)
            .build()
    }
}
