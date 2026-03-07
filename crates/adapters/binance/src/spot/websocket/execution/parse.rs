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

//! Parse functions for converting Binance Spot venue types to Nautilus reports.
//!
//! Pure functions that take venue message + instrument + account_id + ts_init
//! and return Nautilus report types.

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

use super::messages::{BinanceSpotAccountPositionMsg, BinanceSpotExecutionReport};
use crate::common::{
    consts::BINANCE_NAUTILUS_SPOT_BROKER_ID,
    encoder::decode_broker_id,
    enums::{BinanceOrderStatus, BinanceSide, BinanceTimeInForce},
};

/// Converts a Binance Spot execution report to a Nautilus order status report.
///
/// # Errors
///
/// Returns an error if report construction fails.
pub fn parse_spot_exec_report_to_order_status(
    msg: &BinanceSpotExecutionReport,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let client_order_id = ClientOrderId::new(decode_broker_id(
        &msg.client_order_id,
        BINANCE_NAUTILUS_SPOT_BROKER_ID,
    ));
    let venue_order_id = VenueOrderId::new(msg.order_id.to_string());
    let ts_event = UnixNanos::from((msg.event_time * 1_000_000) as u64);

    let order_side = match msg.side {
        BinanceSide::Buy => OrderSide::Buy,
        BinanceSide::Sell => OrderSide::Sell,
    };

    let order_status = parse_order_status(msg.order_status);
    let order_type = parse_spot_order_type(&msg.order_type);
    let time_in_force = parse_time_in_force(msg.time_in_force);

    let quantity: f64 = msg.original_qty.parse().unwrap_or(0.0);
    let filled_qty: f64 = msg.cumulative_filled_qty.parse().unwrap_or(0.0);
    let price: f64 = msg.price.parse().unwrap_or(0.0);

    let avg_px = if filled_qty > 0.0 {
        let cum_quote: f64 = msg.cumulative_quote_qty.parse().unwrap_or(0.0);
        Some(Price::new(cum_quote / filled_qty, price_precision))
    } else {
        None
    };

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        Quantity::new(quantity, size_precision),
        Quantity::new(filled_qty, size_precision),
        ts_event,
        ts_event,
        ts_init,
        None, // report_id
    );

    report.price = Some(Price::new(price, price_precision));
    report.post_only = msg.order_type == "LIMIT_MAKER";

    let stop_price: f64 = msg.stop_price.parse().unwrap_or(0.0);
    if stop_price > 0.0 {
        report.trigger_price = Some(Price::new(stop_price, price_precision));
    }

    if let Some(avg) = avg_px {
        report.avg_px = Some(avg.as_decimal());
    }

    Ok(report)
}

/// Converts a Binance Spot execution report (Trade type) to a Nautilus fill report.
///
/// # Errors
///
/// Returns an error if report construction fails.
pub fn parse_spot_exec_report_to_fill(
    msg: &BinanceSpotExecutionReport,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let client_order_id = ClientOrderId::new(decode_broker_id(
        &msg.client_order_id,
        BINANCE_NAUTILUS_SPOT_BROKER_ID,
    ));
    let venue_order_id = VenueOrderId::new(msg.order_id.to_string());
    let trade_id = TradeId::new(msg.trade_id.to_string());
    let ts_event = UnixNanos::from((msg.event_time * 1_000_000) as u64);

    let order_side = match msg.side {
        BinanceSide::Buy => OrderSide::Buy,
        BinanceSide::Sell => OrderSide::Sell,
    };

    let liquidity_side = if msg.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    let last_qty: f64 = msg.last_filled_qty.parse().unwrap_or(0.0);
    let last_px: f64 = msg.last_filled_price.parse().unwrap_or(0.0);
    let commission: f64 = msg.commission.parse().unwrap_or(0.0);
    let commission_currency = msg
        .commission_asset
        .as_ref()
        .map_or_else(Currency::USDT, |a| {
            Currency::get_or_create_crypto(a.as_str())
        });

    Ok(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        Quantity::new(last_qty, size_precision),
        Price::new(last_px, price_precision),
        Money::new(commission, commission_currency),
        liquidity_side,
        Some(client_order_id),
        None, // venue_position_id
        ts_event,
        ts_init,
        None, // report_id
    ))
}

/// Converts a Binance Spot account position update to a Nautilus account state.
pub fn parse_spot_account_position(
    msg: &BinanceSpotAccountPositionMsg,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> AccountState {
    let ts_event = UnixNanos::from((msg.event_time * 1_000_000) as u64);

    let balances: Vec<AccountBalance> = msg
        .balances
        .iter()
        .map(|b| {
            let free: f64 = b.free.parse().unwrap_or(0.0);
            let locked: f64 = b.locked.parse().unwrap_or(0.0);
            let total = free + locked;

            let currency = Currency::get_or_create_crypto(b.asset.as_str());
            AccountBalance::new(
                Money::new(total, currency),
                Money::new(locked, currency),
                Money::new(free, currency),
            )
        })
        .collect();

    AccountState::new(
        account_id,
        AccountType::Cash,
        balances,
        vec![], // No margins for spot
        true,   // is_reported
        UUID4::new(),
        ts_event,
        ts_init,
        None, // base_currency
    )
}

fn parse_order_status(status: BinanceOrderStatus) -> OrderStatus {
    match status {
        BinanceOrderStatus::New => OrderStatus::Accepted,
        BinanceOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        BinanceOrderStatus::Filled => OrderStatus::Filled,
        BinanceOrderStatus::Canceled | BinanceOrderStatus::PendingCancel => OrderStatus::Canceled,
        BinanceOrderStatus::Rejected => OrderStatus::Rejected,
        BinanceOrderStatus::Expired | BinanceOrderStatus::ExpiredInMatch => OrderStatus::Expired,
        BinanceOrderStatus::Unknown => OrderStatus::Accepted,
    }
}

fn parse_spot_order_type(order_type: &str) -> OrderType {
    match order_type {
        "LIMIT" | "LIMIT_MAKER" => OrderType::Limit,
        "MARKET" => OrderType::Market,
        "STOP_LOSS" => OrderType::StopMarket,
        "STOP_LOSS_LIMIT" => OrderType::StopLimit,
        "TAKE_PROFIT" => OrderType::MarketIfTouched,
        "TAKE_PROFIT_LIMIT" => OrderType::LimitIfTouched,
        _ => OrderType::Market,
    }
}

fn parse_time_in_force(tif: BinanceTimeInForce) -> TimeInForce {
    match tif {
        BinanceTimeInForce::Gtc | BinanceTimeInForce::Gtx => TimeInForce::Gtc,
        BinanceTimeInForce::Ioc => TimeInForce::Ioc,
        BinanceTimeInForce::Fok => TimeInForce::Fok,
        BinanceTimeInForce::Gtd => TimeInForce::Gtd,
        BinanceTimeInForce::Unknown => TimeInForce::Gtc,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::spot::websocket::execution::messages::BinanceSpotExecutionReport;

    const PRICE_PRECISION: u8 = 2;
    const SIZE_PRECISION: u8 = 5;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("ETHUSDT.BINANCE")
    }

    #[rstest]
    fn test_parse_execution_report_to_order_status_report() {
        let json = include_str!("../../../../test_data/ws_spot_execution_report_new.json");
        let msg: BinanceSpotExecutionReport = serde_json::from_str(json).unwrap();
        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = parse_spot_exec_report_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id,
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id());
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.venue_order_id, VenueOrderId::new("12345678"));
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-20200101-000000-000-000-0")),
        );
        assert!(report.trigger_price.is_none());
    }

    #[rstest]
    fn test_parse_execution_report_stop_loss_has_trigger_price() {
        let json = include_str!("../../../../test_data/ws_spot_execution_report_stop_loss.json");
        let msg: BinanceSpotExecutionReport = serde_json::from_str(json).unwrap();
        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = parse_spot_exec_report_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id,
            ts_init,
        )
        .unwrap();

        assert_eq!(report.order_type, OrderType::StopLimit);
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-20200101-000000-000-000-1")),
        );
        assert_eq!(
            report.trigger_price,
            Some(Price::new(2450.0, PRICE_PRECISION))
        );
        assert_eq!(report.price, Some(Price::new(2400.0, PRICE_PRECISION)));
    }

    #[rstest]
    fn test_parse_execution_report_to_fill_report() {
        let json = include_str!("../../../../test_data/ws_spot_execution_report_trade.json");
        let msg: BinanceSpotExecutionReport = serde_json::from_str(json).unwrap();
        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = parse_spot_exec_report_to_fill(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id,
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id());
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(report.trade_id, TradeId::new("98765432"));
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-20200101-000000-000-000-0")),
        );
    }

    #[rstest]
    fn test_parse_account_position() {
        let json = include_str!("../../../../test_data/ws_spot_account_position.json");
        let msg: BinanceSpotAccountPositionMsg = serde_json::from_str(json).unwrap();
        let account_id = AccountId::from("BINANCE-001");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let state = parse_spot_account_position(&msg, account_id, ts_init);

        assert_eq!(state.account_id, account_id);
        assert_eq!(state.account_type, AccountType::Cash);
        assert!(state.is_reported);
        assert_eq!(state.balances.len(), 2);
    }
}
