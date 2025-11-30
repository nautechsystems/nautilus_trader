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
    data::{
        Bar, BarType, BookOrder, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate,
        OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{
        AggressorSide, BookAction, LiquiditySide, OrderSide, OrderStatus, OrderType, RecordFlag,
        TimeInForce,
    },
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::{
    Decimal,
    prelude::{FromPrimitive, ToPrimitive},
};

use super::messages::{
    CandleData, WsActiveAssetCtxData, WsBboData, WsBookData, WsFillData, WsOrderData, WsTradeData,
};
use crate::common::parse::{
    is_conditional_order_data, parse_millis_to_nanos, parse_trigger_order_type,
};

/// Helper to parse a price string with instrument precision.
fn parse_price(
    price_str: &str,
    instrument: &InstrumentAny,
    field_name: &str,
) -> anyhow::Result<Price> {
    let decimal = Decimal::from_str(price_str)
        .with_context(|| format!("Failed to parse price from '{price_str}' for {field_name}"))?;

    let value = decimal.to_f64().ok_or_else(|| {
        anyhow::anyhow!(
            "Failed to convert price '{price_str}' to f64 for {field_name} (out of range or too much precision)"
        )
    })?;

    Ok(Price::new(value, instrument.price_precision()))
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

    let value = decimal.abs().to_f64().ok_or_else(|| {
        anyhow::anyhow!(
            "Failed to convert quantity '{quantity_str}' to f64 for {field_name} (out of range or too much precision)"
        )
    })?;

    Ok(Quantity::new(value, instrument.size_precision()))
}

/// Parses a WebSocket trade frame into a [`TradeTick`].
pub fn parse_ws_trade_tick(
    trade: &WsTradeData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.px, instrument, "trade.px")?;
    let size = parse_quantity(&trade.sz, instrument, "trade.sz")?;
    let aggressor = AggressorSide::from(trade.side);
    let trade_id = TradeId::new_checked(trade.tid.to_string())
        .context("invalid trade identifier in Hyperliquid trade message")?;
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
    .context("failed to construct TradeTick from Hyperliquid trade message")
}

/// Parses a WebSocket L2 order book message into [`OrderBookDeltas`].
pub fn parse_ws_order_book_deltas(
    book: &WsBookData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = parse_millis_to_nanos(book.time);
    let mut deltas = Vec::new();

    // Treat every book payload as a snapshot: clear existing depth and rebuild it
    deltas.push(OrderBookDelta::clear(instrument.id(), 0, ts_event, ts_init));

    // Parse bids
    for level in &book.levels[0] {
        let price = parse_price(&level.px, instrument, "book.bid.px")?;
        let size = parse_quantity(&level.sz, instrument, "book.bid.sz")?;

        if !size.is_positive() {
            continue;
        }

        let order = BookOrder::new(OrderSide::Buy, price, size, 0);

        let delta = OrderBookDelta::new(
            instrument.id(),
            BookAction::Add,
            order,
            RecordFlag::F_LAST as u8,
            0, // sequence
            ts_event,
            ts_init,
        );

        deltas.push(delta);
    }

    // Parse asks
    for level in &book.levels[1] {
        let price = parse_price(&level.px, instrument, "book.ask.px")?;
        let size = parse_quantity(&level.sz, instrument, "book.ask.sz")?;

        if !size.is_positive() {
            continue;
        }

        let order = BookOrder::new(OrderSide::Sell, price, size, 0);

        let delta = OrderBookDelta::new(
            instrument.id(),
            BookAction::Add,
            order,
            RecordFlag::F_LAST as u8,
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
    .context("failed to construct QuoteTick from Hyperliquid BBO message")
}

/// Parses a WebSocket candle message into a [`Bar`].
pub fn parse_ws_candle(
    candle: &CandleData,
    instrument: &InstrumentAny,
    bar_type: &BarType,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = parse_price(&candle.o, instrument, "candle.o")?;
    let high = parse_price(&candle.h, instrument, "candle.h")?;
    let low = parse_price(&candle.l, instrument, "candle.l")?;
    let close = parse_price(&candle.c, instrument, "candle.c")?;
    let volume = parse_quantity(&candle.v, instrument, "candle.v")?;

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
    let order_side = OrderSide::from(order.order.side);

    // Determine order type based on trigger info
    let order_type = if is_conditional_order_data(
        order.order.trigger_px.as_deref(),
        order.order.tpsl.as_ref(),
    ) {
        if let (Some(is_market), Some(tpsl)) = (order.order.is_market, order.order.tpsl.as_ref()) {
            parse_trigger_order_type(is_market, tpsl)
        } else {
            OrderType::Limit // fallback
        }
    } else {
        OrderType::Limit // Regular limit order
    };

    let time_in_force = TimeInForce::Gtc;
    let order_status = OrderStatus::from(order.status);
    let quantity = parse_quantity(&order.order.sz, instrument, "order.sz")?;

    // Calculate filled quantity (orig_sz - sz)
    let orig_qty = parse_quantity(&order.order.orig_sz, instrument, "order.orig_sz")?;
    let filled_qty = Quantity::from_raw(
        orig_qty.raw.saturating_sub(quantity.raw),
        instrument.size_precision(),
    );

    let price = parse_price(&order.order.limit_px, instrument, "order.limitPx")?;

    let ts_accepted = parse_millis_to_nanos(order.order.timestamp);
    let ts_last = parse_millis_to_nanos(order.status_timestamp);

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

    if let Some(ref cloid) = order.order.cloid {
        report = report.with_client_order_id(ClientOrderId::new(cloid.as_str()));
    }

    report = report.with_price(price);

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
        .context("invalid trade identifier in Hyperliquid fill message")?;

    let order_side = OrderSide::from(fill.side);
    let last_qty = parse_quantity(&fill.sz, instrument, "fill.sz")?;
    let last_px = parse_price(&fill.px, instrument, "fill.px")?;
    let liquidity_side = if fill.crossed {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    let commission_amount = Decimal::from_str(&fill.fee)
        .with_context(|| format!("Failed to parse fee='{}' as decimal", fill.fee))?
        .abs()
        .to_string()
        .parse::<f64>()
        .unwrap_or(0.0);

    let commission_currency = if fill.fee_token == "USDC" {
        Currency::from("USDC")
    } else {
        // Default to quote currency if fee_token is something else
        instrument.quote_currency()
    };

    let commission = Money::new(commission_amount, commission_currency);
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

/// Parses a WebSocket ActiveAssetCtx message into mark price, index price, and funding rate updates.
///
/// This converts Hyperliquid asset context data into Nautilus price and funding rate updates.
/// Returns a tuple of (`MarkPriceUpdate`, `Option<IndexPriceUpdate>`, `Option<FundingRateUpdate>`).
/// Index price and funding rate are only present for perpetual contracts.
pub fn parse_ws_asset_context(
    ctx: &WsActiveAssetCtxData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<(
    MarkPriceUpdate,
    Option<IndexPriceUpdate>,
    Option<FundingRateUpdate>,
)> {
    let instrument_id = instrument.id();

    match ctx {
        WsActiveAssetCtxData::Perp { coin: _, ctx } => {
            let mark_px_f64 = ctx
                .shared
                .mark_px
                .parse::<f64>()
                .context("Failed to parse mark_px as f64")?;
            let mark_price = parse_f64_price(mark_px_f64, instrument, "ctx.mark_px")?;
            let mark_price_update =
                MarkPriceUpdate::new(instrument_id, mark_price, ts_init, ts_init);

            let oracle_px_f64 = ctx
                .oracle_px
                .parse::<f64>()
                .context("Failed to parse oracle_px as f64")?;
            let index_price = parse_f64_price(oracle_px_f64, instrument, "ctx.oracle_px")?;
            let index_price_update =
                IndexPriceUpdate::new(instrument_id, index_price, ts_init, ts_init);

            let funding_f64 = ctx
                .funding
                .parse::<f64>()
                .context("Failed to parse funding as f64")?;
            let funding_rate_decimal = Decimal::from_f64(funding_f64)
                .context("Failed to convert funding rate to Decimal")?;
            let funding_rate_update = FundingRateUpdate::new(
                instrument_id,
                funding_rate_decimal,
                None, // Hyperliquid doesn't provide next funding time in this message
                ts_init,
                ts_init,
            );

            Ok((
                mark_price_update,
                Some(index_price_update),
                Some(funding_rate_update),
            ))
        }
        WsActiveAssetCtxData::Spot { coin: _, ctx } => {
            let mark_px_f64 = ctx
                .shared
                .mark_px
                .parse::<f64>()
                .context("Failed to parse mark_px as f64")?;
            let mark_price = parse_f64_price(mark_px_f64, instrument, "ctx.mark_px")?;
            let mark_price_update =
                MarkPriceUpdate::new(instrument_id, mark_price, ts_init, ts_init);

            Ok((mark_price_update, None, None))
        }
    }
}

/// Helper to parse an f64 price into a Price with instrument precision.
fn parse_f64_price(
    price: f64,
    instrument: &InstrumentAny,
    field_name: &str,
) -> anyhow::Result<Price> {
    if !price.is_finite() {
        anyhow::bail!("Invalid price value for {field_name}: {price} (must be finite)");
    }
    Ok(Price::new(price, instrument.price_precision()))
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
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{
            HyperliquidFillDirection, HyperliquidOrderStatus as HyperliquidOrderStatusEnum,
            HyperliquidSide,
        },
        websocket::messages::{
            PerpsAssetCtx, SharedAssetCtx, SpotAssetCtx, WsBasicOrderData, WsBookData, WsLevelData,
        },
    };

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

    #[rstest]
    fn test_parse_ws_order_status_report_basic() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("HYPERLIQUID-001");
        let ts_init = UnixNanos::default();

        let order_data = WsOrderData {
            order: WsBasicOrderData {
                coin: Ustr::from("BTC"),
                side: HyperliquidSide::Buy,
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
            status: HyperliquidOrderStatusEnum::Open,
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

    #[rstest]
    fn test_parse_ws_fill_report_basic() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("HYPERLIQUID-001");
        let ts_init = UnixNanos::default();

        let fill_data = WsFillData {
            coin: Ustr::from("BTC"),
            px: "50000.0".to_string(),
            sz: "0.1".to_string(),
            side: HyperliquidSide::Buy,
            time: 1704470400000,
            start_position: "0.0".to_string(),
            dir: HyperliquidFillDirection::OpenLong,
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

    #[rstest]
    fn test_parse_ws_order_book_deltas_snapshot_behavior() {
        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let book = WsBookData {
            coin: Ustr::from("BTC"),
            levels: [
                vec![WsLevelData {
                    px: "50000.0".to_string(),
                    sz: "1.0".to_string(),
                    n: 1,
                }],
                vec![WsLevelData {
                    px: "50001.0".to_string(),
                    sz: "2.0".to_string(),
                    n: 1,
                }],
            ],
            time: 1_704_470_400_000,
        };

        let deltas = parse_ws_order_book_deltas(&book, &instrument, ts_init).unwrap();

        assert_eq!(deltas.deltas.len(), 3); // clear + bid + ask
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        let bid_delta = &deltas.deltas[1];
        assert_eq!(bid_delta.action, BookAction::Add);
        assert_eq!(bid_delta.order.side, OrderSide::Buy);
        assert!(bid_delta.order.size.is_positive());
        assert_eq!(bid_delta.order.order_id, 0);

        let ask_delta = &deltas.deltas[2];
        assert_eq!(ask_delta.action, BookAction::Add);
        assert_eq!(ask_delta.order.side, OrderSide::Sell);
        assert!(ask_delta.order.size.is_positive());
        assert_eq!(ask_delta.order.order_id, 0);
    }

    #[rstest]
    fn test_parse_ws_asset_context_perp() {
        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let ctx_data = WsActiveAssetCtxData::Perp {
            coin: Ustr::from("BTC"),
            ctx: PerpsAssetCtx {
                shared: SharedAssetCtx {
                    day_ntl_vlm: "1000000.0".to_string(),
                    prev_day_px: "49000.0".to_string(),
                    mark_px: "50000.0".to_string(),
                    mid_px: Some("50001.0".to_string()),
                    impact_pxs: Some(vec!["50000.0".to_string(), "50002.0".to_string()]),
                    day_base_vlm: Some("100.0".to_string()),
                },
                funding: "0.0001".to_string(),
                open_interest: "100000.0".to_string(),
                oracle_px: "50005.0".to_string(),
                premium: Some("-0.0001".to_string()),
            },
        };

        let result = parse_ws_asset_context(&ctx_data, &instrument, ts_init);
        assert!(result.is_ok());

        let (mark_price, index_price, funding_rate) = result.unwrap();

        assert_eq!(mark_price.instrument_id, instrument.id());
        assert_eq!(mark_price.value.as_f64(), 50_000.0);

        assert!(index_price.is_some());
        let index = index_price.unwrap();
        assert_eq!(index.instrument_id, instrument.id());
        assert_eq!(index.value.as_f64(), 50_005.0);

        assert!(funding_rate.is_some());
        let funding = funding_rate.unwrap();
        assert_eq!(funding.instrument_id, instrument.id());
        assert_eq!(funding.rate.to_string(), "0.0001");
    }

    #[rstest]
    fn test_parse_ws_asset_context_spot() {
        let instrument = create_test_instrument();
        let ts_init = UnixNanos::default();

        let ctx_data = WsActiveAssetCtxData::Spot {
            coin: Ustr::from("BTC"),
            ctx: SpotAssetCtx {
                shared: SharedAssetCtx {
                    day_ntl_vlm: "1000000.0".to_string(),
                    prev_day_px: "49000.0".to_string(),
                    mark_px: "50000.0".to_string(),
                    mid_px: Some("50001.0".to_string()),
                    impact_pxs: Some(vec!["50000.0".to_string(), "50002.0".to_string()]),
                    day_base_vlm: Some("100.0".to_string()),
                },
                circulating_supply: "19000000.0".to_string(),
            },
        };

        let result = parse_ws_asset_context(&ctx_data, &instrument, ts_init);
        assert!(result.is_ok());

        let (mark_price, index_price, funding_rate) = result.unwrap();

        assert_eq!(mark_price.instrument_id, instrument.id());
        assert_eq!(mark_price.value.as_f64(), 50_000.0);
        assert!(index_price.is_none());
        assert!(funding_rate.is_none());
    }
}
