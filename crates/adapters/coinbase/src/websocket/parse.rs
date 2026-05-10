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

//! Parsing functions for converting Coinbase WebSocket messages to Nautilus domain types.

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{BookAction, LiquiditySide, OrderSide, OrderStatus, RecordFlag},
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};

use crate::{
    http::parse::{
        coinbase_side_to_aggressor, parse_epoch_secs_timestamp, parse_order_side,
        parse_order_status, parse_order_type, parse_price, parse_quantity, parse_rfc3339_timestamp,
        parse_time_in_force,
    },
    websocket::messages::{
        WsBookSide, WsCandle, WsL2DataEvent, WsL2Update, WsOrderUpdate, WsTicker, WsTrade,
    },
};

/// Parses a WebSocket trade into a [`TradeTick`].
pub fn parse_ws_trade(
    trade: &WsTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.price, instrument.price_precision())?;
    let size = parse_quantity(&trade.size, instrument.size_precision())?;
    let aggressor_side = coinbase_side_to_aggressor(&trade.side);
    let trade_id = TradeId::new(&trade.trade_id);
    let ts_event = parse_rfc3339_timestamp(&trade.time)?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Parses a WebSocket ticker into a [`QuoteTick`].
pub fn parse_ws_ticker(
    ticker: &WsTicker,
    instrument: &InstrumentAny,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let bid_price = parse_price(&ticker.best_bid, instrument.price_precision())?;
    let ask_price = parse_price(&ticker.best_ask, instrument.price_precision())?;
    let bid_size = parse_quantity(&ticker.best_bid_quantity, instrument.size_precision())?;
    let ask_size = parse_quantity(&ticker.best_ask_quantity, instrument.size_precision())?;

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

/// Parses a WebSocket candle into a [`Bar`].
pub fn parse_ws_candle(
    candle: &WsCandle,
    bar_type: BarType,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = parse_price(&candle.open, instrument.price_precision())?;
    let high = parse_price(&candle.high, instrument.price_precision())?;
    let low = parse_price(&candle.low, instrument.price_precision())?;
    let close = parse_price(&candle.close, instrument.price_precision())?;
    let volume = parse_quantity(&candle.volume, instrument.size_precision())?;
    let ts_event = parse_epoch_secs_timestamp(&candle.start)?;

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Parses a WebSocket L2 snapshot event into [`OrderBookDeltas`].
///
/// All deltas in the batch share `ts_event`, which the caller derives from the
/// message-level `timestamp`. Per-level `event_time` values are not monotonic
/// across batches and would trigger out-of-order warnings in the managed book.
pub fn parse_ws_l2_snapshot(
    event: &WsL2DataEvent,
    instrument: &InstrumentAny,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();

    let total = event.updates.len();
    let mut deltas = Vec::with_capacity(total + 1);

    let mut clear = OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init);

    if total == 0 {
        clear.flags |= RecordFlag::F_LAST as u8;
    }
    deltas.push(clear);

    for (i, update) in event.updates.iter().enumerate() {
        let is_last = i == total - 1;
        let delta = parse_l2_delta(
            update,
            instrument_id,
            instrument.price_precision(),
            instrument.size_precision(),
            is_last,
            ts_event,
            ts_init,
        )?;
        deltas.push(delta);
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

/// Parses a WebSocket L2 update event into [`OrderBookDeltas`].
///
/// All deltas in the batch share `ts_event`, which the caller derives from the
/// message-level `timestamp`. Per-level `event_time` values are not monotonic
/// across batches and would trigger out-of-order warnings in the managed book.
pub fn parse_ws_l2_update(
    event: &WsL2DataEvent,
    instrument: &InstrumentAny,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let total = event.updates.len();
    let mut deltas = Vec::with_capacity(total);

    for (i, update) in event.updates.iter().enumerate() {
        let is_last = i == total - 1;
        let price = parse_price(&update.price_level, instrument.price_precision())?;
        let size = parse_quantity(&update.new_quantity, instrument.size_precision())?;
        let side = ws_book_side_to_order_side(update.side);

        let action = if size == Quantity::zero(instrument.size_precision()) {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let mut flags = RecordFlag::F_MBP as u8;

        if is_last {
            flags |= RecordFlag::F_LAST as u8;
        }

        let order = BookOrder::new(side, price, size, 0);
        let delta =
            OrderBookDelta::new_checked(instrument_id, action, order, flags, 0, ts_event, ts_init)?;
        deltas.push(delta);
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

/// Parses a single L2 snapshot level into an [`OrderBookDelta`].
fn parse_l2_delta(
    update: &WsL2Update,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    is_last: bool,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDelta> {
    let price = parse_price(&update.price_level, price_precision)?;
    let size = parse_quantity(&update.new_quantity, size_precision)?;
    let side = ws_book_side_to_order_side(update.side);

    let mut flags = RecordFlag::F_MBP as u8;

    if is_last {
        flags |= RecordFlag::F_LAST as u8;
    }

    let order = BookOrder::new(side, price, size, 0);
    OrderBookDelta::new_checked(
        instrument_id,
        BookAction::Add,
        order,
        flags,
        0,
        ts_event,
        ts_init,
    )
}

/// Converts a Coinbase WebSocket book side to a Nautilus order side.
fn ws_book_side_to_order_side(side: WsBookSide) -> OrderSide {
    match side {
        WsBookSide::Bid => OrderSide::Buy,
        WsBookSide::Offer => OrderSide::Sell,
    }
}

/// Parses a Coinbase user channel [`WsOrderUpdate`] into an [`OrderStatusReport`].
///
/// Derives the total quantity as `cumulative_quantity + leaves_quantity` and
/// promotes the `Accepted` status to `PartiallyFilled` when the cumulative
/// fill is positive but below the total quantity, mirroring the REST parser.
///
/// # Errors
///
/// Returns an error when any numeric field cannot be parsed against the
/// instrument precision.
pub fn parse_ws_user_event_to_order_status_report(
    update: &WsOrderUpdate,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let size_precision = instrument.size_precision();

    let order_side = parse_order_side(&update.order_side);
    let order_type = parse_order_type(update.order_type);
    let time_in_force = parse_time_in_force(Some(update.time_in_force));
    let mut order_status = parse_order_status(update.status);

    let venue_order_id = VenueOrderId::new(&update.order_id);
    let client_order_id = if update.client_order_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(&update.client_order_id))
    };

    let filled_qty = if update.cumulative_quantity.is_empty() {
        Quantity::zero(size_precision)
    } else {
        parse_quantity(&update.cumulative_quantity, size_precision)
            .context("failed to parse cumulative_quantity")?
    };
    let leaves_qty = if update.leaves_quantity.is_empty() {
        Quantity::zero(size_precision)
    } else {
        parse_quantity(&update.leaves_quantity, size_precision)
            .context("failed to parse leaves_quantity")?
    };

    let quantity = filled_qty + leaves_qty;

    if order_status == OrderStatus::Accepted && filled_qty.is_positive() && filled_qty < quantity {
        order_status = OrderStatus::PartiallyFilled;
    }

    let ts_accepted = if update.creation_time.is_empty() {
        ts_event
    } else {
        parse_rfc3339_timestamp(&update.creation_time).unwrap_or(ts_event)
    };

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
        ts_event,
        ts_init,
        None,
    );

    if !update.avg_price.is_empty()
        && let Ok(avg_px) = update.avg_price.parse::<f64>()
        && avg_px > 0.0
    {
        report = report.with_avg_px(avg_px)?;
    }

    Ok(report)
}

/// Parses a Coinbase user channel [`WsOrderUpdate`] into a [`FillReport`].
///
/// Coinbase's user channel reports cumulative totals rather than per-trade
/// fills, so the caller must supply:
/// - `last_qty`: the quantity delta since the previous cumulative state
/// - `last_px`: the price of the new fill, derived by the caller from the
///   cumulative notional delta (Coinbase's `avg_price` is the *cumulative*
///   weighted average and is not safe to use as the new fill's price for
///   multi-fill orders)
/// - `commission`: the commission delta since the previous cumulative state
/// - `trade_id`: synthesized from the order ID plus the new cumulative total
#[allow(clippy::too_many_arguments)]
pub fn parse_ws_user_event_to_fill_report(
    update: &WsOrderUpdate,
    last_qty: Quantity,
    last_px: Price,
    commission: Money,
    trade_id: TradeId,
    instrument: &InstrumentAny,
    account_id: AccountId,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> FillReport {
    let instrument_id = instrument.id();

    let venue_order_id = VenueOrderId::new(&update.order_id);
    let client_order_id = if update.client_order_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(&update.client_order_id))
    };
    let order_side = parse_order_side(&update.order_side);

    FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None,
        ts_event,
        ts_init,
        None,
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::{
        data::bar::BarSpecification,
        enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
        identifiers::{Symbol, Venue},
        instruments::CurrencyPair,
        types::{Currency, Price},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::testing::load_test_fixture,
        websocket::messages::{CoinbaseWsMessage, WsEventType},
    };

    fn test_instrument() -> InstrumentAny {
        let instrument_id =
            InstrumentId::new(Symbol::new("BTC-USD"), Venue::new(Ustr::from("COINBASE")));
        let raw_symbol = Symbol::new("BTC-USD");
        let base_currency = Currency::get_or_create_crypto("BTC");
        let quote_currency = Currency::get_or_create_crypto("USD");

        InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            raw_symbol,
            base_currency,
            quote_currency,
            2,
            8,
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            Some(Quantity::from("0.00000001")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[rstest]
    fn test_parse_ws_trade() {
        let json = load_test_fixture("ws_market_trades.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::MarketTrades { events, .. } => {
                let trade_data = &events[0].trades[0];
                let tick = parse_ws_trade(trade_data, &instrument, ts_init).unwrap();

                assert_eq!(tick.instrument_id, instrument.id());
                assert_eq!(tick.price, Price::from("68900.50"));
                assert_eq!(tick.size, Quantity::from("0.00150000"));
                assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
                assert_eq!(tick.trade_id.as_str(), "995098700");
                assert!(tick.ts_event.as_u64() > 0);
            }
            _ => panic!("Expected MarketTrades"),
        }
    }

    #[rstest]
    fn test_parse_ws_trade_sell_side() {
        let json = load_test_fixture("ws_market_trades.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::MarketTrades { events, .. } => {
                let trade_data = &events[0].trades[1];
                let tick = parse_ws_trade(trade_data, &instrument, ts_init).unwrap();

                assert_eq!(tick.aggressor_side, AggressorSide::Seller);
                assert_eq!(tick.price, Price::from("68900.00"));
                assert_eq!(tick.size, Quantity::from("0.05000000"));
            }
            _ => panic!("Expected MarketTrades"),
        }
    }

    #[rstest]
    fn test_parse_ws_ticker() {
        let json = load_test_fixture("ws_ticker.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::Ticker {
                timestamp, events, ..
            } => {
                let ticker_data = &events[0].tickers[0];
                let ts_event = parse_rfc3339_timestamp(&timestamp).unwrap();
                let quote = parse_ws_ticker(ticker_data, &instrument, ts_event, ts_init).unwrap();

                assert_eq!(quote.instrument_id, instrument.id());
                assert_eq!(quote.bid_price, Price::from("68900.00"));
                assert_eq!(quote.ask_price, Price::from("68901.00"));
                assert_eq!(quote.bid_size, Quantity::from("1.50000000"));
                assert_eq!(quote.ask_size, Quantity::from("0.50000000"));
            }
            _ => panic!("Expected Ticker"),
        }
    }

    #[rstest]
    fn test_parse_ws_candle() {
        let json = load_test_fixture("ws_candles.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        let bar_spec = BarSpecification::new(5, BarAggregation::Minute, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::External);

        match msg {
            CoinbaseWsMessage::Candles { events, .. } => {
                let candle_data = &events[0].candles[0];
                let bar = parse_ws_candle(candle_data, bar_type, &instrument, ts_init).unwrap();

                assert_eq!(bar.bar_type, bar_type);
                assert_eq!(bar.open, Price::from("68900.00"));
                assert_eq!(bar.high, Price::from("68950.00"));
                assert_eq!(bar.low, Price::from("68850.00"));
                assert_eq!(bar.close, Price::from("68920.50"));
                assert_eq!(bar.volume, Quantity::from("42.15000000"));
                assert_eq!(bar.ts_event.as_u64(), 1_775_521_800_000_000_000);
            }
            _ => panic!("Expected Candles"),
        }
    }

    #[rstest]
    fn test_parse_ws_l2_snapshot() {
        let json = load_test_fixture("ws_l2_data_snapshot.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::L2Data {
                timestamp, events, ..
            } => {
                let event = &events[0];
                assert_eq!(event.event_type, WsEventType::Snapshot);
                let ts_event = parse_rfc3339_timestamp(&timestamp).unwrap();

                let deltas = parse_ws_l2_snapshot(event, &instrument, ts_event, ts_init).unwrap();
                assert_eq!(deltas.instrument_id, instrument.id());
                for delta in &deltas.deltas {
                    assert_eq!(delta.ts_event, ts_event);
                }

                // 6 levels + 1 clear = 7 deltas
                assert_eq!(deltas.deltas.len(), 7);

                // First delta is clear
                assert_eq!(deltas.deltas[0].action, BookAction::Clear);

                // Bids
                assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
                assert_eq!(deltas.deltas[1].order.price, Price::from("68900.00"));
                assert_eq!(deltas.deltas[1].order.size, Quantity::from("1.50000000"));

                // Asks
                assert_eq!(deltas.deltas[4].order.side, OrderSide::Sell);
                assert_eq!(deltas.deltas[4].order.price, Price::from("68901.00"));

                // Last delta has F_LAST flag
                let last = deltas.deltas.last().unwrap();
                assert_ne!(last.flags & RecordFlag::F_LAST as u8, 0);
            }
            _ => panic!("Expected L2Data"),
        }
    }

    #[rstest]
    fn test_parse_ws_l2_update() {
        let json = load_test_fixture("ws_l2_data_update.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::L2Data {
                timestamp, events, ..
            } => {
                let event = &events[0];
                assert_eq!(event.event_type, WsEventType::Update);
                let ts_event = parse_rfc3339_timestamp(&timestamp).unwrap();

                let deltas = parse_ws_l2_update(event, &instrument, ts_event, ts_init).unwrap();
                assert_eq!(deltas.deltas.len(), 2);
                for delta in &deltas.deltas {
                    assert_eq!(delta.ts_event, ts_event);
                }

                // First update: bid at 68900.00, qty 2.0 -> Update action
                assert_eq!(deltas.deltas[0].order.side, OrderSide::Buy);
                assert_eq!(deltas.deltas[0].order.price, Price::from("68900.00"));
                assert_eq!(deltas.deltas[0].order.size, Quantity::from("2.00000000"));
                assert_eq!(deltas.deltas[0].action, BookAction::Update);

                // Second update: offer at 68901.00, qty 0.0 -> Delete action
                assert_eq!(deltas.deltas[1].order.side, OrderSide::Sell);
                assert_eq!(deltas.deltas[1].action, BookAction::Delete);
                assert_eq!(deltas.deltas[1].order.size, Quantity::from("0.00000000"));

                // Last delta has F_LAST flag
                assert_ne!(deltas.deltas[1].flags & RecordFlag::F_LAST as u8, 0);
            }
            _ => panic!("Expected L2Data"),
        }
    }

    #[rstest]
    fn test_parse_ws_l2_update_zero_quantity_is_delete() {
        let json = load_test_fixture("ws_l2_data_update.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::L2Data {
                timestamp, events, ..
            } => {
                let event = &events[0];
                let ts_event = parse_rfc3339_timestamp(&timestamp).unwrap();
                let deltas = parse_ws_l2_update(event, &instrument, ts_event, ts_init).unwrap();

                // The offer with new_quantity "0.00000000" should be a Delete
                let delete_delta = deltas
                    .deltas
                    .iter()
                    .find(|d| d.action == BookAction::Delete)
                    .expect("should have a delete action for zero quantity");
                assert_eq!(delete_delta.order.side, OrderSide::Sell);
                assert_eq!(delete_delta.ts_event, ts_event);
            }
            _ => panic!("Expected L2Data"),
        }
    }

    #[rstest]
    fn test_ws_book_side_conversion() {
        assert_eq!(ws_book_side_to_order_side(WsBookSide::Bid), OrderSide::Buy);
        assert_eq!(
            ws_book_side_to_order_side(WsBookSide::Offer),
            OrderSide::Sell
        );
    }

    #[rstest]
    fn test_parse_ws_user_event_to_order_status_report_open() {
        let json = load_test_fixture("ws_user.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let account_id = AccountId::new("COINBASE-001");
        let ts_event = UnixNanos::from(1_705_314_600_000_000_000u64);
        let ts_init = UnixNanos::from(1_705_314_700_000_000_000u64);

        let order = match msg {
            CoinbaseWsMessage::User { events, .. } => events[0].orders[0].clone(),
            other => panic!("expected User, was {other:?}"),
        };

        let report = parse_ws_user_event_to_order_status_report(
            &order,
            &instrument,
            account_id,
            ts_event,
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(
            report.venue_order_id.as_str(),
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
        );
        assert_eq!(
            report.client_order_id.unwrap().as_str(),
            "11111-000000-000001"
        );
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.filled_qty, Quantity::from("0.00000000"));
        assert_eq!(report.quantity, Quantity::from("0.00100000"));
        assert_eq!(report.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_ws_user_event_to_order_status_report_promotes_partial_fill() {
        let mut update = WsOrderUpdate {
            order_id: "venue-1".to_string(),
            client_order_id: "client-1".to_string(),
            contract_expiry_type: crate::common::enums::CoinbaseContractExpiryType::Unknown,
            cumulative_quantity: "0.5".to_string(),
            leaves_quantity: "0.5".to_string(),
            avg_price: "100.00".to_string(),
            total_fees: "0.05".to_string(),
            status: crate::common::enums::CoinbaseOrderStatus::Open,
            product_id: ustr::Ustr::from("BTC-USD"),
            product_type: crate::common::enums::CoinbaseProductType::Spot,
            creation_time: String::new(),
            order_side: crate::common::enums::CoinbaseOrderSide::Buy,
            order_type: crate::common::enums::CoinbaseOrderType::Limit,
            risk_managed_by: crate::common::enums::CoinbaseRiskManagedBy::Unknown,
            time_in_force: crate::common::enums::CoinbaseTimeInForce::GoodUntilCancelled,
            trigger_status: crate::common::enums::CoinbaseTriggerStatus::InvalidOrderType,
            cancel_reason: String::new(),
            reject_reason: String::new(),
            total_value_after_fees: String::new(),
        };
        update.creation_time = String::new();

        let instrument = test_instrument();
        let report = parse_ws_user_event_to_order_status_report(
            &update,
            &instrument,
            AccountId::new("COINBASE-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();

        // Coinbase Open + positive cumulative + leaves > 0 should promote to PartiallyFilled.
        assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(report.filled_qty, Quantity::from("0.50000000"));
        assert_eq!(report.quantity, Quantity::from("1.00000000"));
    }

    #[rstest]
    fn test_parse_ws_user_event_to_fill_report_uses_supplied_last_px_and_commission() {
        let update = WsOrderUpdate {
            order_id: "venue-1".to_string(),
            client_order_id: "client-1".to_string(),
            contract_expiry_type: crate::common::enums::CoinbaseContractExpiryType::Unknown,
            cumulative_quantity: "0.5".to_string(),
            leaves_quantity: "0.5".to_string(),
            avg_price: "100.00".to_string(),
            total_fees: "0.05".to_string(),
            status: crate::common::enums::CoinbaseOrderStatus::Open,
            product_id: ustr::Ustr::from("BTC-USD"),
            product_type: crate::common::enums::CoinbaseProductType::Spot,
            creation_time: String::new(),
            order_side: crate::common::enums::CoinbaseOrderSide::Sell,
            order_type: crate::common::enums::CoinbaseOrderType::Limit,
            risk_managed_by: crate::common::enums::CoinbaseRiskManagedBy::Unknown,
            time_in_force: crate::common::enums::CoinbaseTimeInForce::GoodUntilCancelled,
            trigger_status: crate::common::enums::CoinbaseTriggerStatus::InvalidOrderType,
            cancel_reason: String::new(),
            reject_reason: String::new(),
            total_value_after_fees: String::new(),
        };

        let instrument = test_instrument();
        let usd = Currency::USD();
        let last_px = Price::from("120.00");
        let commission =
            Money::from_decimal(rust_decimal::Decimal::from_str("0.10").unwrap(), usd).unwrap();
        let trade_id = TradeId::new("venue-1-0.5");

        let report = parse_ws_user_event_to_fill_report(
            &update,
            Quantity::from("0.50000000"),
            last_px,
            commission,
            trade_id,
            &instrument,
            AccountId::new("COINBASE-001"),
            LiquiditySide::Maker,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert_eq!(report.venue_order_id.as_str(), "venue-1");
        assert_eq!(report.client_order_id.unwrap().as_str(), "client-1");
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.last_qty, Quantity::from("0.50000000"));
        assert_eq!(report.last_px, Price::from("120.00"));
        assert_eq!(report.commission, commission);
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(report.trade_id, trade_id);
    }
}
