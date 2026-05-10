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
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, VenueOrderId},
    reports::OrderStatusReport,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{BETFAIR_PRICE_PRECISION, BETFAIR_QUANTITY_PRECISION},
        enums::resolve_order_status,
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

    // Prefer lifecycle sum when price_size.size is zero (e.g. MOC/LOC orders)
    let total_size = order.price_size.size;
    let qty = if total_size > Decimal::ZERO {
        total_size
    } else {
        size_matched + size_remaining + size_cancelled + size_lapsed + size_voided
    };
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
}
