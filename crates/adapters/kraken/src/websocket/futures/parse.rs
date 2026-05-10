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

//! WebSocket message parsers for converting Kraken Futures streaming data to Nautilus domain models.

use anyhow::Context;
use nautilus_core::{UUID4, datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos};
use nautilus_model::{
    data::{
        BookOrder, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta, QuoteTick,
        TradeTick,
    },
    enums::{
        AggressorSide, BookAction, ContingencyType, LiquiditySide, OrderSide, OrderStatus,
        OrderType, TimeInForce, TrailingOffsetType, TriggerType,
    },
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    instruments::{Instrument, any::InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::prelude::FromPrimitive;

use super::messages::{
    KrakenFuturesBookDelta, KrakenFuturesBookSnapshot, KrakenFuturesFill, KrakenFuturesOpenOrder,
    KrakenFuturesTickerData, KrakenFuturesTradeData,
};
use crate::common::enums::{KrakenFillType, KrakenOrderSide};

fn millis_to_nanos(millis: i64) -> UnixNanos {
    UnixNanos::from((millis as u64) * NANOSECONDS_IN_MILLISECOND)
}

pub fn parse_futures_ws_quote_tick(
    ticker: &KrakenFuturesTickerData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid = ticker.bid.context("Ticker missing bid")?;
    let ask = ticker.ask.context("Ticker missing ask")?;
    let bid_size = ticker.bid_size.unwrap_or(0.0);
    let ask_size = ticker.ask_size.unwrap_or(0.0);

    let bid_price =
        Price::new_checked(bid, price_precision).context("Failed to construct bid Price")?;
    let ask_price =
        Price::new_checked(ask, price_precision).context("Failed to construct ask Price")?;
    let bid_qty = Quantity::new_checked(bid_size, size_precision)
        .context("Failed to construct bid Quantity")?;
    let ask_qty = Quantity::new_checked(ask_size, size_precision)
        .context("Failed to construct ask Quantity")?;

    let ts_event = ticker.time.map_or(ts_init, millis_to_nanos);

    Ok(QuoteTick::new(
        instrument.id(),
        bid_price,
        ask_price,
        bid_qty,
        ask_qty,
        ts_event,
        ts_init,
    ))
}

pub fn parse_futures_ws_trade_tick(
    trade: &KrakenFuturesTradeData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = Price::new_checked(trade.price, price_precision)
        .context("Failed to construct trade Price")?;
    let size = Quantity::new_checked(trade.qty, size_precision)
        .context("Failed to construct trade Quantity")?;

    let aggressor = match trade.side {
        KrakenOrderSide::Buy => AggressorSide::Buyer,
        KrakenOrderSide::Sell => AggressorSide::Seller,
    };

    let trade_id = trade
        .uid
        .as_deref()
        .map_or_else(|| TradeId::new(trade.seq.to_string()), TradeId::new);

    let ts_event = millis_to_nanos(trade.time);

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Kraken futures trade")
}

pub fn parse_futures_ws_book_snapshot_deltas(
    snapshot: &KrakenFuturesBookSnapshot,
    instrument: &InstrumentAny,
    sequence: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<OrderBookDelta>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = millis_to_nanos(snapshot.timestamp);

    let capacity = snapshot.bids.len() + snapshot.asks.len() + 1;
    let mut deltas = Vec::with_capacity(capacity);
    let mut seq = sequence;

    // Leading CLEAR delta to reset the book
    deltas.push(OrderBookDelta::clear(instrument_id, seq, ts_event, ts_init));
    seq += 1;

    for level in &snapshot.bids {
        if level.qty <= 0.0 {
            continue;
        }
        let price = Price::new_checked(level.price, price_precision)?;
        let size = Quantity::new_checked(level.qty, size_precision)?;
        let order_id = price.raw as u64;
        let order = BookOrder::new(OrderSide::Buy, price, size, order_id);
        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            0,
            seq,
            ts_event,
            ts_init,
        ));
        seq += 1;
    }

    for level in &snapshot.asks {
        if level.qty <= 0.0 {
            continue;
        }
        let price = Price::new_checked(level.price, price_precision)?;
        let size = Quantity::new_checked(level.qty, size_precision)?;
        let order_id = price.raw as u64;
        let order = BookOrder::new(OrderSide::Sell, price, size, order_id);
        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            0,
            seq,
            ts_event,
            ts_init,
        ));
        seq += 1;
    }

    Ok(deltas)
}

pub fn parse_futures_ws_book_delta(
    delta: &KrakenFuturesBookDelta,
    instrument: &InstrumentAny,
    sequence: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDelta> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = Price::new_checked(delta.price, price_precision)?;
    let size = Quantity::new_checked(delta.qty, size_precision)?;

    let action = if size.raw == 0 {
        BookAction::Delete
    } else {
        BookAction::Update
    };

    let side = match delta.side {
        KrakenOrderSide::Buy => OrderSide::Buy,
        KrakenOrderSide::Sell => OrderSide::Sell,
    };

    let order_id = price.raw as u64;
    let order = BookOrder::new(side, price, size, order_id);
    let ts_event = millis_to_nanos(delta.timestamp);

    Ok(OrderBookDelta::new(
        instrument.id(),
        action,
        order,
        0,
        sequence,
        ts_event,
        ts_init,
    ))
}

fn parse_ws_direction(direction: i32) -> OrderSide {
    if direction == 0 {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    }
}

fn infer_order_status(order: &KrakenFuturesOpenOrder, is_cancel: bool) -> OrderStatus {
    if order.filled >= order.qty && order.qty > 0.0 {
        OrderStatus::Filled
    } else if is_cancel {
        OrderStatus::Canceled
    } else if order.filled > 0.0 {
        OrderStatus::PartiallyFilled
    } else {
        OrderStatus::Accepted
    }
}

pub fn parse_futures_ws_order_status_report(
    order: &KrakenFuturesOpenOrder,
    is_cancel: bool,
    reason: Option<&str>,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let venue_order_id = VenueOrderId::new(&order.order_id);
    let order_side = parse_ws_direction(order.direction);
    let order_type = OrderType::from(order.order_type);
    let order_type = if order_type == OrderType::MarketIfTouched && order.limit_price.is_some() {
        OrderType::LimitIfTouched
    } else {
        order_type
    };
    let order_status = infer_order_status(order, is_cancel);

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let quantity =
        Quantity::new_checked(order.qty, size_precision).context("Failed to parse order qty")?;
    let filled_qty = Quantity::new_checked(order.filled, size_precision)
        .context("Failed to parse order filled")?;

    let ts_accepted = millis_to_nanos(order.time);
    let ts_last = millis_to_nanos(order.last_update_time);

    let mut report = OrderStatusReport {
        account_id,
        instrument_id: instrument.id(),
        client_order_id: order.cli_ord_id.as_ref().map(ClientOrderId::new),
        venue_order_id,
        order_side,
        order_type,
        time_in_force: TimeInForce::Gtc,
        order_status,
        quantity,
        filled_qty,
        report_id: UUID4::new(),
        ts_accepted,
        ts_last,
        ts_init,
        order_list_id: None,
        venue_position_id: None,
        linked_order_ids: None,
        parent_order_id: None,
        contingency_type: ContingencyType::NoContingency,
        expire_time: None,
        price: None,
        trigger_price: None,
        trigger_type: None,
        limit_offset: None,
        trailing_offset: None,
        trailing_offset_type: TrailingOffsetType::NoTrailingOffset,
        display_qty: None,
        avg_px: None,
        post_only: false,
        reduce_only: order.reduce_only,
        cancel_reason: None,
        ts_triggered: None,
    };

    if let Some(px) = order.limit_price {
        report.price = Some(Price::new(px, price_precision));
    }

    if let Some(px) = order.stop_price {
        report.trigger_price = Some(Price::new(px, price_precision));
        report.trigger_type = Some(order.trigger_signal.as_deref().map_or(
            TriggerType::Default,
            |s| match s {
                "mark" | "mark_price" => TriggerType::MarkPrice,
                "spot" | "spot_price" | "index" | "index_price" => TriggerType::IndexPrice,
                _ => TriggerType::LastPrice,
            },
        ));
    }

    if let Some(reason) = reason
        && !reason.is_empty()
    {
        report.cancel_reason = Some(reason.to_string());
    }

    Ok(report)
}

pub fn parse_futures_ws_fill_report(
    fill: &KrakenFuturesFill,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let venue_order_id = VenueOrderId::new(&fill.order_id);
    let trade_id = TradeId::new(&fill.fill_id);
    let order_side = if fill.buy {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    let last_qty =
        Quantity::new_checked(fill.qty, size_precision).context("Failed to parse fill qty")?;
    let last_px =
        Price::new_checked(fill.price, price_precision).context("Failed to parse fill price")?;

    let liquidity_side = match fill.fill_type {
        KrakenFillType::Maker => LiquiditySide::Maker,
        KrakenFillType::Taker => LiquiditySide::Taker,
    };

    let fee = fill.fee_paid.unwrap_or(0.0);
    let commission_currency = instrument.quote_currency();
    let commission = Money::new(fee, commission_currency);

    let ts_event = millis_to_nanos(fill.time);

    let client_order_id = fill
        .cli_ord_id
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(ClientOrderId::new);

    Ok(FillReport::new(
        account_id,
        instrument.id(),
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None, // venue_position_id
        ts_event,
        ts_init,
        None, // report_id
    ))
}

pub fn parse_futures_ws_mark_price(
    ticker: &KrakenFuturesTickerData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Option<MarkPriceUpdate> {
    let mark_price = ticker.mark_price?;
    let price = Price::new(mark_price, instrument.price_precision());
    let ts_event = ticker.time.map_or(ts_init, millis_to_nanos);
    Some(MarkPriceUpdate::new(
        instrument.id(),
        price,
        ts_event,
        ts_init,
    ))
}

pub fn parse_futures_ws_index_price(
    ticker: &KrakenFuturesTickerData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Option<IndexPriceUpdate> {
    let index = ticker.index?;
    let price = Price::new(index, instrument.price_precision());
    let ts_event = ticker.time.map_or(ts_init, millis_to_nanos);
    Some(IndexPriceUpdate::new(
        instrument.id(),
        price,
        ts_event,
        ts_init,
    ))
}

pub fn parse_futures_ws_funding_rate(
    ticker: &KrakenFuturesTickerData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Option<FundingRateUpdate> {
    let rate_f64 = ticker.relative_funding_rate?;
    let rate = rust_decimal::Decimal::from_f64(rate_f64)?;
    let ts_event = ticker.time.map_or(ts_init, millis_to_nanos);
    let next_funding_ns = ticker
        .next_funding_rate_time
        .map(|t| millis_to_nanos(t as i64));
    Some(FundingRateUpdate::new(
        instrument.id(),
        rate,
        None,
        next_funding_ns,
        ts_event,
        ts_init,
    ))
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::CurrencyType,
        identifiers::{InstrumentId, Symbol},
        instruments::crypto_perpetual::CryptoPerpetual,
        types::Currency,
    };
    use rstest::rstest;

    use super::*;
    use crate::common::{consts::KRAKEN_VENUE, enums::KrakenFuturesOrderType};

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    fn create_mock_perp() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("PI_XBTUSD"), *KRAKEN_VENUE);
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("PI_XBTUSD"),
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            1,
            0,
            Price::from("0.5"),
            Quantity::from("1"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // info
            TS,
            TS,
        ))
    }

    #[rstest]
    fn test_parse_futures_ws_quote_tick() {
        let json = include_str!("../../../test_data/ws_futures_ticker.json");
        let ticker: KrakenFuturesTickerData = serde_json::from_str(json).unwrap();
        let instrument = create_mock_perp();

        let quote = parse_futures_ws_quote_tick(&ticker, &instrument, TS).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price.as_f64(), 21978.5);
        assert_eq!(quote.ask_price.as_f64(), 21987.0);
        assert!(quote.bid_size.as_f64() > 0.0);
        assert!(quote.ask_size.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_futures_ws_trade_tick() {
        let json = include_str!("../../../test_data/ws_futures_trade.json");
        let trade: KrakenFuturesTradeData = serde_json::from_str(json).unwrap();
        let instrument = create_mock_perp();

        let tick = parse_futures_ws_trade_tick(&trade, &instrument, TS).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.price.as_f64(), 34969.5);
        assert_eq!(tick.size.as_f64(), 15000.0);
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
    }

    #[rstest]
    fn test_parse_futures_ws_book_snapshot() {
        let json = include_str!("../../../test_data/ws_futures_book_snapshot.json");
        let snapshot: KrakenFuturesBookSnapshot = serde_json::from_str(json).unwrap();
        let instrument = create_mock_perp();

        let deltas = parse_futures_ws_book_snapshot_deltas(&snapshot, &instrument, 0, TS).unwrap();

        // CLEAR + 2 bids + 2 asks = 5
        assert_eq!(deltas.len(), 5);
        assert_eq!(deltas[0].action, BookAction::Clear);
        assert_eq!(deltas[1].action, BookAction::Add);
        assert_eq!(deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas[3].order.side, OrderSide::Sell);
    }

    #[rstest]
    fn test_parse_futures_ws_book_snapshot_skips_zero_qty() {
        let json = include_str!("../../../test_data/ws_futures_book_snapshot_with_zero_qty.json");
        let snapshot: KrakenFuturesBookSnapshot = serde_json::from_str(json).unwrap();
        let instrument = create_mock_perp();

        let deltas = parse_futures_ws_book_snapshot_deltas(&snapshot, &instrument, 0, TS).unwrap();

        // CLEAR + 2 bids (skipped qty=0) + 1 ask (skipped qty=0) = 4
        assert_eq!(deltas.len(), 4);
        assert_eq!(deltas[0].action, BookAction::Clear);
        assert_eq!(deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas[1].order.price.as_f64(), 34892.5);
        assert_eq!(deltas[2].order.side, OrderSide::Buy);
        assert_eq!(deltas[2].order.price.as_f64(), 34891.5);
        assert_eq!(deltas[3].order.side, OrderSide::Sell);
        assert_eq!(deltas[3].order.price.as_f64(), 34912.0);
    }

    #[rstest]
    fn test_parse_futures_ws_book_delta() {
        let json = include_str!("../../../test_data/ws_futures_book_delta.json");
        let delta_msg: KrakenFuturesBookDelta = serde_json::from_str(json).unwrap();
        let instrument = create_mock_perp();

        let delta = parse_futures_ws_book_delta(&delta_msg, &instrument, 10, TS).unwrap();

        assert_eq!(delta.instrument_id, instrument.id());
        assert_eq!(delta.order.side, OrderSide::Sell);
        assert_eq!(delta.action, BookAction::Delete); // qty=0
        assert_eq!(delta.sequence, 10);
    }

    #[rstest]
    fn test_parse_futures_ws_order_status_report_new_order() {
        let order = KrakenFuturesOpenOrder {
            instrument: ustr::Ustr::from("PI_XBTUSD"),
            time: 1700000000000,
            last_update_time: 1700000000100,
            qty: 1000.0,
            filled: 0.0,
            limit_price: Some(35000.0),
            stop_price: None,
            order_type: KrakenFuturesOrderType::Limit,
            order_id: "abc-123".to_string(),
            cli_ord_id: Some("my-order-1".to_string()),
            direction: 0,
            reduce_only: false,
            trigger_signal: None,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::from("KRAKEN-001");

        let report =
            parse_futures_ws_order_status_report(&order, false, None, &instrument, account_id, TS)
                .unwrap();

        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.quantity.as_f64(), 1000.0);
        assert_eq!(report.filled_qty.as_f64(), 0.0);
        assert_eq!(report.price.unwrap().as_f64(), 35000.0);
    }

    #[rstest]
    fn test_parse_futures_ws_order_status_report_canceled() {
        let order = KrakenFuturesOpenOrder {
            instrument: ustr::Ustr::from("PI_XBTUSD"),
            time: 1700000000000,
            last_update_time: 1700000001000,
            qty: 1000.0,
            filled: 0.0,
            limit_price: Some(35000.0),
            stop_price: None,
            order_type: KrakenFuturesOrderType::Limit,
            order_id: "abc-123".to_string(),
            cli_ord_id: None,
            direction: 1,
            reduce_only: false,
            trigger_signal: None,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::from("KRAKEN-001");

        let report = parse_futures_ws_order_status_report(
            &order,
            true,
            Some("cancelled_by_user"),
            &instrument,
            account_id,
            TS,
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.cancel_reason.as_deref(), Some("cancelled_by_user"));
    }

    #[rstest]
    fn test_parse_futures_ws_order_status_report_market_if_touched() {
        let order = KrakenFuturesOpenOrder {
            instrument: ustr::Ustr::from("PI_XBTUSD"),
            time: 1700000000000,
            last_update_time: 1700000000100,
            qty: 500.0,
            filled: 0.0,
            limit_price: None,
            stop_price: Some(36000.0),
            order_type: KrakenFuturesOrderType::TakeProfit,
            order_id: "tp-001".to_string(),
            cli_ord_id: Some("my-tp-1".to_string()),
            direction: 0,
            reduce_only: true,
            trigger_signal: None,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::from("KRAKEN-001");

        let report =
            parse_futures_ws_order_status_report(&order, false, None, &instrument, account_id, TS)
                .unwrap();

        assert_eq!(report.order_type, OrderType::MarketIfTouched);
        assert_eq!(report.trigger_price.unwrap().as_f64(), 36000.0);
        assert!(report.price.is_none());
        assert!(report.reduce_only);
    }

    #[rstest]
    fn test_parse_futures_ws_order_status_report_limit_if_touched() {
        let order = KrakenFuturesOpenOrder {
            instrument: ustr::Ustr::from("PI_XBTUSD"),
            time: 1700000000000,
            last_update_time: 1700000000100,
            qty: 500.0,
            filled: 0.0,
            limit_price: Some(35500.0),
            stop_price: Some(36000.0),
            order_type: KrakenFuturesOrderType::TakeProfit,
            order_id: "tpl-001".to_string(),
            cli_ord_id: Some("my-tpl-1".to_string()),
            direction: 1,
            reduce_only: false,
            trigger_signal: None,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::from("KRAKEN-001");

        let report =
            parse_futures_ws_order_status_report(&order, false, None, &instrument, account_id, TS)
                .unwrap();

        assert_eq!(report.order_type, OrderType::LimitIfTouched);
        assert_eq!(report.trigger_price.unwrap().as_f64(), 36000.0);
        assert_eq!(report.price.unwrap().as_f64(), 35500.0);
        assert_eq!(report.order_side, OrderSide::Sell);
    }

    #[rstest]
    fn test_parse_futures_ws_order_status_report_spot_trigger_signal() {
        let order = KrakenFuturesOpenOrder {
            instrument: ustr::Ustr::from("PI_XBTUSD"),
            time: 1700000000000,
            last_update_time: 1700000000100,
            qty: 500.0,
            filled: 0.0,
            limit_price: None,
            stop_price: Some(36000.0),
            order_type: KrakenFuturesOrderType::TakeProfit,
            order_id: "tp-spot-001".to_string(),
            cli_ord_id: Some("my-tp-spot-1".to_string()),
            direction: 0,
            reduce_only: false,
            trigger_signal: Some("spot".to_string()),
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::from("KRAKEN-001");

        let report =
            parse_futures_ws_order_status_report(&order, false, None, &instrument, account_id, TS)
                .unwrap();

        assert_eq!(report.trigger_type, Some(TriggerType::IndexPrice));
    }

    #[rstest]
    fn test_parse_futures_ws_fill_report() {
        let json = include_str!("../../../test_data/ws_futures_fills_delta.json");
        let fills_delta: super::super::messages::KrakenFuturesFillsDelta =
            serde_json::from_str(json).unwrap();
        let fill = &fills_delta.fills[0];

        let instrument_id = InstrumentId::new(Symbol::new("PF_ETHUSD"), *KRAKEN_VENUE);
        let usd = Currency::new("USD", 6, 0, "USD", CurrencyType::Fiat);
        let instrument = InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("PF_ETHUSD"),
            Currency::ETH(),
            usd,
            usd,
            false,
            1,
            3,
            Price::from("0.5"),
            Quantity::from("0.001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // info
            TS,
            TS,
        ));

        let account_id = AccountId::from("KRAKEN-001");
        let report = parse_futures_ws_fill_report(fill, &instrument, account_id, TS).unwrap();

        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.last_px.as_f64(), 3162.0);
        assert_eq!(report.last_qty.as_f64(), 0.001);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(report.commission.as_f64(), 0.001581);
    }
}
