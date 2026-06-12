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

//! Parsers from Lighter streaming payloads to Nautilus domain types.

use std::sync::LazyLock;

use anyhow::Context;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate,
        OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick,
        depth::DEPTH10_LEN,
    },
    enums::{
        AccountType, AggregationSource, BookAction, LiquiditySide, OrderSide, OrderStatus,
        OrderType, PositionSideSpecified, RecordFlag, TimeInForce, TriggerType,
    },
    events::{
        AccountState, OrderAccepted, OrderCanceled, OrderExpired, OrderFilled, OrderRejected,
        OrderTriggered, OrderUpdated,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        enums::{
            LighterCandleResolution, LighterOrderKind, LighterOrderSide, LighterOrderStatus,
            LighterOrderTimeInForce, LighterTriggerStatus, order_side_from_is_ask,
        },
        parse::{parse_millis_to_nanos, price_from_decimal, quantity_from_decimal},
    },
    http::{
        models::{LighterOrder, LighterPriceLevel, LighterTrade},
        parse::parse_trade_tick,
    },
    websocket::{
        dispatch::{OrderIdentity, OrderShapeSnapshot},
        messages::{
            LighterAsset, LighterMarketStats, LighterPosition, LighterSpotMarketStats,
            LighterTicker, LighterUserStats, LighterWsCandle, LighterWsOrderBook,
        },
    },
};

/// Lighter encodes per-trade fees as integer micro-USDC ticks (1 unit = `1e-6` USDC),
/// matching the venue's quote-decimal precision. The fee scale (6) lets us
/// build the commission Decimal via `Decimal::new(ticks, FEE_DECIMALS)` —
/// directly populating mantissa+scale, avoiding the heavier division path
/// the prior implementation used.
const FEE_DECIMALS: u32 = 6;

/// Pre-built USDC `Currency` handle; the per-fill commission path used to
/// look up this currency on every call.
static FEE_USDC: LazyLock<Currency> = LazyLock::new(|| Currency::get_or_create_crypto("USDC"));

/// Parses a Lighter trade stream item into a Nautilus [`TradeTick`].
///
/// # Errors
///
/// Returns an error if the trade cannot be converted into a Nautilus tick.
pub fn parse_ws_trade_tick(
    trade: &LighterTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    parse_trade_tick(trade, instrument, ts_init)
}

/// Parses a Lighter order book update into Nautilus deltas.
///
/// `is_snapshot` must be supplied by the caller because Lighter sends
/// `subscribed/order_book` for the full book on subscription and
/// `update/order_book` for incremental level changes afterwards.
///
/// # Errors
///
/// Returns an error if any price or size cannot be converted.
pub fn parse_ws_order_book_deltas(
    book: &LighterWsOrderBook,
    instrument: &InstrumentAny,
    timestamp_ms: u64,
    is_snapshot: bool,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = parse_millis_to_nanos(timestamp_ms)?;
    let sequence = u64::try_from(book.nonce).context("negative Lighter book nonce")?;
    let total_levels = book.bids.len() + book.asks.len();

    anyhow::ensure!(
        is_snapshot || total_levels > 0,
        "empty Lighter WebSocket order book update",
    );

    let mut deltas = Vec::with_capacity(total_levels + usize::from(is_snapshot));

    if is_snapshot {
        let mut clear = OrderBookDelta::clear(instrument.id(), sequence, ts_event, ts_init);
        if total_levels == 0 {
            clear.flags |= RecordFlag::F_LAST as u8;
        }
        deltas.push(clear);
    }

    let mut processed = 0_usize;

    for bid in &book.bids {
        processed += 1;
        deltas.push(parse_book_level_delta(
            bid,
            instrument,
            OrderSide::Buy,
            sequence,
            ts_event,
            ts_init,
            book_flags(is_snapshot, processed, total_levels),
        )?);
    }

    for ask in &book.asks {
        processed += 1;
        deltas.push(parse_book_level_delta(
            ask,
            instrument,
            OrderSide::Sell,
            sequence,
            ts_event,
            ts_init,
            book_flags(is_snapshot, processed, total_levels),
        )?);
    }

    OrderBookDeltas::new_checked(instrument.id(), deltas)
        .context("failed to construct OrderBookDeltas from Lighter WebSocket book")
}

/// Parses a full Lighter order book payload into a Nautilus [`OrderBookDepth10`].
///
/// Call this only for snapshot or depth payloads that contain the full visible
/// book. Incremental updates should be parsed as deltas.
///
/// # Errors
///
/// Returns an error if any price or size cannot be converted.
pub fn parse_ws_order_book_depth10(
    book: &LighterWsOrderBook,
    instrument: &InstrumentAny,
    timestamp_ms: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDepth10> {
    let ts_event = parse_millis_to_nanos(timestamp_ms)?;
    let sequence = u64::try_from(book.nonce).context("negative Lighter book nonce")?;
    let mut bids = [BookOrder::default(); DEPTH10_LEN];
    let mut asks = [BookOrder::default(); DEPTH10_LEN];
    let mut bid_counts = [0_u32; DEPTH10_LEN];
    let mut ask_counts = [0_u32; DEPTH10_LEN];

    for (idx, level) in book.bids.iter().take(DEPTH10_LEN).enumerate() {
        bids[idx] = BookOrder::new(
            OrderSide::Buy,
            price_from_decimal(level.price, instrument.price_precision())?,
            quantity_from_decimal(level.size, instrument.size_precision())?,
            0,
        );
        bid_counts[idx] = 1;
    }

    for bid in bids.iter_mut().skip(book.bids.len().min(DEPTH10_LEN)) {
        *bid = BookOrder::new(
            OrderSide::Buy,
            Price::zero(instrument.price_precision()),
            Quantity::zero(instrument.size_precision()),
            0,
        );
    }

    for (idx, level) in book.asks.iter().take(DEPTH10_LEN).enumerate() {
        asks[idx] = BookOrder::new(
            OrderSide::Sell,
            price_from_decimal(level.price, instrument.price_precision())?,
            quantity_from_decimal(level.size, instrument.size_precision())?,
            0,
        );
        ask_counts[idx] = 1;
    }

    for ask in asks.iter_mut().skip(book.asks.len().min(DEPTH10_LEN)) {
        *ask = BookOrder::new(
            OrderSide::Sell,
            Price::zero(instrument.price_precision()),
            Quantity::zero(instrument.size_precision()),
            0,
        );
    }

    Ok(OrderBookDepth10::new(
        instrument.id(),
        bids,
        asks,
        bid_counts,
        ask_counts,
        RecordFlag::F_SNAPSHOT as u8,
        sequence,
        ts_event,
        ts_init,
    ))
}

/// Parses a Lighter ticker stream payload into a Nautilus [`QuoteTick`].
///
/// Returns `Ok(None)` if either side carries an empty price/size string,
/// which Lighter emits when one book side is currently uninhabited (no
/// resting orders). A one-sided book cannot be expressed as a [`QuoteTick`],
/// so the frame is skipped rather than rejected.
///
/// # Errors
///
/// Returns an error if a non-empty bid or ask field cannot be converted.
pub fn parse_ws_quote_tick(
    ticker: &LighterTicker,
    instrument: &InstrumentAny,
    timestamp_ms: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<QuoteTick>> {
    // Lighter sends zero (or empty string, mapped to zero by the wire deserializer)
    // for a side that currently has no resting orders; a one-sided book cannot be
    // expressed as a `QuoteTick`, so the frame is skipped rather than rejected.
    if ticker.b.price.is_zero()
        || ticker.b.size.is_zero()
        || ticker.a.price.is_zero()
        || ticker.a.size.is_zero()
    {
        return Ok(None);
    }

    let bid_price = price_from_decimal(ticker.b.price, instrument.price_precision())?;
    let ask_price = price_from_decimal(ticker.a.price, instrument.price_precision())?;
    let bid_size = quantity_from_decimal(ticker.b.size, instrument.size_precision())?;
    let ask_size = quantity_from_decimal(ticker.a.size, instrument.size_precision())?;
    let ts_event = parse_millis_to_nanos(timestamp_ms)?;

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .map(Some)
    .context("failed to construct QuoteTick from Lighter ticker")
}

/// Parses a Lighter perpetual market-stat update into a mark price update.
///
/// # Errors
///
/// Returns an error if the mark price or timestamp cannot be converted.
pub fn parse_ws_mark_price_update(
    stats: &LighterMarketStats,
    instrument: &InstrumentAny,
    timestamp_ms: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<MarkPriceUpdate> {
    build_price_update(
        instrument,
        stats.mark_price,
        timestamp_ms,
        ts_init,
        MarkPriceUpdate::new,
    )
}

/// Parses a Lighter perpetual market-stat update into an index price update.
///
/// # Errors
///
/// Returns an error if the index price or timestamp cannot be converted.
pub fn parse_ws_index_price_update(
    stats: &LighterMarketStats,
    instrument: &InstrumentAny,
    timestamp_ms: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<IndexPriceUpdate> {
    build_price_update(
        instrument,
        stats.index_price,
        timestamp_ms,
        ts_init,
        IndexPriceUpdate::new,
    )
}

/// Parses a Lighter spot market-stat update into an index price update.
///
/// # Errors
///
/// Returns an error if the index price or timestamp cannot be converted.
pub fn parse_ws_spot_index_price_update(
    stats: &LighterSpotMarketStats,
    instrument: &InstrumentAny,
    timestamp_ms: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<IndexPriceUpdate> {
    build_price_update(
        instrument,
        stats.index_price,
        timestamp_ms,
        ts_init,
        IndexPriceUpdate::new,
    )
}

fn build_price_update<T>(
    instrument: &InstrumentAny,
    price: Decimal,
    timestamp_ms: u64,
    ts_init: UnixNanos,
    constructor: impl FnOnce(InstrumentId, Price, UnixNanos, UnixNanos) -> T,
) -> anyhow::Result<T> {
    let price = price_from_decimal(price, instrument.price_precision())?;
    let ts_event = parse_millis_to_nanos(timestamp_ms)?;
    Ok(constructor(instrument.id(), price, ts_event, ts_init))
}

/// Parses a Lighter perpetual market-stat update into a funding-rate update.
///
/// Lighter exposes `current_funding_rate` as the estimate for the upcoming
/// payment. The `funding_rate` field is the last completed payment, so it is
/// not used for the streaming Nautilus update.
///
/// # Errors
///
/// Returns an error if the funding rate, event timestamp, or funding timestamp
/// cannot be converted.
pub fn parse_ws_funding_rate_update(
    stats: &LighterMarketStats,
    instrument: &InstrumentAny,
    timestamp_ms: u64,
    ts_init: UnixNanos,
) -> anyhow::Result<FundingRateUpdate> {
    let rate = stats.current_funding_rate;
    let next_funding_ns = if stats.funding_timestamp == 0 {
        None
    } else {
        Some(parse_millis_to_nanos(stats.funding_timestamp)?)
    };
    let ts_event = parse_millis_to_nanos(timestamp_ms)?;
    Ok(FundingRateUpdate::new(
        instrument.id(),
        rate,
        None,
        next_funding_ns,
        ts_event,
        ts_init,
    ))
}

/// Parses a Lighter WebSocket candle into a Nautilus [`Bar`] with `ts_event` set to the bar open.
///
/// # Errors
///
/// Returns an error if the OHLCV decimals overflow the instrument's precision, or if the
/// timestamp cannot be converted.
pub fn parse_ws_bar(
    instrument: &InstrumentAny,
    candle: &LighterWsCandle,
    resolution: LighterCandleResolution,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = Price::from_decimal_dp(candle.o, price_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle open: {e}"))?;
    let high = Price::from_decimal_dp(candle.h, price_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle high: {e}"))?;
    let low = Price::from_decimal_dp(candle.l, price_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle low: {e}"))?;
    let close = Price::from_decimal_dp(candle.c, price_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle close: {e}"))?;
    let volume = Quantity::from_decimal_dp(candle.v, size_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle volume: {e}"))?;

    let t_ms = u64::try_from(candle.t)
        .map_err(|_| anyhow::anyhow!("negative candle timestamp: {}", candle.t))?;
    let ts_event = parse_millis_to_nanos(t_ms)?;

    let bar_type = BarType::new(
        instrument.id(),
        resolution.to_bar_spec(),
        AggregationSource::External,
    );

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .map_err(|e| anyhow::anyhow!("invalid candle bar: {e}"))
}

fn parse_book_level_delta(
    level: &LighterPriceLevel,
    instrument: &InstrumentAny,
    side: OrderSide,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    flags: u8,
) -> anyhow::Result<OrderBookDelta> {
    let price = price_from_decimal(level.price, instrument.price_precision())?;
    let size = quantity_from_decimal(level.size, instrument.size_precision())?;
    let action = if flags & RecordFlag::F_SNAPSHOT as u8 != 0 {
        BookAction::Add
    } else if size.is_zero() {
        BookAction::Delete
    } else {
        BookAction::Update
    };
    let order = BookOrder::new(side, price, size, 0);

    OrderBookDelta::new_checked(
        instrument.id(),
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
    .context("failed to construct Lighter WebSocket book delta")
}

fn book_flags(is_snapshot: bool, processed: usize, total_levels: usize) -> u8 {
    let mut flags = if is_snapshot {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };

    if processed == total_levels {
        flags |= RecordFlag::F_LAST as u8;
    }

    flags
}

/// Parses a Lighter account-stream order payload into an [`OrderStatusReport`].
///
/// The venue exposes partial fills implicitly: the order remains in `Open`
/// status with `filled_base_amount > 0`. The mapping promotes such an order
/// to [`OrderStatus::PartiallyFilled`] so downstream consumers do not need to
/// re-derive it.
///
/// # Errors
///
/// Returns an error if any quantity, price, or timestamp field cannot be
/// converted, or if the venue order kind has no Nautilus equivalent.
pub fn parse_ws_order_status_report(
    order: &LighterOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order.order_id.as_str());
    let order_side = order
        .side
        .map_or_else(|| order_side_from_is_ask(order.is_ask), nautilus_order_side);
    let order_type = nautilus_order_type(order.order_type)?;

    let (time_in_force, expire_time) =
        nautilus_time_in_force(order.time_in_force, order.order_expiry);
    let post_only = order.time_in_force == LighterOrderTimeInForce::PostOnly;

    let quantity = quantity_from_decimal(order.initial_base_amount, instrument.size_precision())?;
    let filled_qty = quantity_from_decimal(order.filled_base_amount, instrument.size_precision())?;
    let order_status = nautilus_order_status(order.status, &filled_qty);
    let cancel_reason = order.status.as_cancel_reason();

    let ts_accepted = parse_optional_event_millis(order.created_at)?;
    let ts_last = parse_optional_event_millis(order.updated_at)?;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None, // client_order_id set below when present
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
    )
    .with_post_only(post_only)
    .with_reduce_only(order.reduce_only);

    if !order.client_order_id.is_empty() && order.client_order_id != "0" {
        report = report.with_client_order_id(ClientOrderId::new(order.client_order_id.as_str()));
    }

    if let Some(price) = parse_optional_price(order.price, instrument.price_precision())? {
        report = report.with_price(price);
    }

    if let Some(trigger_price) =
        parse_optional_price(order.trigger_price, instrument.price_precision())?
    {
        report = report.with_trigger_price(trigger_price);
    }

    if order_type_requires_trigger_type(order_type) {
        report = report.with_trigger_type(TriggerType::Default);
    }

    if let Some(expire) = expire_time {
        report = report.with_expire_time(expire);
    }

    if let Some(reason) = cancel_reason {
        report = report.with_cancel_reason(reason.to_string());
    }

    // Lighter publishes `parent_order_id` in the venue namespace (matching
    // `order_id`), not the client namespace. Nautilus
    // `OrderStatusReport::parent_order_id` expects a `ClientOrderId`, so
    // populating it from the venue id would mislabel namespaces. Contingency
    // linking for OTO/OCO groups must be applied at the execution-client
    // layer, where the venue-to-client id mapping is tracked.

    Ok(report)
}

fn order_type_requires_trigger_type(order_type: OrderType) -> bool {
    matches!(
        order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
            | OrderType::TrailingStopMarket
            | OrderType::TrailingStopLimit
    )
}

/// Parses a Lighter account-trade payload into a [`FillReport`] when the trade
/// involves the supplied account.
///
/// Returns `Ok(None)` if `account_index` is neither the bid nor ask account on
/// the trade. The handler routes account-stream trades through this helper, so
/// crossed pairs the user is not part of (e.g. when sharing a market with
/// other participants) are skipped silently rather than misattributed.
///
/// # Errors
///
/// Returns an error if any price, size, or timestamp field cannot be converted.
pub fn parse_ws_fill_report(
    trade: &LighterTrade,
    account_index: i64,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<FillReport>> {
    let user_is_bidder = trade.bid_account_id == account_index;
    let user_is_asker = trade.ask_account_id == account_index;
    if !user_is_bidder && !user_is_asker {
        return Ok(None);
    }

    let order_side = if user_is_bidder {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    // `is_maker_ask` says which book side rested as the maker; combined with
    // which side the user filled, that determines whether the user provided
    // or removed liquidity.
    let liquidity_side = if user_is_asker == trade.is_maker_ask {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    let venue_order_id = if user_is_bidder {
        venue_order_id_from(trade.bid_id_str.as_deref(), trade.bid_id)
    } else {
        venue_order_id_from(trade.ask_id_str.as_deref(), trade.ask_id)
    };

    let trade_id = parse_lighter_trade_id(trade)?;

    let last_qty = quantity_from_decimal(trade.size, instrument.size_precision())?;
    let last_px = price_from_decimal(trade.price, instrument.price_precision())?;

    let fee_value = if liquidity_side == LiquiditySide::Maker {
        trade.maker_fee
    } else {
        trade.taker_fee
    };
    let commission = lighter_fee_to_commission(fee_value)?;

    let client_order_id = if user_is_bidder {
        client_order_id_from(trade.bid_client_id_str.as_deref(), trade.bid_client_id)
    } else {
        client_order_id_from(trade.ask_client_id_str.as_deref(), trade.ask_client_id)
    };

    let timestamp_ms =
        u64::try_from(trade.timestamp).context("negative Lighter trade timestamp")?;
    let ts_event = parse_millis_to_nanos(timestamp_ms)?;

    Ok(Some(FillReport::new(
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
        None, // venue_position_id: Lighter perps run NETTING (one position per market)
        ts_event,
        ts_init,
        Some(UUID4::new()),
    )))
}

pub(crate) fn parse_lighter_trade_id(trade: &LighterTrade) -> anyhow::Result<TradeId> {
    match trade.trade_id_str.as_deref() {
        Some(s) => TradeId::new_checked(s),
        None => TradeId::new_checked(trade.trade_id.to_string()),
    }
    .context("invalid Lighter trade identifier")
}

/// Outcome of [`parse_lighter_order_event`] for tracked orders.
///
/// Mirrors `ParsedOrderEvent` in the BitMEX adapter (see
/// `crates/adapters/bitmex/src/websocket/parse.rs`). The execution
/// consumption loop maps these into [`nautilus_model::events::OrderEventAny`]
/// variants for tracked orders; untracked orders flow through the
/// `OrderStatusReport` path instead.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ParsedOrderEvent {
    Accepted(OrderAccepted),
    Canceled(OrderCanceled),
    Expired(OrderExpired),
    Triggered(OrderTriggered),
    Rejected(OrderRejected),
    Updated(OrderUpdated),
}

/// Inputs that the consumption-loop dispatcher hands to
/// [`parse_lighter_order_event`] for an `Open` frame. The dispatcher pre-
/// computes the accept/trigger gates and the modify-detection diff against
/// [`crate::websocket::dispatch::WsDispatchState::order_snapshots`]; the
/// parser uses the flags to pick the correct typed event without doing
/// dispatch state lookups itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OpenFrameContext {
    /// `true` if an `OrderAccepted` has already been emitted for the cloid.
    pub(crate) accepted_already_emitted: bool,
    /// `true` if an `OrderTriggered` has already been emitted for the cloid.
    pub(crate) triggered_already_emitted: bool,
    /// `true` if (qty, price, trigger_price) differ from the last stored
    /// snapshot. Computed by the caller; only meaningful when
    /// `accepted_already_emitted` is `true`.
    pub(crate) shape_changed: bool,
}

/// Extract the mutable shape (qty / price / trigger) from a Lighter order
/// payload so the dispatcher can diff against the stored snapshot. Returns
/// the values in the instrument's precision so equality is well-defined.
///
/// # Errors
///
/// Returns an error if any price or quantity field cannot be converted at
/// the instrument's precision.
pub(crate) fn lighter_order_shape(
    order: &LighterOrder,
    instrument: &InstrumentAny,
) -> anyhow::Result<OrderShapeSnapshot> {
    let quantity = quantity_from_decimal(order.initial_base_amount, instrument.size_precision())?;
    let price = parse_optional_price(order.price, instrument.price_precision())?;
    let trigger_price = parse_optional_price(order.trigger_price, instrument.price_precision())?;
    Ok(OrderShapeSnapshot {
        quantity,
        price,
        trigger_price,
    })
}

/// Build a typed order event from a Lighter `account_orders` payload, using
/// the identity context captured at submit time.
///
/// The caller (the execution consumption loop) decides between this path and
/// the [`OrderStatusReport`] fallback based on whether the cloid is in
/// `WsDispatchState::order_identities`. Returns `None` for transitional
/// statuses (`InProgress`, `Pending`) and for `Filled`: fills flow through
/// the trade stream and are converted via [`parse_lighter_order_filled`].
///
/// The `Open` branch decision matrix (in order):
///
/// - `trigger_status == Ready` and not yet emitted → `Triggered`.
/// - Not yet accepted → `Accepted` (the dispatcher seeds the shape
///   snapshot so subsequent diffs are meaningful).
/// - Already accepted and the order shape changed (qty / price / trigger)
///   → `Updated` (the dispatcher refreshes the shape snapshot).
/// - Already accepted with no shape change → `None` (snapshot replay).
///
/// # Errors
///
/// Returns an error if any price, quantity, or timestamp field cannot be
/// converted.
#[expect(
    clippy::too_many_arguments,
    reason = "identity and the precomputed open-frame context are independent inputs threaded by the caller"
)]
pub(crate) fn parse_lighter_order_event(
    order: &LighterOrder,
    instrument: &InstrumentAny,
    identity: &OrderIdentity,
    cloid: ClientOrderId,
    account_id: AccountId,
    trader_id: TraderId,
    open_ctx: OpenFrameContext,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<ParsedOrderEvent>> {
    let venue_order_id = VenueOrderId::new(order.order_id.as_str());
    let ts_event = parse_optional_event_millis(order.updated_at)?;
    let ts_accept = parse_optional_event_millis(order.created_at)?;

    match order.status {
        LighterOrderStatus::InProgress | LighterOrderStatus::Pending => Ok(None),
        LighterOrderStatus::Open => {
            if order.trigger_status == LighterTriggerStatus::Ready
                && !open_ctx.triggered_already_emitted
            {
                let triggered = OrderTriggered::new(
                    trader_id,
                    identity.strategy_id,
                    identity.instrument_id,
                    cloid,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );
                Ok(Some(ParsedOrderEvent::Triggered(triggered)))
            } else if !open_ctx.accepted_already_emitted {
                let accepted = OrderAccepted::new(
                    trader_id,
                    identity.strategy_id,
                    identity.instrument_id,
                    cloid,
                    venue_order_id,
                    account_id,
                    UUID4::new(),
                    ts_accept,
                    ts_init,
                    false,
                );
                Ok(Some(ParsedOrderEvent::Accepted(accepted)))
            } else if open_ctx.shape_changed {
                let new_qty =
                    quantity_from_decimal(order.initial_base_amount, instrument.size_precision())?;
                let new_price = parse_optional_price(order.price, instrument.price_precision())?;
                let new_trigger =
                    parse_optional_price(order.trigger_price, instrument.price_precision())?;
                let updated = OrderUpdated::new(
                    trader_id,
                    identity.strategy_id,
                    identity.instrument_id,
                    cloid,
                    new_qty,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                    new_price,
                    new_trigger,
                    None,
                    false,
                );
                Ok(Some(ParsedOrderEvent::Updated(updated)))
            } else {
                Ok(None)
            }
        }
        LighterOrderStatus::Filled => {
            // The trade stream drives `OrderFilled`; the order frame is a
            // status echo and does not carry per-fill trade ids.
            Ok(None)
        }
        LighterOrderStatus::CanceledExpired => {
            let expired = OrderExpired::new(
                trader_id,
                identity.strategy_id,
                identity.instrument_id,
                cloid,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
                Some(venue_order_id),
                Some(account_id),
            );
            Ok(Some(ParsedOrderEvent::Expired(expired)))
        }
        LighterOrderStatus::CanceledPostOnly => {
            let rejected = OrderRejected::new(
                trader_id,
                identity.strategy_id,
                identity.instrument_id,
                cloid,
                account_id,
                Ustr::from("post-only"),
                UUID4::new(),
                ts_event,
                ts_init,
                false,
                true, // due_post_only
            );
            Ok(Some(ParsedOrderEvent::Rejected(rejected)))
        }
        LighterOrderStatus::Canceled
        | LighterOrderStatus::CanceledReduceOnly
        | LighterOrderStatus::CanceledPositionNotAllowed
        | LighterOrderStatus::CanceledMarginNotAllowed
        | LighterOrderStatus::CanceledTooMuchSlippage
        | LighterOrderStatus::CanceledNotEnoughLiquidity
        | LighterOrderStatus::CanceledSelfTrade
        | LighterOrderStatus::CanceledOco
        | LighterOrderStatus::CanceledChild
        | LighterOrderStatus::CanceledLiquidation
        | LighterOrderStatus::CanceledInvalidBalance => {
            let canceled = OrderCanceled::new(
                trader_id,
                identity.strategy_id,
                identity.instrument_id,
                cloid,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
                Some(venue_order_id),
                Some(account_id),
            );
            Ok(Some(ParsedOrderEvent::Canceled(canceled)))
        }
    }
}

/// Build an [`OrderFilled`] event from a Lighter `account_trades` payload,
/// using the identity context captured at submit time.
///
/// Returns `Ok(None)` if the trade does not involve `account_index` (the
/// venue forwards crossed pairs the account is not part of on the same
/// channel). The caller dedupes by `TradeId` against
/// `WsDispatchState::seen_trade_ids` to drop repeats on reconnect.
///
/// # Errors
///
/// Returns an error if any price, size, fee, or timestamp field cannot be
/// converted.
#[expect(
    clippy::too_many_arguments,
    reason = "identity and account context are independent inputs threaded by the dispatcher"
)]
pub(crate) fn parse_lighter_order_filled(
    trade: &LighterTrade,
    instrument: &InstrumentAny,
    identity: &OrderIdentity,
    cloid: ClientOrderId,
    account_id: AccountId,
    trader_id: TraderId,
    account_index: i64,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderFilled>> {
    let user_is_bidder = trade.bid_account_id == account_index;
    let user_is_asker = trade.ask_account_id == account_index;
    if !user_is_bidder && !user_is_asker {
        return Ok(None);
    }

    let liquidity_side = if user_is_asker == trade.is_maker_ask {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    let venue_order_id = if user_is_bidder {
        venue_order_id_from(trade.bid_id_str.as_deref(), trade.bid_id)
    } else {
        venue_order_id_from(trade.ask_id_str.as_deref(), trade.ask_id)
    };

    let trade_id = match trade.trade_id_str.as_deref() {
        Some(s) => TradeId::new_checked(s),
        None => TradeId::new_checked(trade.trade_id.to_string()),
    }
    .context("invalid Lighter trade identifier")?;

    let last_qty = quantity_from_decimal(trade.size, instrument.size_precision())?;
    let last_px = price_from_decimal(trade.price, instrument.price_precision())?;

    let fee_value = if liquidity_side == LiquiditySide::Maker {
        trade.maker_fee
    } else {
        trade.taker_fee
    };
    let commission = lighter_fee_to_commission(fee_value)?;

    let timestamp_ms =
        u64::try_from(trade.timestamp).context("negative Lighter trade timestamp")?;
    let ts_event = parse_millis_to_nanos(timestamp_ms)?;

    Ok(Some(OrderFilled::new(
        trader_id,
        identity.strategy_id,
        identity.instrument_id,
        cloid,
        venue_order_id,
        account_id,
        trade_id,
        identity.order_side,
        identity.order_type,
        last_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        ts_event,
        ts_init,
        false, // reconciliation
        None,  // venue_position_id: Lighter perps run NETTING
        Some(commission),
    )))
}

/// Parses a Lighter position payload into a [`PositionStatusReport`].
///
/// The `account_all_positions` frame carries no top-level event timestamp;
/// callers should pass the wall-clock arrival time captured by the handler.
///
/// # Errors
///
/// Returns an error if the size or entry price cannot be converted.
pub fn parse_ws_position_status_report(
    position: &LighterPosition,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let quantity = quantity_from_decimal(position.position, instrument.size_precision())?;
    let position_side = if quantity.is_zero() {
        PositionSideSpecified::Flat
    } else if position.sign < 0 {
        PositionSideSpecified::Short
    } else {
        PositionSideSpecified::Long
    };

    let avg_px_open = if position_side == PositionSideSpecified::Flat {
        None
    } else {
        Some(position.avg_entry_price)
    };

    Ok(PositionStatusReport::new(
        account_id,
        instrument.id(),
        position_side,
        quantity,
        ts_event,
        ts_init,
        Some(UUID4::new()),
        None, // venue_position_id: NETTING, Nautilus identifies by instrument
        avg_px_open,
    ))
}

/// Builds the per-asset [`AccountBalance`] for one Lighter wallet entry.
///
/// Lighter is a unified-margin venue: spot-side `balance` and perp-side
/// `margin_balance` are both deployable equity. The split shows where the
/// money currently sits, not whether it's usable. The only true lock on
/// the trading side is `locked_balance` (resting spot orders); perp
/// margin currently in use is tracked separately via [`MarginBalance`],
/// not via `AccountBalance.locked`.
///
/// We map:
/// - `total  = balance + margin_balance`: every unit of that currency
///   the account owns at the venue
/// - `locked = locked_balance`: only spot-order reservations
/// - `free   = total - locked` (derived by `from_total_and_locked`):
///   all deployable equity, whether currently sitting on spot or perp
///
/// Perp-side margin currently allocated to positions is **not** folded
/// into `locked`. It lives on [`MarginBalance`] and the portfolio's
/// risk engine consumes it from there. See
/// [`margin_balance_from_user_stats`].
///
/// # Errors
///
/// Returns an error if `AccountBalance::from_total_and_locked` rejects
/// the computed values.
pub fn account_balance_from_lighter_asset(asset: &LighterAsset) -> anyhow::Result<AccountBalance> {
    let currency = Currency::get_or_create_crypto(asset.symbol.as_str());
    let total = asset.balance + asset.margin_balance;
    let locked = asset.locked_balance;
    AccountBalance::from_total_and_locked(total, locked, currency)
        .context("failed to construct Lighter account balance")
}

/// Builds the cross-margin [`MarginBalance`] from a `user_stats` frame.
///
/// Lighter is USDC-collateralized end-to-end. `user_stats` is the perp-side
/// rollup; we derive:
/// - `initial = max(collateral - available_balance, 0)`: collateral
///   currently allocated to open positions/orders
/// - `maintenance = 0`: Lighter does not publish maintenance margin on
///   `user_stats`. `margin_usage` looks like a maintenance ratio but is
///   actually the initial-margin-usage percentage
///   (`(collateral - available) / collateral * 100`), so it carries no
///   information about maintenance. Computing maintenance per-position
///   from the positions stream would be more accurate; until that's
///   wired we report zero rather than fabricate a value.
///
/// `instrument_id = None` routes this into `MarginAccount.account_margins`
/// (cross margin keyed by currency), which is what the portfolio reads for
/// venues running unified cross-margin mode.
///
/// # Errors
///
/// Returns an error if either `Money::from_decimal` call rejects the value.
pub fn margin_balance_from_user_stats(stats: &LighterUserStats) -> anyhow::Result<MarginBalance> {
    let usdc = Currency::get_or_create_crypto("USDC");
    let initial_dec = (stats.collateral - stats.available_balance).max(Decimal::ZERO);
    let initial = Money::from_decimal(initial_dec, usdc)
        .map_err(|e| anyhow::anyhow!("failed to construct initial margin: {e}"))?;
    let maintenance = Money::from_decimal(Decimal::ZERO, usdc)
        .map_err(|e| anyhow::anyhow!("failed to construct maintenance margin: {e}"))?;
    Ok(MarginBalance::new(initial, maintenance, None))
}

/// Assembles the unified [`AccountState`] from already-parsed components.
///
/// The reconciler in the `websocket::account_state` module owns the latest
/// snapshot of each input stream and calls this once per emission.
/// `AccountType::Margin` is invariant for Lighter; `base_currency` is `None`
/// because the account holds multiple spot currencies.
#[must_use]
pub fn build_unified_account_state(
    balances: Vec<AccountBalance>,
    margin: Option<MarginBalance>,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> AccountState {
    let margins = margin.map(|m| vec![m]).unwrap_or_default();
    AccountState::new(
        account_id,
        AccountType::Margin,
        balances,
        margins,
        true,
        UUID4::new(),
        ts_event,
        ts_init,
        None,
    )
}

fn parse_optional_event_millis(millis: i64) -> anyhow::Result<UnixNanos> {
    if millis <= 0 {
        return Ok(UnixNanos::default());
    }
    parse_millis_to_nanos(millis as u64)
}

fn parse_optional_price(value: Decimal, precision: u8) -> anyhow::Result<Option<Price>> {
    if value.is_zero() {
        return Ok(None);
    }
    Price::from_decimal_dp(value, precision)
        .map(Some)
        .map_err(|e| anyhow::anyhow!("invalid price `{value}` at precision {precision}: {e}"))
}

fn lighter_fee_to_commission(fee_ticks: Option<i32>) -> anyhow::Result<Money> {
    let ticks = fee_ticks.unwrap_or(0);
    let amount = Decimal::new(i64::from(ticks), FEE_DECIMALS);
    Money::from_decimal(amount, *FEE_USDC)
        .map_err(|e| anyhow::anyhow!("failed to construct Lighter commission: {e}"))
}

fn nautilus_order_side(side: LighterOrderSide) -> OrderSide {
    match side {
        LighterOrderSide::Buy => OrderSide::Buy,
        LighterOrderSide::Sell => OrderSide::Sell,
    }
}

fn nautilus_order_type(kind: LighterOrderKind) -> anyhow::Result<OrderType> {
    match kind {
        LighterOrderKind::Limit => Ok(OrderType::Limit),
        LighterOrderKind::Market => Ok(OrderType::Market),
        LighterOrderKind::StopLoss => Ok(OrderType::StopMarket),
        LighterOrderKind::StopLossLimit => Ok(OrderType::StopLimit),
        LighterOrderKind::TakeProfit => Ok(OrderType::MarketIfTouched),
        LighterOrderKind::TakeProfitLimit => Ok(OrderType::LimitIfTouched),
        LighterOrderKind::Twap | LighterOrderKind::TwapSub | LighterOrderKind::Liquidation => Err(
            anyhow::anyhow!("Lighter `{kind:?}` has no Nautilus order-type equivalent",),
        ),
    }
}

fn nautilus_time_in_force(
    tif: LighterOrderTimeInForce,
    order_expiry: i64,
) -> (TimeInForce, Option<UnixNanos>) {
    match tif {
        LighterOrderTimeInForce::ImmediateOrCancel => (TimeInForce::Ioc, None),
        // Lighter has no Nautilus PostOnly TIF; Nautilus models it as Gtc + post_only flag.
        LighterOrderTimeInForce::PostOnly => (TimeInForce::Gtc, None),
        LighterOrderTimeInForce::GoodTillTime => {
            // Lighter overloads `good-till-time` for both true GTD (positive
            // expiry timestamp) and venue-default GTC (`order_expiry == -1`).
            if order_expiry > 0 {
                match parse_millis_to_nanos(order_expiry as u64) {
                    Ok(expiry) => (TimeInForce::Gtd, Some(expiry)),
                    Err(_) => (TimeInForce::Gtc, None),
                }
            } else {
                (TimeInForce::Gtc, None)
            }
        }
        LighterOrderTimeInForce::Unknown => (TimeInForce::Gtc, None),
    }
}

fn nautilus_order_status(status: LighterOrderStatus, filled_qty: &Quantity) -> OrderStatus {
    match status {
        LighterOrderStatus::InProgress | LighterOrderStatus::Pending => OrderStatus::Submitted,
        LighterOrderStatus::Open => {
            if filled_qty.is_zero() {
                OrderStatus::Accepted
            } else {
                OrderStatus::PartiallyFilled
            }
        }
        LighterOrderStatus::Filled => OrderStatus::Filled,
        LighterOrderStatus::CanceledExpired => OrderStatus::Expired,
        LighterOrderStatus::CanceledPostOnly => OrderStatus::Rejected,
        LighterOrderStatus::Canceled
        | LighterOrderStatus::CanceledReduceOnly
        | LighterOrderStatus::CanceledPositionNotAllowed
        | LighterOrderStatus::CanceledMarginNotAllowed
        | LighterOrderStatus::CanceledTooMuchSlippage
        | LighterOrderStatus::CanceledNotEnoughLiquidity
        | LighterOrderStatus::CanceledSelfTrade
        | LighterOrderStatus::CanceledOco
        | LighterOrderStatus::CanceledChild
        | LighterOrderStatus::CanceledLiquidation
        | LighterOrderStatus::CanceledInvalidBalance => OrderStatus::Canceled,
    }
}

/// Resolve a venue order id from the venue-provided string field, falling
/// back to the numeric `i64` mirror when the string field is absent. Skips
/// the `Option<String>::clone` the previous code paid on every fill.
fn venue_order_id_from(str_field: Option<&str>, numeric_fallback: i64) -> VenueOrderId {
    match str_field {
        Some(s) => VenueOrderId::new(s),
        None => VenueOrderId::new(numeric_fallback.to_string()),
    }
}

/// Resolve an optional client order id from the venue's string field, treating
/// empty and the sentinel `"0"` as absent; falls back to the numeric `i64`
/// mirror when the string field is `None`.
fn client_order_id_from(str_field: Option<&str>, numeric_fallback: i64) -> Option<ClientOrderId> {
    match str_field {
        Some(s) if !s.is_empty() && s != "0" => Some(ClientOrderId::new(s)),
        None if numeric_fallback != 0 => Some(ClientOrderId::new(numeric_fallback.to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::{
        enums::{BarAggregation, ContingencyType, PriceType},
        identifiers::{InstrumentId, StrategyId, Symbol, Venue},
        instruments::CryptoPerpetual,
        types::{Price, Quantity, currency::Currency},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::LighterTradeType,
        http::models::LighterTrade,
        websocket::messages::{LighterMarketStats, LighterSpotMarketStats},
    };

    fn create_test_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), Venue::new("LIGHTER"));

        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("ETH-PERP"),
            Currency::from("ETH"),
            Currency::from("USDC"),
            Currency::from("USDC"),
            false,
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
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
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn stub_book() -> LighterWsOrderBook {
        LighterWsOrderBook {
            code: 0,
            asks: vec![LighterPriceLevel {
                price: Decimal::from_str("2064.54").unwrap(),
                size: Decimal::from_str("0.3285").unwrap(),
            }],
            bids: vec![LighterPriceLevel {
                price: Decimal::from_str("2064.30").unwrap(),
                size: Decimal::from_str("1.0392").unwrap(),
            }],
            offset: 1558300,
            nonce: 9182390020,
            last_updated_at: 1774884082309144,
            begin_nonce: 9182389998,
        }
    }

    fn stub_market_stats() -> LighterMarketStats {
        LighterMarketStats {
            symbol: Ustr::from("ETH"),
            market_id: 0,
            index_price: Decimal::from_str("2064.48").unwrap(),
            mark_price: Decimal::from_str("2064.47").unwrap(),
            mid_price: Decimal::from_str("2064.39").unwrap(),
            open_interest: Decimal::from_str("27250.8411").unwrap(),
            open_interest_limit: Decimal::from_str("50000.0000").unwrap(),
            funding_clamp_small: Decimal::from_str("0.0001").unwrap(),
            funding_clamp_big: Decimal::from_str("0.0002").unwrap(),
            last_trade_price: Decimal::from_str("2064.50").unwrap(),
            current_funding_rate: Decimal::from_str("0.000001").unwrap(),
            funding_rate: Decimal::from_str("0.000002").unwrap(),
            funding_timestamp: 1_774_886_400_000,
            daily_base_token_volume: Decimal::new(1_999_586_931, 4),
            daily_quote_token_volume: Decimal::new(471_193_598_847_246, 6),
            daily_price_low: Decimal::new(231_181, 2),
            daily_price_high: Decimal::new(2_398, 0),
            daily_price_change: Decimal::new(16_854_147_780_232_130, 17),
        }
    }

    fn stub_spot_market_stats() -> LighterSpotMarketStats {
        LighterSpotMarketStats {
            symbol: Ustr::from("ETH"),
            market_id: 2048,
            index_price: Decimal::from_str("1.000000").unwrap(),
            mid_price: Decimal::from_str("1.000001").unwrap(),
            last_trade_price: Decimal::from_str("1.000002").unwrap(),
            daily_base_token_volume: Decimal::from(1000),
            daily_quote_token_volume: Decimal::new(10_001, 1),
            daily_price_low: Decimal::new(999_999, 6),
            daily_price_high: Decimal::new(1_000_002, 6),
            daily_price_change: Decimal::new(1, 6),
        }
    }

    #[rstest]
    fn test_parse_ws_order_book_deltas_snapshot() {
        let instrument = create_test_instrument();
        let ts_init = UnixNanos::from(1);

        let deltas =
            parse_ws_order_book_deltas(&stub_book(), &instrument, 1774884082326, true, ts_init)
                .unwrap();

        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].action, BookAction::Add);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[1].order.price, Price::from("2064.30"));
        assert_eq!(deltas.deltas[1].order.size, Quantity::from("1.0392"));
        assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell);
        assert_eq!(deltas.deltas[2].order.price, Price::from("2064.54"));
        assert_eq!(deltas.deltas[2].order.size, Quantity::from("0.3285"));
        assert_eq!(deltas.deltas[0].sequence, 9_182_390_020);
        assert_eq!(deltas.deltas[1].sequence, 9_182_390_020);
        assert_eq!(deltas.deltas[2].sequence, 9_182_390_020);
        assert_eq!(deltas.sequence, 9_182_390_020);
        assert_eq!(
            deltas.deltas[2].flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8,
        );
    }

    #[rstest]
    fn test_parse_ws_order_book_deltas_update_delete_zero_size() {
        let instrument = create_test_instrument();
        let mut book = stub_book();
        book.asks[0].size = Decimal::ZERO;

        let deltas = parse_ws_order_book_deltas(
            &book,
            &instrument,
            1774884082326,
            false,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(deltas.deltas.len(), 2);
        assert_eq!(deltas.deltas[0].action, BookAction::Update);
        assert_eq!(deltas.deltas[0].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[0].order.price, Price::from("2064.30"));
        assert_eq!(deltas.deltas[1].action, BookAction::Delete);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Sell);
        assert_eq!(deltas.deltas[1].order.price, Price::from("2064.54"));
    }

    #[rstest]
    fn test_parse_ws_order_book_deltas_rejects_negative_nonce() {
        let instrument = create_test_instrument();
        let mut book = stub_book();
        book.nonce = -1;

        let err = parse_ws_order_book_deltas(
            &book,
            &instrument,
            1774884082326,
            false,
            UnixNanos::from(1),
        )
        .unwrap_err();

        assert!(err.to_string().contains("negative Lighter book nonce"));
    }

    #[rstest]
    fn test_parse_ws_order_book_deltas_rejects_empty_update() {
        let instrument = create_test_instrument();
        let mut book = stub_book();
        book.asks.clear();
        book.bids.clear();

        let err = parse_ws_order_book_deltas(
            &book,
            &instrument,
            1774884082326,
            false,
            UnixNanos::from(1),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("empty Lighter WebSocket order book update")
        );
    }

    #[rstest]
    fn test_parse_ws_order_book_deltas_rejects_zero_size_snapshot_level() {
        let instrument = create_test_instrument();
        let mut book = stub_book();
        book.bids[0].size = Decimal::ZERO;

        let err =
            parse_ws_order_book_deltas(&book, &instrument, 1774884082326, true, UnixNanos::from(1))
                .unwrap_err();

        assert!(
            err.to_string()
                .contains("failed to construct Lighter WebSocket book delta")
        );
    }

    #[rstest]
    fn test_parse_ws_quote_tick() {
        let instrument = create_test_instrument();
        let ticker = LighterTicker {
            s: Ustr::from("ETH"),
            a: LighterPriceLevel {
                price: Decimal::from_str("2064.48").unwrap(),
                size: Decimal::from_str("0.4950").unwrap(),
            },
            b: LighterPriceLevel {
                price: Decimal::from_str("2064.30").unwrap(),
                size: Decimal::from_str("1.0392").unwrap(),
            },
            last_updated_at: 1774883844921166,
        };

        let quote = parse_ws_quote_tick(&ticker, &instrument, 1774883844933, UnixNanos::from(1))
            .unwrap()
            .expect("two-sided ticker yields a quote");

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, Price::from("2064.30"));
        assert_eq!(quote.ask_price, Price::from("2064.48"));
        assert_eq!(quote.bid_size, Quantity::from("1.0392"));
        assert_eq!(quote.ask_size, Quantity::from("0.4950"));
        assert_eq!(quote.ts_event, UnixNanos::from(1_774_883_844_933_000_000),);
    }

    #[rstest]
    fn test_parse_ws_quote_tick_skips_one_sided_book() {
        let instrument = create_test_instrument();
        let ticker = LighterTicker {
            s: Ustr::from("ETH"),
            a: LighterPriceLevel {
                price: Decimal::from_str("2064.48").unwrap(),
                size: Decimal::from_str("0.4950").unwrap(),
            },
            b: LighterPriceLevel {
                // Lighter emits empty strings when one side has no resting
                // orders; the wire deserializer maps those to `Decimal::ZERO`.
                price: Decimal::ZERO,
                size: Decimal::ZERO,
            },
            last_updated_at: 1774883844921166,
        };

        let result =
            parse_ws_quote_tick(&ticker, &instrument, 1774883844933, UnixNanos::from(1)).unwrap();

        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_ws_quote_tick_rejects_invalid_price() {
        // With Decimal model fields, wire-malformed prices are rejected at
        // JSON deserialize time; this test guards the deserialize boundary so
        // a bad payload does not silently construct a zero-priced quote.
        let payload = serde_json::json!({
            "s": "ETH",
            "a": {"price": "not-a-price", "size": "0.4950"},
            "b": {"price": "2064.30", "size": "1.0392"},
            "last_updated_at": 1774883844921166u64,
        });

        let err = serde_json::from_value::<LighterTicker>(payload).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("decimal"));
    }

    #[rstest]
    fn test_parse_ws_mark_price_update() {
        let instrument = create_test_instrument();

        let update = parse_ws_mark_price_update(
            &stub_market_stats(),
            &instrument,
            1_774_883_844_933,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(update.instrument_id, instrument.id());
        assert_eq!(update.value, Price::from("2064.47"));
        assert_eq!(update.ts_event, UnixNanos::from(1_774_883_844_933_000_000));
        assert_eq!(update.ts_init, UnixNanos::from(1));
    }

    #[rstest]
    fn test_parse_ws_index_price_update() {
        let instrument = create_test_instrument();

        let update = parse_ws_index_price_update(
            &stub_market_stats(),
            &instrument,
            1_774_883_844_933,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(update.instrument_id, instrument.id());
        assert_eq!(update.value, Price::from("2064.48"));
        assert_eq!(update.ts_event, UnixNanos::from(1_774_883_844_933_000_000));
    }

    #[rstest]
    fn test_parse_ws_spot_index_price_update() {
        let instrument = create_test_instrument();

        let update = parse_ws_spot_index_price_update(
            &stub_spot_market_stats(),
            &instrument,
            1_774_883_844_933,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(update.instrument_id, instrument.id());
        assert_eq!(update.value, Price::from("1.00"));
        assert_eq!(update.ts_event, UnixNanos::from(1_774_883_844_933_000_000));
    }

    #[rstest]
    fn test_parse_ws_funding_rate_update_uses_current_funding_rate() {
        let instrument = create_test_instrument();

        let update = parse_ws_funding_rate_update(
            &stub_market_stats(),
            &instrument,
            1_774_883_844_933,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(update.instrument_id, instrument.id());
        assert_eq!(update.rate.to_string(), "0.000001");
        assert_eq!(
            update.next_funding_ns,
            Some(UnixNanos::from(1_774_886_400_000_000_000))
        );
        assert_eq!(update.ts_event, UnixNanos::from(1_774_883_844_933_000_000));
    }

    // Pins which field each price-update parser reads from `LighterMarketStats`.
    // The three parsers share `build_price_update`; without these guards a
    // regression that swaps `mark_price` and `index_price` (or that has
    // `funding_rate_update` accidentally reading `funding_rate` instead of
    // `current_funding_rate`) would only fail one of the three happy-path
    // tests above and might pass them all if their fixtures happen to align.
    #[rstest]
    fn test_parse_ws_mark_price_update_reads_mark_field_only() {
        let instrument = create_test_instrument();
        let mut stats = stub_market_stats();
        let sentinel = Decimal::from_str("1234.56").unwrap();
        let other = Decimal::from_str("9999.99").unwrap();
        stats.mark_price = sentinel;
        stats.index_price = other;
        stats.mid_price = other;
        stats.last_trade_price = other;

        let update =
            parse_ws_mark_price_update(&stats, &instrument, 1_774_883_844_933, UnixNanos::from(1))
                .unwrap();

        assert_eq!(update.value, Price::from("1234.56"));
    }

    #[rstest]
    fn test_parse_ws_index_price_update_reads_index_field_only() {
        let instrument = create_test_instrument();
        let mut stats = stub_market_stats();
        let sentinel = Decimal::from_str("1234.56").unwrap();
        let other = Decimal::from_str("9999.99").unwrap();
        stats.index_price = sentinel;
        stats.mark_price = other;
        stats.mid_price = other;
        stats.last_trade_price = other;

        let update =
            parse_ws_index_price_update(&stats, &instrument, 1_774_883_844_933, UnixNanos::from(1))
                .unwrap();

        assert_eq!(update.value, Price::from("1234.56"));
    }

    #[rstest]
    fn test_parse_ws_funding_rate_update_reads_current_funding_rate_field_only() {
        let instrument = create_test_instrument();
        let mut stats = stub_market_stats();
        // `funding_rate` is the prior payment; the parser must read
        // `current_funding_rate` instead. Setting distinct sentinel values
        // catches a field-swap regression.
        let sentinel = Decimal::from_str("0.0000123").unwrap();
        let other = Decimal::from_str("0.9999").unwrap();
        stats.current_funding_rate = sentinel;
        stats.funding_rate = other;

        let update = parse_ws_funding_rate_update(
            &stats,
            &instrument,
            1_774_883_844_933,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(update.rate, sentinel);
    }

    #[rstest]
    fn test_parse_ws_funding_rate_update_treats_zero_next_funding_as_none() {
        let instrument = create_test_instrument();
        let mut stats = stub_market_stats();
        stats.funding_timestamp = 0;

        let update = parse_ws_funding_rate_update(
            &stats,
            &instrument,
            1_774_883_844_933,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(update.instrument_id, instrument.id());
        assert_eq!(update.next_funding_ns, None);
    }

    #[rstest]
    fn test_parse_ws_order_book_depth10_pads_levels() {
        let instrument = create_test_instrument();
        let depth = parse_ws_order_book_depth10(
            &stub_book(),
            &instrument,
            1774884082326,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(depth.bids[0].price, Price::from("2064.30"));
        // Populated level must round-trip price AND size, otherwise a
        // future refactor that swaps fields or drops precision would not
        // be caught by this test.
        assert_eq!(depth.bids[0].size, Quantity::from("1.0392"));
        assert_eq!(depth.bids[0].side, OrderSide::Buy);
        assert_eq!(depth.asks[0].price, Price::from("2064.54"));
        assert_eq!(depth.asks[0].size, Quantity::from("0.3285"));
        assert_eq!(depth.asks[0].side, OrderSide::Sell);
        assert_eq!(depth.sequence, 9_182_390_020);
        assert_eq!(depth.bid_counts[0], 1);
        assert_eq!(depth.ask_counts[0], 1);
        assert_eq!(depth.bid_counts[1], 0);
        assert_eq!(depth.ask_counts[1], 0);
        assert!(depth.bids[1].size.is_zero());
        assert!(depth.asks[1].size.is_zero());
    }

    #[rstest]
    fn test_parse_ws_trade_tick_delegates_trade_parser() {
        let instrument = create_test_instrument();
        let trade = LighterTrade {
            trade_id: 16164557907,
            trade_id_str: Some("16164557907".to_string()),
            tx_hash: "019f2b9c".to_string(),
            trade_type: LighterTradeType::Trade,
            market_id: 0,
            size: Decimal::from_str("0.1336").unwrap(),
            price: Decimal::from_str("2181.83").unwrap(),
            usd_amount: Decimal::from_str("291.492488").unwrap(),
            ask_id: 281476612587355,
            ask_id_str: Some("281476612587355".to_string()),
            bid_id: 562948334068259,
            bid_id_str: Some("562948334068259".to_string()),
            ask_client_id: 363283,
            ask_client_id_str: Some("363283".to_string()),
            bid_client_id: 23004521241,
            bid_client_id_str: Some("23004521241".to_string()),
            ask_account_id: 57890,
            bid_account_id: 317068,
            is_maker_ask: false,
            block_height: 198321831,
            timestamp: 1773854156654,
            taker_fee: Some(196),
            taker_position_size_before: None,
            taker_entry_quote_before: None,
            taker_initial_margin_fraction_before: None,
            taker_position_sign_changed: None,
            maker_fee: Some(28),
            maker_position_size_before: None,
            maker_entry_quote_before: None,
            maker_initial_margin_fraction_before: None,
            maker_position_sign_changed: None,
            transaction_time: 1773854156686065,
            ask_account_pnl: None,
            bid_account_pnl: None,
        };

        let tick = parse_ws_trade_tick(&trade, &instrument, UnixNanos::from(1)).unwrap();

        assert_eq!(tick.trade_id.to_string(), "16164557907");
        assert_eq!(tick.price, Price::from("2181.83"));
    }

    fn account_id() -> AccountId {
        AccountId::from("LIGHTER-1234")
    }

    fn stub_order(status: LighterOrderStatus) -> LighterOrder {
        LighterOrder {
            order_index: 281476929510110,
            client_order_index: 42,
            order_id: "281476929510110".to_string(),
            client_order_id: "42".to_string(),
            market_index: 0,
            owner_account_index: 1234,
            initial_base_amount: Decimal::from_str("0.0050").unwrap(),
            price: Decimal::from_str("2352.74").unwrap(),
            nonce: 9182390020,
            remaining_base_amount: Decimal::from_str("0.0030").unwrap(),
            is_ask: true,
            base_size: 50,
            base_price: 235274,
            filled_base_amount: Decimal::from_str("0.0020").unwrap(),
            filled_quote_amount: Decimal::from_str("4.705480").unwrap(),
            side: Some(LighterOrderSide::Sell),
            order_type: LighterOrderKind::Limit,
            time_in_force: LighterOrderTimeInForce::GoodTillTime,
            reduce_only: false,
            trigger_price: Decimal::ZERO,
            order_expiry: 1_780_360_584_479,
            status,
            trigger_status: LighterTriggerStatus::Na,
            trigger_time: 0,
            parent_order_index: 0,
            parent_order_id: "0".to_string(),
            to_trigger_order_id_0: "0".to_string(),
            to_trigger_order_id_1: "0".to_string(),
            to_cancel_order_id_0: "0".to_string(),
            integrator_fee_collector_index: "0".to_string(),
            integrator_taker_fee: Decimal::ZERO,
            integrator_maker_fee: Decimal::ZERO,
            block_height: 227_535_532,
            timestamp: 1_777_941_383_576,
            created_at: 1_777_941_383_576,
            updated_at: 1_777_941_383_900,
            transaction_time: 1_777_941_383_576_735,
        }
    }

    fn stub_account_trade(
        account_index: i64,
        is_maker_ask: bool,
        user_is_bidder: bool,
    ) -> LighterTrade {
        LighterTrade {
            trade_id: 19_209_006_902,
            trade_id_str: Some("19209006902".to_string()),
            tx_hash: "000000128b1ee814".to_string(),
            trade_type: LighterTradeType::Trade,
            market_id: 0,
            size: Decimal::from_str("0.1336").unwrap(),
            price: Decimal::from_str("2352.73").unwrap(),
            usd_amount: Decimal::from_str("314.324728").unwrap(),
            ask_id: 281_476_929_510_102,
            ask_id_str: Some("281476929510102".to_string()),
            bid_id: 562_947_905_631_053,
            bid_id_str: Some("562947905631053".to_string()),
            ask_client_id: 0,
            ask_client_id_str: Some("0".to_string()),
            bid_client_id: 7_001_011_966,
            bid_client_id_str: Some("7001011966".to_string()),
            ask_account_id: if user_is_bidder {
                91_249
            } else {
                account_index
            },
            bid_account_id: if user_is_bidder {
                account_index
            } else {
                91_249
            },
            is_maker_ask,
            block_height: 227_535_535,
            timestamp: 1_777_941_384_181,
            taker_fee: Some(196),
            taker_position_size_before: None,
            taker_entry_quote_before: None,
            taker_initial_margin_fraction_before: None,
            taker_position_sign_changed: None,
            maker_fee: Some(28),
            maker_position_size_before: None,
            maker_entry_quote_before: None,
            maker_initial_margin_fraction_before: None,
            maker_position_sign_changed: None,
            transaction_time: 1_777_941_384_181_586,
            ask_account_pnl: None,
            bid_account_pnl: None,
        }
    }

    #[rstest]
    fn test_parse_ws_order_status_report_partial_fill_promotes_status() {
        let instrument = create_test_instrument();
        let order = stub_order(LighterOrderStatus::Open);

        let report =
            parse_ws_order_status_report(&order, &instrument, account_id(), UnixNanos::from(7))
                .unwrap();

        assert_eq!(report.venue_order_id.to_string(), "281476929510110");
        assert_eq!(report.client_order_id.unwrap().to_string(), "42");
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.order_type, OrderType::Limit);
        // Open + filled_qty > 0 must surface as PartiallyFilled.
        assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(report.filled_qty, Quantity::from("0.0020"));
        assert_eq!(report.quantity, Quantity::from("0.0050"));
        assert_eq!(report.price, Some(Price::from("2352.74")));
        assert_eq!(report.trigger_price, None);
        assert_eq!(report.time_in_force, TimeInForce::Gtd);
        assert!(report.expire_time.is_some());
        assert_eq!(report.ts_init, UnixNanos::from(7));
    }

    #[rstest]
    fn test_parse_ws_order_status_report_post_only_cancel_is_rejected() {
        let instrument = create_test_instrument();
        let order = stub_order(LighterOrderStatus::CanceledPostOnly);

        let report =
            parse_ws_order_status_report(&order, &instrument, account_id(), UnixNanos::from(1))
                .unwrap();

        assert_eq!(report.order_status, OrderStatus::Rejected);
        assert_eq!(report.cancel_reason.as_deref(), Some("post-only"));
    }

    #[rstest]
    fn test_parse_ws_order_status_report_falls_back_to_is_ask() {
        let instrument = create_test_instrument();
        let mut order = stub_order(LighterOrderStatus::Open);
        order.side = None;
        order.is_ask = false;

        let report =
            parse_ws_order_status_report(&order, &instrument, account_id(), UnixNanos::from(7))
                .unwrap();

        assert_eq!(report.order_side, OrderSide::Buy);
    }

    #[rstest]
    fn test_parse_ws_order_status_report_omits_parent_order_id() {
        // Lighter's `parent_order_id` lives in the venue namespace; populating
        // Nautilus `OrderStatusReport::parent_order_id` (a `ClientOrderId`)
        // from it would mislabel namespaces. This guards against re-introducing
        // that mapping.
        let instrument = create_test_instrument();
        let mut order = stub_order(LighterOrderStatus::Open);
        order.parent_order_id = "999999".to_string();
        order.parent_order_index = 999_999;

        let report =
            parse_ws_order_status_report(&order, &instrument, account_id(), UnixNanos::from(1))
                .unwrap();

        assert!(report.parent_order_id.is_none());
        assert_eq!(report.contingency_type, ContingencyType::NoContingency);
    }

    #[rstest]
    #[case::stop_market(LighterOrderKind::StopLoss, OrderType::StopMarket)]
    #[case::stop_limit(LighterOrderKind::StopLossLimit, OrderType::StopLimit)]
    #[case::market_if_touched(LighterOrderKind::TakeProfit, OrderType::MarketIfTouched)]
    #[case::limit_if_touched(LighterOrderKind::TakeProfitLimit, OrderType::LimitIfTouched)]
    fn test_parse_ws_order_status_report_conditional_sets_default_trigger_type(
        #[case] lighter_order_type: LighterOrderKind,
        #[case] expected_order_type: OrderType,
    ) {
        let instrument = create_test_instrument();
        let mut order = stub_order(LighterOrderStatus::Open);
        order.order_type = lighter_order_type;
        order.trigger_price = Decimal::from_str("2200.00").unwrap();

        let report =
            parse_ws_order_status_report(&order, &instrument, account_id(), UnixNanos::from(1))
                .unwrap();

        assert_eq!(report.order_type, expected_order_type);
        assert_eq!(report.trigger_price, Some(Price::from("2200.00")));
        assert_eq!(report.trigger_type, Some(TriggerType::Default));
    }

    // Lighter overloads `good-till-time` for both true GTD (positive expiry)
    // and venue-default GTC (`order_expiry <= 0`). PostOnly maps to Gtc plus
    // the post_only flag because Nautilus has no PostOnly TIF. This matrix
    // pins each combination so silent regressions in nautilus_time_in_force
    // surface immediately.
    #[rstest]
    #[case::ioc(
        LighterOrderTimeInForce::ImmediateOrCancel,
        0,
        TimeInForce::Ioc,
        false,
        false
    )]
    #[case::post_only(LighterOrderTimeInForce::PostOnly, 0, TimeInForce::Gtc, false, true)]
    #[case::gtt_negative_expiry(LighterOrderTimeInForce::GoodTillTime, -1, TimeInForce::Gtc, false, false)]
    #[case::gtt_zero_expiry(
        LighterOrderTimeInForce::GoodTillTime,
        0,
        TimeInForce::Gtc,
        false,
        false
    )]
    #[case::gtt_positive_expiry(
        LighterOrderTimeInForce::GoodTillTime,
        1_780_000_000_000,
        TimeInForce::Gtd,
        true,
        false
    )]
    #[case::unknown(LighterOrderTimeInForce::Unknown, 0, TimeInForce::Gtc, false, false)]
    fn test_parse_ws_order_status_report_time_in_force_matrix(
        #[case] tif: LighterOrderTimeInForce,
        #[case] order_expiry: i64,
        #[case] expected_tif: TimeInForce,
        #[case] expects_expire_time: bool,
        #[case] expected_post_only: bool,
    ) {
        let instrument = create_test_instrument();
        let mut order = stub_order(LighterOrderStatus::Open);
        order.time_in_force = tif;
        order.order_expiry = order_expiry;

        let report =
            parse_ws_order_status_report(&order, &instrument, account_id(), UnixNanos::from(1))
                .unwrap();

        assert_eq!(report.time_in_force, expected_tif);
        assert_eq!(report.expire_time.is_some(), expects_expire_time);
        assert_eq!(report.post_only, expected_post_only);
    }

    #[rstest]
    fn test_parse_ws_order_status_report_rejects_twap() {
        let instrument = create_test_instrument();
        let mut order = stub_order(LighterOrderStatus::Open);
        order.order_type = LighterOrderKind::Twap;

        let err =
            parse_ws_order_status_report(&order, &instrument, account_id(), UnixNanos::from(1))
                .unwrap_err();

        assert!(
            err.to_string()
                .contains("no Nautilus order-type equivalent")
        );
    }

    // Liquidity side is decided by `user_is_asker == trade.is_maker_ask`:
    // when the user sat on the same side that rested as the maker, they
    // were the maker; otherwise they crossed the book as the taker. The
    // four combinations exhaustively cover that branch.
    #[rstest]
    #[case::bidder_maker_ask_is_taker(
        true,
        true,
        OrderSide::Buy,
        LiquiditySide::Taker,
        "0.000196 USDC"
    )]
    #[case::asker_maker_ask_is_maker(
        false,
        true,
        OrderSide::Sell,
        LiquiditySide::Maker,
        "0.000028 USDC"
    )]
    #[case::bidder_maker_bid_is_maker(
        true,
        false,
        OrderSide::Buy,
        LiquiditySide::Maker,
        "0.000028 USDC"
    )]
    #[case::asker_maker_bid_is_taker(
        false,
        false,
        OrderSide::Sell,
        LiquiditySide::Taker,
        "0.000196 USDC"
    )]
    fn test_parse_ws_fill_report_liquidity_side_matrix(
        #[case] user_is_bidder: bool,
        #[case] is_maker_ask: bool,
        #[case] expected_side: OrderSide,
        #[case] expected_liquidity: LiquiditySide,
        #[case] expected_commission: &str,
    ) {
        let instrument = create_test_instrument();
        let trade = stub_account_trade(1234, is_maker_ask, user_is_bidder);

        let report =
            parse_ws_fill_report(&trade, 1234, &instrument, account_id(), UnixNanos::from(9))
                .unwrap()
                .expect("user-side fill");

        assert_eq!(report.order_side, expected_side);
        assert_eq!(report.liquidity_side, expected_liquidity);
        assert_eq!(report.last_qty, Quantity::from("0.1336"));
        assert_eq!(report.last_px, Price::from("2352.73"));
        assert_eq!(report.commission, Money::from(expected_commission));
        let expected_voi = if user_is_bidder {
            "562947905631053"
        } else {
            "281476929510102"
        };
        assert_eq!(report.venue_order_id.to_string(), expected_voi);
    }

    #[rstest]
    fn test_parse_ws_fill_report_skips_other_accounts() {
        let instrument = create_test_instrument();
        let trade = stub_account_trade(9999, false, true);

        let report =
            parse_ws_fill_report(&trade, 1234, &instrument, account_id(), UnixNanos::from(1))
                .unwrap();

        assert!(report.is_none());
    }

    // When the venue drops the `*_id_str` field (the typed numeric fields are
    // always populated), the parser must fall back to stringifying the i64
    // mirror to seed VenueOrderId. Pins the numeric-fallback branch in
    // `venue_order_id_from`; the matrix above always populates the strings.
    #[rstest]
    fn test_parse_ws_fill_report_venue_order_id_falls_back_to_numeric() {
        let instrument = create_test_instrument();
        let mut trade = stub_account_trade(1234, false, true);
        trade.bid_id_str = None;
        trade.ask_id_str = None;

        let report =
            parse_ws_fill_report(&trade, 1234, &instrument, account_id(), UnixNanos::from(1))
                .unwrap()
                .expect("user-side fill");

        // user_is_bidder=true => VenueOrderId derives from bid_id.
        assert_eq!(report.venue_order_id.to_string(), "562947905631053");
    }

    // Same fallback contract for `client_order_id_from`: when `*_client_id_str`
    // is absent but the numeric mirror is non-zero, the cloid must surface as
    // the numeric value; when both string is absent and numeric is zero, the
    // cloid is `None`.
    #[rstest]
    fn test_parse_ws_fill_report_client_order_id_falls_back_to_numeric() {
        let instrument = create_test_instrument();
        let mut trade = stub_account_trade(1234, false, true);
        trade.bid_client_id_str = None;
        trade.ask_client_id_str = None;

        let report =
            parse_ws_fill_report(&trade, 1234, &instrument, account_id(), UnixNanos::from(1))
                .unwrap()
                .expect("user-side fill");

        // user_is_bidder=true with bid_client_id=7_001_011_966 (non-zero).
        assert_eq!(report.client_order_id.unwrap().to_string(), "7001011966");
    }

    #[rstest]
    fn test_parse_ws_fill_report_client_order_id_absent_when_zero_numeric() {
        let instrument = create_test_instrument();
        // user_is_bidder=false => looks at ask_client_id, which is 0 in the stub.
        let mut trade = stub_account_trade(1234, true, false);
        trade.ask_client_id_str = None;

        let report =
            parse_ws_fill_report(&trade, 1234, &instrument, account_id(), UnixNanos::from(1))
                .unwrap()
                .expect("user-side fill");

        assert!(report.client_order_id.is_none());
    }

    #[rstest]
    fn test_parse_ws_fill_report_client_order_id_absent_when_string_is_zero_sentinel() {
        let instrument = create_test_instrument();
        // user_is_bidder=false => looks at ask_client_id_str, which is "0" in the stub.
        let trade = stub_account_trade(1234, true, false);

        let report =
            parse_ws_fill_report(&trade, 1234, &instrument, account_id(), UnixNanos::from(1))
                .unwrap()
                .expect("user-side fill");

        assert!(report.client_order_id.is_none());
    }

    #[rstest]
    fn test_parse_ws_fill_report_handles_missing_fee() {
        let instrument = create_test_instrument();
        let mut trade = stub_account_trade(1234, true, true);
        trade.taker_fee = None;
        trade.maker_fee = None;

        let report =
            parse_ws_fill_report(&trade, 1234, &instrument, account_id(), UnixNanos::from(1))
                .unwrap()
                .expect("user-side fill");

        assert_eq!(report.commission, Money::from("0 USDC"));
    }

    #[rstest]
    fn test_parse_ws_position_status_report_long_position() {
        let instrument = create_test_instrument();
        let position = LighterPosition {
            market_id: 0,
            symbol: Ustr::from("ETH"),
            initial_margin_fraction: Decimal::from_str("0.0500").unwrap(),
            open_order_count: 1,
            pending_order_count: 0,
            position_tied_order_count: 0,
            sign: 1,
            position: Decimal::from_str("1.5000").unwrap(),
            avg_entry_price: Decimal::from_str("2350.10").unwrap(),
            position_value: Decimal::from_str("3525.15").unwrap(),
            unrealized_pnl: Decimal::from_str("3.45").unwrap(),
            realized_pnl: Decimal::ZERO,
            liquidation_price: Decimal::from_str("1900.00").unwrap(),
            total_funding_paid_out: Some(Decimal::from_str("0.05").unwrap()),
            margin_mode: 0,
            allocated_margin: Decimal::from_str("176.25").unwrap(),
            total_discount: Some(Decimal::ZERO),
        };

        let report = parse_ws_position_status_report(
            &position,
            &instrument,
            account_id(),
            UnixNanos::from(50),
            UnixNanos::from(50),
        )
        .unwrap();

        assert_eq!(report.position_side, PositionSideSpecified::Long);
        assert_eq!(report.quantity, Quantity::from("1.5000"));
        assert_eq!(report.signed_decimal_qty, Decimal::new(15, 1));
        assert_eq!(report.avg_px_open, Some(Decimal::new(235010, 2)));
        assert!(report.venue_position_id.is_none());
    }

    #[rstest]
    fn test_parse_ws_position_status_report_short_position() {
        let instrument = create_test_instrument();
        let position = LighterPosition {
            market_id: 0,
            symbol: Ustr::from("ETH"),
            initial_margin_fraction: Decimal::from_str("0.0500").unwrap(),
            open_order_count: 0,
            pending_order_count: 0,
            position_tied_order_count: 0,
            sign: -1,
            position: Decimal::from_str("0.7500").unwrap(),
            avg_entry_price: Decimal::from_str("2400.00").unwrap(),
            position_value: Decimal::from_str("1800.00").unwrap(),
            unrealized_pnl: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            liquidation_price: Decimal::from_str("3000.00").unwrap(),
            total_funding_paid_out: None,
            margin_mode: 0,
            allocated_margin: Decimal::from_str("90.00").unwrap(),
            total_discount: None,
        };

        let report = parse_ws_position_status_report(
            &position,
            &instrument,
            account_id(),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.position_side, PositionSideSpecified::Short);
        assert_eq!(report.quantity, Quantity::from("0.7500"));
        assert_eq!(report.signed_decimal_qty, Decimal::new(-75, 2));
    }

    #[rstest]
    fn test_parse_ws_position_status_report_flat_position() {
        let instrument = create_test_instrument();
        let position = LighterPosition {
            market_id: 0,
            symbol: Ustr::from("ETH"),
            initial_margin_fraction: Decimal::from_str("0.0500").unwrap(),
            open_order_count: 0,
            pending_order_count: 0,
            position_tied_order_count: 0,
            sign: 0,
            position: Decimal::ZERO,
            avg_entry_price: Decimal::ZERO,
            position_value: Decimal::ZERO,
            unrealized_pnl: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            liquidation_price: Decimal::ZERO,
            total_funding_paid_out: None,
            margin_mode: 0,
            allocated_margin: Decimal::ZERO,
            total_discount: None,
        };

        let report = parse_ws_position_status_report(
            &position,
            &instrument,
            account_id(),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.position_side, PositionSideSpecified::Flat);
        assert!(report.quantity.is_zero());
        assert_eq!(report.signed_decimal_qty, Decimal::ZERO);
        assert!(report.avg_px_open.is_none());
    }

    #[rstest]
    fn test_account_balance_from_lighter_asset_spot_only() {
        // Asset with only a spot balance (margin_balance=0): perp leg is
        // empty so total == spot balance, locked == locked_balance only.
        let asset = LighterAsset {
            symbol: Ustr::from("USDC"),
            asset_id: 0,
            balance: Decimal::from_str("100.000000").unwrap(),
            locked_balance: Decimal::from_str("1.000000").unwrap(),
            margin_balance: Decimal::ZERO,
            margin_mode: Ustr::default(),
        };

        let balance = account_balance_from_lighter_asset(&asset).unwrap();
        let usdc = Currency::get_or_create_crypto("USDC");
        assert_eq!(balance.currency, usdc);
        assert_eq!(balance.total, Money::from("100.000000 USDC"));
        assert_eq!(balance.locked, Money::from("1.000000 USDC"));
        assert_eq!(balance.free, Money::from("99.000000 USDC"));
    }

    #[rstest]
    fn test_account_balance_from_lighter_asset_combines_spot_and_perp() {
        // Worked example: 10 USDC sitting on spot, 40 USDC pledged as
        // perp collateral, no resting spot orders. Lighter runs unified
        // margin: both legs are deployable equity, so the merged view
        // is total=50, locked=0, free=50. Perp margin currently in use
        // is tracked separately via MarginBalance, not via locked here.
        let asset = LighterAsset {
            symbol: Ustr::from("USDC"),
            asset_id: 3,
            balance: Decimal::from_str("10.000000").unwrap(),
            locked_balance: Decimal::ZERO,
            margin_balance: Decimal::from_str("40.000000").unwrap(),
            margin_mode: Ustr::from("disabled"),
        };

        let balance = account_balance_from_lighter_asset(&asset).unwrap();
        assert_eq!(balance.total, Money::from("50.000000 USDC"));
        assert_eq!(balance.locked, Money::from("0 USDC"));
        assert_eq!(balance.free, Money::from("50.000000 USDC"));
    }

    #[rstest]
    fn test_account_balance_from_lighter_asset_locks_only_spot_order_reservation() {
        // A resting spot limit order locks 1 USDC. total still reflects
        // both legs (10 + 40 = 50); locked tracks the spot reservation
        // only, free = 49.
        let asset = LighterAsset {
            symbol: Ustr::from("USDC"),
            asset_id: 3,
            balance: Decimal::from_str("10.000000").unwrap(),
            locked_balance: Decimal::from_str("1.000000").unwrap(),
            margin_balance: Decimal::from_str("40.000000").unwrap(),
            margin_mode: Ustr::from("disabled"),
        };

        let balance = account_balance_from_lighter_asset(&asset).unwrap();
        assert_eq!(balance.total, Money::from("50.000000 USDC"));
        assert_eq!(balance.locked, Money::from("1.000000 USDC"));
        assert_eq!(balance.free, Money::from("49.000000 USDC"));
    }

    #[rstest]
    fn test_margin_balance_from_user_stats_no_positions() {
        // 40 USDC collateral, 40 available, no positions open: both initial
        // and maintenance must be zero; strategies should see "full
        // collateral free to deploy".
        let stats = LighterUserStats {
            account_trading_mode: 0,
            available_balance: Decimal::from_str("40.000000").unwrap(),
            buying_power: Decimal::ZERO,
            collateral: Decimal::from_str("40.000000").unwrap(),
            leverage: Decimal::ZERO,
            margin_usage: Decimal::ZERO,
            portfolio_value: Decimal::from_str("40.000000").unwrap(),
            cross_stats: None,
            total_stats: None,
        };

        let margin = margin_balance_from_user_stats(&stats).unwrap();
        let usdc = Currency::get_or_create_crypto("USDC");
        assert_eq!(margin.currency, usdc);
        assert_eq!(margin.initial, Money::from("0 USDC"));
        assert_eq!(margin.maintenance, Money::from("0 USDC"));
        assert_eq!(margin.instrument_id, None);
    }

    #[rstest]
    fn test_margin_balance_from_user_stats_with_position() {
        // 40 USDC collateral, 35 available -> 5 USDC initial margin in use.
        // Maintenance is always 0 here: Lighter's `margin_usage` is an
        // initial-margin-usage percent, not a maintenance ratio, so we
        // don't derive maintenance from `user_stats` at all (see comment
        // on `margin_balance_from_user_stats`).
        let stats = LighterUserStats {
            account_trading_mode: 0,
            available_balance: Decimal::from_str("35.000000").unwrap(),
            buying_power: Decimal::from_str("100.000000").unwrap(),
            collateral: Decimal::from_str("40.000000").unwrap(),
            leverage: Decimal::from_str("5.00").unwrap(),
            margin_usage: Decimal::from_str("12.50").unwrap(),
            portfolio_value: Decimal::from_str("40.000000").unwrap(),
            cross_stats: None,
            total_stats: None,
        };

        let margin = margin_balance_from_user_stats(&stats).unwrap();
        assert_eq!(margin.initial, Money::from("5.000000 USDC"));
        assert_eq!(margin.maintenance, Money::from("0 USDC"));
    }

    #[rstest]
    fn test_build_unified_account_state_emits_margin_account() {
        let asset = LighterAsset {
            symbol: Ustr::from("USDC"),
            asset_id: 3,
            balance: Decimal::from_str("10.000000").unwrap(),
            locked_balance: Decimal::ZERO,
            margin_balance: Decimal::from_str("40.000000").unwrap(),
            margin_mode: Ustr::from("disabled"),
        };
        let balances = vec![account_balance_from_lighter_asset(&asset).unwrap()];

        let stats = LighterUserStats {
            account_trading_mode: 0,
            available_balance: Decimal::from_str("40.000000").unwrap(),
            buying_power: Decimal::ZERO,
            collateral: Decimal::from_str("40.000000").unwrap(),
            leverage: Decimal::ZERO,
            margin_usage: Decimal::ZERO,
            portfolio_value: Decimal::from_str("40.000000").unwrap(),
            cross_stats: None,
            total_stats: None,
        };
        let margin = margin_balance_from_user_stats(&stats).unwrap();

        let state = build_unified_account_state(
            balances,
            Some(margin),
            account_id(),
            UnixNanos::from(1_000),
            UnixNanos::from(1_001),
        );

        let usdc = Currency::get_or_create_crypto("USDC");
        assert_eq!(state.account_id, account_id());
        assert_eq!(state.account_type, AccountType::Margin);
        assert_eq!(state.base_currency, None);
        assert!(state.is_reported);
        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.balances[0].total, Money::from("50.000000 USDC"));
        assert_eq!(state.balances[0].locked, Money::from("0 USDC"));
        assert_eq!(state.balances[0].free, Money::from("50.000000 USDC"));
        assert_eq!(state.margins.len(), 1);
        assert_eq!(state.margins[0].currency, usdc);
        assert_eq!(state.margins[0].initial, Money::from("0 USDC"));
        assert!(state.margins[0].instrument_id.is_none());
    }

    fn stub_candle() -> LighterWsCandle {
        LighterWsCandle {
            t: 1_778_821_440_000,
            o: Decimal::new(226_420, 2),
            h: Decimal::new(226_434, 2),
            l: Decimal::new(226_336, 2),
            c: Decimal::new(226_397, 2),
            v: Decimal::new(132_237, 4),
            quote_volume: Decimal::ZERO,
            i: 0,
        }
    }

    #[rstest]
    fn test_parse_ws_bar_emits_open_timestamp_and_external_last_spec() {
        let instrument = create_test_instrument();
        let candle = stub_candle();

        let bar = parse_ws_bar(
            &instrument,
            &candle,
            LighterCandleResolution::OneMinute,
            UnixNanos::from(99_999),
        )
        .unwrap();

        assert_eq!(bar.bar_type.instrument_id(), instrument.id());
        assert_eq!(bar.bar_type.spec().step.get(), 1);
        assert_eq!(bar.bar_type.spec().aggregation, BarAggregation::Minute);
        assert_eq!(bar.bar_type.spec().price_type, PriceType::Last);
        assert_eq!(
            bar.bar_type.aggregation_source(),
            AggregationSource::External
        );
        assert_eq!(bar.open, Price::from("2264.20"));
        assert_eq!(bar.high, Price::from("2264.34"));
        assert_eq!(bar.low, Price::from("2263.36"));
        assert_eq!(bar.close, Price::from("2263.97"));
        assert_eq!(bar.volume, Quantity::from("13.2237"));
        assert_eq!(bar.ts_event, UnixNanos::from(1_778_821_440_000_000_000));
        assert_eq!(bar.ts_init, UnixNanos::from(99_999));
    }

    #[rstest]
    fn test_parse_ws_bar_rejects_negative_timestamp() {
        let instrument = create_test_instrument();
        let mut candle = stub_candle();
        candle.t = -1;

        let err = parse_ws_bar(
            &instrument,
            &candle,
            LighterCandleResolution::OneMinute,
            UnixNanos::default(),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("negative candle timestamp"),
            "expected negative-timestamp error, was: {err}",
        );
    }

    fn test_identity() -> OrderIdentity {
        OrderIdentity {
            instrument_id: create_test_instrument().id(),
            strategy_id: StrategyId::new("S-TEST"),
            order_side: OrderSide::Sell,
            order_type: OrderType::Limit,
        }
    }

    fn test_trader_id() -> TraderId {
        TraderId::new("TRADER-001")
    }

    fn test_cloid() -> ClientOrderId {
        ClientOrderId::new("MY-ORDER-001")
    }

    #[rstest]
    fn parse_lighter_order_event_emits_accepted_on_open() {
        let instrument = create_test_instrument();
        let mut order = stub_order(LighterOrderStatus::Open);
        order.filled_base_amount = Decimal::ZERO;

        let event = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: false,
                triggered_already_emitted: false,
                shape_changed: false,
            },
            UnixNanos::from(7),
        )
        .unwrap()
        .expect("Open with no prior accept emits Accepted");

        match event {
            ParsedOrderEvent::Accepted(e) => {
                assert_eq!(e.client_order_id, test_cloid());
                assert_eq!(e.venue_order_id.to_string(), "281476929510110");
            }
            other => panic!("expected Accepted, was {other:?}"),
        }
    }

    #[rstest]
    fn parse_lighter_order_event_emits_updated_only_when_shape_changed() {
        // Lighter's modify-as-restate: the venue echoes the modified order
        // as `Open`. Updated must fire only when the shape (qty / price /
        // trigger) actually changed; otherwise repeat `Open` echoes
        // (partial-fill snapshots, reconnect replays) would mint spurious
        // modify events.
        let instrument = create_test_instrument();
        let order = stub_order(LighterOrderStatus::Open);

        let event = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: true,
                triggered_already_emitted: false,
                shape_changed: true,
            },
            UnixNanos::from(7),
        )
        .unwrap()
        .expect("Open with shape_changed emits Updated");

        match event {
            ParsedOrderEvent::Updated(e) => {
                assert_eq!(e.client_order_id, test_cloid());
                assert_eq!(e.quantity, Quantity::from("0.0050"));
                assert_eq!(e.price, Some(Price::from("2352.74")));
            }
            other => panic!("expected Updated, was {other:?}"),
        }
    }

    #[rstest]
    fn parse_lighter_order_event_repeat_open_after_accept_is_silent() {
        // Without a shape change, a repeat `Open` for an already-accepted
        // tracked order must return `None` so partial-fill snapshots and
        // reconnect replays do not flood the engine with phantom updates.
        let instrument = create_test_instrument();
        let order = stub_order(LighterOrderStatus::Open);

        let event = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: true,
                triggered_already_emitted: false,
                shape_changed: false,
            },
            UnixNanos::from(7),
        )
        .unwrap();

        assert!(event.is_none());
    }

    #[rstest]
    fn parse_lighter_order_event_triggered_dedup_via_open_ctx() {
        // A subsequent `Open` frame after `Triggered` already fired must
        // not re-emit `Triggered`. The dispatcher tracks this via the
        // `triggered_already_emitted` flag.
        let instrument = create_test_instrument();
        let mut order = stub_order(LighterOrderStatus::Open);
        order.trigger_status = LighterTriggerStatus::Ready;

        let event = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: true,
                triggered_already_emitted: true,
                shape_changed: false,
            },
            UnixNanos::from(7),
        )
        .unwrap();

        // After triggered_already_emitted and accepted_already_emitted,
        // with no shape change, the frame is silent.
        assert!(event.is_none());
    }

    #[rstest]
    fn parse_lighter_order_event_emits_triggered_after_accept() {
        // A conditional order's trigger can fire AFTER the initial accept
        // landed. The Open frame's `trigger_status = Ready` must produce
        // `Triggered` regardless of `accepted_already_emitted`.
        let instrument = create_test_instrument();
        let mut order = stub_order(LighterOrderStatus::Open);
        order.trigger_status = LighterTriggerStatus::Ready;

        let event = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: true,
                triggered_already_emitted: false,
                shape_changed: false,
            },
            UnixNanos::from(7),
        )
        .unwrap()
        .expect("trigger Ready after accept emits Triggered");

        match event {
            ParsedOrderEvent::Triggered(_) => {}
            other => panic!("expected Triggered, was {other:?}"),
        }
    }

    #[rstest]
    fn parse_lighter_order_event_emits_triggered_when_trigger_ready_fresh() {
        let instrument = create_test_instrument();
        let mut order = stub_order(LighterOrderStatus::Open);
        order.filled_base_amount = Decimal::ZERO;
        order.trigger_status = LighterTriggerStatus::Ready;

        let event_fresh = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: false,
                triggered_already_emitted: false,
                shape_changed: false,
            },
            UnixNanos::from(7),
        )
        .unwrap()
        .expect("trigger_status=Ready on fresh open emits Triggered");

        match event_fresh {
            ParsedOrderEvent::Triggered(_) => {}
            other => panic!("expected Triggered for fresh ready trigger, was {other:?}"),
        }
    }

    #[rstest]
    fn lighter_order_shape_round_trips_values() {
        let instrument = create_test_instrument();
        let order = stub_order(LighterOrderStatus::Open);

        let shape = lighter_order_shape(&order, &instrument).unwrap();

        assert_eq!(shape.quantity, Quantity::from("0.0050"));
        assert_eq!(shape.price, Some(Price::from("2352.74")));
        assert_eq!(shape.trigger_price, None);
    }

    #[rstest]
    fn lighter_order_shape_distinguishes_modified_payload() {
        let instrument = create_test_instrument();
        let original = stub_order(LighterOrderStatus::Open);
        let mut modified = original.clone();
        modified.price = Decimal::from_str("2400.00").unwrap();

        let shape_original = lighter_order_shape(&original, &instrument).unwrap();
        let shape_modified = lighter_order_shape(&modified, &instrument).unwrap();

        assert_ne!(shape_original, shape_modified);
    }

    #[rstest]
    fn parse_lighter_order_event_emits_rejected_for_post_only_cancel() {
        let instrument = create_test_instrument();
        let order = stub_order(LighterOrderStatus::CanceledPostOnly);

        let event = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: false,
                triggered_already_emitted: false,
                shape_changed: false,
            },
            UnixNanos::from(7),
        )
        .unwrap()
        .expect("post-only cancel emits Rejected");

        match event {
            ParsedOrderEvent::Rejected(e) => {
                assert!(e.due_post_only);
                assert_eq!(e.reason.as_str(), "post-only");
            }
            other => panic!("expected Rejected, was {other:?}"),
        }
    }

    #[rstest]
    fn parse_lighter_order_event_emits_expired_for_canceled_expired() {
        let instrument = create_test_instrument();
        let order = stub_order(LighterOrderStatus::CanceledExpired);

        let event = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: false,
                triggered_already_emitted: false,
                shape_changed: false,
            },
            UnixNanos::from(7),
        )
        .unwrap()
        .expect("canceled-expired emits Expired");

        match event {
            ParsedOrderEvent::Expired(_) => {}
            other => panic!("expected Expired, was {other:?}"),
        }
    }

    #[rstest]
    #[case::canceled(LighterOrderStatus::Canceled)]
    #[case::reduce_only(LighterOrderStatus::CanceledReduceOnly)]
    #[case::self_trade(LighterOrderStatus::CanceledSelfTrade)]
    #[case::liquidation(LighterOrderStatus::CanceledLiquidation)]
    fn parse_lighter_order_event_emits_canceled_for_other_cancel_variants(
        #[case] status: LighterOrderStatus,
    ) {
        let instrument = create_test_instrument();
        let order = stub_order(status);

        let event = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: false,
                triggered_already_emitted: false,
                shape_changed: false,
            },
            UnixNanos::from(7),
        )
        .unwrap()
        .expect("cancel variant emits Canceled");

        match event {
            ParsedOrderEvent::Canceled(_) => {}
            other => panic!("expected Canceled, was {other:?}"),
        }
    }

    #[rstest]
    #[case::in_progress(LighterOrderStatus::InProgress)]
    #[case::pending(LighterOrderStatus::Pending)]
    #[case::filled(LighterOrderStatus::Filled)]
    fn parse_lighter_order_event_returns_none_for_silent_statuses(
        #[case] status: LighterOrderStatus,
    ) {
        // Transitional statuses (InProgress/Pending) carry no actionable
        // event; Filled flows through the trade stream and produces
        // `OrderFilled` from `parse_lighter_order_filled`.
        let instrument = create_test_instrument();
        let order = stub_order(status);

        let event = parse_lighter_order_event(
            &order,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            OpenFrameContext {
                accepted_already_emitted: false,
                triggered_already_emitted: false,
                shape_changed: false,
            },
            UnixNanos::from(7),
        )
        .unwrap();

        assert!(event.is_none(), "expected None for {status:?}");
    }

    #[rstest]
    fn parse_lighter_order_filled_builds_order_filled_for_account() {
        let instrument = create_test_instrument();
        let trade = stub_account_trade(1234, true, true);

        let filled = parse_lighter_order_filled(
            &trade,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            1234,
            UnixNanos::from(7),
        )
        .unwrap()
        .expect("trade involving account emits OrderFilled");

        assert_eq!(filled.client_order_id, test_cloid());
        assert_eq!(filled.order_side, OrderSide::Sell); // identity wins
        assert_eq!(filled.order_type, OrderType::Limit);
        assert_eq!(filled.last_qty, Quantity::from("0.1336"));
        assert_eq!(filled.last_px, Price::from("2352.73"));
        assert!(filled.commission.is_some());
    }

    #[rstest]
    fn parse_lighter_order_filled_returns_none_for_other_account() {
        let instrument = create_test_instrument();
        // account_index does not appear on either side of the trade.
        let trade = stub_account_trade(1234, true, true);

        let filled = parse_lighter_order_filled(
            &trade,
            &instrument,
            &test_identity(),
            test_cloid(),
            account_id(),
            test_trader_id(),
            99_999, // mismatched account_index
            UnixNanos::from(7),
        )
        .unwrap();

        assert!(filled.is_none());
    }
}
