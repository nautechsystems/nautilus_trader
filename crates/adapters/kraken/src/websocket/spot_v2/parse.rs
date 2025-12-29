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

//! WebSocket message parsers for converting Kraken streaming data to Nautilus domain models.

use anyhow::Context;
use nautilus_core::{UUID4, nanos::UnixNanos};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, BookOrder, OrderBookDelta, QuoteTick, TradeTick},
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, LiquiditySide, OrderSide,
        OrderStatus, OrderType, PriceType, TimeInForce, TriggerType,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::{Instrument, any::InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};

use super::{
    enums::{KrakenExecType, KrakenLiquidityInd, KrakenWsOrderStatus},
    messages::{
        KrakenWsBookData, KrakenWsBookLevel, KrakenWsExecutionData, KrakenWsOhlcData,
        KrakenWsTickerData, KrakenWsTradeData,
    },
};
use crate::common::enums::{KrakenOrderSide, KrakenOrderType, KrakenTimeInForce};

/// Parses Kraken WebSocket ticker data into a Nautilus quote tick.
///
/// # Errors
///
/// Returns an error if:
/// - Bid or ask price/quantity cannot be parsed.
pub fn parse_quote_tick(
    ticker: &KrakenWsTickerData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = Price::new_checked(ticker.bid, price_precision).with_context(|| {
        format!("Failed to construct bid Price with precision {price_precision}")
    })?;
    let bid_size = Quantity::new_checked(ticker.bid_qty, size_precision).with_context(|| {
        format!("Failed to construct bid Quantity with precision {size_precision}")
    })?;

    let ask_price = Price::new_checked(ticker.ask, price_precision).with_context(|| {
        format!("Failed to construct ask Price with precision {price_precision}")
    })?;
    let ask_size = Quantity::new_checked(ticker.ask_qty, size_precision).with_context(|| {
        format!("Failed to construct ask Quantity with precision {size_precision}")
    })?;

    // Kraken ticker doesn't include timestamp
    let ts_event = ts_init;

    Ok(QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    ))
}

/// Parses Kraken WebSocket trade data into a Nautilus trade tick.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity cannot be parsed.
/// - Timestamp is invalid.
pub fn parse_trade_tick(
    trade: &KrakenWsTradeData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = Price::new_checked(trade.price, price_precision)
        .with_context(|| format!("Failed to construct Price with precision {price_precision}"))?;
    let size = Quantity::new_checked(trade.qty, size_precision)
        .with_context(|| format!("Failed to construct Quantity with precision {size_precision}"))?;

    let aggressor = match trade.side {
        KrakenOrderSide::Buy => AggressorSide::Buyer,
        KrakenOrderSide::Sell => AggressorSide::Seller,
    };

    let trade_id = TradeId::new_checked(trade.trade_id.to_string())?;
    let ts_event = parse_rfc3339_timestamp(&trade.timestamp, "trade.timestamp")?;

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Kraken WebSocket trade")
}

/// Parses Kraken WebSocket book data into Nautilus order book deltas.
///
/// Returns a vector of deltas, one for each bid and ask level.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity cannot be parsed.
/// - Timestamp is invalid.
pub fn parse_book_deltas(
    book: &KrakenWsBookData,
    instrument: &InstrumentAny,
    sequence: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<OrderBookDelta>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    // Parse timestamp if available, otherwise use ts_init
    let ts_event = if let Some(ref timestamp) = book.timestamp {
        parse_rfc3339_timestamp(timestamp, "book.timestamp")?
    } else {
        ts_init
    };

    let mut deltas = Vec::new();
    let mut current_sequence = sequence;

    if let Some(ref bids) = book.bids {
        for level in bids {
            let delta = parse_book_level(
                level,
                OrderSide::Buy,
                instrument_id,
                price_precision,
                size_precision,
                current_sequence,
                ts_event,
                ts_init,
            )?;
            deltas.push(delta);
            current_sequence += 1;
        }
    }

    if let Some(ref asks) = book.asks {
        for level in asks {
            let delta = parse_book_level(
                level,
                OrderSide::Sell,
                instrument_id,
                price_precision,
                size_precision,
                current_sequence,
                ts_event,
                ts_init,
            )?;
            deltas.push(delta);
            current_sequence += 1;
        }
    }

    Ok(deltas)
}

#[allow(clippy::too_many_arguments)]
fn parse_book_level(
    level: &KrakenWsBookLevel,
    side: OrderSide,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDelta> {
    let price = Price::new_checked(level.price, price_precision)
        .with_context(|| format!("Failed to construct Price with precision {price_precision}"))?;
    let size = Quantity::new_checked(level.qty, size_precision)
        .with_context(|| format!("Failed to construct Quantity with precision {size_precision}"))?;

    // Determine action based on quantity
    let action = if size.raw == 0 {
        BookAction::Delete
    } else {
        BookAction::Update
    };

    // Create order ID from price (Kraken doesn't provide order IDs)
    let order_id = price.raw as u64;
    let order = BookOrder::new(side, price, size, order_id);

    Ok(OrderBookDelta::new(
        instrument_id,
        action,
        order,
        0, // flags
        sequence,
        ts_event,
        ts_init,
    ))
}

fn parse_rfc3339_timestamp(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    value
        .parse::<UnixNanos>()
        .map_err(|e| anyhow::anyhow!("Failed to parse {field}='{value}': {e}"))
}

/// Parses Kraken WebSocket OHLC data into a Nautilus bar.
///
/// The bar's `ts_event` is computed as `interval_begin` + `interval` minutes.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity values cannot be parsed.
/// - The interval cannot be converted to a valid bar specification.
pub fn parse_ws_bar(
    ohlc: &KrakenWsOhlcData,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = Price::new_checked(ohlc.open, price_precision)?;
    let high = Price::new_checked(ohlc.high, price_precision)?;
    let low = Price::new_checked(ohlc.low, price_precision)?;
    let close = Price::new_checked(ohlc.close, price_precision)?;
    let volume = Quantity::new_checked(ohlc.volume, size_precision)?;

    let bar_spec = interval_to_bar_spec(ohlc.interval)?;
    let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);

    // Compute bar close time: interval_begin + interval minutes
    let interval_secs = i64::from(ohlc.interval) * 60;
    let close_time = ohlc.interval_begin + chrono::Duration::seconds(interval_secs);
    let ts_event = UnixNanos::from(close_time.timestamp_nanos_opt().unwrap_or(0) as u64);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Converts a Kraken OHLC interval (minutes) to a Nautilus bar specification.
fn interval_to_bar_spec(interval: u32) -> anyhow::Result<BarSpecification> {
    let (step, aggregation) = match interval {
        1 => (1, BarAggregation::Minute),
        5 => (5, BarAggregation::Minute),
        15 => (15, BarAggregation::Minute),
        30 => (30, BarAggregation::Minute),
        60 => (1, BarAggregation::Hour),
        240 => (4, BarAggregation::Hour),
        1440 => (1, BarAggregation::Day),
        10080 => (1, BarAggregation::Week),
        21600 => (15, BarAggregation::Day), // 21600 min = 360 hours = 15 days
        _ => anyhow::bail!("Unsupported Kraken OHLC interval: {interval}"),
    };

    Ok(BarSpecification::new(step, aggregation, PriceType::Last))
}

/// Parses Kraken execution type and order status to Nautilus order status.
fn parse_order_status(
    exec_type: KrakenExecType,
    order_status: Option<KrakenWsOrderStatus>,
) -> OrderStatus {
    // First check exec_type for terminal states
    match exec_type {
        KrakenExecType::Canceled => return OrderStatus::Canceled,
        KrakenExecType::Expired => return OrderStatus::Expired,
        _ => {}
    }

    // Then check order_status field
    match order_status {
        Some(KrakenWsOrderStatus::PendingNew) => OrderStatus::Submitted,
        Some(KrakenWsOrderStatus::New) => OrderStatus::Accepted,
        Some(KrakenWsOrderStatus::PartiallyFilled) => OrderStatus::PartiallyFilled,
        Some(KrakenWsOrderStatus::Filled) => OrderStatus::Filled,
        Some(KrakenWsOrderStatus::Canceled) => OrderStatus::Canceled,
        Some(KrakenWsOrderStatus::Expired) => OrderStatus::Expired,
        Some(KrakenWsOrderStatus::Triggered) => OrderStatus::Triggered,
        None => OrderStatus::Accepted,
    }
}

/// Parses Kraken order type to Nautilus order type.
fn parse_order_type(order_type: Option<KrakenOrderType>) -> OrderType {
    match order_type {
        Some(KrakenOrderType::Market) => OrderType::Market,
        Some(KrakenOrderType::Limit) => OrderType::Limit,
        Some(KrakenOrderType::StopLoss) => OrderType::StopMarket,
        Some(KrakenOrderType::TakeProfit) => OrderType::MarketIfTouched,
        Some(KrakenOrderType::StopLossLimit) => OrderType::StopLimit,
        Some(KrakenOrderType::TakeProfitLimit) => OrderType::LimitIfTouched,
        Some(KrakenOrderType::SettlePosition) => OrderType::Market,
        None => OrderType::Limit,
    }
}

/// Parses Kraken order side to Nautilus order side.
fn parse_order_side(side: Option<KrakenOrderSide>) -> OrderSide {
    match side {
        Some(KrakenOrderSide::Buy) => OrderSide::Buy,
        Some(KrakenOrderSide::Sell) => OrderSide::Sell,
        None => OrderSide::Buy,
    }
}

/// Parses Kraken time-in-force to Nautilus time-in-force.
fn parse_time_in_force(
    time_in_force: Option<KrakenTimeInForce>,
    post_only: Option<bool>,
) -> TimeInForce {
    // Handle post_only flag
    if post_only == Some(true) {
        return TimeInForce::Gtc;
    }

    match time_in_force {
        Some(KrakenTimeInForce::GoodTilCancelled) => TimeInForce::Gtc,
        Some(KrakenTimeInForce::ImmediateOrCancel) => TimeInForce::Ioc,
        Some(KrakenTimeInForce::GoodTilDate) => TimeInForce::Gtd,
        None => TimeInForce::Gtc,
    }
}

/// Parses Kraken liquidity indicator to Nautilus liquidity side.
fn parse_liquidity_side(liquidity_ind: Option<KrakenLiquidityInd>) -> LiquiditySide {
    match liquidity_ind {
        Some(KrakenLiquidityInd::Maker) => LiquiditySide::Maker,
        Some(KrakenLiquidityInd::Taker) => LiquiditySide::Taker,
        None => LiquiditySide::NoLiquiditySide,
    }
}

/// Parses a Kraken WebSocket execution message into an [`OrderStatusReport`].
///
/// # Errors
///
/// Returns an error if required fields are missing or cannot be parsed.
pub fn parse_ws_order_status_report(
    exec: &KrakenWsExecutionData,
    instrument: &InstrumentAny,
    account_id: AccountId,
    cached_order_qty: Option<f64>,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&exec.order_id);
    let order_side = parse_order_side(exec.side);
    let order_type = parse_order_type(exec.order_type);
    let time_in_force = parse_time_in_force(exec.time_in_force, exec.post_only);
    let order_status = parse_order_status(exec.exec_type, exec.order_status);

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    // Quantity fallback: order_qty -> cached -> cum_qty -> last_qty (for trade snapshots)
    let last_qty = exec
        .last_qty
        .map(|qty| Quantity::new_checked(qty, size_precision))
        .transpose()
        .context("Failed to parse last_qty")?;

    let filled_qty = exec
        .cum_qty
        .map(|qty| Quantity::new_checked(qty, size_precision))
        .transpose()
        .context("Failed to parse cum_qty")?
        .or(last_qty)
        .unwrap_or_else(|| Quantity::new(0.0, size_precision));

    let quantity = exec
        .order_qty
        .or(cached_order_qty)
        .map(|qty| Quantity::new_checked(qty, size_precision))
        .transpose()
        .context("Failed to parse order_qty")?
        .unwrap_or(filled_qty);

    let ts_event = parse_rfc3339_timestamp(&exec.timestamp, "execution.timestamp")?;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None, // client_order_id set below if present
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_event,
        ts_event,
        ts_init,
        Some(UUID4::new()),
    );

    if let Some(ref cl_ord_id) = exec.cl_ord_id
        && !cl_ord_id.is_empty()
    {
        report = report.with_client_order_id(ClientOrderId::new(cl_ord_id));
    }

    // Price fallback: limit_price -> avg_price -> last_price
    // Note: pending_new messages may not include any price fields, which is fine for
    // orders we submitted (engine already has the price from submission)
    let price_value = exec
        .limit_price
        .filter(|&p| p > 0.0)
        .or(exec.avg_price.filter(|&p| p > 0.0))
        .or(exec.last_price.filter(|&p| p > 0.0));

    if let Some(px) = price_value {
        let price =
            Price::new_checked(px, price_precision).context("Failed to parse order price")?;
        report = report.with_price(price);
    }

    // avg_px fallback: avg_price -> cum_cost / cum_qty -> last_price (for single trades/snapshots)
    let avg_px = exec
        .avg_price
        .filter(|&p| p > 0.0)
        .or_else(|| match (exec.cum_cost, exec.cum_qty) {
            (Some(cost), Some(qty)) if qty > 0.0 => Some(cost / qty),
            _ => None,
        })
        .or_else(|| exec.last_price.filter(|&p| p > 0.0));

    if let Some(avg_price) = avg_px {
        report = report.with_avg_px(avg_price)?;
    }

    if exec.post_only == Some(true) {
        report = report.with_post_only(true);
    }

    if exec.reduce_only == Some(true) {
        report = report.with_reduce_only(true);
    }

    if let Some(ref reason) = exec.reason
        && !reason.is_empty()
    {
        report = report.with_cancel_reason(reason.clone());
    }

    // Set trigger type for conditional orders (WebSocket doesn't provide trigger field)
    let is_conditional = matches!(
        order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
    );
    if is_conditional {
        report = report.with_trigger_type(TriggerType::Default);
    }

    Ok(report)
}

/// Parses a Kraken WebSocket trade execution into a [`FillReport`].
///
/// This should only be called when exec_type is "trade".
///
/// # Errors
///
/// Returns an error if required fields are missing or cannot be parsed.
pub fn parse_ws_fill_report(
    exec: &KrakenWsExecutionData,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&exec.order_id);

    let exec_id = exec
        .exec_id
        .as_ref()
        .context("Missing exec_id for trade execution")?;
    let trade_id =
        TradeId::new_checked(exec_id).context("Invalid exec_id in Kraken trade execution")?;

    let order_side = parse_order_side(exec.side);

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let last_qty = exec
        .last_qty
        .map(|qty| Quantity::new_checked(qty, size_precision))
        .transpose()
        .context("Failed to parse last_qty")?
        .context("Missing last_qty for trade execution")?;

    let last_px = exec
        .last_price
        .map(|px| Price::new_checked(px, price_precision))
        .transpose()
        .context("Failed to parse last_price")?
        .context("Missing last_price for trade execution")?;

    let liquidity_side = parse_liquidity_side(exec.liquidity_ind);

    // Calculate commission from fees array
    let commission = if let Some(ref fees) = exec.fees {
        if let Some(fee) = fees.first() {
            let currency = Currency::get_or_create_crypto(&fee.asset);
            Money::new(fee.qty.abs(), currency)
        } else {
            Money::new(0.0, instrument.quote_currency())
        }
    } else {
        Money::new(0.0, instrument.quote_currency())
    };

    let ts_event = parse_rfc3339_timestamp(&exec.timestamp, "execution.timestamp")?;

    let client_order_id = exec
        .cl_ord_id
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(ClientOrderId::new);

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

#[cfg(test)]
mod tests {
    use nautilus_model::{identifiers::Symbol, types::Currency};
    use rstest::rstest;

    use super::*;
    use crate::{common::consts::KRAKEN_VENUE, websocket::spot_v2::messages::KrakenWsMessage};

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    fn load_test_json(filename: &str) -> String {
        let path = format!("test_data/{filename}");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load test data from {path}: {e}"))
    }

    fn create_mock_instrument() -> InstrumentAny {
        use nautilus_model::instruments::currency_pair::CurrencyPair;

        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("XBTUSDT"),
            Currency::BTC(),
            Currency::USDT(),
            1, // price_precision
            8, // size_precision
            Price::from("0.1"),
            Quantity::from("0.00000001"),
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
            TS,
            TS,
        ))
    }

    #[rstest]
    fn test_parse_quote_tick() {
        let json = load_test_json("ws_ticker_snapshot.json");
        let message: KrakenWsMessage = serde_json::from_str(&json).unwrap();
        let ticker: KrakenWsTickerData = serde_json::from_value(message.data[0].clone()).unwrap();

        let instrument = create_mock_instrument();
        let quote_tick = parse_quote_tick(&ticker, &instrument, TS).unwrap();

        assert_eq!(quote_tick.instrument_id, instrument.id());
        assert!(quote_tick.bid_price.as_f64() > 0.0);
        assert!(quote_tick.ask_price.as_f64() > 0.0);
        assert!(quote_tick.bid_size.as_f64() > 0.0);
        assert!(quote_tick.ask_size.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_trade_tick() {
        let json = load_test_json("ws_trade_update.json");
        let message: KrakenWsMessage = serde_json::from_str(&json).unwrap();
        let trade: KrakenWsTradeData = serde_json::from_value(message.data[0].clone()).unwrap();

        let instrument = create_mock_instrument();
        let trade_tick = parse_trade_tick(&trade, &instrument, TS).unwrap();

        assert_eq!(trade_tick.instrument_id, instrument.id());
        assert!(trade_tick.price.as_f64() > 0.0);
        assert!(trade_tick.size.as_f64() > 0.0);
        assert!(matches!(
            trade_tick.aggressor_side,
            AggressorSide::Buyer | AggressorSide::Seller
        ));
    }

    #[rstest]
    fn test_parse_book_deltas_snapshot() {
        let json = load_test_json("ws_book_snapshot.json");
        let message: KrakenWsMessage = serde_json::from_str(&json).unwrap();
        let book: KrakenWsBookData = serde_json::from_value(message.data[0].clone()).unwrap();

        let instrument = create_mock_instrument();
        let deltas = parse_book_deltas(&book, &instrument, 1, TS).unwrap();

        assert!(!deltas.is_empty());

        // Check that we have both bids and asks
        let bid_count = deltas
            .iter()
            .filter(|d| d.order.side == OrderSide::Buy)
            .count();
        let ask_count = deltas
            .iter()
            .filter(|d| d.order.side == OrderSide::Sell)
            .count();

        assert!(bid_count > 0);
        assert!(ask_count > 0);

        // Check first delta
        let first_delta = &deltas[0];
        assert_eq!(first_delta.instrument_id, instrument.id());
        assert!(first_delta.order.price.as_f64() > 0.0);
        assert!(first_delta.order.size.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_book_deltas_update() {
        let json = load_test_json("ws_book_update.json");
        let message: KrakenWsMessage = serde_json::from_str(&json).unwrap();
        let book: KrakenWsBookData = serde_json::from_value(message.data[0].clone()).unwrap();

        let instrument = create_mock_instrument();
        let deltas = parse_book_deltas(&book, &instrument, 1, TS).unwrap();

        assert!(!deltas.is_empty());

        // Check that we have at least one delta
        let first_delta = &deltas[0];
        assert_eq!(first_delta.instrument_id, instrument.id());
        assert!(first_delta.order.price.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_rfc3339_timestamp() {
        let timestamp = "2023-10-06T17:35:55.440295Z";
        let result = parse_rfc3339_timestamp(timestamp, "test").unwrap();
        assert!(result.as_u64() > 0);
    }

    #[rstest]
    fn test_parse_ws_bar() {
        let json = load_test_json("ws_ohlc_update.json");
        let message: KrakenWsMessage = serde_json::from_str(&json).unwrap();
        let ohlc: KrakenWsOhlcData = serde_json::from_value(message.data[0].clone()).unwrap();

        let instrument = create_mock_instrument();
        let bar = parse_ws_bar(&ohlc, &instrument, TS).unwrap();

        assert_eq!(bar.bar_type.instrument_id(), instrument.id());
        assert!(bar.open.as_f64() > 0.0);
        assert!(bar.high.as_f64() > 0.0);
        assert!(bar.low.as_f64() > 0.0);
        assert!(bar.close.as_f64() > 0.0);
        assert!(bar.volume.as_f64() > 0.0);

        let spec = bar.bar_type.spec();
        assert_eq!(spec.step.get(), 1);
        assert_eq!(spec.aggregation, BarAggregation::Minute);
        assert_eq!(spec.price_type, PriceType::Last);

        // Verify ts_event is computed as interval_begin + interval (close time)
        // interval_begin is 2023-10-04T16:25:00Z, interval is 1 minute, so close is 16:26:00Z
        let expected_close = ohlc.interval_begin + chrono::Duration::minutes(1);
        let expected_ts_event =
            UnixNanos::from(expected_close.timestamp_nanos_opt().unwrap() as u64);
        assert_eq!(bar.ts_event, expected_ts_event);
    }

    #[rstest]
    fn test_interval_to_bar_spec() {
        let test_cases = [
            (1, 1, BarAggregation::Minute),
            (5, 5, BarAggregation::Minute),
            (15, 15, BarAggregation::Minute),
            (30, 30, BarAggregation::Minute),
            (60, 1, BarAggregation::Hour),
            (240, 4, BarAggregation::Hour),
            (1440, 1, BarAggregation::Day),
            (10080, 1, BarAggregation::Week),
            (21600, 15, BarAggregation::Day), // 21600 min = 15 days
        ];

        for (interval, expected_step, expected_aggregation) in test_cases {
            let spec = interval_to_bar_spec(interval).unwrap();
            assert_eq!(
                spec.step.get(),
                expected_step,
                "Failed for interval {interval}"
            );
            assert_eq!(
                spec.aggregation, expected_aggregation,
                "Failed for interval {interval}"
            );
            assert_eq!(spec.price_type, PriceType::Last);
        }
    }

    #[rstest]
    fn test_interval_to_bar_spec_invalid() {
        let result = interval_to_bar_spec(999);
        assert!(result.is_err());
    }
}
