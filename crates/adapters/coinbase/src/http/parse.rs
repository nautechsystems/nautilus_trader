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

//! Parsing functions for converting Coinbase API responses to Nautilus domain types.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{
        AccountType, AggressorSide, BookAction, LiquiditySide, OrderSide, OrderStatus, OrderType,
        PositionSideSpecified, RecordFlag, TimeInForce, TriggerType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{CryptoFuture, CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{
            COINBASE_VENUE, ORDER_CONFIG_BASE_SIZE, ORDER_CONFIG_END_TIME,
            ORDER_CONFIG_LIMIT_PRICE, ORDER_CONFIG_POST_ONLY, ORDER_CONFIG_STOP_PRICE,
        },
        enums::{
            CoinbaseContractExpiryType, CoinbaseFcmPositionSide, CoinbaseLiquidityIndicator,
            CoinbaseOrderSide, CoinbaseOrderStatus, CoinbaseOrderType, CoinbaseProductType,
            CoinbaseTimeInForce,
        },
    },
    http::models::{
        Account, BookLevel, Candle, CfmBalanceSummary, CfmPosition, Fill, Order, PriceBook,
        Product, Trade,
    },
    websocket::messages::WsFcmBalanceSummary,
};

/// Parses an RFC 3339 timestamp string to `UnixNanos`.
pub fn parse_rfc3339_timestamp(timestamp: &str) -> anyhow::Result<UnixNanos> {
    let dt = chrono::DateTime::parse_from_rfc3339(timestamp)
        .context(format!("Failed to parse timestamp '{timestamp}'"))?;
    let nanos = dt
        .timestamp_nanos_opt()
        .context(format!("Timestamp out of range: '{timestamp}'"))?;
    anyhow::ensure!(nanos >= 0, "Negative timestamp: '{timestamp}'");
    Ok(UnixNanos::from(nanos as u64))
}

/// Parses a Unix epoch seconds string to `UnixNanos`.
pub fn parse_epoch_secs_timestamp(epoch_secs: &str) -> anyhow::Result<UnixNanos> {
    let secs: u64 = epoch_secs
        .parse()
        .context(format!("Failed to parse epoch seconds '{epoch_secs}'"))?;
    Ok(UnixNanos::from(secs * 1_000_000_000))
}

/// Parses a price string with the given precision.
pub fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    let decimal = Decimal::from_str(value).context(format!("Failed to parse price '{value}'"))?;
    Price::from_decimal_dp(decimal, precision).context(format!(
        "Failed to create Price from '{value}' with precision {precision}"
    ))
}

/// Parses a quantity string with the given precision.
pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    let decimal =
        Decimal::from_str(value).context(format!("Failed to parse quantity '{value}'"))?;
    Quantity::from_decimal_dp(decimal, precision).context(format!(
        "Failed to create Quantity from '{value}' with precision {precision}"
    ))
}

/// Derives precision (number of decimal places) from an increment string.
///
/// For example, `"0.01"` returns 2, `"0.00000001"` returns 8, `"1"` returns 0.
pub fn precision_from_increment(increment: &str) -> u8 {
    match increment.find('.') {
        Some(pos) => {
            let decimals = &increment[pos + 1..];
            let trimmed_len = decimals.trim_end_matches('0').len();
            let min = usize::from(!decimals.chars().all(|c| c == '0'));
            trimmed_len.max(min) as u8
        }
        None => 0,
    }
}

/// Converts a Coinbase order side to a Nautilus aggressor side.
pub fn coinbase_side_to_aggressor(side: &CoinbaseOrderSide) -> AggressorSide {
    match side {
        CoinbaseOrderSide::Buy => AggressorSide::Buyer,
        CoinbaseOrderSide::Sell => AggressorSide::Seller,
        CoinbaseOrderSide::Unknown => AggressorSide::NoAggressor,
    }
}

/// Parses an optional quantity from a string, returning `None` for empty,
/// zero, or values that exceed Nautilus's `QUANTITY_RAW_MAX`.
fn parse_optional_quantity(value: &str) -> Option<Quantity> {
    if value.is_empty() || value == "0" {
        None
    } else {
        Quantity::from_str(value).ok()
    }
}

/// Derives the base currency from the product, falling back to the first word
/// in `display_name` when `base_currency_id` is empty (Coinbase futures).
fn derive_base_currency(product: &Product) -> Currency {
    if product.base_currency_id.is_empty() {
        let base_str = product
            .display_name
            .split_whitespace()
            .next()
            .unwrap_or("UNKNOWN");
        Currency::get_or_create_crypto(base_str)
    } else {
        Currency::get_or_create_crypto(product.base_currency_id)
    }
}

/// Extracts the contract size as a multiplier from future product details.
fn contract_size_multiplier(product: &Product) -> Option<Quantity> {
    product.future_product_details.as_ref().and_then(|d| {
        if d.contract_size.is_empty() || d.contract_size == "0" {
            None
        } else {
            Some(Quantity::from(d.contract_size.as_str()))
        }
    })
}

/// Parses a Coinbase spot product into a `CurrencyPair`.
pub fn parse_spot_instrument(
    product: &Product,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(product.product_id), *COINBASE_VENUE);
    let raw_symbol = Symbol::new(product.product_id);

    let base_currency = Currency::get_or_create_crypto(product.base_currency_id);
    let quote_currency = Currency::get_or_create_crypto(product.quote_currency_id);

    let price_precision = precision_from_increment(&product.price_increment);
    let size_precision = precision_from_increment(&product.base_increment);

    let price_increment = Price::from(product.price_increment.as_str());
    let size_increment = Quantity::from(product.base_increment.as_str());

    let min_quantity = parse_optional_quantity(&product.base_min_size);
    let max_quantity = parse_optional_quantity(&product.base_max_size);

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None, // multiplier
        None, // lot_size
        max_quantity,
        min_quantity,
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee (loaded separately via transaction_summary)
        None, // taker_fee
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a Coinbase perpetual futures product into a `CryptoPerpetual`.
pub fn parse_perpetual_instrument(
    product: &Product,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(product.product_id), *COINBASE_VENUE);
    let raw_symbol = Symbol::new(product.product_id);

    let base_currency = derive_base_currency(product);
    let quote_currency = Currency::get_or_create_crypto(product.quote_currency_id);
    let settlement_currency = quote_currency;

    let price_precision = precision_from_increment(&product.price_increment);
    let size_precision = precision_from_increment(&product.base_increment);

    let price_increment = Price::from(product.price_increment.as_str());
    let size_increment = Quantity::from(product.base_increment.as_str());

    let min_quantity = parse_optional_quantity(&product.base_min_size);
    let max_quantity = parse_optional_quantity(&product.base_max_size);

    let multiplier = contract_size_multiplier(product);

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false, // is_inverse
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        multiplier,
        None, // lot_size
        max_quantity,
        min_quantity,
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Parses a Coinbase dated future into a `CryptoFuture`.
pub fn parse_future_instrument(
    product: &Product,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(product.product_id), *COINBASE_VENUE);
    let raw_symbol = Symbol::new(product.product_id);

    let underlying = derive_base_currency(product);
    let quote_currency = Currency::get_or_create_crypto(product.quote_currency_id);
    let settlement_currency = quote_currency;

    let price_precision = precision_from_increment(&product.price_increment);
    let size_precision = precision_from_increment(&product.base_increment);

    let price_increment = Price::from(product.price_increment.as_str());
    let size_increment = Quantity::from(product.base_increment.as_str());

    let min_quantity = parse_optional_quantity(&product.base_min_size);
    let max_quantity = parse_optional_quantity(&product.base_max_size);

    let expiry_str = product
        .future_product_details
        .as_ref()
        .map_or("", |d| d.contract_expiry.as_str());

    anyhow::ensure!(
        !expiry_str.is_empty(),
        "Missing contract_expiry for dated future '{}'",
        product.product_id
    );

    let expiration_ns = parse_rfc3339_timestamp(expiry_str).context(format!(
        "Failed to parse contract_expiry for '{}'",
        product.product_id
    ))?;

    let multiplier = contract_size_multiplier(product);

    let instrument = CryptoFuture::new(
        instrument_id,
        raw_symbol,
        underlying,
        quote_currency,
        settlement_currency,
        false, // is_inverse
        ts_init,
        expiration_ns,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        multiplier,
        None, // lot_size
        max_quantity,
        min_quantity,
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CryptoFuture(instrument))
}

/// Parses a Coinbase product into the appropriate Nautilus instrument type.
pub fn parse_instrument(product: &Product, ts_init: UnixNanos) -> anyhow::Result<InstrumentAny> {
    match product.product_type {
        CoinbaseProductType::Spot => parse_spot_instrument(product, ts_init),
        CoinbaseProductType::Future => {
            if is_perpetual_product(product) {
                parse_perpetual_instrument(product, ts_init)
            } else {
                parse_future_instrument(product, ts_init)
            }
        }
        CoinbaseProductType::Unknown => {
            anyhow::bail!("Unknown product type for '{}'", product.product_id)
        }
    }
}

/// Determines whether a futures product is a perpetual contract.
///
/// Coinbase returns `contract_expiry_type: "EXPIRING"` for both perpetuals
/// and dated futures, so the `CoinbaseContractExpiryType::Perpetual` variant
/// alone is not sufficient. We check three signals in order:
///
/// 1. `contract_expiry_type == Perpetual` (forward compat if Coinbase fixes the API)
/// 2. Non-empty `funding_rate` in `future_product_details` (structural signal:
///    only perpetuals have ongoing funding)
/// 3. `display_name` contains "PERP" or "Perpetual" (heuristic fallback)
pub(crate) fn is_perpetual_product(product: &Product) -> bool {
    if let Some(details) = &product.future_product_details {
        if details.contract_expiry_type == CoinbaseContractExpiryType::Perpetual {
            return true;
        }

        if !details.funding_rate.is_empty() {
            return true;
        }
    }
    product.display_name.contains("PERP") || product.display_name.contains("Perpetual")
}

/// Parses a Coinbase trade into a `TradeTick`.
pub fn parse_trade_tick(
    trade: &Trade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.price, price_precision)?;
    let size = parse_quantity(&trade.size, size_precision)?;
    let aggressor_side = coinbase_side_to_aggressor(&trade.side);
    let trade_id = TradeId::new(&trade.trade_id);
    let ts_event = parse_rfc3339_timestamp(&trade.time)?;

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Parses a Coinbase candle into a `Bar`.
pub fn parse_bar(
    candle: &Candle,
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = parse_price(&candle.open, price_precision)?;
    let high = parse_price(&candle.high, price_precision)?;
    let low = parse_price(&candle.low, price_precision)?;
    let close = parse_price(&candle.close, price_precision)?;
    let volume = parse_quantity(&candle.volume, size_precision)?;

    // Coinbase candle "start" is epoch seconds for the candle open time
    let ts_event = parse_epoch_secs_timestamp(&candle.start)?;

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Parses a Coinbase order book snapshot into `OrderBookDeltas`.
pub fn parse_product_book_snapshot(
    book: &PriceBook,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = parse_rfc3339_timestamp(&book.time)?;
    let total_levels = book.bids.len() + book.asks.len();
    let mut deltas = Vec::with_capacity(total_levels + 1);

    let mut clear = OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init);

    if total_levels == 0 {
        clear.flags |= RecordFlag::F_LAST as u8;
    }
    deltas.push(clear);

    let mut processed = 0usize;

    for level in &book.bids {
        processed += 1;
        let delta = parse_book_delta(
            level,
            OrderSide::Buy,
            instrument_id,
            price_precision,
            size_precision,
            processed == total_levels,
            ts_event,
            ts_init,
        )?;
        deltas.push(delta);
    }

    for level in &book.asks {
        processed += 1;
        let delta = parse_book_delta(
            level,
            OrderSide::Sell,
            instrument_id,
            price_precision,
            size_precision,
            processed == total_levels,
            ts_event,
            ts_init,
        )?;
        deltas.push(delta);
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

#[expect(clippy::too_many_arguments)]
fn parse_book_delta(
    level: &BookLevel,
    side: OrderSide,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    is_last: bool,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDelta> {
    let price = parse_price(&level.price, price_precision)?;
    let size = parse_quantity(&level.size, size_precision)?;

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

/// Converts a Coinbase order side to the Nautilus [`OrderSide`].
pub fn parse_order_side(side: &CoinbaseOrderSide) -> OrderSide {
    match side {
        CoinbaseOrderSide::Buy => OrderSide::Buy,
        CoinbaseOrderSide::Sell => OrderSide::Sell,
        CoinbaseOrderSide::Unknown => OrderSide::NoOrderSide,
    }
}

/// Converts a Coinbase order status to the Nautilus [`OrderStatus`].
///
/// `Pending` and `Queued` are transient pre-`Open` states the venue passes
/// through after acknowledging the order. They are mapped to `Accepted`
/// (rather than `Submitted`) so user-channel updates that race the REST
/// `OrderAccepted` event do not appear as a backwards transition to the
/// reconciler. `Open` also maps to `Accepted` because Nautilus differentiates
/// the initial accept event from later partial-fill states; callers should
/// promote the status to `PartiallyFilled` / `Filled` based on `filled_qty`.
pub fn parse_order_status(status: CoinbaseOrderStatus) -> OrderStatus {
    match status {
        CoinbaseOrderStatus::Pending | CoinbaseOrderStatus::Queued | CoinbaseOrderStatus::Open => {
            OrderStatus::Accepted
        }
        CoinbaseOrderStatus::Filled => OrderStatus::Filled,
        CoinbaseOrderStatus::Cancelled => OrderStatus::Canceled,
        CoinbaseOrderStatus::CancelQueued => OrderStatus::PendingCancel,
        CoinbaseOrderStatus::EditQueued => OrderStatus::PendingUpdate,
        CoinbaseOrderStatus::Expired => OrderStatus::Expired,
        CoinbaseOrderStatus::Failed => OrderStatus::Rejected,
        CoinbaseOrderStatus::Unknown => OrderStatus::Rejected,
    }
}

/// Converts a Coinbase time-in-force to the Nautilus [`TimeInForce`].
pub fn parse_time_in_force(tif: Option<CoinbaseTimeInForce>) -> TimeInForce {
    match tif {
        Some(CoinbaseTimeInForce::GoodUntilCancelled) => TimeInForce::Gtc,
        Some(CoinbaseTimeInForce::GoodUntilDateTime) => TimeInForce::Gtd,
        Some(CoinbaseTimeInForce::ImmediateOrCancel) => TimeInForce::Ioc,
        Some(CoinbaseTimeInForce::FillOrKill) => TimeInForce::Fok,
        Some(CoinbaseTimeInForce::Unknown) | None => TimeInForce::Gtc,
    }
}

/// Converts a Coinbase liquidity indicator to the Nautilus [`LiquiditySide`].
pub fn parse_liquidity_side(indicator: &CoinbaseLiquidityIndicator) -> LiquiditySide {
    match indicator {
        CoinbaseLiquidityIndicator::Maker => LiquiditySide::Maker,
        CoinbaseLiquidityIndicator::Taker => LiquiditySide::Taker,
        CoinbaseLiquidityIndicator::Unknown => LiquiditySide::NoLiquiditySide,
    }
}

/// Converts a Coinbase order type to the Nautilus [`OrderType`].
///
/// Coinbase uses `BRACKET` on history endpoints for multi-leg orders. Nautilus
/// has no bracket order type, so the parser falls back to [`OrderType::Limit`].
pub fn parse_order_type(order_type: CoinbaseOrderType) -> OrderType {
    match order_type {
        CoinbaseOrderType::Market => OrderType::Market,
        CoinbaseOrderType::Limit => OrderType::Limit,
        CoinbaseOrderType::Stop => OrderType::StopMarket,
        CoinbaseOrderType::StopLimit => OrderType::StopLimit,
        CoinbaseOrderType::Liquidation => OrderType::Market,
        CoinbaseOrderType::Bracket
        | CoinbaseOrderType::Twap
        | CoinbaseOrderType::RollOpen
        | CoinbaseOrderType::RollClose
        | CoinbaseOrderType::Scaled
        | CoinbaseOrderType::Unknown => OrderType::Limit,
    }
}

/// Parses a Coinbase [`Order`] into an [`OrderStatusReport`].
///
/// Uses the given instrument's price and size precision to build quantities
/// and prices, and derives the limit price from the order configuration when
/// present. Timestamps default to `ts_init` when Coinbase omits them.
///
/// # Errors
///
/// Returns an error when any numeric field cannot be parsed against the
/// instrument precision.
pub fn parse_order_status_report(
    order: &Order,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let order_side = parse_order_side(&order.side);
    let order_type = parse_order_type(order.order_type);
    let time_in_force = parse_time_in_force(order.time_in_force);
    let mut order_status = parse_order_status(order.status);

    let venue_order_id = VenueOrderId::new(&order.order_id);
    let client_order_id = if order.client_order_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(&order.client_order_id))
    };

    let filled_qty = if order.filled_size.is_empty() {
        Quantity::zero(size_precision)
    } else {
        parse_quantity(&order.filled_size, size_precision).context("failed to parse filled_size")?
    };

    // Derive the ordered quantity from the order_configuration. For quote-sized
    // market orders the base quantity is not reported pre-fill; fall back to
    // filled_qty when the order is terminal.
    let quantity = base_quantity_from_configuration(order, size_precision).unwrap_or(filled_qty);

    // Promote Accepted to PartiallyFilled when some fill has landed but the
    // order is still open, matching Nautilus' lifecycle.
    if order_status == OrderStatus::Accepted && filled_qty.is_positive() && filled_qty < quantity {
        order_status = OrderStatus::PartiallyFilled;
    }

    let ts_accepted = if order.created_time.is_empty() {
        ts_init
    } else {
        parse_rfc3339_timestamp(&order.created_time).unwrap_or(ts_init)
    };
    let ts_last = order
        .last_fill_time
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| parse_rfc3339_timestamp(s).ok())
        .unwrap_or(ts_accepted);

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
    );

    if let Some(price) = limit_price_from_configuration(order, price_precision) {
        report = report.with_price(price);
    }

    if let Some(trigger_price) = stop_price_from_configuration(order, price_precision) {
        report = report
            .with_trigger_price(trigger_price)
            .with_trigger_type(TriggerType::LastPrice);
    }

    if !order.average_filled_price.is_empty()
        && let Ok(avg_px) = order.average_filled_price.parse::<f64>()
        && avg_px > 0.0
    {
        report = report.with_avg_px(avg_px)?;
    }

    if post_only_from_configuration(order) {
        report = report.with_post_only(true);
    }

    if let Some(expire_time) = end_time_from_configuration(order) {
        report = report.with_expire_time(expire_time);
    }

    Ok(report)
}

/// Parses a Coinbase [`Fill`] into a [`FillReport`].
///
/// Commission currency defaults to the instrument's quote currency, which
/// matches how Coinbase reports fees for spot products. Negates the fee sign
/// to follow the Nautilus convention where commissions are positive when
/// paid by the taker.
///
/// # Errors
///
/// Returns an error when the price or size cannot be parsed, or the commission cannot be converted to `Money`.
pub fn parse_fill_report(
    fill: &Fill,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let venue_order_id = VenueOrderId::new(&fill.order_id);
    let trade_id = TradeId::new(&fill.trade_id);
    let order_side = parse_order_side(&fill.side);
    let last_px = parse_price(&fill.price, price_precision)?;
    let last_qty = parse_quantity(&fill.size, size_precision)?;

    let commission_currency = instrument.quote_currency();
    let commission = Money::from_decimal(fill.commission, commission_currency)
        .context("failed to build commission Money")?;

    let liquidity_side = parse_liquidity_side(&fill.liquidity_indicator);
    let ts_event = parse_rfc3339_timestamp(&fill.trade_time)?;

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
        None, // client_order_id not carried on Coinbase fill records
        None, // venue_position_id not provided
        ts_event,
        ts_init,
        None,
    ))
}

/// Parses a list of Coinbase [`Account`] entries into a Nautilus [`AccountState`].
///
/// Builds one [`AccountBalance`] per currency where
/// `total = available_balance + hold`, `free = available_balance`, and
/// `locked = hold`. Accounts with invalid balances are skipped with a debug
/// log. Always emits at least one balance so the resulting
/// [`AccountState`] is valid.
///
/// # Errors
///
/// Returns an error when building a balance fails after all accounts have
/// been exhausted (i.e. every entry was malformed).
pub fn parse_account_state(
    accounts: &[Account],
    account_id: AccountId,
    is_reported: bool,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    // Coinbase returns one row per wallet, so the same currency may appear
    // multiple times (per retail portfolio or sub-account). Aggregate by
    // currency before emitting balances: Nautilus stores balances keyed by
    // `Currency`, so emitting duplicates would drop funds via last-write-wins.
    let mut aggregated: ahash::AHashMap<Currency, (Money, Money)> = ahash::AHashMap::new();

    for account in accounts {
        let currency_code = account.currency.as_str().trim();
        if currency_code.is_empty() {
            log::debug!(
                "Skipping account with empty currency code: uuid={}",
                account.uuid
            );
            continue;
        }

        let currency =
            Currency::get_or_create_crypto_with_context(currency_code, Some("coinbase account"));

        let Some(free) = parse_money_field(
            account.available_balance.value,
            "available_balance",
            currency,
        ) else {
            continue;
        };

        let locked = match account.hold.as_ref() {
            Some(hold) => {
                parse_money_field(hold.value, "hold", currency).unwrap_or(Money::zero(currency))
            }
            None => Money::zero(currency),
        };

        aggregated
            .entry(currency)
            .and_modify(|(acc_free, acc_locked)| {
                *acc_free = *acc_free + free;
                *acc_locked = *acc_locked + locked;
            })
            .or_insert((free, locked));
    }

    let mut balances: Vec<AccountBalance> = aggregated
        .into_iter()
        .map(|(currency, (free, locked))| {
            let total = free + locked;
            AccountBalance::from_total_and_locked(total.as_decimal(), locked.as_decimal(), currency)
                .map_err(anyhow::Error::from)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    if balances.is_empty() {
        let fallback_currency = Currency::USD();
        let zero = Money::zero(fallback_currency);
        balances.push(AccountBalance::new(zero, zero, zero));
    }

    Ok(AccountState::new(
        account_id,
        AccountType::Cash,
        balances,
        Vec::new(),
        is_reported,
        UUID4::new(),
        ts_event,
        ts_init,
        None,
    ))
}

fn parse_money_field(value: Decimal, field: &str, currency: Currency) -> Option<Money> {
    match Money::from_decimal(value, currency) {
        Ok(money) => Some(money),
        Err(e) => {
            log::debug!(
                "Skipping {field}='{value}' for currency {}: {e}",
                currency.code
            );
            None
        }
    }
}

/// Parses a CFM balance summary into a single consolidated [`MarginBalance`].
///
/// Coinbase reports two windows (intraday and overnight) with identical
/// currency, but `MarginAccount::split_event_margins` keys account-level
/// margins by currency only, so emitting both would have one overwrite the
/// other. Selecting per-field maxima could synthesize a pair that matches
/// neither window, so we pick the whole window with the larger
/// `initial_margin` (ties broken by `maintenance_margin`) and emit its pair
/// verbatim; the stricter capital requirement governs risk.
///
/// # Errors
///
/// Returns an error when any balance cannot be built as [`Money`].
pub fn parse_cfm_margin_balances(
    summary: &CfmBalanceSummary,
) -> anyhow::Result<Vec<MarginBalance>> {
    let Some(window) = [
        summary.intraday_margin_window_measure.as_ref(),
        summary.overnight_margin_window_measure.as_ref(),
    ]
    .into_iter()
    .flatten()
    .max_by(|a, b| {
        a.initial_margin
            .value
            .cmp(&b.initial_margin.value)
            .then(a.maintenance_margin.value.cmp(&b.maintenance_margin.value))
    }) else {
        return Ok(Vec::new());
    };

    let currency = Currency::get_or_create_crypto(window.initial_margin.currency.as_str());
    let initial = Money::from_decimal(window.initial_margin.value, currency)
        .context("failed to build initial margin")?;
    let maintenance = Money::from_decimal(window.maintenance_margin.value, currency)
        .context("failed to build maintenance margin")?;

    Ok(vec![MarginBalance::new(initial, maintenance, None)])
}

/// Builds a margin [`AccountState`] from the CFM balance summary and the
/// current CBI / CFM USD balances.
///
/// # Errors
///
/// Returns an error if balances cannot be built from the summary values.
pub fn parse_cfm_account_state(
    summary: &CfmBalanceSummary,
    account_id: AccountId,
    is_reported: bool,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let usd_currency = Currency::get_or_create_crypto(summary.total_usd_balance.currency.as_str());

    // `total_usd_balance` is the venue's equity figure and includes collateral
    // already consumed by open positions; using it as total (with
    // `available_margin` as free) preserves equity so `Portfolio::equity`
    // matches the venue. `from_total_and_free` derives locked as total - free
    // so the `total == free + locked` invariant holds by construction.
    let balance = AccountBalance::from_total_and_free(
        summary.total_usd_balance.value,
        summary.available_margin.value,
        usd_currency,
    )
    .context("failed to build CFM account balance")?;

    let margins = parse_cfm_margin_balances(summary)?;

    Ok(AccountState::new(
        account_id,
        AccountType::Margin,
        vec![balance],
        margins,
        is_reported,
        UUID4::new(),
        ts_event,
        ts_init,
        None,
    ))
}

/// Builds a margin [`AccountState`] from a WebSocket-delivered FCM balance
/// summary.
///
/// The WebSocket payload does not carry explicit currency codes, so the
/// balance is reported in USD (the only CFM settlement currency).
///
/// # Errors
///
/// Returns an error when any component balance cannot be constructed.
pub fn parse_ws_cfm_account_state(
    summary: &WsFcmBalanceSummary,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let usd = Currency::USD();

    // See `parse_cfm_account_state`: `total_usd_balance` is the venue's
    // equity and must be kept so cached balance and `Portfolio::equity` align
    // with the venue.
    let balance = AccountBalance::from_total_and_free(
        summary.total_usd_balance,
        summary.available_margin,
        usd,
    )
    .context("failed to build WS CFM account balance")?;

    // Pick the window with the larger `initial_margin` (ties by maintenance)
    // and emit its pair verbatim so the emitted MarginBalance matches a real
    // venue window. See `parse_cfm_margin_balances` for why.
    let window = if summary
        .intraday_margin_window_measure
        .initial_margin
        .cmp(&summary.overnight_margin_window_measure.initial_margin)
        .then(
            summary
                .intraday_margin_window_measure
                .maintenance_margin
                .cmp(&summary.overnight_margin_window_measure.maintenance_margin),
        )
        .is_ge()
    {
        &summary.intraday_margin_window_measure
    } else {
        &summary.overnight_margin_window_measure
    };

    let initial = Money::from_decimal(window.initial_margin, usd)
        .context("failed to build initial margin")?;
    let maintenance = Money::from_decimal(window.maintenance_margin, usd)
        .context("failed to build maintenance margin")?;

    Ok(AccountState::new(
        account_id,
        AccountType::Margin,
        vec![balance],
        vec![MarginBalance::new(initial, maintenance, None)],
        true,
        UUID4::new(),
        ts_event,
        ts_init,
        None,
    ))
}

/// Parses a single CFM position into a Nautilus [`PositionStatusReport`].
///
/// The position's quantity is scaled by `contract_size` (expressed in the
/// instrument's size precision). Callers are expected to supply the
/// matching instrument so precision lines up with the venue's reported
/// number of contracts.
///
/// # Errors
///
/// Returns an error when the quantity or average entry price cannot be
/// represented with the instrument's precision.
pub fn parse_cfm_position_status_report(
    position: &CfmPosition,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = instrument.id();
    let size_precision = instrument.size_precision();

    let position_side = match position.side {
        CoinbaseFcmPositionSide::Long => PositionSideSpecified::Long,
        CoinbaseFcmPositionSide::Short => PositionSideSpecified::Short,
        CoinbaseFcmPositionSide::Unspecified => PositionSideSpecified::Flat,
    };

    let quantity = Quantity::from_decimal_dp(position.number_of_contracts, size_precision)
        .context("failed to build CFM position quantity")?;

    let avg_px_open = if position.avg_entry_price.value.is_zero() {
        None
    } else {
        Some(position.avg_entry_price.value)
    };

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_init,
        ts_init,
        None,
        None,
        avg_px_open,
    ))
}

// Coinbase history endpoints return a wider set of configuration shapes than
// `OrderConfiguration` covers (bracket, TWAP, trigger variants). History
// `Order.order_configuration` is kept as a raw `serde_json::Value`; these
// helpers dig into the value by key so unknown shapes simply return `None`
// instead of failing the whole batch.
fn base_quantity_from_configuration(order: &Order, size_precision: u8) -> Option<Quantity> {
    let config = order.order_configuration.as_ref()?.as_object()?;

    for (_key, inner) in config {
        let Some(inner_obj) = inner.as_object() else {
            continue;
        };

        if let Some(size) = inner_obj
            .get(ORDER_CONFIG_BASE_SIZE)
            .and_then(|v| v.as_str())
            && !size.is_empty()
            && let Ok(qty) = parse_quantity(size, size_precision)
        {
            return Some(qty);
        }
    }

    None
}

fn limit_price_from_configuration(order: &Order, price_precision: u8) -> Option<Price> {
    let config = order.order_configuration.as_ref()?.as_object()?;

    for (_key, inner) in config {
        let Some(inner_obj) = inner.as_object() else {
            continue;
        };

        if let Some(price) = inner_obj
            .get(ORDER_CONFIG_LIMIT_PRICE)
            .and_then(|v| v.as_str())
            && !price.is_empty()
            && let Ok(parsed) = parse_price(price, price_precision)
        {
            return Some(parsed);
        }
    }

    None
}

fn stop_price_from_configuration(order: &Order, price_precision: u8) -> Option<Price> {
    let config = order.order_configuration.as_ref()?.as_object()?;

    for (_key, inner) in config {
        let Some(inner_obj) = inner.as_object() else {
            continue;
        };

        if let Some(stop) = inner_obj
            .get(ORDER_CONFIG_STOP_PRICE)
            .and_then(|v| v.as_str())
            && !stop.is_empty()
            && let Ok(parsed) = parse_price(stop, price_precision)
        {
            return Some(parsed);
        }
    }

    None
}

fn post_only_from_configuration(order: &Order) -> bool {
    let Some(config) = order
        .order_configuration
        .as_ref()
        .and_then(|v| v.as_object())
    else {
        return false;
    };

    for (_key, inner) in config {
        if let Some(inner_obj) = inner.as_object()
            && let Some(post_only) = inner_obj
                .get(ORDER_CONFIG_POST_ONLY)
                .and_then(|v| v.as_bool())
        {
            return post_only;
        }
    }
    false
}

fn end_time_from_configuration(order: &Order) -> Option<UnixNanos> {
    let config = order.order_configuration.as_ref()?.as_object()?;

    for (_key, inner) in config {
        if let Some(inner_obj) = inner.as_object()
            && let Some(end_time) = inner_obj
                .get(ORDER_CONFIG_END_TIME)
                .and_then(|v| v.as_str())
            && !end_time.is_empty()
            && let Ok(ts) = parse_rfc3339_timestamp(end_time)
        {
            return Some(ts);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::bar::{BarSpecification, BarType},
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::Venue,
        instruments::Instrument,
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::{
            enums::{CoinbaseMarginLevel, CoinbaseMarginWindowType},
            testing::load_test_fixture,
        },
        http::models::{Account, Balance},
    };

    fn coinbase_venue() -> Venue {
        Venue::new(Ustr::from("COINBASE"))
    }

    #[rstest]
    #[case("0.01", 2)]
    #[case("0.00000001", 8)]
    #[case("1", 0)]
    #[case("5", 0)]
    #[case("0.1", 1)]
    #[case("0.001", 3)]
    fn test_precision_from_increment(#[case] increment: &str, #[case] expected: u8) {
        assert_eq!(precision_from_increment(increment), expected);
    }

    #[rstest]
    fn test_parse_rfc3339_timestamp() {
        let ts = parse_rfc3339_timestamp("2026-04-07T00:28:32.643779Z").unwrap();
        assert_eq!(ts.as_u64(), 1_775_521_712_643_779_000);
    }

    #[rstest]
    #[case("")]
    #[case("not-a-date")]
    #[case("2026-13-01T00:00:00Z")]
    fn test_parse_rfc3339_timestamp_rejects_invalid(#[case] input: &str) {
        assert!(parse_rfc3339_timestamp(input).is_err());
    }

    #[rstest]
    fn test_parse_epoch_secs_timestamp() {
        let ts = parse_epoch_secs_timestamp("1712192400").unwrap();
        assert_eq!(ts.as_u64(), 1_712_192_400_000_000_000);
    }

    #[rstest]
    #[case("")]
    #[case("abc")]
    fn test_parse_epoch_secs_timestamp_rejects_invalid(#[case] input: &str) {
        assert!(parse_epoch_secs_timestamp(input).is_err());
    }

    #[rstest]
    fn test_parse_price_valid() {
        let price = parse_price("68913.87", 2).unwrap();
        assert_eq!(price, Price::from("68913.87"));
    }

    #[rstest]
    #[case("")]
    #[case("abc")]
    fn test_parse_price_rejects_invalid(#[case] input: &str) {
        assert!(parse_price(input, 2).is_err());
    }

    #[rstest]
    fn test_parse_quantity_valid() {
        let qty = parse_quantity("0.00014004", 8).unwrap();
        assert_eq!(qty, Quantity::from("0.00014004"));
    }

    #[rstest]
    #[case("")]
    #[case("abc")]
    fn test_parse_quantity_rejects_invalid(#[case] input: &str) {
        assert!(parse_quantity(input, 8).is_err());
    }

    #[rstest]
    fn test_parse_spot_instrument() {
        let json = load_test_fixture("http_product.json");
        let product: crate::http::models::Product = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let instrument = parse_spot_instrument(&product, ts).unwrap();
        let pair = match &instrument {
            InstrumentAny::CurrencyPair(p) => p,
            other => panic!("Expected CurrencyPair, was{other:?}"),
        };

        assert_eq!(pair.id().symbol.as_str(), "BTC-USD");
        assert_eq!(pair.id().venue, coinbase_venue());
        assert_eq!(pair.base_currency().unwrap().code.as_str(), "BTC");
        assert_eq!(pair.quote_currency().code.as_str(), "USD");
        assert_eq!(pair.price_precision(), 2);
        assert_eq!(pair.size_precision(), 8);
        assert_eq!(pair.price_increment(), Price::from("0.01"));
        assert_eq!(pair.size_increment(), Quantity::from("0.00000001"));
        assert_eq!(pair.min_quantity(), Some(Quantity::from("0.00000001")));
        assert_eq!(pair.max_quantity(), Some(Quantity::from("3400")));
    }

    #[rstest]
    fn test_parse_spot_instruments_from_list() {
        let json = load_test_fixture("http_products.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let instruments: Vec<InstrumentAny> = response
            .products
            .iter()
            .map(|p| parse_instrument(p, ts).unwrap())
            .collect();

        assert_eq!(instruments.len(), 2);
        for inst in &instruments {
            assert!(matches!(inst, InstrumentAny::CurrencyPair(_)));
        }
    }

    #[rstest]
    fn test_parse_future_instruments_distinguishes_perp_and_dated() {
        let json = load_test_fixture("http_products_future.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let instruments: Vec<InstrumentAny> = response
            .products
            .iter()
            .map(|p| parse_instrument(p, ts).unwrap())
            .collect();

        assert_eq!(instruments.len(), 2);

        // First product is "BTC PERP" -> CryptoPerpetual
        assert!(
            matches!(&instruments[0], InstrumentAny::CryptoPerpetual(_)),
            "Expected CryptoPerpetual for BTC PERP, was{:?}",
            &instruments[0]
        );

        // Second product is "BTC 24 APR 26" -> CryptoFuture
        assert!(
            matches!(&instruments[1], InstrumentAny::CryptoFuture(_)),
            "Expected CryptoFuture for dated future, was{:?}",
            &instruments[1]
        );
    }

    #[rstest]
    fn test_parse_perpetual_instrument_derives_base_from_display_name() {
        let json = load_test_fixture("http_products_future.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        // The first future product has empty base_currency_id and display_name "BTC PERP"
        let perp_product = response
            .products
            .iter()
            .find(|p| p.display_name.contains("PERP"))
            .expect("should have a PERP product");

        let instrument = parse_perpetual_instrument(perp_product, ts).unwrap();
        let perp = match &instrument {
            InstrumentAny::CryptoPerpetual(p) => p,
            other => panic!("Expected CryptoPerpetual, was{other:?}"),
        };

        assert_eq!(perp.base_currency().unwrap().code.as_str(), "BTC");
        assert_eq!(perp.quote_currency().code.as_str(), "USD");
    }

    #[rstest]
    fn test_parse_perpetual_instrument_has_contract_size_multiplier() {
        let json = load_test_fixture("http_products_future.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let perp_product = response
            .products
            .iter()
            .find(|p| p.display_name.contains("PERP"))
            .expect("should have a PERP product");

        let instrument = parse_perpetual_instrument(perp_product, ts).unwrap();
        let perp = match &instrument {
            InstrumentAny::CryptoPerpetual(p) => p,
            other => panic!("Expected CryptoPerpetual, was {other:?}"),
        };

        assert_eq!(perp.multiplier, Quantity::from("0.01"));
    }

    #[rstest]
    fn test_parse_future_instrument_has_expiry_and_multiplier() {
        let json = load_test_fixture("http_products_future.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let future_product = response
            .products
            .iter()
            .find(|p| !p.display_name.contains("PERP") && !p.display_name.contains("Perpetual"))
            .expect("should have a dated future product");

        let instrument = parse_future_instrument(future_product, ts).unwrap();
        let future = match &instrument {
            InstrumentAny::CryptoFuture(f) => f,
            other => panic!("Expected CryptoFuture, was {other:?}"),
        };

        // Verify contract_expiry "2026-04-24T15:00:00Z" parsed correctly
        let expected_expiry = parse_rfc3339_timestamp("2026-04-24T15:00:00Z").unwrap();
        assert_eq!(future.expiration_ns, expected_expiry);
        assert_eq!(future.multiplier, Quantity::from("0.01"));
        assert_eq!(future.base_currency().unwrap().code.as_str(), "BTC");
        assert_eq!(future.quote_currency().code.as_str(), "USD");
    }

    #[rstest]
    fn test_parse_trade_tick() {
        let json = load_test_fixture("http_ticker.json");
        let response: crate::http::models::TickerResponse = serde_json::from_str(&json).unwrap();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), coinbase_venue());
        let ts_init = UnixNanos::default();

        let trades: Vec<TradeTick> = response
            .trades
            .iter()
            .map(|t| parse_trade_tick(t, instrument_id, 2, 8, ts_init).unwrap())
            .collect();

        assert_eq!(trades.len(), 3);

        // Verify exact values from first fixture trade
        assert_eq!(trades[0].instrument_id, instrument_id);
        assert_eq!(trades[0].price, Price::from("68923.67"));
        assert_eq!(trades[0].size, Quantity::from("0.00064000"));
        assert_eq!(trades[0].trade_id.as_str(), "995098663");
        assert!(trades[0].ts_event.as_u64() > 0);
    }

    #[rstest]
    fn test_parse_trade_tick_aggressor_side() {
        let json = load_test_fixture("http_ticker.json");
        let response: crate::http::models::TickerResponse = serde_json::from_str(&json).unwrap();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), coinbase_venue());
        let ts_init = UnixNanos::default();

        for trade_data in &response.trades {
            let trade = parse_trade_tick(trade_data, instrument_id, 2, 8, ts_init).unwrap();
            match trade_data.side {
                CoinbaseOrderSide::Buy => {
                    assert_eq!(trade.aggressor_side, AggressorSide::Buyer);
                }
                CoinbaseOrderSide::Sell => {
                    assert_eq!(trade.aggressor_side, AggressorSide::Seller);
                }
                _ => {}
            }
        }
    }

    #[rstest]
    fn test_parse_bar() {
        let json = load_test_fixture("http_candles.json");
        let response: crate::http::models::CandlesResponse = serde_json::from_str(&json).unwrap();

        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), coinbase_venue());
        let bar_spec = BarSpecification::new(1, BarAggregation::Hour, PriceType::Last);
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);
        let ts_init = UnixNanos::default();

        let bars: Vec<Bar> = response
            .candles
            .iter()
            .map(|c| parse_bar(c, bar_type, 2, 8, ts_init).unwrap())
            .collect();

        assert_eq!(bars.len(), 2);

        // Verify exact OHLCV from first fixture candle (start=1712192400)
        let bar = &bars[0];
        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open, Price::from("66312.40"));
        assert_eq!(bar.high, Price::from("66331.99"));
        assert_eq!(bar.low, Price::from("66055.14"));
        assert_eq!(bar.close, Price::from("66181.60"));
        assert_eq!(bar.volume, Quantity::from("355.82896243"));
        assert_eq!(bar.ts_event.as_u64(), 1_712_192_400_000_000_000);
    }

    #[rstest]
    fn test_parse_product_book_snapshot() {
        let json = load_test_fixture("http_product_book.json");
        let response: crate::http::models::ProductBookResponse =
            serde_json::from_str(&json).unwrap();

        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), coinbase_venue());
        let ts_init = UnixNanos::default();

        let deltas =
            parse_product_book_snapshot(&response.pricebook, instrument_id, 2, 8, ts_init).unwrap();

        assert_eq!(deltas.instrument_id, instrument_id);
        let total_levels = response.pricebook.bids.len() + response.pricebook.asks.len();
        assert_eq!(deltas.deltas.len(), total_levels + 1);

        // First delta is a clear
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        // Verify first bid side and price
        let first_bid = &deltas.deltas[1];
        assert_eq!(first_bid.order.side, OrderSide::Buy);
        assert_eq!(first_bid.action, BookAction::Add);
        assert!(first_bid.order.price.as_f64() > 0.0);

        // Verify first ask comes after bids
        let first_ask_idx = response.pricebook.bids.len() + 1;
        let first_ask = &deltas.deltas[first_ask_idx];
        assert_eq!(first_ask.order.side, OrderSide::Sell);
        assert_eq!(first_ask.action, BookAction::Add);

        // Last delta has F_LAST flag
        let last = deltas.deltas.last().unwrap();
        assert_ne!(last.flags & RecordFlag::F_LAST as u8, 0);
    }

    fn btc_usd_instrument() -> InstrumentAny {
        let json = load_test_fixture("http_product.json");
        let product: crate::http::models::Product = serde_json::from_str(&json).unwrap();
        parse_spot_instrument(&product, UnixNanos::default()).unwrap()
    }

    #[rstest]
    fn test_parse_order_status_report_fully_filled_limit_gtc() {
        let json = load_test_fixture("http_order.json");
        let response: crate::http::models::OrderResponse = serde_json::from_str(&json).unwrap();
        let instrument = btc_usd_instrument();
        let account_id = AccountId::new("COINBASE-001");
        let ts_init = UnixNanos::from(1);

        let report =
            parse_order_status_report(&response.order, &instrument, account_id, ts_init).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id.symbol.as_str(), "BTC-USD");
        assert_eq!(report.venue_order_id.as_str(), "0000-000000-000000");
        assert_eq!(
            report.client_order_id.unwrap().as_str(),
            "11111-000000-000000"
        );
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.time_in_force, TimeInForce::Gtc);
        // filled_size (0.001) == base_size (0.001), so status stays Accepted
        // rather than promoting to PartiallyFilled.
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.quantity, Quantity::from("0.001"));
        assert_eq!(report.filled_qty, Quantity::from("0.001"));
        assert_eq!(report.price, Some(Price::from("10000.00")));
        assert_eq!(report.avg_px, Some(Decimal::from(50)));
    }

    #[rstest]
    fn test_parse_order_status_report_filled_market_order() {
        let json = load_test_fixture("http_orders_list.json");
        let response: crate::http::models::OrdersListResponse =
            serde_json::from_str(&json).unwrap();
        let instrument = btc_usd_instrument();
        let account_id = AccountId::new("COINBASE-001");
        let ts_init = UnixNanos::from(1);

        // Second order in the list is a filled MARKET order
        let filled_order = &response.orders[1];
        let report =
            parse_order_status_report(filled_order, &instrument, account_id, ts_init).unwrap();

        assert_eq!(report.order_status, OrderStatus::Filled);
        assert_eq!(report.order_type, OrderType::Market);
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.time_in_force, TimeInForce::Ioc);
        // Market quote-size orders fall back to filled_qty for total quantity
        assert_eq!(report.filled_qty, Quantity::from("0.0325"));
        assert_eq!(report.quantity, report.filled_qty);
        assert!(report.price.is_none());
    }

    #[rstest]
    fn test_parse_fill_report_maker() {
        let json = load_test_fixture("http_fills.json");
        let response: crate::http::models::FillsResponse = serde_json::from_str(&json).unwrap();
        let instrument = btc_usd_instrument();
        let account_id = AccountId::new("COINBASE-001");
        let ts_init = UnixNanos::from(1);

        let maker_fill = &response.fills[0];
        let report = parse_fill_report(maker_fill, &instrument, account_id, ts_init).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.trade_id.as_str(), "1111-11111-111111");
        assert_eq!(report.venue_order_id.as_str(), "0000-000000-000000");
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(report.last_px, Price::from("45123.45"));
        assert_eq!(report.last_qty, Quantity::from("0.00500000"));
        assert_eq!(
            report.commission.as_decimal(),
            Decimal::from_str("1.14").unwrap()
        );
        assert_eq!(report.commission.currency.code.as_str(), "USD");
    }

    #[rstest]
    fn test_parse_account_state_spot_cash() {
        let json = load_test_fixture("http_accounts.json");
        let response: crate::http::models::AccountsResponse = serde_json::from_str(&json).unwrap();
        let account_id = AccountId::new("COINBASE-001");
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let state =
            parse_account_state(&response.accounts, account_id, true, ts_event, ts_init).unwrap();

        assert_eq!(state.account_id, account_id);
        assert_eq!(state.account_type, AccountType::Cash);
        assert!(state.is_reported);
        assert_eq!(state.margins.len(), 0);
        assert_eq!(state.balances.len(), 2);

        let btc_balance = state
            .balances
            .iter()
            .find(|b| b.currency.code.as_str() == "BTC")
            .expect("BTC balance present");
        assert_eq!(
            btc_balance.free.as_decimal(),
            Decimal::from_str("1.23456789").unwrap()
        );
        assert_eq!(
            btc_balance.locked.as_decimal(),
            Decimal::from_str("0.00500000").unwrap()
        );
        assert_eq!(
            btc_balance.total.as_decimal(),
            btc_balance.free.as_decimal() + btc_balance.locked.as_decimal()
        );

        let usd_balance = state
            .balances
            .iter()
            .find(|b| b.currency.code.as_str() == "USD")
            .expect("USD balance present");
        assert_eq!(
            usd_balance.free.as_decimal(),
            Decimal::from_str("10000.50").unwrap()
        );
        assert_eq!(
            usd_balance.locked.as_decimal(),
            Decimal::from_str("450.00").unwrap()
        );
    }

    #[rstest]
    fn test_parse_account_state_aggregates_same_currency() {
        fn make_account(
            currency: &str,
            available: &str,
            hold: &str,
            uuid: &str,
            portfolio: &str,
        ) -> Account {
            Account {
                uuid: uuid.to_string(),
                name: "wallet".to_string(),
                currency: Ustr::from(currency),
                available_balance: Balance {
                    value: Decimal::from_str(available).unwrap(),
                    currency: Ustr::from(currency),
                },
                default: false,
                active: true,
                created_at: String::new(),
                updated_at: String::new(),
                deleted_at: None,
                account_type: crate::common::enums::CoinbaseAccountType::Fiat,
                ready: true,
                hold: Some(Balance {
                    value: Decimal::from_str(hold).unwrap(),
                    currency: Ustr::from(currency),
                }),
                retail_portfolio_id: portfolio.to_string(),
            }
        }

        let accounts = vec![
            make_account("USD", "1000.00", "50.00", "uuid-1", "portfolio-a"),
            make_account("USD", "2500.00", "25.00", "uuid-2", "portfolio-b"),
            make_account("BTC", "0.5", "0.1", "uuid-3", "portfolio-a"),
        ];

        let account_id = AccountId::new("COINBASE-001");
        let state = parse_account_state(
            &accounts,
            account_id,
            true,
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
        .unwrap();

        assert_eq!(state.balances.len(), 2);

        let usd = state
            .balances
            .iter()
            .find(|b| b.currency.code.as_str() == "USD")
            .expect("USD balance aggregated");
        assert_eq!(usd.free.as_decimal(), Decimal::from_str("3500.00").unwrap());
        assert_eq!(usd.locked.as_decimal(), Decimal::from_str("75.00").unwrap());
        assert_eq!(
            usd.total.as_decimal(),
            Decimal::from_str("3575.00").unwrap()
        );

        let btc = state
            .balances
            .iter()
            .find(|b| b.currency.code.as_str() == "BTC")
            .expect("BTC balance present");
        assert_eq!(btc.free.as_decimal(), Decimal::from_str("0.5").unwrap());
        assert_eq!(btc.locked.as_decimal(), Decimal::from_str("0.1").unwrap());
    }

    #[rstest]
    fn test_parse_account_state_empty_falls_back_to_zero_usd() {
        let account_id = AccountId::new("COINBASE-001");
        let state = parse_account_state(
            &[],
            account_id,
            true,
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
        .unwrap();

        assert_eq!(state.balances.len(), 1);
        let balance = &state.balances[0];
        assert_eq!(balance.currency.code.as_str(), "USD");
        assert_eq!(balance.total.as_decimal(), Decimal::ZERO);
    }

    #[rstest]
    #[case(CoinbaseOrderType::Market, OrderType::Market)]
    #[case(CoinbaseOrderType::Limit, OrderType::Limit)]
    #[case(CoinbaseOrderType::Stop, OrderType::StopMarket)]
    #[case(CoinbaseOrderType::StopLimit, OrderType::StopLimit)]
    #[case(CoinbaseOrderType::Bracket, OrderType::Limit)]
    #[case(CoinbaseOrderType::Twap, OrderType::Limit)]
    #[case(CoinbaseOrderType::RollOpen, OrderType::Limit)]
    #[case(CoinbaseOrderType::RollClose, OrderType::Limit)]
    #[case(CoinbaseOrderType::Liquidation, OrderType::Market)]
    #[case(CoinbaseOrderType::Scaled, OrderType::Limit)]
    #[case(CoinbaseOrderType::Unknown, OrderType::Limit)]
    fn test_parse_order_type(#[case] input: CoinbaseOrderType, #[case] expected: OrderType) {
        assert_eq!(parse_order_type(input), expected);
    }

    #[rstest]
    #[case(CoinbaseOrderStatus::Open, OrderStatus::Accepted)]
    #[case(CoinbaseOrderStatus::Filled, OrderStatus::Filled)]
    #[case(CoinbaseOrderStatus::Cancelled, OrderStatus::Canceled)]
    #[case(CoinbaseOrderStatus::CancelQueued, OrderStatus::PendingCancel)]
    #[case(CoinbaseOrderStatus::EditQueued, OrderStatus::PendingUpdate)]
    #[case(CoinbaseOrderStatus::Expired, OrderStatus::Expired)]
    #[case(CoinbaseOrderStatus::Failed, OrderStatus::Rejected)]
    #[case(CoinbaseOrderStatus::Pending, OrderStatus::Accepted)]
    #[case(CoinbaseOrderStatus::Queued, OrderStatus::Accepted)]
    fn test_parse_order_status(#[case] input: CoinbaseOrderStatus, #[case] expected: OrderStatus) {
        assert_eq!(parse_order_status(input), expected);
    }

    // Builds a minimal limit-GTC order with overridable size fields so tests
    // can exercise partial-fill, error, and boundary paths without adding a
    // fixture per permutation.
    fn make_limit_gtc_order(
        base_size: &str,
        limit_price: &str,
        filled_size: &str,
        status: CoinbaseOrderStatus,
    ) -> crate::http::models::Order {
        crate::http::models::Order {
            order_id: "venue-abc".to_string(),
            product_id: Ustr::from("BTC-USD"),
            user_id: "user-1".to_string(),
            order_configuration: Some(serde_json::json!({
                "limit_limit_gtc": {
                    "base_size": base_size,
                    "limit_price": limit_price,
                    "post_only": false,
                }
            })),
            side: CoinbaseOrderSide::Buy,
            client_order_id: "client-abc".to_string(),
            status,
            time_in_force: Some(CoinbaseTimeInForce::GoodUntilCancelled),
            created_time: "2024-01-15T10:00:00Z".to_string(),
            completion_percentage: String::new(),
            filled_size: filled_size.to_string(),
            average_filled_price: String::new(),
            fee: Decimal::ZERO,
            number_of_fills: 0,
            filled_value: Decimal::ZERO,
            pending_cancel: false,
            size_in_quote: false,
            total_fees: Decimal::ZERO,
            size_inclusive_of_fees: false,
            total_value_after_fees: Decimal::ZERO,
            trigger_status: crate::common::enums::CoinbaseTriggerStatus::Unknown,
            order_type: CoinbaseOrderType::Limit,
            reject_reason: String::new(),
            settled: false,
            product_type: CoinbaseProductType::Spot,
            reject_message: String::new(),
            cancel_message: String::new(),
            order_placement_source:
                crate::common::enums::CoinbaseOrderPlacementSource::RetailAdvanced,
            outstanding_hold_amount: Decimal::ZERO,
            is_liquidation: false,
            last_fill_time: None,
            leverage: String::new(),
            margin_type: None,
            retail_portfolio_id: String::new(),
            originating_order_id: String::new(),
            attached_order_id: String::new(),
        }
    }

    #[rstest]
    #[case::partially_filled("0.001", "0.0005", OrderStatus::PartiallyFilled)]
    #[case::fully_equals_boundary("0.001", "0.001", OrderStatus::Accepted)]
    #[case::zero_filled("0.001", "0", OrderStatus::Accepted)]
    fn test_parse_order_status_report_promotes_to_partially_filled(
        #[case] base_size: &str,
        #[case] filled_size: &str,
        #[case] expected_status: OrderStatus,
    ) {
        let order = make_limit_gtc_order(
            base_size,
            "50000.00",
            filled_size,
            CoinbaseOrderStatus::Open,
        );
        let instrument = btc_usd_instrument();
        let account_id = AccountId::new("COINBASE-001");

        let report =
            parse_order_status_report(&order, &instrument, account_id, UnixNanos::from(1)).unwrap();

        assert_eq!(report.order_status, expected_status);
        assert_eq!(report.quantity, Quantity::from(base_size));
    }

    #[rstest]
    fn test_parse_order_status_report_rejects_malformed_filled_size() {
        let mut order = make_limit_gtc_order("0.001", "50000.00", "0", CoinbaseOrderStatus::Open);
        order.filled_size = "not-a-number".to_string();
        let instrument = btc_usd_instrument();

        let err = parse_order_status_report(
            &order,
            &instrument,
            AccountId::new("COINBASE-001"),
            UnixNanos::from(1),
        )
        .unwrap_err();

        let chain = format!("{err:#}");
        assert!(
            chain.contains("failed to parse filled_size"),
            "expected failed to parse filled_size in error chain, was: {chain}"
        );
    }

    fn make_fill(commission: &str, price: &str, size: &str, trade_time: &str) -> Fill {
        Fill {
            entry_id: "entry-1".to_string(),
            trade_id: "trade-1".to_string(),
            order_id: "venue-1".to_string(),
            trade_time: trade_time.to_string(),
            trade_type: crate::common::enums::CoinbaseFillTradeType::Fill,
            price: price.to_string(),
            size: size.to_string(),
            commission: Decimal::from_str(commission).unwrap(),
            product_id: Ustr::from("BTC-USD"),
            sequence_timestamp: "2024-01-15T10:30:00.000Z".to_string(),
            liquidity_indicator: CoinbaseLiquidityIndicator::Maker,
            size_in_quote: false,
            user_id: "user-1".to_string(),
            side: CoinbaseOrderSide::Buy,
            retail_portfolio_id: String::new(),
        }
    }

    #[rstest]
    fn test_parse_fill_report_rejects_out_of_range_commission() {
        let fill = make_fill(
            "9999999999999999999999999999",
            "45000.00",
            "0.001",
            "2024-01-15T10:30:00Z",
        );
        let instrument = btc_usd_instrument();

        let err = parse_fill_report(
            &fill,
            &instrument,
            AccountId::new("COINBASE-001"),
            UnixNanos::from(1),
        )
        .unwrap_err();

        let chain = format!("{err:#}");
        assert!(
            chain.contains("failed to build commission Money"),
            "expected failed to build commission Money in error chain, was: {chain}"
        );
    }

    #[rstest]
    fn test_parse_fill_report_rejects_non_rfc3339_trade_time() {
        let fill = make_fill("0.50", "45000.00", "0.001", "not-a-timestamp");
        let instrument = btc_usd_instrument();

        let result = parse_fill_report(
            &fill,
            &instrument,
            AccountId::new("COINBASE-001"),
            UnixNanos::from(1),
        );
        assert!(result.is_err(), "expected parse failure on bad trade_time");
    }

    #[rstest]
    fn test_parse_account_state_skips_entry_with_out_of_range_money() {
        let valid = Account {
            uuid: "uuid-valid".to_string(),
            name: "USD Wallet".to_string(),
            currency: Ustr::from("USD"),
            available_balance: Balance {
                value: Decimal::from_str("1000.00").unwrap(),
                currency: Ustr::from("USD"),
            },
            default: false,
            active: true,
            created_at: String::new(),
            updated_at: String::new(),
            deleted_at: None,
            account_type: crate::common::enums::CoinbaseAccountType::Fiat,
            ready: true,
            hold: Some(Balance {
                value: Decimal::from_str("50.00").unwrap(),
                currency: Ustr::from("USD"),
            }),
            retail_portfolio_id: String::new(),
        };

        let over_precision = Account {
            available_balance: Balance {
                value: Decimal::from_str("9999999999999999999999999999").unwrap(),
                currency: Ustr::from("USD"),
            },
            hold: Some(Balance {
                value: Decimal::ZERO,
                currency: Ustr::from("USD"),
            }),
            currency: Ustr::from("USD"),
            uuid: "uuid-over-precision".to_string(),
            ..valid.clone()
        };

        let state = parse_account_state(
            &[over_precision, valid],
            AccountId::new("COINBASE-001"),
            true,
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
        .unwrap();

        // Out-of-range entry was skipped; only the valid USD row survives.
        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.balances[0].currency.code.as_str(), "USD");
        assert_eq!(
            state.balances[0].free.as_decimal(),
            Decimal::from_str("1000.00").unwrap()
        );
    }

    #[rstest]
    fn test_parse_order_status_report_extracts_stop_limit_trigger_price() {
        let order = crate::http::models::Order {
            order_configuration: Some(serde_json::json!({
                "stop_limit_stop_limit_gtc": {
                    "base_size": "0.001",
                    "limit_price": "49500.00",
                    "stop_price": "49000.00",
                    "stop_direction": "STOP_DIRECTION_STOP_DOWN"
                }
            })),
            order_type: CoinbaseOrderType::StopLimit,
            ..make_limit_gtc_order("0.001", "0", "0", CoinbaseOrderStatus::Open)
        };
        let instrument = btc_usd_instrument();

        let report = parse_order_status_report(
            &order,
            &instrument,
            AccountId::new("COINBASE-001"),
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(report.order_type, OrderType::StopLimit);
        assert_eq!(report.price, Some(Price::from("49500.00")));
        assert_eq!(report.trigger_price, Some(Price::from("49000.00")));
        assert_eq!(report.trigger_type, Some(TriggerType::LastPrice));
    }

    #[rstest]
    #[case::limit_gtc_post_only_true("limit_limit_gtc", true)]
    #[case::limit_gtc_post_only_false("limit_limit_gtc", false)]
    fn test_parse_order_status_report_propagates_post_only(
        #[case] config_key: &str,
        #[case] post_only: bool,
    ) {
        let config = serde_json::json!({
            config_key: {
                "base_size": "0.001",
                "limit_price": "50000.00",
                "post_only": post_only,
            }
        });
        let order = crate::http::models::Order {
            order_configuration: Some(config),
            ..make_limit_gtc_order("0.001", "50000.00", "0", CoinbaseOrderStatus::Open)
        };

        let report = parse_order_status_report(
            &order,
            &btc_usd_instrument(),
            AccountId::new("COINBASE-001"),
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(report.post_only, post_only);
    }

    #[rstest]
    fn test_parse_order_with_unknown_configuration_does_not_fail() {
        // Coinbase history may return bracket, TWAP, or trigger configs that
        // the submit-side OrderConfiguration enum does not model. The raw
        // JSON field must tolerate these without failing deserialization.
        let json_str = r#"{
            "order": {
                "order_id": "venue-bracket-1",
                "product_id": "BTC-USD",
                "user_id": "user-1",
                "order_configuration": {
                    "trigger_bracket_gtd": {
                        "limit_price": "55000.00",
                        "stop_trigger_price": "45000.00",
                        "end_time": "2024-12-31T23:59:59Z"
                    }
                },
                "side": "BUY",
                "client_order_id": "client-bracket-1",
                "status": "OPEN",
                "time_in_force": "GOOD_UNTIL_DATE_TIME",
                "created_time": "2024-01-15T10:00:00Z",
                "completion_percentage": "0",
                "filled_size": "0",
                "average_filled_price": "0",
                "fee": "0",
                "number_of_fills": "0",
                "filled_value": "0",
                "pending_cancel": false,
                "size_in_quote": false,
                "total_fees": "0",
                "size_inclusive_of_fees": false,
                "total_value_after_fees": "0",
                "trigger_status": "INVALID_ORDER_TYPE",
                "order_type": "BRACKET",
                "reject_reason": "",
                "settled": false,
                "product_type": "SPOT",
                "reject_message": "",
                "cancel_message": "",
                "order_placement_source": "RETAIL_ADVANCED",
                "outstanding_hold_amount": "0",
                "is_liquidation": false,
                "last_fill_time": null,
                "leverage": "",
                "margin_type": "",
                "retail_portfolio_id": "",
                "originating_order_id": "",
                "attached_order_id": ""
            }
        }"#;

        let response: crate::http::models::OrderResponse =
            serde_json::from_str(json_str).expect("unknown config must deserialize");

        let report = parse_order_status_report(
            &response.order,
            &btc_usd_instrument(),
            AccountId::new("COINBASE-001"),
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(report.venue_order_id.as_str(), "venue-bracket-1");
        // The bracket config has no `base_size`, so quantity falls back to
        // filled_qty (zero). The `limit_price` key still matches the
        // permissive walker and is extracted opportunistically; this is the
        // right tolerant default for unknown shapes.
        assert_eq!(report.filled_qty, Quantity::zero(8));
        assert_eq!(report.price, Some(Price::from("55000.00")));
    }

    #[rstest]
    fn test_parse_order_status_report_gtd_carries_expire_time() {
        let order = crate::http::models::Order {
            order_configuration: Some(serde_json::json!({
                "limit_limit_gtd": {
                    "base_size": "0.001",
                    "limit_price": "50000.00",
                    "end_time": "2024-12-31T23:59:59Z",
                    "post_only": false
                }
            })),
            time_in_force: Some(CoinbaseTimeInForce::GoodUntilDateTime),
            order_type: CoinbaseOrderType::Limit,
            ..make_limit_gtc_order("0.001", "50000.00", "0", CoinbaseOrderStatus::Open)
        };

        let report = parse_order_status_report(
            &order,
            &btc_usd_instrument(),
            AccountId::new("COINBASE-001"),
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(report.time_in_force, TimeInForce::Gtd);

        let expected_expire = parse_rfc3339_timestamp("2024-12-31T23:59:59Z").unwrap();
        assert_eq!(report.expire_time, Some(expected_expire));
    }

    #[rstest]
    fn test_parse_optional_quantity_returns_none_on_overflow() {
        // Values exceeding QUANTITY_RAW_MAX must return None instead of panicking
        let result = parse_optional_quantity("99999999999999999999999999999999");
        assert!(result.is_none());
    }

    // Confirms the "pick one whole window" invariant: when intraday has the
    // larger initial_margin but overnight has the larger maintenance_margin,
    // the emitted MarginBalance must match one of the venue windows verbatim
    // rather than mixing fields across windows.
    #[rstest]
    fn test_parse_cfm_margin_balances_picks_whole_window_not_per_field_max() {
        let summary = cfm_summary_with_windows(
            Some(cfm_window(
                CoinbaseMarginWindowType::Intraday,
                "800.00",
                "100.00",
            )),
            Some(cfm_window(
                CoinbaseMarginWindowType::Overnight,
                "500.00",
                "400.00",
            )),
        );

        let margins = parse_cfm_margin_balances(&summary).unwrap();
        assert_eq!(margins.len(), 1);
        let m = &margins[0];
        // Intraday wins on initial (800 > 500); its maintenance (100) must
        // come along, not the overnight 400 that would dominate a per-field
        // max strategy.
        assert_eq!(m.initial.as_decimal(), Decimal::from_str("800.00").unwrap());
        assert_eq!(
            m.maintenance.as_decimal(),
            Decimal::from_str("100.00").unwrap()
        );
    }

    #[rstest]
    fn test_parse_cfm_margin_balances_returns_empty_when_no_windows() {
        let summary = cfm_summary_with_windows(None, None);
        assert!(parse_cfm_margin_balances(&summary).unwrap().is_empty());
    }

    #[rstest]
    fn test_parse_cfm_margin_balances_uses_sole_intraday_window_verbatim() {
        let summary = cfm_summary_with_windows(
            Some(cfm_window(
                CoinbaseMarginWindowType::Intraday,
                "250.00",
                "125.00",
            )),
            None,
        );
        let margins = parse_cfm_margin_balances(&summary).unwrap();
        assert_eq!(margins.len(), 1);
        assert_eq!(
            margins[0].initial.as_decimal(),
            Decimal::from_str("250.00").unwrap()
        );
        assert_eq!(
            margins[0].maintenance.as_decimal(),
            Decimal::from_str("125.00").unwrap()
        );
    }

    #[rstest]
    fn test_parse_cfm_margin_balances_uses_sole_overnight_window_verbatim() {
        let summary = cfm_summary_with_windows(
            None,
            Some(cfm_window(
                CoinbaseMarginWindowType::Overnight,
                "900.00",
                "450.00",
            )),
        );
        let margins = parse_cfm_margin_balances(&summary).unwrap();
        assert_eq!(margins.len(), 1);
        assert_eq!(
            margins[0].initial.as_decimal(),
            Decimal::from_str("900.00").unwrap()
        );
        assert_eq!(
            margins[0].maintenance.as_decimal(),
            Decimal::from_str("450.00").unwrap()
        );
    }

    // Mirrors `parse_cfm_margin_balances` selector tests for the WS variant
    // so a future drift between the two selectors is caught before it ships.
    #[rstest]
    fn test_parse_ws_cfm_account_state_picks_whole_window_not_per_field_max() {
        use nautilus_model::enums::AccountType;

        use crate::websocket::messages::{WsFcmBalanceSummary, WsMarginWindowMeasure};

        fn ws_window(
            kind: CoinbaseMarginWindowType,
            initial: &str,
            maintenance: &str,
        ) -> WsMarginWindowMeasure {
            WsMarginWindowMeasure {
                margin_window_type: kind,
                margin_level: CoinbaseMarginLevel::Base,
                initial_margin: Decimal::from_str(initial).unwrap(),
                maintenance_margin: Decimal::from_str(maintenance).unwrap(),
                liquidation_buffer_percentage: Decimal::ZERO,
                total_hold: Decimal::ZERO,
                futures_buying_power: Decimal::ZERO,
            }
        }

        let summary = WsFcmBalanceSummary {
            futures_buying_power: Decimal::from_str("100.00").unwrap(),
            total_usd_balance: Decimal::from_str("500.00").unwrap(),
            cbi_usd_balance: Decimal::ZERO,
            cfm_usd_balance: Decimal::ZERO,
            total_open_orders_hold_amount: Decimal::from_str("25.00").unwrap(),
            unrealized_pnl: Decimal::ZERO,
            daily_realized_pnl: Decimal::ZERO,
            initial_margin: Decimal::ZERO,
            available_margin: Decimal::from_str("350.00").unwrap(),
            liquidation_threshold: Decimal::ZERO,
            liquidation_buffer_amount: Decimal::ZERO,
            liquidation_buffer_percentage: Decimal::ZERO,
            intraday_margin_window_measure: ws_window(
                CoinbaseMarginWindowType::Intraday,
                "800.00",
                "100.00",
            ),
            overnight_margin_window_measure: ws_window(
                CoinbaseMarginWindowType::Overnight,
                "500.00",
                "400.00",
            ),
        };

        let state = parse_ws_cfm_account_state(
            &summary,
            AccountId::new("COINBASE-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(state.account_type, AccountType::Margin);
        // Balance invariant: total == venue total_usd_balance; free == available_margin.
        assert_eq!(
            state.balances[0].total.as_decimal(),
            Decimal::from_str("500.00").unwrap()
        );
        assert_eq!(
            state.balances[0].free.as_decimal(),
            Decimal::from_str("350.00").unwrap()
        );
        // Intraday wins on initial (800 > 500); its maintenance comes along.
        assert_eq!(state.margins.len(), 1);
        assert_eq!(
            state.margins[0].initial.as_decimal(),
            Decimal::from_str("800.00").unwrap()
        );
        assert_eq!(
            state.margins[0].maintenance.as_decimal(),
            Decimal::from_str("100.00").unwrap()
        );
    }

    #[rstest]
    #[case(CoinbaseFcmPositionSide::Long, PositionSideSpecified::Long)]
    #[case(CoinbaseFcmPositionSide::Short, PositionSideSpecified::Short)]
    #[case(CoinbaseFcmPositionSide::Unspecified, PositionSideSpecified::Flat)]
    fn test_parse_cfm_position_side_maps_all_variants(
        #[case] venue_side: CoinbaseFcmPositionSide,
        #[case] expected: PositionSideSpecified,
    ) {
        let report = parse_cfm_position_status_report(
            &cfm_position(venue_side, "1", "49000.00"),
            &btc_perp_instrument(),
            AccountId::new("COINBASE-001"),
            UnixNanos::default(),
        )
        .unwrap();
        assert_eq!(report.position_side, expected);
    }

    #[rstest]
    fn test_parse_cfm_position_drops_avg_px_when_entry_zero() {
        // Coinbase reports `avg_entry_price=0` on freshly-opened positions
        // before a fill lands; Nautilus represents "no open price" as None.
        let report = parse_cfm_position_status_report(
            &cfm_position(CoinbaseFcmPositionSide::Long, "1", "0"),
            &btc_perp_instrument(),
            AccountId::new("COINBASE-001"),
            UnixNanos::default(),
        )
        .unwrap();
        assert!(report.avg_px_open.is_none());
    }

    fn cfm_amount(value: &str) -> crate::http::models::CfmAmount {
        crate::http::models::CfmAmount {
            value: Decimal::from_str(value).unwrap(),
            currency: Ustr::from("USD"),
        }
    }

    fn cfm_window(
        kind: CoinbaseMarginWindowType,
        initial: &str,
        maintenance: &str,
    ) -> crate::http::models::CfmMarginWindowMeasure {
        crate::http::models::CfmMarginWindowMeasure {
            margin_window_type: kind,
            margin_level: CoinbaseMarginLevel::Base,
            initial_margin: cfm_amount(initial),
            maintenance_margin: cfm_amount(maintenance),
            liquidation_buffer_percentage: String::new(),
            total_hold: cfm_amount("0"),
            futures_buying_power: cfm_amount("0"),
        }
    }

    fn cfm_summary_with_windows(
        intraday: Option<crate::http::models::CfmMarginWindowMeasure>,
        overnight: Option<crate::http::models::CfmMarginWindowMeasure>,
    ) -> CfmBalanceSummary {
        CfmBalanceSummary {
            futures_buying_power: cfm_amount("0"),
            total_usd_balance: cfm_amount("0"),
            cbi_usd_balance: cfm_amount("0"),
            cfm_usd_balance: cfm_amount("0"),
            total_open_orders_hold_amount: cfm_amount("0"),
            unrealized_pnl: cfm_amount("0"),
            daily_realized_pnl: cfm_amount("0"),
            initial_margin: cfm_amount("0"),
            available_margin: cfm_amount("0"),
            liquidation_threshold: cfm_amount("0"),
            liquidation_buffer_amount: cfm_amount("0"),
            liquidation_buffer_percentage: String::new(),
            intraday_margin_window_measure: intraday,
            overnight_margin_window_measure: overnight,
        }
    }

    fn cfm_position(
        side: CoinbaseFcmPositionSide,
        contracts: &str,
        avg_entry: &str,
    ) -> CfmPosition {
        CfmPosition {
            product_id: Ustr::from("BIP-20DEC30-CDE"),
            expiration_time: String::new(),
            side,
            number_of_contracts: Decimal::from_str(contracts).unwrap(),
            current_price: cfm_amount("50000.00"),
            avg_entry_price: cfm_amount(avg_entry),
            unrealized_pnl: cfm_amount("0"),
            daily_realized_pnl: cfm_amount("0"),
            total_fees: None,
            contract_size: "0.01".to_string(),
            entry_vwap: None,
            liquidation_price: None,
            leverage: String::new(),
            im_contribution: None,
            mm_contribution: None,
            position_notional: None,
        }
    }

    fn btc_perp_instrument() -> InstrumentAny {
        let json = load_test_fixture("http_products_future.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        parse_instrument(&response.products[0], UnixNanos::default()).unwrap()
    }
}
