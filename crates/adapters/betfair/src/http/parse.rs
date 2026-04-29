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

//! Parsing utilities that convert Betfair HTTP/REST responses into Nautilus domain models.

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{BETFAIR_PRICE_PRECISION, BETFAIR_QUANTITY_PRECISION},
        enums::{BetfairOrderType, resolve_order_status},
        parse::{make_instrument_id, parse_betfair_timestamp},
    },
    http::models::CurrentOrderSummary,
};

/// Parses a Betfair [`CurrentOrderSummary`] into a Nautilus [`OrderStatusReport`].
///
/// # Errors
///
/// Returns an error if the placed date cannot be parsed.
pub fn parse_current_order_report(
    order: &CurrentOrderSummary,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = make_instrument_id(&order.market_id, order.selection_id, order.handicap);

    let order_side = OrderSide::from(order.side);
    let order_type = OrderType::from(order.order_type);
    let time_in_force = TimeInForce::from(order.persistence_type);

    let size_matched = order.size_matched.unwrap_or(Decimal::ZERO);
    let size_remaining = order.size_remaining.unwrap_or(Decimal::ZERO);
    let size_cancelled = order.size_cancelled.unwrap_or(Decimal::ZERO);
    let size_lapsed = order.size_lapsed.unwrap_or(Decimal::ZERO);
    let size_voided = order.size_voided.unwrap_or(Decimal::ZERO);

    // Include lapsed/voided in the closed quantity for status resolution
    let size_closed = size_cancelled + size_lapsed + size_voided;
    let order_status = resolve_order_status(order.status, size_matched, size_closed);

    // Prefer lifecycle sum when price_size.size is zero. Use bsp_liability for
    // on-close orders that report liability without stake/size.
    let total_size = order.price_size.size;
    let lifecycle_qty = size_matched + size_remaining + size_cancelled + size_lapsed + size_voided;
    let qty = if total_size > Decimal::ZERO {
        total_size
    } else if lifecycle_qty > Decimal::ZERO {
        lifecycle_qty
    } else if uses_liability_based_quantity(order) && order.bsp_liability > Decimal::ZERO {
        order.bsp_liability
    } else {
        Decimal::ZERO
    };
    anyhow::ensure!(
        qty > Decimal::ZERO,
        "failed to resolve positive quantity for current order {} \
         (order_type={:?}, persistence_type={:?}, price_size={}, bsp_liability={}, \
         size_matched={}, size_remaining={}, size_cancelled={}, size_lapsed={}, size_voided={})",
        order.bet_id,
        order.order_type,
        order.persistence_type,
        order.price_size.size,
        order.bsp_liability,
        size_matched,
        size_remaining,
        size_cancelled,
        size_lapsed,
        size_voided,
    );
    let quantity = Quantity::from_decimal_dp(qty, BETFAIR_QUANTITY_PRECISION)?;
    let filled_qty = Quantity::from_decimal_dp(size_matched, BETFAIR_QUANTITY_PRECISION)?;

    let ts_accepted = parse_betfair_timestamp(&order.placed_date)?;
    let ts_last = order
        .matched_date
        .as_deref()
        .and_then(|d| parse_betfair_timestamp(d).ok())
        .unwrap_or(ts_accepted);

    let venue_order_id = VenueOrderId::from(order.bet_id.as_str());
    let client_order_id = order.customer_order_ref.as_deref().map(ClientOrderId::from);

    let price = Price::from_decimal_dp(order.price_size.price, BETFAIR_PRICE_PRECISION)?;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        None,
    )
    .with_price(price);

    report.avg_px = order.average_price_matched;

    Ok(report)
}

fn uses_liability_based_quantity(order: &CurrentOrderSummary) -> bool {
    matches!(
        order.order_type,
        BetfairOrderType::LimitOnClose
            | BetfairOrderType::MarketOnClose
            | BetfairOrderType::MarketAtTheClose
    )
}

/// Parses a Betfair [`CurrentOrderSummary`] into a Nautilus [`FillReport`].
///
/// Uses cumulative `size_matched` and `average_price_matched` to produce a
/// single fill representing the total execution. Trade IDs use the format
/// `{bet_id}-{size_matched}` for deterministic uniqueness.
///
/// # Errors
///
/// Returns an error if timestamps or decimal values cannot be parsed.
pub fn parse_current_order_fill_report(
    order: &CurrentOrderSummary,
    account_id: AccountId,
    currency: Currency,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = make_instrument_id(&order.market_id, order.selection_id, order.handicap);
    let venue_order_id = VenueOrderId::from(order.bet_id.as_str());
    let client_order_id = order.customer_order_ref.as_deref().map(ClientOrderId::from);
    let order_side = OrderSide::from(order.side);

    let size_matched = order.size_matched.unwrap_or(Decimal::ZERO);
    let avg_px = order
        .average_price_matched
        .unwrap_or(order.price_size.price);

    let last_qty = Quantity::from_decimal_dp(size_matched, BETFAIR_QUANTITY_PRECISION)?;
    let last_px = Price::from_decimal_dp(avg_px, BETFAIR_PRICE_PRECISION)?;

    let trade_id = TradeId::new(format!("{}-{size_matched}", order.bet_id));

    let ts_event = order
        .matched_date
        .as_deref()
        .and_then(|d| parse_betfair_timestamp(d).ok())
        .unwrap_or(parse_betfair_timestamp(&order.placed_date)?);

    Ok(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        Money::new(0.0, currency),
        LiquiditySide::NoLiquiditySide,
        client_order_id,
        None,
        ts_event,
        ts_init,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::OrderStatus;
    use rstest::rstest;

    use super::*;
    use crate::{
        common::testing::{load_test_json, parse_jsonrpc},
        http::models::CurrentOrderSummaryReport,
    };

    #[rstest]
    fn test_parse_current_order_single() {
        let data = load_test_json("rest/list_current_orders_single.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);
        let order = &resp.current_orders[0];

        let report =
            parse_current_order_report(order, AccountId::from("BETFAIR-001"), UnixNanos::default())
                .unwrap();

        assert_eq!(
            report.venue_order_id,
            VenueOrderId::from(order.bet_id.as_str())
        );
        assert_eq!(report.order_side, OrderSide::from(order.side));
        assert!(report.price.is_some());
    }

    #[rstest]
    fn test_parse_current_order_executable() {
        let data = load_test_json("rest/list_current_orders_executable.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        for order in &resp.current_orders {
            let report = parse_current_order_report(
                order,
                AccountId::from("BETFAIR-001"),
                UnixNanos::default(),
            )
            .unwrap();

            // Executable orders are either Accepted or PartiallyFilled
            assert!(
                report.order_status == OrderStatus::Accepted
                    || report.order_status == OrderStatus::PartiallyFilled,
                "unexpected status: {:?}",
                report.order_status,
            );
        }
    }

    #[rstest]
    fn test_parse_current_order_execution_complete() {
        let data = load_test_json("rest/list_current_orders_execution_complete.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        // Fixture contains a mix of Executable and ExecutionComplete orders
        let mut has_filled = false;

        for order in &resp.current_orders {
            let report = parse_current_order_report(
                order,
                AccountId::from("BETFAIR-001"),
                UnixNanos::default(),
            )
            .unwrap();

            assert!(
                matches!(
                    report.order_status,
                    OrderStatus::Filled
                        | OrderStatus::Canceled
                        | OrderStatus::Accepted
                        | OrderStatus::PartiallyFilled,
                ),
                "unexpected status: {:?}",
                report.order_status,
            );

            if report.order_status == OrderStatus::Filled {
                has_filled = true;
            }
        }

        assert!(
            has_filled,
            "fixture should contain at least one filled order"
        );
    }

    #[rstest]
    fn test_parse_current_order_lapsed() {
        let data = load_test_json("rest/list_current_orders_lapsed.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        // First order: BACK, fully lapsed, no matches
        let order = &resp.current_orders[0];
        let report =
            parse_current_order_report(order, AccountId::from("BETFAIR-001"), UnixNanos::default())
                .unwrap();

        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.filled_qty, Quantity::from("0.00"));
        assert_eq!(report.quantity, Quantity::from("20.00"));
        assert_eq!(report.venue_order_id, VenueOrderId::from("229430281400"));
    }

    #[rstest]
    fn test_parse_current_order_partially_filled_and_voided() {
        let data = load_test_json("rest/list_current_orders_lapsed.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        // Second order: LAY, sizeMatched=30, sizeLapsed=10, sizeVoided=10
        let order = &resp.current_orders[1];
        let report =
            parse_current_order_report(order, AccountId::from("BETFAIR-001"), UnixNanos::default())
                .unwrap();

        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.filled_qty, Quantity::from("30.00"));
        assert_eq!(report.quantity, Quantity::from("50.00"));
        assert_eq!(report.avg_px, Some(Decimal::new(24, 1)));
    }

    #[rstest]
    fn test_parse_current_order_market_on_close_uses_bsp_liability() {
        let data = r#"{
          "jsonrpc": "2.0",
          "id": 1,
          "result": {
            "currentOrders": [
              {
                "betId": "424009603606",
                "marketId": "1.256134154",
                "selectionId": 86018523,
                "handicap": 0.0,
                "priceSize": {
                  "price": 1.01,
                  "size": 0.0
                },
                "bspLiability": 2.0,
                "side": "BACK",
                "status": "EXECUTABLE",
                "persistenceType": "MARKET_ON_CLOSE",
                "orderType": "MARKET_ON_CLOSE",
                "placedDate": "2026-04-03T00:51:29.000Z",
                "averagePriceMatched": 0.0,
                "sizeMatched": 0.0,
                "sizeRemaining": 0.0,
                "sizeLapsed": 0.0,
                "sizeCancelled": 0.0,
                "sizeVoided": 0.0
              }
            ],
            "moreAvailable": false
          }
        }"#;
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(data);
        let order = &resp.current_orders[0];

        let report =
            parse_current_order_report(order, AccountId::from("BETFAIR-001"), UnixNanos::default())
                .unwrap();

        assert_eq!(report.order_type, OrderType::Market);
        assert_eq!(report.time_in_force, TimeInForce::AtTheClose);
        assert_eq!(report.quantity, Quantity::from("2.00"));
    }

    #[rstest]
    fn test_parse_current_order_zero_quantity_sources_fails() {
        let data = r#"{
          "jsonrpc": "2.0",
          "id": 1,
          "result": {
            "currentOrders": [
              {
                "betId": "424009603607",
                "marketId": "1.256134154",
                "selectionId": 86018523,
                "handicap": 0.0,
                "priceSize": {
                  "price": 1.01,
                  "size": 0.0
                },
                "bspLiability": 0.0,
                "side": "BACK",
                "status": "EXECUTABLE",
                "persistenceType": "LAPSE",
                "orderType": "LIMIT",
                "placedDate": "2026-04-03T00:51:29.000Z",
                "averagePriceMatched": 0.0,
                "sizeMatched": 0.0,
                "sizeRemaining": 0.0,
                "sizeLapsed": 0.0,
                "sizeCancelled": 0.0,
                "sizeVoided": 0.0
              }
            ],
            "moreAvailable": false
          }
        }"#;
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(data);
        let order = &resp.current_orders[0];

        let result =
            parse_current_order_report(order, AccountId::from("BETFAIR-001"), UnixNanos::default());

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("failed to resolve positive quantity for current order 424009603607")
        );
    }

    #[rstest]
    fn test_parse_current_order_customer_order_ref() {
        let data = load_test_json("rest/list_current_orders_lapsed.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        // First order has customerOrderRef, second does not
        let report1 = parse_current_order_report(
            &resp.current_orders[0],
            AccountId::from("BETFAIR-001"),
            UnixNanos::default(),
        )
        .unwrap();
        let report2 = parse_current_order_report(
            &resp.current_orders[1],
            AccountId::from("BETFAIR-001"),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(
            report1.client_order_id,
            Some(ClientOrderId::from("O-20210730-001"))
        );
        assert!(report2.client_order_id.is_none());
    }

    #[rstest]
    fn test_parse_fill_report_matched_order() {
        let data = load_test_json("rest/list_current_orders_execution_complete.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        // Second order: BACK, fully matched, sizeMatched=10, avgPx=1.9
        let order = &resp.current_orders[1];
        let currency = Currency::from("GBP");
        let report = parse_current_order_fill_report(
            order,
            AccountId::from("BETFAIR-001"),
            currency,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.venue_order_id, VenueOrderId::from("228059821049"));
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.last_qty, Quantity::from("10.00"));
        assert_eq!(report.last_px, Price::from("1.90"));
        assert_eq!(report.trade_id, TradeId::new("228059821049-10"));
        assert_eq!(report.commission, Money::new(0.0, currency));
        assert_eq!(report.liquidity_side, LiquiditySide::NoLiquiditySide);
    }

    #[rstest]
    fn test_parse_fill_report_unmatched_order_skips() {
        let data = load_test_json("rest/list_current_orders_execution_complete.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        // First order: sizeMatched=0, should still parse but with zero qty
        let order = &resp.current_orders[0];
        let report = parse_current_order_fill_report(
            order,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.last_qty, Quantity::from("0.00"));
    }

    #[rstest]
    fn test_parse_fill_report_lay_side() {
        let data = load_test_json("rest/list_current_orders_execution_complete.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        // Third order: LAY side
        let order = &resp.current_orders[2];
        let report = parse_current_order_fill_report(
            order,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.last_qty, Quantity::from("10.00"));
        assert_eq!(report.last_px, Price::from("1.92"));
    }

    #[rstest]
    fn test_parse_fill_report_partially_matched() {
        let data = load_test_json("rest/list_current_orders_lapsed.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        // Second order: sizeMatched=30, avgPx=2.4
        let order = &resp.current_orders[1];
        let report = parse_current_order_fill_report(
            order,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.last_qty, Quantity::from("30.00"));
        assert_eq!(report.last_px, Price::from("2.40"));
        assert_eq!(report.trade_id, TradeId::new("229430281401-30"));
    }

    #[rstest]
    fn test_parse_fill_report_customer_order_ref() {
        let data = load_test_json("rest/list_current_orders_lapsed.json");
        let resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);

        // First order has customerOrderRef
        let order = &resp.current_orders[0];
        let report = parse_current_order_fill_report(
            order,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-20210730-001"))
        );

        // Second order has no customerOrderRef
        let order2 = &resp.current_orders[1];
        let report2 = parse_current_order_fill_report(
            order2,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
        )
        .unwrap();

        assert!(report2.client_order_id.is_none());
    }
}
