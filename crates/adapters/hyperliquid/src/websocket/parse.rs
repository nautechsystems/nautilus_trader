// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Parsing helpers for Hyperliquid WebSocket payloads.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, LiquiditySide, OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity, price::PriceRaw, quantity::QuantityRaw},
};
use rust_decimal::Decimal;

use super::messages::{CandleData, WsBboData, WsBookData, WsFillData, WsOrderData, WsTradeData};
use crate::common::{
    enums::hyperliquid_status_to_order_status,
    parse::{is_conditional_order_data, parse_trigger_order_type},
};

/// Helper to parse a price string with instrument precision.
fn parse_price(
    price_str: &str,
    instrument: &InstrumentAny,
    field_name: &str,
) -> anyhow::Result<Price> {
    let decimal = Decimal::from_str(price_str)
        .with_context(|| format!("Failed to parse price from '{price_str}' for {field_name}"))?;

    let raw = decimal.mantissa() as PriceRaw;

    Ok(Price::from_raw(raw, instrument.price_precision()))
}

/// Helper to parse a quantity string with instrument precision.
fn parse_quantity(
    quantity_str: &str,
    instrument: &InstrumentAny,
    field_name: &str,
) -> anyhow::Result<Quantity> {
    let decimal = Decimal::from_str(quantity_str).with_context(|| {
        format!("Failed to parse quantity from '{quantity_str}' for {field_name}")
    })?;

    let raw = decimal.mantissa().unsigned_abs() as QuantityRaw;

    Ok(Quantity::from_raw(raw, instrument.size_precision()))
}

/// Helper to parse millisecond timestamp to UnixNanos.
fn parse_millis_to_nanos(millis: u64) -> UnixNanos {
    UnixNanos::from(millis * 1_000_000)
}

/// Parses a WebSocket trade frame into a [`TradeTick`].
pub fn parse_ws_trade_tick(
    trade: &WsTradeData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.px, instrument, "trade.px")?;
    let size = parse_quantity(&trade.sz, instrument, "trade.sz")?;

    // Determine aggressor side from the 'side' field
    // In Hyperliquid: "A" = Ask (sell), "B" = Bid (buy)
    let aggressor = match trade.side.as_str() {
        "A" => AggressorSide::Seller, // Sell side was aggressor
        "B" => AggressorSide::Buyer,  // Buy side was aggressor
        _ => AggressorSide::NoAggressor,
    };

    let trade_id = TradeId::new_checked(trade.tid.to_string())
        .context("Invalid trade identifier in Hyperliquid trade message")?;

    let ts_event = parse_millis_to_nanos(trade.time);

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Hyperliquid trade message")
}

/// Parses a WebSocket L2 order book message into [`OrderBookDeltas`].
pub fn parse_ws_order_book_deltas(
    book: &WsBookData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = parse_millis_to_nanos(book.time);
    let mut deltas = Vec::new();

    // Parse bids (index 0)
    for level in &book.levels[0] {
        let price = parse_price(&level.px, instrument, "book.bid.px")?;
        let size = parse_quantity(&level.sz, instrument, "book.bid.sz")?;

        let action = if size.raw == 0 {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let order = BookOrder::new(
            nautilus_model::enums::OrderSide::Buy,
            price,
            size,
            0, // order_id not provided in Hyperliquid L2 data
        );

        let delta = OrderBookDelta::new(
            instrument.id(),
            action,
            order,
            0, // flags
            0, // sequence
            ts_event,
            ts_init,
        );

        deltas.push(delta);
    }

    // Parse asks (index 1)
    for level in &book.levels[1] {
        let price = parse_price(&level.px, instrument, "book.ask.px")?;
        let size = parse_quantity(&level.sz, instrument, "book.ask.sz")?;

        let action = if size.raw == 0 {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let order = BookOrder::new(
            nautilus_model::enums::OrderSide::Sell,
            price,
            size,
            0, // order_id not provided in Hyperliquid L2 data
        );

        let delta = OrderBookDelta::new(
            instrument.id(),
            action,
            order,
            0, // flags
            0, // sequence
            ts_event,
            ts_init,
        );

        deltas.push(delta);
    }

    Ok(OrderBookDeltas::new(instrument.id(), deltas))
}

/// Parses a WebSocket BBO (best bid/offer) message into a [`QuoteTick`].
pub fn parse_ws_quote_tick(
    bbo: &WsBboData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let bid_level = bbo.bbo[0]
        .as_ref()
        .context("BBO message missing bid level")?;
    let ask_level = bbo.bbo[1]
        .as_ref()
        .context("BBO message missing ask level")?;

    let bid_price = parse_price(&bid_level.px, instrument, "bbo.bid.px")?;
    let ask_price = parse_price(&ask_level.px, instrument, "bbo.ask.px")?;
    let bid_size = parse_quantity(&bid_level.sz, instrument, "bbo.bid.sz")?;
    let ask_size = parse_quantity(&ask_level.sz, instrument, "bbo.ask.sz")?;

    let ts_event = parse_millis_to_nanos(bbo.time);

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("Failed to construct QuoteTick from Hyperliquid BBO message")
}

/// Parses a WebSocket candle message into a [`Bar`].
pub fn parse_ws_candle(
    candle: &CandleData,
    instrument: &InstrumentAny,
    bar_type: &BarType,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    // Get precision from the instrument
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open_decimal = Decimal::from_str(&candle.o).context("Failed to parse open price")?;
    let open_raw = open_decimal.mantissa() as PriceRaw;
    let open = Price::from_raw(open_raw, price_precision);

    let high_decimal = Decimal::from_str(&candle.h).context("Failed to parse high price")?;
    let high_raw = high_decimal.mantissa() as PriceRaw;
    let high = Price::from_raw(high_raw, price_precision);

    let low_decimal = Decimal::from_str(&candle.l).context("Failed to parse low price")?;
    let low_raw = low_decimal.mantissa() as PriceRaw;
    let low = Price::from_raw(low_raw, price_precision);

    let close_decimal = Decimal::from_str(&candle.c).context("Failed to parse close price")?;
    let close_raw = close_decimal.mantissa() as PriceRaw;
    let close = Price::from_raw(close_raw, price_precision);

    let volume_decimal = Decimal::from_str(&candle.v).context("Failed to parse volume")?;
    let volume_raw = volume_decimal.mantissa().unsigned_abs() as QuantityRaw;
    let volume = Quantity::from_raw(volume_raw, size_precision);

    let ts_event = parse_millis_to_nanos(candle.t);

    Ok(Bar::new(
        *bar_type, open, high, low, close, volume, ts_event, ts_init,
    ))
}

/// Parses a WebSocket order update message into an [`OrderStatusReport`].
///
/// This converts Hyperliquid order data from WebSocket into Nautilus order status reports.
/// Handles both regular and conditional orders (stop/limit-if-touched).
pub fn parse_ws_order_status_report(
    order: &WsOrderData,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order.order.oid.to_string());

    // Parse order side
    let order_side: OrderSide = match order.order.side.as_str() {
        "B" => OrderSide::Buy,
        "A" => OrderSide::Sell,
        _ => anyhow::bail!("Unknown order side: {}", order.order.side),
    };

    // Determine order type based on trigger info
    let order_type = if is_conditional_order_data(
        order.order.trigger_px.as_deref(),
        order.order.tpsl.as_deref(),
    ) {
        if let (Some(is_market), Some(tpsl)) = (order.order.is_market, order.order.tpsl.as_deref())
        {
            parse_trigger_order_type(is_market, tpsl)
        } else {
            OrderType::Limit // fallback
        }
    } else {
        OrderType::Limit // Regular limit order
    };

    // Parse time in force (assuming GTC for now, could be derived from order data)
    let time_in_force = TimeInForce::Gtc;

    // Parse order status
    let order_status = hyperliquid_status_to_order_status(&order.status);

    // Parse quantity
    let quantity = parse_quantity(&order.order.sz, instrument, "order.sz")?;

    // Calculate filled quantity (orig_sz - sz)
    let orig_qty = parse_quantity(&order.order.orig_sz, instrument, "order.orig_sz")?;
    let filled_qty = Quantity::from_raw(
        orig_qty.raw.saturating_sub(quantity.raw),
        instrument.size_precision(),
    );

    // Parse price
    let price = parse_price(&order.order.limit_px, instrument, "order.limitPx")?;

    // Parse timestamps
    let ts_accepted = parse_millis_to_nanos(order.order.timestamp);
    let ts_last = parse_millis_to_nanos(order.status_timestamp);

    // Build the report
    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None, // venue_order_id_modified
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
        Some(UUID4::new()),
    );

    // Add client order ID if present
    if let Some(ref cloid) = order.order.cloid {
        report = report.with_client_order_id(ClientOrderId::new(cloid.as_str()));
    }

    // Add price
    report = report.with_price(price);

    // Add trigger price for conditional orders
    if let Some(ref trigger_px_str) = order.order.trigger_px {
        let trigger_price = parse_price(trigger_px_str, instrument, "order.triggerPx")?;
        report = report.with_trigger_price(trigger_price);
    }

    Ok(report)
}

/// Parses a WebSocket fill message into a [`FillReport`].
///
/// This converts Hyperliquid fill data from WebSocket user events into Nautilus fill reports.
pub fn parse_ws_fill_report(
    fill: &WsFillData,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(fill.oid.to_string());
    let trade_id = TradeId::new_checked(fill.tid.to_string())
        .context("Invalid trade identifier in Hyperliquid fill message")?;

    // Parse order side
    let order_side: OrderSide = match fill.side.as_str() {
        "B" => OrderSide::Buy,
        "A" => OrderSide::Sell,
        _ => anyhow::bail!("Unknown fill side: {}", fill.side),
    };

    // Parse quantities and prices
    let last_qty = parse_quantity(&fill.sz, instrument, "fill.sz")?;
    let last_px = parse_price(&fill.px, instrument, "fill.px")?;

    // Parse liquidity side
    let liquidity_side = if fill.crossed {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    // Parse commission
    let commission_amount = Decimal::from_str(&fill.fee)
        .with_context(|| format!("Failed to parse fee='{}' as decimal", fill.fee))?
        .abs()
        .to_string()
        .parse::<f64>()
        .unwrap_or(0.0);

    // Determine commission currency from fee_token
    let commission_currency = if fill.fee_token == "USDC" {
        Currency::from("USDC")
    } else {
        // Default to quote currency if fee_token is something else
        instrument.quote_currency()
    };

    let commission = Money::new(commission_amount, commission_currency);

    // Parse timestamp
    let ts_event = parse_millis_to_nanos(fill.time);

    // No client order ID available in fill data directly
    let client_order_id = None;

    Ok(FillReport::new(
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
        None, // venue_position_id
        ts_event,
        ts_init,
        None, // report_id
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::{
        identifiers::{InstrumentId, Symbol, Venue},
        instruments::CryptoPerpetual,
        types::currency::Currency,
    };
    use ustr::Ustr;

    use super::*;

    fn create_test_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("BTC-PERP"), Venue::new("HYPERLIQUID"));

        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("BTC-PERP"),
            Currency::from("BTC"),
            Currency::from("USDC"),
            Currency::from("USDC"),
            false, // is_inverse
            2,     // price_precision
            3,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.001"),
            None, // multiplier
            None, // lot_size
            None, // max_quantity
            None, // min_quantity
            None, // max_notional
            None, // min_notional
            None, // max_price
            None, // min_price
            None, // margin_init
            None, // margin_maint
            None, // maker_fee
            None, // taker_fee
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[test]
    fn test_parse_ws_order_status_report_basic() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("HYPERLIQUID-001");
        let ts_init = UnixNanos::default();

        let order_data = WsOrderData {
            order: super::super::messages::WsBasicOrderData {
                coin: Ustr::from("BTC"),
                side: "B".to_string(),
                limit_px: "50000.0".to_string(),
                sz: "0.5".to_string(),
                oid: 12345,
                timestamp: 1704470400000,
                orig_sz: "1.0".to_string(),
                cloid: Some("test-order-1".to_string()),
                trigger_px: None,
                is_market: None,
                tpsl: None,
                trigger_activated: None,
                trailing_stop: None,
            },
            status: "open".to_string(),
            status_timestamp: 1704470400000,
        };

        let result = parse_ws_order_status_report(&order_data, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(
            report.order_status,
            nautilus_model::enums::OrderStatus::Accepted
        );
    }

    #[test]
    fn test_parse_ws_fill_report_basic() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("HYPERLIQUID-001");
        let ts_init = UnixNanos::default();

        let fill_data = super::super::messages::WsFillData {
            coin: Ustr::from("BTC"),
            px: "50000.0".to_string(),
            sz: "0.1".to_string(),
            side: "B".to_string(),
            time: 1704470400000,
            start_position: "0.0".to_string(),
            dir: "Open Long".to_string(),
            closed_pnl: "0.0".to_string(),
            hash: "0xabc123".to_string(),
            oid: 12345,
            crossed: true,
            fee: "0.05".to_string(),
            tid: 98765,
            liquidation: None,
            fee_token: "USDC".to_string(),
            builder_fee: None,
        };

        let result = parse_ws_fill_report(&fill_data, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
    }
}
