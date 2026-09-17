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

//! Conversions from Kalshi order, fill, and position payloads into Nautilus execution reports.
//!
//! Kalshi quotes a market from its YES side, and so does the adapter's instrument, so every price
//! reported here is a YES price: a NO position is reported as a short position in the instrument at
//! one minus the price its NO leg was bought at.

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce},
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Money, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{KALSHI_CURRENCY, KALSHI_SIZE_PRECISION},
        enums::{KalshiBookSide, KalshiOrderStatus, KalshiOrderType},
    },
    http::{
        error::{Error, Result},
        models::{KalshiFill, KalshiMarketPosition, KalshiOrder},
        parse::{
            instrument_id_for, parse_count_fp, parse_datetime_to_nanos, parse_money_dollars,
            parse_price_dollars,
        },
    },
};

/// Returns the number of decimal places a fixed-point dollar string carries.
///
/// The exchange emits as many decimal places as a value needs, in the range the market's price grid
/// allows, so the scale of the string is the only precision the value itself carries. Callers that
/// know the instrument's price precision should use that instead, so that prices parsed from
/// different responses stay comparable.
///
/// # Errors
///
/// Returns an error if the value carries more decimal places than a Nautilus price can represent.
pub fn fixed_point_precision(value: &str) -> Result<u8> {
    let fractional = value
        .trim()
        .split_once('.')
        .map_or(0, |(_, frac)| frac.trim_end_matches('0').len());

    u8::try_from(fractional).map_err(|_| {
        Error::Serde(format!(
            "Kalshi fixed-point value '{value}' carries more decimal places than can be represented"
        ))
    })
}

/// Maps the book side an order or fill touched to the order side on a YES-quoted instrument.
#[must_use]
pub const fn order_side_for(book_side: KalshiBookSide) -> OrderSide {
    match book_side {
        KalshiBookSide::Bid => OrderSide::Buy,
        KalshiBookSide::Ask => OrderSide::Sell,
    }
}

/// Returns the order type of a Kalshi order.
#[must_use]
pub const fn order_type_for(order_type: KalshiOrderType) -> OrderType {
    match order_type {
        KalshiOrderType::Limit => OrderType::Limit,
        KalshiOrderType::Market => OrderType::Market,
    }
}

/// Returns the liquidity side of a fill from the taker flag the exchange publishes.
#[must_use]
pub const fn liquidity_side_for(is_taker: bool) -> LiquiditySide {
    if is_taker {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    }
}

/// Parses the lifecycle state the exchange reports.
///
/// A state the adapter does not know is reported as pending rather than failing the caller: the
/// exchange adds order states without notice, and one unrecognized order must not stop a whole
/// reconciliation pass.
fn parse_order_status(status: &str) -> KalshiOrderStatus {
    match status.trim().parse::<KalshiOrderStatus>() {
        Ok(status) => status,
        Err(e) => {
            log::warn!("{e}, reporting the order as pending");
            KalshiOrderStatus::Pending
        }
    }
}

/// Maps an exchange order state onto a Nautilus order status.
fn order_status_for(
    status: KalshiOrderStatus,
    filled_qty: Quantity,
    quantity: Quantity,
) -> OrderStatus {
    let unfilled = filled_qty == Quantity::zero(filled_qty.precision);

    match status {
        KalshiOrderStatus::Pending => OrderStatus::Submitted,
        KalshiOrderStatus::Resting => {
            if unfilled {
                OrderStatus::Accepted
            } else {
                OrderStatus::PartiallyFilled
            }
        }
        KalshiOrderStatus::Canceled => OrderStatus::Canceled,
        KalshiOrderStatus::Executed => {
            if filled_qty < quantity {
                OrderStatus::Canceled
            } else {
                OrderStatus::Filled
            }
        }
    }
}

/// Parses an exchange timestamp into nanoseconds, falling back to `ts_default` when the exchange
/// omits it.
fn parse_optional_time(
    value: Option<&String>,
    field: &str,
    ts_default: UnixNanos,
) -> Result<UnixNanos> {
    match value.map(|value| value.trim()) {
        Some(value) if !value.is_empty() => parse_datetime_to_nanos(value, field),
        _ => Ok(ts_default),
    }
}

/// Builds an order status report from a Kalshi order.
///
/// The exchange does not echo the time in force of an order, so a resting order is reported as
/// good-till-canceled and a terminal order, which has already left the book, is reported the same
/// way: the value cannot change how the report is applied.
///
/// # Errors
///
/// Returns an error if the order's counts or timestamps cannot be parsed.
pub fn create_order_status_report(
    account_id: AccountId,
    order: &KalshiOrder,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<OrderStatusReport> {
    let instrument_id = instrument_id_for(&order.ticker);
    let filled_qty = parse_count_fp(&order.fill_count_fp, KALSHI_SIZE_PRECISION, "fill_count_fp")?;
    let remaining = parse_count_fp(
        &order.remaining_count_fp,
        KALSHI_SIZE_PRECISION,
        "remaining_count_fp",
    )?;
    let quantity = if order.initial_count_fp.trim().is_empty() {
        filled_qty + remaining
    } else {
        parse_count_fp(
            &order.initial_count_fp,
            KALSHI_SIZE_PRECISION,
            "initial_count_fp",
        )?
    };
    let status = parse_order_status(&order.status);
    let ts_accepted = parse_optional_time(order.created_time.as_ref(), "created_time", ts_init)?;
    let ts_last = parse_optional_time(
        order.last_update_time.as_ref(),
        "last_update_time",
        ts_accepted,
    )?;
    // The venue reports a limit order's price, and a consumer materializing an order from this report
    // needs it: a limit order without a price cannot be built.
    let price = parse_price_dollars(
        &order.yes_price_dollars,
        price_precision,
        "yes_price_dollars",
    )?;

    Ok(OrderStatusReport::new(
        account_id,
        instrument_id,
        order
            .client_order_id
            .as_ref()
            .map(|id| ClientOrderId::from(id.as_str())),
        VenueOrderId::from(order.order_id.as_str()),
        Some(order_side_for(order.book_side)),
        order_type_for(order.r#type),
        TimeInForce::Gtc,
        order_status_for(status, filled_qty, quantity),
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        None,
    )
    .with_price(price))
}

/// Builds a fill report from a Kalshi fill.
///
/// # Errors
///
/// Returns an error if the fill's count, price, fee, or timestamp cannot be parsed.
pub fn create_fill_report(
    account_id: AccountId,
    fill: &KalshiFill,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<FillReport> {
    let instrument_id = instrument_id_for(&fill.ticker);
    let last_qty = parse_count_fp(&fill.count_fp, KALSHI_SIZE_PRECISION, "count_fp")?;
    let last_px = parse_price_dollars(
        &fill.yes_price_dollars,
        price_precision,
        "yes_price_dollars",
    )?;
    let currency = Currency::from(KALSHI_CURRENCY);
    let commission = match fill.fee_cost.as_deref().map(str::trim) {
        Some(fee) if !fee.is_empty() => parse_money_dollars(fee, "fee_cost")?,
        _ => Money::zero(currency),
    };
    let ts_event = parse_optional_time(fill.created_time.as_ref(), "created_time", ts_init)?;

    Ok(FillReport::new(
        account_id,
        instrument_id,
        VenueOrderId::from(fill.order_id.as_str()),
        TradeId::from(fill.fill_id.as_str()),
        order_side_for(fill.book_side),
        last_qty,
        last_px,
        commission,
        liquidity_side_for(fill.is_taker),
        None,
        None,
        ts_event,
        ts_init,
        None,
    ))
}

/// Builds a position status report from a Kalshi market position.
///
/// A positive position is long the YES side. A negative position is short the YES side, which is
/// what holding the NO leg amounts to, so its cost basis is reported as one minus the price its NO
/// leg was bought at.
///
/// # Errors
///
/// Returns an error if the position's count, exposure, or timestamp cannot be parsed.
pub fn create_position_status_report(
    account_id: AccountId,
    position: &KalshiMarketPosition,
    ts_init: UnixNanos,
) -> Result<PositionStatusReport> {
    let instrument_id = instrument_id_for(&position.ticker);
    let contracts = Decimal::from_str_exact(position.position_fp.trim()).map_err(|e| {
        Error::Serde(format!(
            "Failed to parse Kalshi position_fp value '{}' as decimal: {e}",
            position.position_fp
        ))
    })?;
    let quantity =
        Quantity::from_decimal_dp(contracts.abs(), KALSHI_SIZE_PRECISION).map_err(|e| {
            Error::Serde(format!(
                "Failed to build a quantity from Kalshi position_fp value '{}': {e}",
                position.position_fp
            ))
        })?;
    let position_side = if contracts.is_zero() {
        PositionSide::Flat
    } else if contracts.is_sign_negative() {
        PositionSide::Short
    } else {
        PositionSide::Long
    };
    let avg_px_open = if contracts.is_zero() {
        None
    } else {
        let exposure =
            Decimal::from_str_exact(position.market_exposure_dollars.trim()).map_err(|e| {
                Error::Serde(format!(
                    "Failed to parse Kalshi market_exposure_dollars value '{}' as decimal: {e}",
                    position.market_exposure_dollars
                ))
            })?;
        let basis = exposure / contracts.abs();

        Some(if contracts.is_sign_negative() {
            Decimal::ONE - basis
        } else {
            basis
        })
    };
    let ts_last = parse_optional_time(Some(&position.last_updated_ts), "last_updated_ts", ts_init)?;

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        Option::<UUID4>::None,
        None,
        avg_px_open,
    ))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::{identifiers::InstrumentId, types::Price};
    use rstest::rstest;

    use super::*;
    use crate::http::models::{KalshiFill, KalshiMarketPosition, KalshiOrder};

    fn order(status: &str, fill_count: &str, remaining: &str, initial: &str) -> KalshiOrder {
        serde_json::from_str(&format!(
            r#"{{
                "order_id": "order-1",
                "client_order_id": "O-20250101-000000-001-001-1",
                "ticker": "KXHIGHNY-25JAN01-T50",
                "outcome_side": "yes",
                "book_side": "bid",
                "type": "limit",
                "status": "{status}",
                "yes_price_dollars": "0.3400",
                "no_price_dollars": "0.6600",
                "fill_count_fp": "{fill_count}",
                "remaining_count_fp": "{remaining}",
                "initial_count_fp": "{initial}",
                "created_time": "2025-01-01T12:00:00Z",
                "last_update_time": "2025-01-01T12:00:05Z"
            }}"#
        ))
        .unwrap()
    }

    fn account_id() -> AccountId {
        AccountId::from("KALSHI-001")
    }

    fn ts_init() -> UnixNanos {
        UnixNanos::from(1_735_732_800_000_000_000u64)
    }

    /// The scale of a value is the decimal places it needs, so trailing zeros carry none: a price
    /// parsed from `0.3400` stays comparable with one parsed from `0.34`.
    #[rstest]
    #[case("0.3400", 2)]
    #[case("0.34", 2)]
    #[case("1", 0)]
    #[case("0.0050", 3)]
    fn test_fixed_point_precision_reads_the_scale_of_the_value(
        #[case] value: &str,
        #[case] expected: u8,
    ) {
        assert_eq!(fixed_point_precision(value).unwrap(), expected);
    }

    #[rstest]
    fn test_resting_order_reports_accepted_and_partial_reports_partially_filled() {
        let accepted = create_order_status_report(
            account_id(),
            &order("resting", "0.00", "100.00", "100.00"),
            4,
            ts_init(),
        )
        .unwrap();

        assert_eq!(accepted.order_status, OrderStatus::Accepted);
        assert_eq!(accepted.quantity, Quantity::from("100.00"));
        assert_eq!(accepted.filled_qty, Quantity::zero(2));
        assert_eq!(accepted.order_side, Some(OrderSide::Buy));
        assert_eq!(accepted.order_type, OrderType::Limit);
        assert_eq!(
            accepted.client_order_id,
            Some(ClientOrderId::from("O-20250101-000000-001-001-1"))
        );
        assert_eq!(accepted.venue_order_id, VenueOrderId::from("order-1"));
        assert_eq!(
            accepted.instrument_id,
            InstrumentId::from("KXHIGHNY-25JAN01-T50.KALSHI")
        );

        let partial = create_order_status_report(
            account_id(),
            &order("resting", "25.00", "75.00", "100.00"),
            4,
            ts_init(),
        )
        .unwrap();

        assert_eq!(partial.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(partial.filled_qty, Quantity::from("25.00"));
    }

    #[rstest]
    fn test_executed_order_is_filled_and_canceled_order_is_canceled() {
        let filled = create_order_status_report(
            account_id(),
            &order("executed", "100.00", "0.00", "100.00"),
            4,
            ts_init(),
        )
        .unwrap();
        let canceled = create_order_status_report(
            account_id(),
            &order("canceled", "0.00", "100.00", "100.00"),
            4,
            ts_init(),
        )
        .unwrap();

        assert_eq!(filled.order_status, OrderStatus::Filled);
        assert_eq!(canceled.order_status, OrderStatus::Canceled);
    }

    #[rstest]
    fn test_executed_order_that_underfilled_is_reported_canceled() {
        // A fill-or-kill order that the exchange reports as executed without filling is short of its
        // quantity, and reporting it filled would leave the engine holding a position the account
        // does not have.
        let report = create_order_status_report(
            account_id(),
            &order("executed", "60.00", "0.00", "100.00"),
            4,
            ts_init(),
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.filled_qty, Quantity::from("60.00"));
    }

    #[rstest]
    fn test_unknown_order_status_is_reported_as_pending_not_failed() {
        let report = create_order_status_report(
            account_id(),
            &order("queued", "0.00", "5.00", "5.00"),
            4,
            ts_init(),
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Submitted);
    }

    #[rstest]
    fn test_order_timestamps_come_from_the_exchange() {
        let report = create_order_status_report(
            account_id(),
            &order("resting", "0.00", "10.00", "10.00"),
            4,
            ts_init(),
        )
        .unwrap();

        assert_eq!(
            report.ts_accepted,
            UnixNanos::from(1_735_732_800_000_000_000u64)
        );
        assert_eq!(
            report.ts_last,
            UnixNanos::from(1_735_732_805_000_000_000u64)
        );
    }

    #[rstest]
    fn test_fill_report_carries_yes_price_fee_and_liquidity() {
        let fill: KalshiFill = serde_json::from_str(
            r#"{
                "fill_id": "fill-1",
                "trade_id": "fill-1",
                "order_id": "order-1",
                "ticker": "KXHIGHNY-25JAN01-T50",
                "outcome_side": "yes",
                "book_side": "bid",
                "count_fp": "25.00",
                "yes_price_dollars": "0.3400",
                "no_price_dollars": "0.6600",
                "is_taker": true,
                "fee_cost": "0.0440",
                "created_time": "2025-01-01T12:00:01Z"
            }"#,
        )
        .unwrap();

        let report = create_fill_report(account_id(), &fill, 4, ts_init()).unwrap();

        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.last_qty, Quantity::from("25.00"));
        assert_eq!(report.last_px, Price::from("0.3400"));
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(report.commission, Money::from_str("0.0440 USD").unwrap());
        assert_eq!(report.trade_id, TradeId::from("fill-1"));
        assert_eq!(report.venue_order_id, VenueOrderId::from("order-1"));
    }

    #[rstest]
    fn test_selling_fill_is_reported_as_a_sell_of_the_instrument() {
        let fill: KalshiFill = serde_json::from_str(
            r#"{
                "fill_id": "fill-2",
                "trade_id": "fill-2",
                "order_id": "order-2",
                "ticker": "KXHIGHNY-25JAN01-T50",
                "outcome_side": "yes",
                "book_side": "ask",
                "count_fp": "10.00",
                "yes_price_dollars": "0.3600",
                "no_price_dollars": "0.6400",
                "is_taker": false
            }"#,
        )
        .unwrap();

        let report = create_fill_report(account_id(), &fill, 4, ts_init()).unwrap();

        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(
            report.commission,
            Money::zero(Currency::from(KALSHI_CURRENCY))
        );
        // A missing timestamp falls back to the caller's clock rather than failing the fill.
        assert_eq!(report.ts_event, ts_init());
    }

    #[rstest]
    fn test_long_position_reports_exposure_as_its_cost_basis() {
        let position: KalshiMarketPosition = serde_json::from_str(
            r#"{
                "ticker": "KXHIGHNY-25JAN01-T50",
                "exchange_index": 0,
                "total_traded_dollars": "350.0000",
                "position_fp": "100.00",
                "market_exposure_dollars": "34.0000",
                "realized_pnl_dollars": "0.0000",
                "fees_paid_dollars": "1.7500",
                "last_updated_ts": "2025-01-01T12:00:00Z"
            }"#,
        )
        .unwrap();

        let report = create_position_status_report(account_id(), &position, ts_init()).unwrap();

        assert_eq!(report.position_side, PositionSide::Long);
        assert_eq!(report.quantity, Quantity::from("100.00"));
        assert_eq!(report.avg_px_open, Some(Decimal::from_str("0.34").unwrap()));
    }

    #[rstest]
    fn test_short_position_reports_one_minus_its_no_leg_cost() {
        let position: KalshiMarketPosition = serde_json::from_str(
            r#"{
                "ticker": "KXHIGHNY-25JAN01-T50",
                "exchange_index": 0,
                "total_traded_dollars": "650.0000",
                "position_fp": "-100.00",
                "market_exposure_dollars": "65.0000",
                "realized_pnl_dollars": "0.0000",
                "fees_paid_dollars": "1.7500",
                "last_updated_ts": "2025-01-01T12:00:00Z"
            }"#,
        )
        .unwrap();

        let report = create_position_status_report(account_id(), &position, ts_init()).unwrap();

        assert_eq!(report.position_side, PositionSide::Short);
        assert_eq!(report.quantity, Quantity::from("100.00"));
        assert_eq!(report.avg_px_open, Some(Decimal::from_str("0.35").unwrap()));
    }

    #[rstest]
    fn test_flat_position_reports_no_cost_basis() {
        let position: KalshiMarketPosition = serde_json::from_str(
            r#"{
                "ticker": "KXHIGHNY-25JAN01-T50",
                "exchange_index": 0,
                "total_traded_dollars": "0.0000",
                "position_fp": "0.00",
                "market_exposure_dollars": "0.0000",
                "realized_pnl_dollars": "0.0000",
                "fees_paid_dollars": "0.0000",
                "last_updated_ts": "2025-01-01T12:00:00Z"
            }"#,
        )
        .unwrap();

        let report = create_position_status_report(account_id(), &position, ts_init()).unwrap();

        assert_eq!(report.position_side, PositionSide::Flat);
        assert_eq!(report.quantity, Quantity::zero(2));
        assert_eq!(report.avg_px_open, None);
    }
}
