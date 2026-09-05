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

//! Converters that translate Kraken API schemas into Nautilus domain models.

use std::{fmt::Display, str::FromStr};

use anyhow::Context;
use nautilus_core::{datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{
        AggressorSide, AssetClass, BarAggregation, LiquiditySide, OrderSide, OrderStatus,
        OrderType, PositionSide, TimeInForce, TriggerType,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{
        Instrument, any::InstrumentAny, crypto_perpetual::CryptoPerpetual,
        currency_pair::CurrencyPair, tokenized_asset::TokenizedAsset,
    },
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Money, Price, Quantity, fixed::FIXED_PRECISION},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::{
    common::{
        consts::KRAKEN_VENUE,
        enums::{
            KrakenFuturesOrderEventType, KrakenInstrumentType, KrakenPositionSide,
            KrakenSpotTrigger, KrakenTriggerSignal,
        },
    },
    http::models::{
        AssetPairInfo, FuturesFill, FuturesInstrument, FuturesOpenOrder, FuturesOrderEvent,
        FuturesPosition, FuturesPublicExecution, OhlcData, SpotOrder, SpotTrade,
    },
};

/// Parse a decimal string, handling empty strings and "0" values.
pub fn parse_decimal(value: &str) -> anyhow::Result<Decimal> {
    if value.is_empty() || value == "0" {
        return Ok(dec!(0));
    }
    value
        .parse::<Decimal>()
        .map_err(|e| anyhow::anyhow!("Failed to parse decimal '{value}': {e}"))
}

fn parse_rfc3339_timestamp(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    value
        .parse::<UnixNanos>()
        .map_err(|e| anyhow::anyhow!("Failed to parse {field}='{value}': {e}"))
}

/// Normalizes a Kraken currency code by stripping the legacy X/Z prefix.
///
/// Kraken uses legacy prefixes for some currencies (e.g., XXBT for Bitcoin, XETH for Ethereum,
/// ZUSD for USD). This function strips those prefixes for consistent lookups.
#[inline]
pub fn normalize_currency_code(code: &str) -> &str {
    code.strip_prefix("X")
        .or_else(|| code.strip_prefix("Z"))
        .unwrap_or(code)
}

/// Maps Kraken REST `wsname` base codes that differ from their WS v2 accepted equivalents.
///
/// Kraken's REST `/0/public/AssetPairs` `wsname` field is supposed to be the WS-ready
/// symbol, but some entries are stale. Each entry is `(rest_wsname_code, ws_v2_code)`.
const KRAKEN_SYMBOL_RENAMES: &[(&str, &str)] = &[
    ("XBT", "BTC"),  // XBT is Bitcoin's ISO 4217 code; WS v2 requires BTC
    ("XDG", "DOGE"), // XDG is Kraken's legacy altname for Dogecoin; WS v2 requires DOGE
];

/// Normalizes a Kraken spot `wsname` symbol to the form accepted by WS v2.
///
/// Kraken's REST API `wsname` field is supposed to be the WS-ready symbol, but some
/// codes are stale and differ from what WS v2 actually accepts. This function applies
/// all known renames so that instruments and subscriptions use consistent symbols.
/// Renames are applied to both the base and quote leg of the pair.
#[inline]
pub fn normalize_spot_symbol(symbol: &str) -> String {
    let Some((base, quote)) = symbol.split_once('/') else {
        return symbol.to_string();
    };
    let base = KRAKEN_SYMBOL_RENAMES
        .iter()
        .find(|(old, _)| *old == base)
        .map_or(base, |(_, new)| new);
    let quote = KRAKEN_SYMBOL_RENAMES
        .iter()
        .find(|(old, _)| *old == quote)
        .map_or(quote, |(_, new)| new);
    format!("{base}/{quote}")
}

/// Parse an optional decimal string.
pub fn parse_decimal_opt(value: Option<&str>) -> anyhow::Result<Option<Decimal>> {
    match value {
        Some(s) if !s.is_empty() && s != "0" => Ok(Some(parse_decimal(s)?)),
        _ => Ok(None),
    }
}

/// Parse Kraken spot trigger to Nautilus TriggerType.
fn parse_trigger_type(
    order_type: OrderType,
    trigger: Option<KrakenSpotTrigger>,
) -> Option<TriggerType> {
    let is_conditional = matches!(
        order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
    );

    if !is_conditional {
        return None;
    }

    match trigger {
        Some(KrakenSpotTrigger::Last) => Some(TriggerType::LastPrice),
        Some(KrakenSpotTrigger::Index) => Some(TriggerType::IndexPrice),
        None => Some(TriggerType::Default),
    }
}

/// Parse Kraken futures trigger signal to Nautilus TriggerType.
fn parse_futures_trigger_type(
    order_type: OrderType,
    trigger_signal: Option<KrakenTriggerSignal>,
) -> Option<TriggerType> {
    let is_conditional = matches!(
        order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
    );

    if !is_conditional {
        return None;
    }

    match trigger_signal {
        Some(KrakenTriggerSignal::Last) => Some(TriggerType::LastPrice),
        Some(KrakenTriggerSignal::Mark) => Some(TriggerType::MarkPrice),
        Some(KrakenTriggerSignal::Index) => Some(TriggerType::IndexPrice),
        Some(KrakenTriggerSignal::Unknown) => {
            log::warn!(
                "KrakenTriggerSignal::Unknown received from venue, defaulting to Default trigger"
            );
            Some(TriggerType::Default)
        }
        None => Some(TriggerType::Default),
    }
}

/// Parses a Kraken asset pair definition into a Nautilus currency pair instrument.
///
/// # Errors
///
/// Returns an error if:
/// - Tick size, order minimum, or cost minimum cannot be parsed.
/// - Price or quantity precision is invalid.
/// - Currency codes are invalid.
pub fn parse_spot_instrument(
    pair_name: &str,
    definition: &AssetPairInfo,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let symbol_str = definition.wsname.as_ref().unwrap_or(&definition.altname);
    let normalized_symbol = normalize_spot_symbol(symbol_str);
    let instrument_id = InstrumentId::new(Symbol::new(&normalized_symbol), *KRAKEN_VENUE);
    let raw_symbol = Symbol::new(pair_name);

    let base_currency = get_currency(definition.base.as_str());
    let quote_currency = get_currency(definition.quote.as_str());

    let price_increment = parse_price(
        definition
            .tick_size
            .as_ref()
            .context("tick_size is required")?,
        "tick_size",
    )?;

    // lot_decimals specifies the decimal precision for the lot size
    let size_precision = definition.lot_decimals;
    let size_increment = Quantity::from_decimal_dp(
        Decimal::try_new(1, u32::from(size_precision)).context("Invalid lot_decimals")?,
        size_precision,
    )?;

    let min_quantity = definition
        .ordermin
        .as_ref()
        .map(|s| parse_quantity(s, "ordermin"))
        .transpose()?;

    // Use base tier fees, convert from percentage
    let taker_fee = definition.fees.first().map(|(_, fee)| *fee / dec!(100));

    let maker_fee = definition
        .fees_maker
        .first()
        .map(|(_, fee)| *fee / dec!(100));

    let instrument = CurrencyPair::builder()
        .instrument_id(instrument_id)
        .raw_symbol(raw_symbol)
        .base_currency(base_currency)
        .quote_currency(quote_currency)
        .price_precision(price_increment.precision)
        .size_precision(size_increment.precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .maybe_min_quantity(min_quantity)
        .maybe_maker_fee(maker_fee)
        .maybe_taker_fee(taker_fee)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()
        .unwrap();

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a Kraken tokenized asset pair into a Nautilus tokenized asset instrument.
///
/// Tokenized assets (xStocks) use the same API schema as spot pairs but represent
/// real-world equities, ETFs, or other tokenized securities.
///
/// # Errors
///
/// Returns an error if tick size, order minimum, or fee fields cannot be parsed.
pub fn parse_tokenized_instrument(
    pair_name: &str,
    definition: &AssetPairInfo,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let symbol_str = definition.wsname.as_ref().unwrap_or(&definition.altname);
    let normalized_symbol = normalize_spot_symbol(symbol_str);
    let instrument_id = InstrumentId::new(Symbol::new(&normalized_symbol), *KRAKEN_VENUE);
    let raw_symbol = Symbol::new(pair_name);

    let base_currency = get_currency(definition.base.as_str());
    let quote_currency = get_currency(definition.quote.as_str());

    let price_increment = parse_price(
        definition
            .tick_size
            .as_ref()
            .context("tick_size is required")?,
        "tick_size",
    )?;

    let size_precision = definition.lot_decimals;
    let size_increment = Quantity::from_decimal_dp(
        Decimal::try_new(1, u32::from(size_precision)).context("Invalid lot_decimals")?,
        size_precision,
    )?;

    let min_quantity = definition
        .ordermin
        .as_ref()
        .map(|s| parse_quantity(s, "ordermin"))
        .transpose()?;

    let taker_fee = definition.fees.first().map(|(_, fee)| *fee / dec!(100));

    let maker_fee = definition
        .fees_maker
        .first()
        .map(|(_, fee)| *fee / dec!(100));

    let instrument = TokenizedAsset::builder()
        .instrument_id(instrument_id)
        .raw_symbol(raw_symbol)
        .asset_class(AssetClass::Equity)
        .base_currency(base_currency)
        .quote_currency(quote_currency)
        .price_precision(price_increment.precision)
        .size_precision(size_increment.precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .maybe_min_quantity(min_quantity)
        .maybe_maker_fee(maker_fee)
        .maybe_taker_fee(taker_fee)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()
        .unwrap();

    Ok(InstrumentAny::TokenizedAsset(instrument))
}

/// Parses a Kraken futures instrument definition into a Nautilus crypto perpetual instrument.
///
/// # Errors
///
/// Returns an error if:
/// - Tick size cannot be parsed as a valid price.
/// - Contract size cannot be parsed as a valid quantity.
/// - Tick size, contract value trade precision, or contract size exceeds the active fixed
///   precision.
/// - Currency codes are invalid.
///
/// In standard-precision builds, an unsupported-precision error identifies the instrument,
/// required precision, supported maximum, and the `high-precision` rebuild action.
pub fn parse_futures_instrument(
    instrument: &FuturesInstrument,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(&instrument.symbol), *KRAKEN_VENUE);
    let raw_symbol = Symbol::new(&instrument.symbol);

    let base_currency = get_currency(&instrument.base);
    let quote_currency = get_currency(&instrument.quote);

    let is_inverse = instrument.instrument_type == KrakenInstrumentType::FuturesInverse;
    let settlement_currency = if is_inverse {
        base_currency
    } else {
        quote_currency
    };

    // Normalize before deriving precision so wire padding does not overstate the tick precision
    let tick_size = instrument.tick_size.normalize();
    let price_precision = tick_size.scale();
    check_futures_precision(&instrument.symbol, "tick_size", tick_size, price_precision)?;
    let price_precision = u8::try_from(price_precision).context("Invalid tick_size precision")?;
    let price_increment = Price::from_decimal_dp(tick_size, price_precision)?;

    // Use contract_value_trade_precision for the tradeable size increment
    // Positive values (e.g., 3) mean fractional sizes (0.001)
    // Negative values (e.g., -3) mean multiples of powers of 10 (1000) - used for meme coins
    // Zero means whole number increments (1)
    let size_increment = if instrument.contract_value_trade_precision >= 0 {
        let precision = u32::try_from(instrument.contract_value_trade_precision)
            .context("Invalid contract_value_trade_precision")?;
        check_futures_precision(
            &instrument.symbol,
            "contract_value_trade_precision",
            instrument.contract_value_trade_precision,
            precision,
        )?;
        let precision =
            u8::try_from(precision).context("Invalid contract_value_trade_precision")?;
        Quantity::from_decimal_dp(
            Decimal::try_new(1, u32::from(precision))
                .context("Invalid contract_value_trade_precision")?,
            precision,
        )?
    } else {
        // Negative precision: increment is 10^abs(precision), e.g., -3 means 1000
        let exponent = instrument.contract_value_trade_precision.unsigned_abs();
        let increment_value = 10_i64
            .checked_pow(exponent)
            .context("contract_value_trade_precision exceeds supported range")?;
        Quantity::from_decimal_dp(Decimal::from(increment_value), 0)?
    };

    let contract_size = instrument.contract_size.normalize();
    let multiplier_precision = contract_size.scale();
    check_futures_precision(
        &instrument.symbol,
        "contract_size",
        contract_size,
        multiplier_precision,
    )?;
    let multiplier_precision =
        u8::try_from(multiplier_precision).context("Invalid contract_size precision")?;
    let multiplier = Some(Quantity::from_decimal_dp(
        contract_size,
        multiplier_precision,
    )?);

    // Use first margin level if available
    let (margin_init, margin_maint) = instrument
        .margin_levels
        .first()
        .map_or((None, None), |level| {
            (Some(level.initial_margin), Some(level.maintenance_margin))
        });

    let instrument = CryptoPerpetual::builder()
        .instrument_id(instrument_id)
        .raw_symbol(raw_symbol)
        .base_currency(base_currency)
        .quote_currency(quote_currency)
        .settlement_currency(settlement_currency)
        .is_inverse(is_inverse)
        .price_precision(price_increment.precision)
        .size_precision(size_increment.precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .maybe_multiplier(multiplier)
        .maybe_margin_init(margin_init)
        .maybe_margin_maint(margin_maint)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()
        .unwrap();

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

fn check_futures_precision(
    symbol: &str,
    field: &str,
    value: impl Display,
    precision: u32,
) -> anyhow::Result<()> {
    if precision <= u32::from(FIXED_PRECISION) {
        return Ok(());
    }

    #[cfg(feature = "high-precision")]
    anyhow::bail!(
        "Cannot parse Kraken Futures instrument '{symbol}': {field} {value} requires precision \
         {precision}, but this build supports at most {FIXED_PRECISION}"
    );

    #[cfg(not(feature = "high-precision"))]
    anyhow::bail!(
        "Cannot parse Kraken Futures instrument '{symbol}': {field} {value} requires precision \
         {precision}, but this build supports at most {FIXED_PRECISION}; enable the \
         'high-precision' Cargo feature and rebuild"
    );
}

fn parse_price(value: &str, field: &str) -> anyhow::Result<Price> {
    Price::from_str(value).map_err(|e| anyhow::anyhow!("Failed to parse {field}='{value}': {e}"))
}

fn parse_quantity(value: &str, field: &str) -> anyhow::Result<Quantity> {
    Quantity::from_str(value).map_err(|e| anyhow::anyhow!("Failed to parse {field}='{value}': {e}"))
}

/// Returns a currency from the internal map or creates a new crypto currency.
///
/// Uses [`Currency::get_or_create_crypto`] to handle unknown currency codes,
/// which automatically registers newly listed Kraken assets.
pub fn get_currency(code: &str) -> Currency {
    Currency::get_or_create_crypto(code)
}

/// Parses a Kraken trade array into a Nautilus trade tick.
///
/// The Kraken API returns trades as arrays: [price, volume, time, side, type, misc, trade_id]
///
/// # Errors
///
/// Returns an error if:
/// - Price or volume cannot be parsed.
/// - Timestamp is invalid.
/// - Trade ID is invalid.
pub fn parse_trade_tick_from_array(
    trade_array: &[serde_json::Value],
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price_str = trade_array
        .first()
        .and_then(|v| v.as_str())
        .context("Missing or invalid price")?;
    let price = parse_price_with_precision(price_str, instrument.price_precision(), "trade.price")?;

    let size_str = trade_array
        .get(1)
        .and_then(|v| v.as_str())
        .context("Missing or invalid volume")?;
    let size = parse_quantity_with_precision(size_str, instrument.size_precision(), "trade.size")?;

    let time = trade_array
        .get(2)
        .and_then(|v| v.as_f64())
        .context("Missing or invalid timestamp")?;
    let ts_event = parse_millis_timestamp(time, "trade.time")?;

    let side_str = trade_array
        .get(3)
        .and_then(|v| v.as_str())
        .context("Missing or invalid side")?;
    let aggressor = match side_str {
        "b" => AggressorSide::Buy,
        "s" => AggressorSide::Sell,
        _ => AggressorSide::NoAggressor,
    };

    let trade_id_value = trade_array.get(6).context("Missing trade_id")?;
    let trade_id = if let Some(id) = trade_id_value.as_i64() {
        TradeId::new_checked(id.to_string())?
    } else if let Some(id_str) = trade_id_value.as_str() {
        TradeId::new_checked(id_str)?
    } else {
        anyhow::bail!("Invalid trade_id format");
    };

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Kraken trade")
}

/// Parses a Kraken Futures public execution into a Nautilus trade tick.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity cannot be parsed.
/// - Trade ID is invalid.
pub fn parse_futures_public_execution(
    execution: &FuturesPublicExecution,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price =
        parse_price_with_precision(&execution.price, instrument.price_precision(), "price")?;
    let size = parse_quantity_with_precision(
        &execution.quantity,
        instrument.size_precision(),
        "quantity",
    )?;

    // Timestamp is in milliseconds
    let ts_event = UnixNanos::from((execution.timestamp as u64) * 1_000_000);

    // Aggressor side is determined by the taker's direction
    let aggressor = match execution.taker_order.direction.to_lowercase().as_str() {
        "buy" => AggressorSide::Buy,
        "sell" => AggressorSide::Sell,
        _ => AggressorSide::NoAggressor,
    };

    let trade_id = TradeId::new_checked(&execution.uid)?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Kraken futures execution")
}

/// Parses a Kraken OHLC entry into a Nautilus bar.
///
/// # Errors
///
/// Returns an error if:
/// - OHLC values cannot be parsed.
/// - Timestamp is invalid.
pub fn parse_bar(
    ohlc: &OhlcData,
    instrument: &InstrumentAny,
    bar_type: BarType,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = parse_price_with_precision(&ohlc.open, price_precision, "ohlc.open")?;
    let high = parse_price_with_precision(&ohlc.high, price_precision, "ohlc.high")?;
    let low = parse_price_with_precision(&ohlc.low, price_precision, "ohlc.low")?;
    let close = parse_price_with_precision(&ohlc.close, price_precision, "ohlc.close")?;
    let volume = parse_quantity_with_precision(&ohlc.volume, size_precision, "ohlc.volume")?;

    let ts_event = UnixNanos::from((ohlc.time as u64) * 1_000_000_000);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("Failed to construct Bar from Kraken OHLC")
}

fn parse_price_with_precision(value: &str, precision: u8, field: &str) -> anyhow::Result<Price> {
    let parsed = value
        .parse::<Decimal>()
        .with_context(|| format!("Failed to parse {field}='{value}' as Decimal"))?;
    Price::from_decimal_dp(parsed, precision).with_context(|| {
        format!("Failed to construct Price for {field} with precision {precision}")
    })
}

fn parse_quantity_with_precision(
    value: &str,
    precision: u8,
    field: &str,
) -> anyhow::Result<Quantity> {
    let parsed = value
        .parse::<Decimal>()
        .with_context(|| format!("Failed to parse {field}='{value}' as Decimal"))?;
    Quantity::from_decimal_dp(parsed, precision).with_context(|| {
        format!("Failed to construct Quantity for {field} with precision {precision}")
    })
}

pub fn parse_millis_timestamp(value: f64, field: &str) -> anyhow::Result<UnixNanos> {
    let millis = (value * 1000.0) as u64;
    let nanos = millis
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .with_context(|| format!("{field} timestamp overflowed when converting to nanoseconds"))?;
    Ok(UnixNanos::from(nanos))
}

/// Parses a Kraken spot order into a Nautilus OrderStatusReport.
///
/// # Errors
///
/// Returns an error if:
/// - Order ID, quantities, or prices cannot be parsed.
/// - Order status mapping fails.
pub fn parse_order_status_report(
    order_id: &str,
    order: &SpotOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order_id);

    let order_side = OrderSide::from(order.descr.order_side).into();
    let order_type = order.descr.ordertype.into();
    let order_status = order.status.into();

    // Kraken returns expiretm=0 for GTC orders, so check for actual expiration value
    let has_expiration = order.expiretm.is_some_and(|t| t > 0.0);
    let time_in_force = if has_expiration {
        TimeInForce::Gtd
    } else if order.oflags.contains("ioc") {
        TimeInForce::Ioc
    } else {
        TimeInForce::Gtc
    };

    let quantity =
        parse_quantity_with_precision(&order.vol, instrument.size_precision(), "order.vol")?;

    let filled_qty = parse_quantity_with_precision(
        &order.vol_exec,
        instrument.size_precision(),
        "order.vol_exec",
    )?;

    let ts_accepted = parse_millis_timestamp(order.opentm, "order.opentm")?;

    let ts_last = order
        .closetm
        .map(|t| parse_millis_timestamp(t, "order.closetm"))
        .transpose()?
        .unwrap_or(ts_accepted);

    let price = if !order.price.is_empty() && order.price != "0" {
        Some(parse_price_with_precision(
            &order.price,
            instrument.price_precision(),
            "order.price",
        )?)
    } else {
        None
    };

    let trigger_price = order
        .stopprice
        .as_ref()
        .and_then(|p| {
            if !p.is_empty() && p != "0" {
                Some(parse_price_with_precision(
                    p,
                    instrument.price_precision(),
                    "order.stopprice",
                ))
            } else {
                None
            }
        })
        .transpose()?;

    let expire_time = if has_expiration {
        order
            .expiretm
            .map(|t| parse_millis_timestamp(t, "order.expiretm"))
            .transpose()?
    } else {
        None
    };

    let trigger_type = parse_trigger_type(order_type, order.trigger);

    Ok(OrderStatusReport {
        account_id,
        instrument_id,
        client_order_id: None,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
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
        contingency_type: None,
        expire_time,
        price,
        activation_price: None,
        trigger_price,
        trigger_type,
        limit_offset: None,
        trailing_offset: None,
        trailing_offset_type: None,
        display_qty: None,
        avg_px: compute_avg_px(order),
        post_only: order.oflags.contains("post"),
        reduce_only: false,
        cancel_reason: order.reason.clone(),
        ts_triggered: None,
    })
}

/// Computes the average price for a Kraken spot order.
///
/// Prefers the direct `avg_price` field if available, otherwise calculates from `cost / vol_exec`.
fn compute_avg_px(order: &SpotOrder) -> Option<Decimal> {
    if let Some(ref avg) = order.avg_price
        && let Ok(v) = parse_decimal(avg)
        && v > dec!(0)
    {
        return Some(v);
    }

    let cost = parse_decimal(&order.cost);
    let vol_exec = parse_decimal(&order.vol_exec);
    match (&cost, &vol_exec) {
        (Ok(c), Ok(v)) if *v > dec!(0) => Some(*c / *v),
        _ => {
            if let Ok(v) = &vol_exec
                && *v > dec!(0)
            {
                log::warn!("Cannot compute avg_px: cost={cost:?}, vol_exec={vol_exec:?}");
            }
            None
        }
    }
}

/// Parses a Kraken spot trade into a Nautilus FillReport.
///
/// # Errors
///
/// Returns an error if:
/// - Trade ID, quantities, or prices cannot be parsed.
pub fn parse_fill_report(
    trade_id: &str,
    trade: &SpotTrade,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&trade.ordertxid);
    let trade_id_obj = TradeId::new(trade_id);

    let order_side = trade.trade_type.into();

    let last_qty =
        parse_quantity_with_precision(&trade.vol, instrument.size_precision(), "trade.vol")?;

    let last_px =
        parse_price_with_precision(&trade.price, instrument.price_precision(), "trade.price")?;

    let fee_decimal = parse_decimal(&trade.fee)?;
    let quote_currency = match instrument {
        InstrumentAny::CurrencyPair(pair) => pair.quote_currency,
        InstrumentAny::CryptoPerpetual(perp) => perp.quote_currency,
        InstrumentAny::TokenizedAsset(ta) => ta.quote_currency,
        _ => anyhow::bail!("Unsupported instrument type for fill report"),
    };

    let commission = Money::from_decimal(fee_decimal, quote_currency)?;

    let liquidity_side = match trade.maker {
        Some(true) => LiquiditySide::Maker,
        Some(false) => LiquiditySide::Taker,
        None => LiquiditySide::NoLiquiditySide,
    };

    let ts_event = parse_millis_timestamp(trade.time, "trade.time")?;

    Ok(FillReport {
        account_id,
        instrument_id,
        venue_order_id,
        trade_id: trade_id_obj,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        avg_px: None,
        report_id: UUID4::new(),
        ts_event,
        ts_init,
        client_order_id: None,
        venue_position_id: None,
    })
}

/// Parses a Kraken futures open order into a Nautilus OrderStatusReport.
///
/// # Errors
///
/// Returns an error if order ID, quantities, or prices cannot be parsed.
pub fn parse_futures_order_status_report(
    order: &FuturesOpenOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    fallback_quantity: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&order.order_id);

    let order_side = OrderSide::from(order.side).into();
    let order_type: OrderType = order.order_type.into();
    let order_type = if order_type == OrderType::MarketIfTouched && order.limit_price.is_some() {
        OrderType::LimitIfTouched
    } else {
        order_type
    };
    let order_status = order.status.into();

    let quantity_value = order
        .unfilled_size
        .map(|unfilled_size| unfilled_size + order.filled_size)
        .or(fallback_quantity)
        .context("missing unfilled size and fallback quantity")?;
    let quantity = Quantity::from_decimal_dp(quantity_value, instrument.size_precision())?;

    let filled_qty = Quantity::from_decimal_dp(order.filled_size, instrument.size_precision())?;

    let ts_accepted = parse_rfc3339_timestamp(&order.received_time, "order.received_time")?;
    let ts_last = parse_rfc3339_timestamp(&order.last_update_time, "order.last_update_time")?;

    let price = order
        .limit_price
        .map(|p| Price::from_decimal_dp(p, instrument.price_precision()))
        .transpose()?;

    let trigger_price = order
        .stop_price
        .map(|p| Price::from_decimal_dp(p, instrument.price_precision()))
        .transpose()?;

    let trigger_type = parse_futures_trigger_type(order_type, order.trigger_signal);

    Ok(OrderStatusReport {
        account_id,
        instrument_id,
        client_order_id: order.cli_ord_id.as_ref().map(|s| s.as_str().into()),
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
        contingency_type: None,
        expire_time: None,
        price,
        activation_price: None,
        trigger_price,
        trigger_type,
        limit_offset: None,
        trailing_offset: None,
        trailing_offset_type: None,
        display_qty: None,
        avg_px: None,
        post_only: false,
        reduce_only: order.reduce_only.unwrap_or(false),
        cancel_reason: None,
        ts_triggered: None,
    })
}

/// Parses a Kraken futures order event (historical order) into a Nautilus OrderStatusReport.
///
/// # Errors
///
/// Returns an error if order ID, quantities, or prices cannot be parsed.
pub fn parse_futures_order_event_status_report(
    event: &FuturesOrderEvent,
    event_type: Option<KrakenFuturesOrderEventType>,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&event.order_id);

    let order_side = OrderSide::from(event.side).into();
    let order_type: OrderType = event.order_type.into();
    let order_type = if order_type == OrderType::MarketIfTouched && event.limit_price.is_some() {
        OrderType::LimitIfTouched
    } else {
        order_type
    };

    let order_status = parse_futures_order_event_status(event_type, event.filled, event.quantity);

    let quantity = Quantity::from_decimal_dp(event.quantity, instrument.size_precision())?;
    let filled_qty = Quantity::from_decimal_dp(event.filled, instrument.size_precision())?;

    let ts_accepted = parse_rfc3339_timestamp(&event.timestamp, "event.timestamp")?;
    let ts_last =
        parse_rfc3339_timestamp(&event.last_update_timestamp, "event.last_update_timestamp")?;

    let price = event
        .limit_price
        .map(|p| Price::from_decimal_dp(p, instrument.price_precision()))
        .transpose()?;

    let trigger_price = event
        .stop_price
        .map(|p| Price::from_decimal_dp(p, instrument.price_precision()))
        .transpose()?;

    let trigger_type = parse_futures_trigger_type(order_type, None);

    Ok(OrderStatusReport {
        account_id,
        instrument_id,
        client_order_id: event.cli_ord_id.as_ref().map(|s| s.as_str().into()),
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
        contingency_type: None,
        expire_time: None,
        price,
        activation_price: None,
        trigger_price,
        trigger_type,
        limit_offset: None,
        trailing_offset: None,
        trailing_offset_type: None,
        display_qty: None,
        avg_px: None,
        post_only: false,
        reduce_only: event.reduce_only,
        cancel_reason: None,
        ts_triggered: None,
    })
}

fn parse_futures_order_event_status(
    event_type: Option<KrakenFuturesOrderEventType>,
    filled: Decimal,
    quantity: Decimal,
) -> OrderStatus {
    match event_type {
        Some(KrakenFuturesOrderEventType::Cancel) => OrderStatus::Canceled,
        Some(KrakenFuturesOrderEventType::Reject) => OrderStatus::Rejected,
        Some(KrakenFuturesOrderEventType::Expire) => OrderStatus::Expired,
        Some(
            KrakenFuturesOrderEventType::Fill
            | KrakenFuturesOrderEventType::Execution
            | KrakenFuturesOrderEventType::Place
            | KrakenFuturesOrderEventType::Edit,
        ) => {
            if filled >= quantity {
                OrderStatus::Filled
            } else if filled > Decimal::ZERO {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::Accepted
            }
        }
        _ => {
            if filled >= quantity {
                OrderStatus::Filled
            } else if filled > Decimal::ZERO {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::Canceled
            }
        }
    }
}

/// Parses a Kraken futures fill into a Nautilus FillReport.
///
/// # Errors
///
/// Returns an error if fill ID, quantities, or prices cannot be parsed.
pub fn parse_futures_fill_report(
    fill: &FuturesFill,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&fill.order_id);
    let trade_id = TradeId::new(&fill.fill_id);

    let order_side = fill.side.into();

    let last_qty = Quantity::from_decimal_dp(fill.size, instrument.size_precision())?;
    let last_px = Price::from_decimal_dp(fill.price, instrument.price_precision())?;

    let quote_currency = match instrument {
        InstrumentAny::CryptoPerpetual(perp) => perp.quote_currency,
        InstrumentAny::CryptoFuture(future) => future.quote_currency,
        _ => anyhow::bail!("Unsupported instrument type for futures fill report"),
    };

    let commission = Money::from_decimal(fill.fee_paid.unwrap_or(Decimal::ZERO), quote_currency)?;

    let liquidity_side = fill.fill_type.into();

    let ts_event = parse_rfc3339_timestamp(&fill.fill_time, "fill.fill_time")?;

    Ok(FillReport {
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        avg_px: None,
        report_id: UUID4::new(),
        ts_event,
        ts_init,
        client_order_id: fill.cli_ord_id.as_ref().map(|s| s.as_str().into()),
        venue_position_id: None,
    })
}

/// Parses a Kraken futures position into a Nautilus PositionStatusReport.
///
/// # Errors
///
/// Returns an error if position quantities or prices cannot be parsed.
pub fn parse_futures_position_status_report(
    position: &FuturesPosition,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = instrument.id();

    let position_side = match position.side {
        KrakenPositionSide::Long => PositionSide::Long,
        KrakenPositionSide::Short => PositionSide::Short,
    };

    let quantity = Quantity::from_decimal_dp(position.size, instrument.size_precision())?;
    let signed_decimal_qty = match position_side {
        PositionSide::Long => position.size,
        PositionSide::Short => -position.size,
        PositionSide::Flat => dec!(0),
    };

    let avg_px_open = Some(position.price);

    Ok(PositionStatusReport {
        account_id,
        instrument_id,
        position_side,
        quantity,
        signed_decimal_qty,
        report_id: UUID4::new(),
        ts_last: ts_init,
        ts_init,
        venue_position_id: None,
        avg_px_open,
    })
}

/// Converts a Nautilus BarType to Kraken Spot API interval (in minutes).
///
/// # Errors
///
/// Returns an error if:
/// - Bar aggregation type is not supported (only Minute, Hour, Day are valid).
/// - Bar step is not supported for the aggregation type.
pub fn bar_type_to_spot_interval(bar_type: BarType) -> anyhow::Result<u32> {
    let step = bar_type.spec().step.get() as u32;
    let base_interval = match bar_type.spec().aggregation {
        BarAggregation::Minute => 1,
        BarAggregation::Hour => 60,
        BarAggregation::Day => 1440,
        other => {
            anyhow::bail!("Unsupported bar aggregation for Kraken Spot: {other:?}");
        }
    };
    Ok(base_interval * step)
}

/// Converts a Nautilus BarType to Kraken Futures API resolution string.
///
/// Supported resolutions: 1m, 5m, 15m, 1h, 4h, 12h, 1d, 1w
///
/// # Errors
///
/// Returns an error if:
/// - Bar aggregation type is not supported.
/// - Bar step is not supported for the aggregation type.
pub fn bar_type_to_futures_resolution(bar_type: BarType) -> anyhow::Result<&'static str> {
    let step = bar_type.spec().step.get() as u32;
    match bar_type.spec().aggregation {
        BarAggregation::Minute => match step {
            1 => Ok("1m"),
            5 => Ok("5m"),
            15 => Ok("15m"),
            _ => anyhow::bail!("Unsupported minute step for Kraken Futures: {step}"),
        },
        BarAggregation::Hour => match step {
            1 => Ok("1h"),
            4 => Ok("4h"),
            12 => Ok("12h"),
            _ => anyhow::bail!("Unsupported hour step for Kraken Futures: {step}"),
        },
        BarAggregation::Day => {
            if step == 1 {
                Ok("1d")
            } else {
                anyhow::bail!("Unsupported day step for Kraken Futures: {step}")
            }
        }
        BarAggregation::Week => {
            if step == 1 {
                Ok("1w")
            } else {
                anyhow::bail!("Unsupported week step for Kraken Futures: {step}")
            }
        }
        other => {
            anyhow::bail!("Unsupported bar aggregation for Kraken Futures: {other:?}");
        }
    }
}

/// Truncates a `ClientOrderId` for Kraken's `cl_ord_id` field.
///
/// Kraken accepts three formats:
/// - Long UUID (36 chars with hyphens): passed through
/// - Short UUID (32 hex chars): passed through
/// - Free text: max 18 chars
///
/// Sequential NautilusTrader IDs (e.g. `O202602270023210040011`) exceed the
/// 18-char free-text limit. These are truncated to 'O' + last 17 chars,
/// preserving the counter portion for maximum entropy.
pub fn truncate_cl_ord_id(client_order_id: &ClientOrderId) -> String {
    let id = client_order_id.as_str();

    if id.len() <= 18 {
        return id.to_string();
    }

    if id.len() == 36 && id.bytes().filter(|b| *b == b'-').count() == 4 {
        return id.to_string();
    }

    if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return id.to_string();
    }

    format!("O{}", &id[id.len() - 17..])
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use nautilus_model::{
        data::BarSpecification,
        enums::{AggregationSource, BarAggregation, OrderSide, OrderStatus, PriceType},
        instruments::crypto_perpetual::CryptoPerpetual,
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::enums::{
            KrakenFuturesOrderEventType, KrakenFuturesOrderStatus, KrakenFuturesOrderType,
            KrakenOrderSide,
        },
        http::{
            futures::models::{FuturesFillsResponse, FuturesOpenOrder, FuturesOrderEvent},
            models::{AssetPairsResponse, KrakenResponse},
        },
    };

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    fn load_test_json(filename: &str) -> String {
        let path = format!("test_data/{filename}");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load test data from {path}: {e}"))
    }

    #[rstest]
    fn test_parse_decimal() {
        assert_eq!(parse_decimal("123.45").unwrap(), dec!(123.45));
        assert_eq!(parse_decimal("0").unwrap(), dec!(0));
        assert_eq!(parse_decimal("").unwrap(), dec!(0));
    }

    #[rstest]
    fn test_parse_decimal_opt() {
        assert_eq!(
            parse_decimal_opt(Some("123.45")).unwrap(),
            Some(dec!(123.45))
        );
        assert_eq!(parse_decimal_opt(Some("0")).unwrap(), None);
        assert_eq!(parse_decimal_opt(Some("")).unwrap(), None);
        assert_eq!(parse_decimal_opt(None).unwrap(), None);
    }

    #[rstest]
    fn test_parse_spot_instrument() {
        let json = load_test_json("http_asset_pairs.json");
        let response: KrakenResponse<AssetPairsResponse> = serde_json::from_str(&json).unwrap();
        let pairs = response.result.unwrap();

        let (pair_name, definition) = pairs.iter().next().unwrap();

        let instrument = parse_spot_instrument(pair_name, definition, TS, TS).unwrap();

        match instrument {
            InstrumentAny::CurrencyPair(pair) => {
                assert_eq!(pair.id.venue.as_str(), "KRAKEN");
                assert_eq!(pair.base_currency.code.as_str(), "XXBT");
                assert_eq!(pair.quote_currency.code.as_str(), "USDT");
                assert_eq!(pair.price_increment.as_decimal(), dec!(0.1));
                assert_eq!(pair.size_increment.as_decimal(), dec!(0.00000001));
                assert!(pair.min_quantity.is_some());
                assert_eq!(pair.maker_fee, dec!(0.0025));
                assert_eq!(pair.taker_fee, dec!(0.004));
                assert_eq!(pair.margin_init, dec!(0));
                assert_eq!(pair.margin_maint, dec!(0));
            }
            _ => panic!("Expected CurrencyPair"),
        }
    }

    #[rstest]
    fn test_parse_futures_instrument_inverse() {
        let json = load_test_json("http_futures_instruments.json");
        let response: crate::http::models::FuturesInstrumentsResponse =
            serde_json::from_str(&json).unwrap();

        let fut_instrument = &response.instruments[0];

        let instrument = parse_futures_instrument(fut_instrument, TS, TS).unwrap();

        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.venue.as_str(), "KRAKEN");
                assert_eq!(perp.id.symbol.as_str(), "PI_XBTUSD");
                assert_eq!(perp.raw_symbol.as_str(), "PI_XBTUSD");
                assert_eq!(perp.base_currency.code.as_str(), "BTC");
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert_eq!(perp.settlement_currency.code.as_str(), "BTC");
                assert!(perp.is_inverse);
                assert_eq!(perp.price_increment.as_decimal(), dec!(0.5));
                assert_eq!(perp.size_increment.as_decimal(), dec!(1));
                assert_eq!(perp.size_precision(), 0);
                assert_eq!(perp.margin_init, dec!(0.02));
                assert_eq!(perp.margin_maint, dec!(0.01));
            }
            _ => panic!("Expected CryptoPerpetual"),
        }
    }

    #[rstest]
    fn test_parse_futures_instrument_flexible() {
        let json = load_test_json("http_futures_instruments.json");
        let response: crate::http::models::FuturesInstrumentsResponse =
            serde_json::from_str(&json).unwrap();

        let fut_instrument = &response.instruments[1];

        let instrument = parse_futures_instrument(fut_instrument, TS, TS).unwrap();

        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.venue.as_str(), "KRAKEN");
                assert_eq!(perp.id.symbol.as_str(), "PF_ETHUSD");
                assert_eq!(perp.raw_symbol.as_str(), "PF_ETHUSD");
                assert_eq!(perp.base_currency.code.as_str(), "ETH");
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert_eq!(perp.settlement_currency.code.as_str(), "USD");
                assert!(!perp.is_inverse);
                assert_eq!(perp.price_increment.as_decimal(), dec!(0.1));
                assert_eq!(perp.size_increment.as_decimal(), dec!(0.001));
                assert_eq!(perp.size_precision(), 3);
                assert_eq!(perp.margin_init, dec!(0.02));
                assert_eq!(perp.margin_maint, dec!(0.01));
            }
            _ => panic!("Expected CryptoPerpetual"),
        }
    }

    #[rstest]
    fn test_parse_futures_instrument_accepts_max_precision() {
        let json = load_test_json("http_futures_instruments.json");
        let response: crate::http::models::FuturesInstrumentsResponse =
            serde_json::from_str(&json).unwrap();
        let mut fut_instrument = response.instruments[1].clone();
        let tick_size = Decimal::try_new(1, u32::from(FIXED_PRECISION)).unwrap();
        fut_instrument.tick_size = tick_size;

        let instrument = parse_futures_instrument(&fut_instrument, TS, TS).unwrap();

        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.price_precision(), FIXED_PRECISION);
                assert_eq!(perp.price_increment.as_decimal(), tick_size);
            }
            _ => panic!("Expected CryptoPerpetual"),
        }
    }

    #[cfg(feature = "high-precision")]
    #[rstest]
    fn test_parse_futures_instrument_negative_precision() {
        let json = load_test_json("http_futures_instruments.json");
        let response: crate::http::models::FuturesInstrumentsResponse =
            serde_json::from_str(&json).unwrap();

        // PF_PEPEUSD has contractValueTradePrecision: -3 (trades in multiples of 1000)
        let fut_instrument = &response.instruments[2];

        let instrument = parse_futures_instrument(fut_instrument, TS, TS).unwrap();

        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.symbol.as_str(), "PF_PEPEUSD");
                assert_eq!(perp.base_currency.code.as_str(), "PEPE");
                assert!(!perp.is_inverse);
                assert_eq!(perp.size_increment.as_decimal(), dec!(1000));
                assert_eq!(perp.size_precision(), 0);
            }
            _ => panic!("Expected CryptoPerpetual"),
        }
    }

    #[cfg(feature = "high-precision")]
    #[rstest]
    fn test_parse_futures_instrument_rejects_precision_above_high_max() {
        let json = load_test_json("http_futures_instruments.json");
        let response: crate::http::models::FuturesInstrumentsResponse =
            serde_json::from_str(&json).unwrap();
        let mut fut_instrument = response.instruments[1].clone();
        fut_instrument.tick_size = dec!(0.00000000000000001);

        let error = parse_futures_instrument(&fut_instrument, TS, TS).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Cannot parse Kraken Futures instrument 'PF_ETHUSD': tick_size \
             0.00000000000000001 requires precision 17, but this build supports at most 16"
        );
    }

    #[cfg(not(feature = "high-precision"))]
    #[rstest]
    fn test_parse_futures_instrument_rejects_unsupported_precision() {
        let json = load_test_json("http_futures_instruments.json");
        let response: crate::http::models::FuturesInstrumentsResponse =
            serde_json::from_str(&json).unwrap();

        let tick_error = parse_futures_instrument(&response.instruments[2], TS, TS).unwrap_err();

        let mut trade_precision_instrument = response.instruments[1].clone();
        trade_precision_instrument.contract_value_trade_precision = 256;
        let trade_precision_error =
            parse_futures_instrument(&trade_precision_instrument, TS, TS).unwrap_err();

        let mut contract_size_instrument = response.instruments[1].clone();
        contract_size_instrument.contract_size = dec!(0.0000000001);
        let contract_size_error =
            parse_futures_instrument(&contract_size_instrument, TS, TS).unwrap_err();

        assert_eq!(
            tick_error.to_string(),
            "Cannot parse Kraken Futures instrument 'PF_PEPEUSD': tick_size 0.0000000001 requires \
             precision 10, but this build supports at most 9; enable the 'high-precision' Cargo \
             feature and rebuild"
        );
        assert_eq!(
            trade_precision_error.to_string(),
            "Cannot parse Kraken Futures instrument 'PF_ETHUSD': contract_value_trade_precision 256 \
             requires precision 256, but this build supports at most 9; enable the 'high-precision' \
             Cargo feature and rebuild"
        );
        assert_eq!(
            contract_size_error.to_string(),
            "Cannot parse Kraken Futures instrument 'PF_ETHUSD': contract_size 0.0000000001 requires \
             precision 10, but this build supports at most 9; enable the 'high-precision' Cargo \
             feature and rebuild"
        );
    }

    #[rstest]
    fn test_parse_futures_instrument_tokenized_underlying() {
        let json = load_test_json("http_futures_instruments.json");
        let response: crate::http::models::FuturesInstrumentsResponse =
            serde_json::from_str(&json).unwrap();

        let fut_instrument = &response.instruments[3];

        let instrument = parse_futures_instrument(fut_instrument, TS, TS).unwrap();

        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.symbol.as_str(), "PF_AAPLxUSD");
                assert_eq!(perp.raw_symbol.as_str(), "PF_AAPLxUSD");
                assert_eq!(perp.base_currency.code.as_str(), "AAPLx");
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert_eq!(perp.settlement_currency.code.as_str(), "USD");
                assert!(!perp.is_inverse);
                assert_eq!(perp.price_increment.as_decimal(), dec!(0.01));
                assert_eq!(perp.size_increment.as_decimal(), dec!(0.01));
                assert_eq!(perp.size_precision(), 2);
                assert_eq!(perp.margin_init, dec!(0.2));
                assert_eq!(perp.margin_maint, dec!(0.1));
            }
            _ => panic!("Expected CryptoPerpetual"),
        }
    }

    #[rstest]
    fn test_parse_futures_instrument_without_fee_schedule_uid() {
        let json = load_test_json("http_futures_instrument_no_fee_schedule.json");
        let response: crate::http::models::FuturesInstrumentsResponse =
            serde_json::from_str(&json).unwrap();

        let fut_instrument = &response.instruments[0];
        assert!(fut_instrument.fee_schedule_uid.is_none());

        let instrument = parse_futures_instrument(fut_instrument, TS, TS).unwrap();
        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.symbol.as_str(), "PF_ETHUSD");
            }
            _ => panic!("Expected CryptoPerpetual"),
        }
    }

    #[rstest]
    fn test_parse_trade_tick_from_array() {
        let json = load_test_json("http_trades.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let trades_map = result.as_object().unwrap();

        // Get first pair's trades
        let (_pair, trades_value) = trades_map.iter().find(|(k, _)| *k != "last").unwrap();
        let trades = trades_value.as_array().unwrap();
        let trade_array = trades[0].as_array().unwrap();

        // Create a mock instrument for testing
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        let instrument = InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("XBTUSDT"))
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USDT())
                .price_precision(1)
                .size_precision(8)
                .price_increment(Price::from("0.1"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(TS)
                .ts_init(TS)
                .build()
                .unwrap(),
        );

        let trade_tick = parse_trade_tick_from_array(trade_array, &instrument, TS).unwrap();

        assert_eq!(trade_tick.instrument_id, instrument_id);
        assert_eq!(trade_tick.price, Price::from("105433.60000"));
        assert_eq!(trade_tick.size, Quantity::from("0.00027625"));
    }

    #[rstest]
    fn test_parse_bar() {
        let json = load_test_json("http_ohlc.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let ohlc_map = result.as_object().unwrap();

        // Get first pair's OHLC data
        let (_pair, ohlc_value) = ohlc_map.iter().find(|(k, _)| *k != "last").unwrap();
        let ohlcs = ohlc_value.as_array().unwrap();

        // Parse first OHLC array into OhlcData
        let ohlc_array = ohlcs[0].as_array().unwrap();
        let ohlc = OhlcData {
            time: ohlc_array[0].as_i64().unwrap(),
            open: ohlc_array[1].as_str().unwrap().to_string(),
            high: ohlc_array[2].as_str().unwrap().to_string(),
            low: ohlc_array[3].as_str().unwrap().to_string(),
            close: ohlc_array[4].as_str().unwrap().to_string(),
            vwap: ohlc_array[5].as_str().unwrap().to_string(),
            volume: ohlc_array[6].as_str().unwrap().to_string(),
            count: ohlc_array[7].as_i64().unwrap(),
        };

        // Create a mock instrument
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        let instrument = InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("XBTUSDT"))
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USDT())
                .price_precision(1)
                .size_precision(8)
                .price_increment(Price::from("0.1"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(TS)
                .ts_init(TS)
                .build()
                .unwrap(),
        );

        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );

        let bar = parse_bar(&ohlc, &instrument, bar_type, TS).unwrap();

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open, Price::from("106038.2"));
        assert_eq!(bar.high, Price::from("106038.2"));
        assert_eq!(bar.low, Price::from("106038.2"));
        assert_eq!(bar.close, Price::from("106038.2"));
        assert_eq!(bar.volume, Quantity::from("0.00000000"));
    }

    #[rstest]
    fn test_parse_millis_timestamp() {
        let timestamp = 1762795433.9717445;
        let result = parse_millis_timestamp(timestamp, "test").unwrap();
        assert!(result.as_u64() > 0);
    }

    #[rstest]
    #[case(1, BarAggregation::Minute, 1)]
    #[case(5, BarAggregation::Minute, 5)]
    #[case(15, BarAggregation::Minute, 15)]
    #[case(1, BarAggregation::Hour, 60)]
    #[case(4, BarAggregation::Hour, 240)]
    #[case(1, BarAggregation::Day, 1440)]
    fn test_bar_type_to_spot_interval(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
        #[case] expected: u32,
    ) {
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(step, aggregation, PriceType::Last),
            AggregationSource::External,
        );

        let result = bar_type_to_spot_interval(bar_type).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_bar_type_to_spot_interval_unsupported() {
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Second, PriceType::Last),
            AggregationSource::External,
        );

        let result = bar_type_to_spot_interval(bar_type);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));
    }

    #[rstest]
    #[case(1, BarAggregation::Minute, "1m")]
    #[case(5, BarAggregation::Minute, "5m")]
    #[case(15, BarAggregation::Minute, "15m")]
    #[case(1, BarAggregation::Hour, "1h")]
    #[case(4, BarAggregation::Hour, "4h")]
    #[case(12, BarAggregation::Hour, "12h")]
    #[case(1, BarAggregation::Day, "1d")]
    #[case(1, BarAggregation::Week, "1w")]
    fn test_bar_type_to_futures_resolution(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
        #[case] expected: &str,
    ) {
        let instrument_id = InstrumentId::new(Symbol::new("PI_XBTUSD"), *KRAKEN_VENUE);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(step, aggregation, PriceType::Last),
            AggregationSource::External,
        );

        let result = bar_type_to_futures_resolution(bar_type).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(30, BarAggregation::Minute)] // Unsupported minute step
    #[case(2, BarAggregation::Hour)] // Unsupported hour step
    #[case(2, BarAggregation::Day)] // Unsupported day step
    #[case(1, BarAggregation::Second)] // Unsupported aggregation
    fn test_bar_type_to_futures_resolution_unsupported(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
    ) {
        let instrument_id = InstrumentId::new(Symbol::new("PI_XBTUSD"), *KRAKEN_VENUE);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(step, aggregation, PriceType::Last),
            AggregationSource::External,
        );

        let result = bar_type_to_futures_resolution(bar_type);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));
    }

    #[rstest]
    fn test_parse_order_status_report() {
        let json = load_test_json("http_open_orders.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let open_map = result.get("open").unwrap();
        let orders: IndexMap<String, SpotOrder> = serde_json::from_value(open_map.clone()).unwrap();

        let account_id = AccountId::new("KRAKEN-001");
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USDT"), *KRAKEN_VENUE);
        let instrument = InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("XBTUSDT"))
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USDT())
                .price_precision(2)
                .size_precision(8)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(TS)
                .ts_init(TS)
                .build()
                .unwrap(),
        );

        let (order_id, order) = orders.iter().next().unwrap();

        let report =
            parse_order_status_report(order_id, order, &instrument, account_id, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.venue_order_id.as_str(), order_id);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.quantity, Quantity::from("0.50000000"));
    }

    fn create_mock_perp() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("PI_XBTUSD"), *KRAKEN_VENUE);
        InstrumentAny::CryptoPerpetual(
            CryptoPerpetual::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("PI_XBTUSD"))
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USD())
                .settlement_currency(Currency::USD())
                .is_inverse(false)
                .price_precision(1)
                .size_precision(0)
                .price_increment(Price::from("0.5"))
                .size_increment(Quantity::from("1"))
                .ts_event(TS)
                .ts_init(TS)
                .build()
                .unwrap(),
        )
    }

    #[rstest]
    fn test_parse_futures_assignee_fill_report() {
        let json = load_test_json("http_futures_fills.json");
        let response: FuturesFillsResponse = serde_json::from_str(&json).unwrap();
        let fill = &response.fills[2];
        let instrument = create_mock_perp();
        let account_id = AccountId::new("KRAKEN-001");

        let report = parse_futures_fill_report(fill, &instrument, account_id, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(
            report.venue_order_id,
            VenueOrderId::new("f8a7b6c5-d4e3-2f1a-0b9c-8d7e6f5a4b3c")
        );
        assert_eq!(
            report.trade_id,
            TradeId::new("d3f4e5a6-b7c8-9d0e-1f2a-3b4c5d6e7f8a")
        );
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.last_qty, Quantity::from("2500"));
        assert_eq!(report.last_px, Price::from("28050.0"));
        assert_eq!(report.commission, Money::zero(Currency::USD()));
        assert_eq!(report.liquidity_side, LiquiditySide::NoLiquiditySide);
        assert_eq!(report.avg_px, None);
        assert_eq!(
            report.ts_event,
            "2023-04-07T15:55:20.123Z".parse::<UnixNanos>().unwrap()
        );
        assert_eq!(report.ts_init, TS);
        assert_eq!(report.client_order_id, None);
        assert_eq!(report.venue_position_id, None);
    }

    #[rstest]
    fn test_parse_futures_order_status_report_market_if_touched() {
        let order = FuturesOpenOrder {
            order_id: "tp-001".to_string(),
            symbol: "PI_XBTUSD".to_string(),
            side: KrakenOrderSide::Buy,
            order_type: KrakenFuturesOrderType::TakeProfit,
            limit_price: None,
            stop_price: Some(dec!(36000)),
            unfilled_size: Some(dec!(500)),
            received_time: "2023-11-14T22:13:20.000Z".to_string(),
            status: KrakenFuturesOrderStatus::PartiallyFilled,
            filled_size: dec!(25),
            reduce_only: Some(true),
            last_update_time: "2023-11-14T22:13:20.000Z".to_string(),
            trigger_signal: None,
            cli_ord_id: Some("my-tp-1".to_string()),
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::new("KRAKEN-001");

        let report =
            parse_futures_order_status_report(&order, &instrument, account_id, None, TS).unwrap();

        assert_eq!(report.order_type, OrderType::MarketIfTouched);
        assert_eq!(report.quantity.as_decimal(), dec!(525));
        assert_eq!(report.filled_qty.as_decimal(), dec!(25));
        assert_eq!(report.trigger_price.unwrap().as_decimal(), dec!(36000));
        assert!(report.price.is_none());
        assert!(report.reduce_only);
        assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
    }

    #[rstest]
    fn test_parse_futures_order_status_report_limit_if_touched() {
        let order = FuturesOpenOrder {
            order_id: "tpl-001".to_string(),
            symbol: "PI_XBTUSD".to_string(),
            side: KrakenOrderSide::Sell,
            order_type: KrakenFuturesOrderType::TakeProfit,
            limit_price: Some(dec!(35500)),
            stop_price: Some(dec!(36000)),
            unfilled_size: Some(dec!(500)),
            received_time: "2023-11-14T22:13:20.000Z".to_string(),
            status: KrakenFuturesOrderStatus::Untouched,
            filled_size: dec!(0),
            reduce_only: None,
            last_update_time: "2023-11-14T22:13:20.000Z".to_string(),
            trigger_signal: None,
            cli_ord_id: Some("my-tpl-1".to_string()),
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::new("KRAKEN-001");

        let report =
            parse_futures_order_status_report(&order, &instrument, account_id, None, TS).unwrap();

        assert_eq!(report.order_type, OrderType::LimitIfTouched);
        assert_eq!(report.trigger_price.unwrap().as_decimal(), dec!(36000));
        assert_eq!(report.price.unwrap().as_decimal(), dec!(35500));
        assert_eq!(report.order_side, OrderSide::Sell.into());
        assert!(!report.reduce_only);
    }

    #[rstest]
    fn test_parse_futures_order_event_market_if_touched() {
        let event = FuturesOrderEvent {
            order_id: "tp-evt-001".to_string(),
            cli_ord_id: None,
            order_type: KrakenFuturesOrderType::TakeProfit,
            symbol: "PI_XBTUSD".to_string(),
            side: KrakenOrderSide::Buy,
            quantity: dec!(100),
            filled: dec!(100),
            limit_price: None,
            stop_price: Some(dec!(40000)),
            timestamp: "2023-11-14T22:13:20.000Z".to_string(),
            last_update_timestamp: "2023-11-14T22:13:21.000Z".to_string(),
            reduce_only: false,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::new("KRAKEN-001");

        let report = parse_futures_order_event_status_report(
            &event,
            Some(KrakenFuturesOrderEventType::Fill),
            &instrument,
            account_id,
            TS,
        )
        .unwrap();

        assert_eq!(report.order_type, OrderType::MarketIfTouched);
        assert_eq!(report.trigger_price.unwrap().as_decimal(), dec!(40000));
        assert!(report.price.is_none());
        assert_eq!(report.order_status, OrderStatus::Filled);
    }

    #[rstest]
    fn test_parse_futures_order_event_limit_if_touched() {
        let event = FuturesOrderEvent {
            order_id: "tpl-evt-001".to_string(),
            cli_ord_id: Some("my-tpl-evt".to_string()),
            order_type: KrakenFuturesOrderType::TakeProfit,
            symbol: "PI_XBTUSD".to_string(),
            side: KrakenOrderSide::Sell,
            quantity: dec!(200),
            filled: Decimal::ZERO,
            limit_price: Some(dec!(39500)),
            stop_price: Some(dec!(40000)),
            timestamp: "2023-11-14T22:13:20.000Z".to_string(),
            last_update_timestamp: "2023-11-14T22:13:21.000Z".to_string(),
            reduce_only: true,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::new("KRAKEN-001");

        let report = parse_futures_order_event_status_report(
            &event,
            Some(KrakenFuturesOrderEventType::Place),
            &instrument,
            account_id,
            TS,
        )
        .unwrap();

        assert_eq!(report.order_type, OrderType::LimitIfTouched);
        assert_eq!(report.trigger_price.unwrap().as_decimal(), dec!(40000));
        assert_eq!(report.price.unwrap().as_decimal(), dec!(39500));
        assert_eq!(report.order_side, OrderSide::Sell.into());
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert!(report.reduce_only);
    }

    #[rstest]
    fn test_parse_futures_order_event_cancel_status() {
        let event = FuturesOrderEvent {
            order_id: "cancel-evt-001".to_string(),
            cli_ord_id: Some("cancel-evt".to_string()),
            order_type: KrakenFuturesOrderType::Stop,
            symbol: "PI_XBTUSD".to_string(),
            side: KrakenOrderSide::Sell,
            quantity: dec!(200),
            filled: Decimal::ZERO,
            limit_price: None,
            stop_price: Some(dec!(39000)),
            timestamp: "2023-11-14T22:13:20.000Z".to_string(),
            last_update_timestamp: "2023-11-14T22:13:21.000Z".to_string(),
            reduce_only: true,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::new("KRAKEN-001");

        let report = parse_futures_order_event_status_report(
            &event,
            Some(KrakenFuturesOrderEventType::Cancel),
            &instrument,
            account_id,
            TS,
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert!(report.reduce_only);
    }

    #[rstest]
    fn test_parse_futures_order_event_reject_status() {
        let event = FuturesOrderEvent {
            order_id: "reject-evt-001".to_string(),
            cli_ord_id: Some("reject-evt".to_string()),
            order_type: KrakenFuturesOrderType::Limit,
            symbol: "PI_XBTUSD".to_string(),
            side: KrakenOrderSide::Buy,
            quantity: dec!(200),
            filled: Decimal::ZERO,
            limit_price: Some(dec!(35000)),
            stop_price: None,
            timestamp: "2023-11-14T22:13:20.000Z".to_string(),
            last_update_timestamp: "2023-11-14T22:13:21.000Z".to_string(),
            reduce_only: false,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::new("KRAKEN-001");

        let report = parse_futures_order_event_status_report(
            &event,
            Some(KrakenFuturesOrderEventType::Reject),
            &instrument,
            account_id,
            TS,
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Rejected);
    }

    #[rstest]
    fn test_parse_futures_order_event_expire_status() {
        let event = FuturesOrderEvent {
            order_id: "expire-evt-001".to_string(),
            cli_ord_id: Some("expire-evt".to_string()),
            order_type: KrakenFuturesOrderType::Limit,
            symbol: "PI_XBTUSD".to_string(),
            side: KrakenOrderSide::Buy,
            quantity: dec!(200),
            filled: Decimal::ZERO,
            limit_price: Some(dec!(35000)),
            stop_price: None,
            timestamp: "2023-11-14T22:13:20.000Z".to_string(),
            last_update_timestamp: "2023-11-14T22:13:21.000Z".to_string(),
            reduce_only: false,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::new("KRAKEN-001");

        let report = parse_futures_order_event_status_report(
            &event,
            Some(KrakenFuturesOrderEventType::Expire),
            &instrument,
            account_id,
            TS,
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Expired);
    }

    #[rstest]
    fn test_parse_futures_order_event_execution_status() {
        let event = FuturesOrderEvent {
            order_id: "execution-evt-001".to_string(),
            cli_ord_id: Some("execution-evt".to_string()),
            order_type: KrakenFuturesOrderType::Limit,
            symbol: "PI_XBTUSD".to_string(),
            side: KrakenOrderSide::Buy,
            quantity: dec!(200),
            filled: dec!(50),
            limit_price: Some(dec!(35000)),
            stop_price: None,
            timestamp: "2023-11-14T22:13:20.000Z".to_string(),
            last_update_timestamp: "2023-11-14T22:13:21.000Z".to_string(),
            reduce_only: false,
        };
        let instrument = create_mock_perp();
        let account_id = AccountId::new("KRAKEN-001");

        let report = parse_futures_order_event_status_report(
            &event,
            Some(KrakenFuturesOrderEventType::Execution),
            &instrument,
            account_id,
            TS,
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
    }

    #[rstest]
    fn test_parse_fill_report() {
        let json = load_test_json("http_trades_history.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let trades_map = result.get("trades").unwrap();
        let trades: IndexMap<String, SpotTrade> =
            serde_json::from_value(trades_map.clone()).unwrap();

        let account_id = AccountId::new("KRAKEN-001");
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USDT"), *KRAKEN_VENUE);
        let instrument = InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("XBTUSDT"))
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USDT())
                .price_precision(2)
                .size_precision(8)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(TS)
                .ts_init(TS)
                .build()
                .unwrap(),
        );

        let (trade_id, trade) = trades.iter().next().unwrap();

        let report = parse_fill_report(trade_id, trade, &instrument, account_id, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.trade_id.to_string(), *trade_id);
        assert_eq!(report.last_qty, Quantity::from("0.50000000"));
        assert_eq!(report.last_px, Price::from("29500.50"));
        assert_eq!(report.commission.as_decimal(), dec!(23.60));
    }

    #[rstest]
    #[case("XXBT", "XBT")]
    #[case("XETH", "ETH")]
    #[case("ZUSD", "USD")]
    #[case("ZEUR", "EUR")]
    #[case("BTC", "BTC")]
    #[case("ETH", "ETH")]
    #[case("USDT", "USDT")]
    #[case("SOL", "SOL")]
    fn test_normalize_currency_code(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(normalize_currency_code(input), expected);
    }

    #[rstest]
    #[case("XBT/EUR", "BTC/EUR")]
    #[case("XBT/USD", "BTC/USD")]
    #[case("XBT/USDT", "BTC/USDT")]
    #[case("ETH/USD", "ETH/USD")]
    #[case("ETH/XBT", "ETH/BTC")]
    #[case("SOL/XBT", "SOL/BTC")]
    #[case("SOL/USD", "SOL/USD")]
    #[case("BTC/USD", "BTC/USD")]
    #[case("ETH/BTC", "ETH/BTC")]
    #[case("XDG/USD", "DOGE/USD")]
    #[case("XDG/EUR", "DOGE/EUR")]
    #[case("XDG/BTC", "DOGE/BTC")]
    #[case("XDG/XBT", "DOGE/BTC")]
    fn test_normalize_spot_symbol(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(normalize_spot_symbol(input), expected);
    }

    #[rstest]
    #[case("A", "A")] // 1 char, minimum
    #[case("O2026022700232", "O2026022700232")] // 14 chars, typical short
    #[case("ABCDEFGHIJKLMNOPQR", "ABCDEFGHIJKLMNOPQR")] // 18 chars, at limit
    fn test_truncate_cl_ord_id_short_passthrough(#[case] input: &str, #[case] expected: &str) {
        let id = ClientOrderId::new(input);
        assert_eq!(truncate_cl_ord_id(&id), expected);
    }

    #[rstest]
    #[case("6d47a5f0-6fd4-4b84-b56e-c23f0f689c20")] // lowercase hex
    #[case("6D47A5F0-6FD4-4B84-B56E-C23F0F689C20")] // uppercase hex
    #[case("00000000-0000-0000-0000-000000000000")] // nil UUID
    #[case("ffffffff-ffff-ffff-ffff-ffffffffffff")] // max UUID
    fn test_truncate_cl_ord_id_uuid_hyphenated_passthrough(#[case] input: &str) {
        let id = ClientOrderId::new(input);
        assert_eq!(truncate_cl_ord_id(&id), input);
    }

    #[rstest]
    #[case("6d47a5f06fd44b84b56ec23f0f689c20")] // lowercase
    #[case("6D47A5F06FD44B84B56EC23F0F689C20")] // uppercase
    #[case("00000000000000000000000000000000")] // all zeros
    #[case("aAbBcCdDeEfF00112233445566778899")] // mixed case
    fn test_truncate_cl_ord_id_uuid_compact_passthrough(#[case] input: &str) {
        let id = ClientOrderId::new(input);
        assert_eq!(truncate_cl_ord_id(&id), input);
    }

    #[rstest]
    #[case("O2026022700232100400", "O26022700232100400")] // 20 chars → O + last 17
    #[case("O202602270023210040011", "O02270023210040011")] // 22 chars, typical sequential
    #[case("O20260227002321004001100", "O27002321004001100")] // 24 chars
    fn test_truncate_cl_ord_id_sequential_truncated(#[case] input: &str, #[case] expected: &str) {
        let id = ClientOrderId::new(input);
        let result = truncate_cl_ord_id(&id);
        assert_eq!(result, expected);
        assert_eq!(result.len(), 18);
        assert!(result.starts_with('O'));
    }

    #[rstest]
    fn test_truncate_cl_ord_id_32_chars_non_hex_truncated() {
        let input = "0123456789abcdef0123456789abcdeg";
        let id = ClientOrderId::new(input);
        let result = truncate_cl_ord_id(&id);
        assert_eq!(result.len(), 18);
        assert!(result.starts_with('O'));
        assert_eq!(result, "Of0123456789abcdeg");
    }

    #[rstest]
    fn test_truncate_cl_ord_id_36_chars_wrong_hyphens_truncated() {
        let input = "6d47a5f0-6fd4-4b84-b56ec23f0f689c200";
        let id = ClientOrderId::new(input);
        let result = truncate_cl_ord_id(&id);
        assert_eq!(result.len(), 18);
        assert!(result.starts_with('O'));
    }

    #[rstest]
    fn test_parse_tokenized_instrument() {
        let json = load_test_json("http_asset_pairs_tokenized.json");
        let response: KrakenResponse<AssetPairsResponse> = serde_json::from_str(&json).unwrap();
        let pairs = response.result.unwrap();

        let (pair_name, definition) = pairs.iter().next().unwrap();

        let instrument = parse_tokenized_instrument(pair_name, definition, TS, TS).unwrap();

        match instrument {
            InstrumentAny::TokenizedAsset(ta) => {
                assert_eq!(ta.id.symbol.as_str(), "AAPLx/USD");
                assert_eq!(ta.id.venue.as_str(), "KRAKEN");
                assert_eq!(ta.raw_symbol.as_str(), "AAPLxUSD");
                assert_eq!(ta.asset_class, AssetClass::Equity);
                assert_eq!(ta.base_currency.code.as_str(), "AAPLx");
                assert_eq!(ta.quote_currency.code.as_str(), "ZUSD");
                assert_eq!(ta.price_precision, 2);
                assert_eq!(ta.size_precision, 8);
                assert_eq!(ta.price_increment.as_decimal(), dec!(0.01));
                assert_eq!(ta.size_increment.as_decimal(), dec!(0.00000001));
                assert!(ta.min_quantity.is_some());
                assert_eq!(ta.maker_fee, dec!(-0.0002));
                assert_eq!(ta.taker_fee, dec!(0.001));
            }
            _ => panic!("Expected TokenizedAsset, received {instrument:?}"),
        }
    }

    #[rstest]
    fn test_parse_fill_report_tokenized_asset() {
        let json = load_test_json("http_trades_history.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let trades_map = result.get("trades").unwrap();
        let trades: IndexMap<String, SpotTrade> =
            serde_json::from_value(trades_map.clone()).unwrap();

        let account_id = AccountId::new("KRAKEN-001");
        let instrument_id = InstrumentId::new(Symbol::new("AAPLx/USD"), *KRAKEN_VENUE);
        let instrument = InstrumentAny::TokenizedAsset(
            TokenizedAsset::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("AAPLxUSD"))
                .asset_class(AssetClass::Equity)
                .base_currency(Currency::get_or_create_crypto("AAPLx"))
                .quote_currency(Currency::USD())
                .price_precision(2)
                .size_precision(8)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(TS)
                .ts_init(TS)
                .build()
                .unwrap(),
        );

        let (trade_id, trade) = trades.iter().next().unwrap();

        let report = parse_fill_report(trade_id, trade, &instrument, account_id, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.trade_id.to_string(), *trade_id);
        assert_eq!(report.last_qty, Quantity::from("0.50000000"));
        assert_eq!(report.last_px, Price::from("29500.50"));
        assert_eq!(report.commission.currency, Currency::USD());
    }

    #[rstest]
    fn test_truncate_cl_ord_id_19_chars_truncated() {
        let input = "O202602270023210040";
        assert_eq!(input.len(), 19);
        let id = ClientOrderId::new(input);
        let result = truncate_cl_ord_id(&id);
        assert_eq!(result.len(), 18);
        assert_eq!(result, "O02602270023210040");
    }

    #[rstest]
    fn test_truncate_cl_ord_id_preserves_tail() {
        let input = "O20260227002321004001100";
        let id = ClientOrderId::new(input);
        let result = truncate_cl_ord_id(&id);
        assert_eq!(&result[1..], &input[input.len() - 17..]);
    }
}
