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

//! Parse functions for converting Binance Futures venue types to Nautilus reports.
//!
//! Pure functions that take venue message + precision + account_id + ts_init
//! and return Nautilus report types.

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{
        AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce,
        TrailingOffsetType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

use super::messages::{
    AlgoOrderUpdateData, BinanceFuturesAccountUpdateMsg, BinanceFuturesOrderUpdateMsg,
    OrderUpdateData,
};
use crate::common::{
    consts::BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    encoder::decode_broker_id,
    enums::{
        BinanceAlgoStatus, BinanceFuturesOrderType, BinanceOrderStatus, BinanceSide,
        BinanceTimeInForce,
    },
};

/// Converts a Binance Futures order update to a Nautilus order status report.
///
/// # Errors
///
/// Returns an error if report construction fails.
pub fn parse_futures_order_update_to_order_status(
    msg: &BinanceFuturesOrderUpdateMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let order = &msg.order;
    let ts_event = UnixNanos::from((msg.event_time * 1_000_000) as u64);

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &order.client_order_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ));
    let venue_order_id = VenueOrderId::new(order.order_id.to_string());

    let order_side = parse_side(order.side);
    let order_status = parse_order_status(order.order_status);
    let order_type = parse_futures_order_type(order.order_type);
    let time_in_force = parse_time_in_force(order.time_in_force);

    let quantity: f64 = order.original_qty.parse().unwrap_or(0.0);
    let filled_qty: f64 = order.cumulative_filled_qty.parse().unwrap_or(0.0);
    let price: f64 = order.original_price.parse().unwrap_or(0.0);

    let avg_px = if filled_qty > 0.0 {
        let avg: f64 = order.average_price.parse().unwrap_or(0.0);
        if avg > 0.0 {
            Some(Price::new(avg, price_precision))
        } else {
            None
        }
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
    report.post_only = order.order_type == BinanceFuturesOrderType::Limit
        && order.time_in_force == BinanceTimeInForce::Gtx;

    let stop_price: f64 = order.stop_price.parse().unwrap_or(0.0);
    if stop_price > 0.0 {
        report.trigger_price = Some(Price::new(stop_price, price_precision));
    }

    if let Some(ref cr) = order.callback_rate {
        let rate: f64 = cr.parse().unwrap_or(0.0);
        if rate > 0.0 {
            // Binance callbackRate is percentage (1 = 1%), convert to basis points (100 = 1%)
            report.trailing_offset = Some(
                rust_decimal::Decimal::from_f64_retain(rate * 100.0)
                    .unwrap_or(rust_decimal::Decimal::ZERO),
            );
            report.trailing_offset_type = TrailingOffsetType::BasisPoints;
        }
    }

    if let Some(avg) = avg_px {
        report.avg_px = Some(avg.as_decimal());
    }

    Ok(report)
}

/// Converts a Binance Futures order update (Trade type) to a Nautilus fill report.
///
/// # Errors
///
/// Returns an error if report construction fails.
pub fn parse_futures_order_update_to_fill(
    msg: &BinanceFuturesOrderUpdateMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let order = &msg.order;
    let ts_event = UnixNanos::from((msg.event_time * 1_000_000) as u64);

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &order.client_order_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ));
    let venue_order_id = VenueOrderId::new(order.order_id.to_string());
    let trade_id = TradeId::new(order.trade_id.to_string());

    let order_side = parse_side(order.side);

    let liquidity_side = if order.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    let last_qty: f64 = order.last_filled_qty.parse().unwrap_or(0.0);
    let last_px: f64 = order.last_filled_price.parse().unwrap_or(0.0);
    let commission: f64 = order
        .commission
        .as_deref()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    let commission_currency = order
        .commission_asset
        .as_ref()
        .map_or_else(Currency::USDT, |a| Currency::from(a.as_str()));

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

/// Converts a Binance Futures algo order update to a Nautilus order status report.
///
/// Returns `None` for algo statuses that don't map to an order status report
/// (e.g. New, Triggering, Triggered, Finished, Unknown).
pub fn parse_futures_algo_update_to_order_status(
    algo_data: &AlgoOrderUpdateData,
    event_time: i64,
    instrument_id: InstrumentId,
    _price_precision: u8,
    size_precision: u8,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> Option<OrderStatusReport> {
    let ts_event = UnixNanos::from((event_time * 1_000_000) as u64);

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &algo_data.client_algo_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ));

    let venue_order_id = algo_data
        .actual_order_id
        .as_ref()
        .filter(|id| !id.is_empty())
        .map_or_else(
            || VenueOrderId::new(algo_data.algo_id.to_string()),
            |id| VenueOrderId::new(id.clone()),
        );

    let order_status = match algo_data.algo_status {
        BinanceAlgoStatus::Canceled | BinanceAlgoStatus::Expired => OrderStatus::Canceled,
        BinanceAlgoStatus::Rejected => OrderStatus::Rejected,
        _ => return None,
    };

    let order_side = parse_side(algo_data.side);
    let order_type = parse_futures_order_type(algo_data.order_type);
    let time_in_force = parse_time_in_force(algo_data.time_in_force);

    let quantity: f64 = algo_data.quantity.parse().unwrap_or(0.0);

    let report = OrderStatusReport::new(
        account_id,
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        Quantity::new(quantity, size_precision),
        Quantity::new(0.0, size_precision),
        ts_event,
        ts_event,
        ts_init,
        None, // report_id
    );

    Some(report)
}

/// Converts a Binance Futures account update to a Nautilus account state.
pub fn parse_futures_account_update(
    msg: &BinanceFuturesAccountUpdateMsg,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> Option<AccountState> {
    let ts_event = UnixNanos::from((msg.event_time * 1_000_000) as u64);

    let balances: Vec<AccountBalance> = msg
        .account
        .balances
        .iter()
        .filter_map(|b| {
            let wallet_balance: f64 = b.wallet_balance.parse().unwrap_or(0.0);
            let cross_wallet: f64 = b.cross_wallet_balance.parse().unwrap_or(0.0);
            let locked = wallet_balance - cross_wallet;

            if wallet_balance == 0.0 {
                return None;
            }

            let currency = Currency::from(&b.asset);
            Some(AccountBalance::new(
                Money::new(wallet_balance, currency),
                Money::new(locked.max(0.0), currency),
                Money::new(cross_wallet, currency),
            ))
        })
        .collect();

    if balances.is_empty() {
        return None;
    }

    Some(AccountState::new(
        account_id,
        AccountType::Margin,
        balances,
        vec![], // Margins handled separately
        true,   // is_reported
        UUID4::new(),
        ts_event,
        ts_init,
        None, // base_currency
    ))
}

/// Returns the decoded client order ID from an [`OrderUpdateData`].
pub fn decode_order_client_id(order: &OrderUpdateData) -> ClientOrderId {
    ClientOrderId::new(decode_broker_id(
        &order.client_order_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ))
}

/// Returns the decoded client order ID from an [`AlgoOrderUpdateData`].
pub fn decode_algo_client_id(algo: &AlgoOrderUpdateData) -> ClientOrderId {
    ClientOrderId::new(decode_broker_id(
        &algo.client_algo_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ))
}

fn parse_side(side: BinanceSide) -> OrderSide {
    match side {
        BinanceSide::Buy => OrderSide::Buy,
        BinanceSide::Sell => OrderSide::Sell,
    }
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

fn parse_futures_order_type(order_type: BinanceFuturesOrderType) -> OrderType {
    match order_type {
        BinanceFuturesOrderType::Limit => OrderType::Limit,
        BinanceFuturesOrderType::Market => OrderType::Market,
        BinanceFuturesOrderType::Stop => OrderType::StopLimit,
        BinanceFuturesOrderType::StopMarket => OrderType::StopMarket,
        BinanceFuturesOrderType::TakeProfit => OrderType::LimitIfTouched,
        BinanceFuturesOrderType::TakeProfitMarket => OrderType::MarketIfTouched,
        BinanceFuturesOrderType::TrailingStopMarket => OrderType::TrailingStopMarket,
        BinanceFuturesOrderType::Liquidation
        | BinanceFuturesOrderType::Adl
        | BinanceFuturesOrderType::Unknown => OrderType::Market,
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
    use crate::futures::websocket::messages::BinanceFuturesOrderUpdateMsg;

    const PRICE_PRECISION: u8 = 2;
    const SIZE_PRECISION: u8 = 3;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("ETHUSDT-PERP.BINANCE")
    }

    fn account_id() -> AccountId {
        AccountId::from("BINANCE-FUTURES-001")
    }

    #[rstest]
    fn test_parse_order_update_to_order_status_new() {
        let json = include_str!("../../../test_data/ws_futures_order_trade_update_new.json");
        let msg: BinanceFuturesOrderUpdateMsg = serde_json::from_str(json).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, account_id());
        assert_eq!(report.instrument_id, instrument_id());
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.venue_order_id, VenueOrderId::new("12345678"));
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-20200101-000000-000-000-0")),
        );
    }

    #[rstest]
    fn test_parse_order_update_to_fill_report() {
        let json = include_str!("../../../test_data/ws_futures_order_trade_update_trade.json");
        let msg: BinanceFuturesOrderUpdateMsg = serde_json::from_str(json).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = parse_futures_order_update_to_fill(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, account_id());
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
    fn test_parse_account_update() {
        let json = include_str!("../../../test_data/ws_futures_account_update.json");
        let msg: BinanceFuturesAccountUpdateMsg = serde_json::from_str(json).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let state = parse_futures_account_update(&msg, account_id(), ts_init).unwrap();

        assert_eq!(state.account_id, account_id());
        assert_eq!(state.account_type, AccountType::Margin);
        assert!(state.is_reported);
    }
}
