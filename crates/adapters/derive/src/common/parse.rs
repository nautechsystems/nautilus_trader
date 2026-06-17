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

//! Parsing utilities for the Derive adapter.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::{
    UnixNanos,
    datetime::{NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
    params::Params,
};
use nautilus_model::{
    enums::{OptionKind, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType},
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoOption, CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{
    Deserializer,
    de::{Error as DeError, Unexpected, Visitor},
};
use ustr::Ustr;

use crate::{
    common::{
        consts::DERIVE_VENUE,
        enums::{
            DeriveInstrumentType, DeriveOptionKind, DeriveOrderSide, DeriveOrderStatus,
            DeriveOrderType, DeriveTimeInForce, DeriveTriggerPriceType, DeriveTriggerType,
        },
    },
    http::models::DeriveInstrument,
};

const DERIVE_DECIMAL_MAX_SCALE: usize = 28;
const DERIVE_POST_ONLY_CROSS_MARKET_MESSAGE: &str = "post only order cannot cross the market";

/// JSON-RPC error code returned when a post-only order crosses the market.
pub const DERIVE_POST_ONLY_CROSS_MARKET_ERROR_CODE: i64 = 11008;

/// Converts a Derive venue symbol to a Nautilus instrument ID.
#[must_use]
pub fn format_instrument_id(venue_symbol: impl AsRef<str>) -> InstrumentId {
    InstrumentId::new(Symbol::new(venue_symbol.as_ref()), *DERIVE_VENUE)
}

/// Converts a Nautilus Derive instrument ID back to the venue symbol.
///
/// # Errors
///
/// Returns an error when `instrument_id` is not for the Derive venue.
pub fn format_venue_symbol(instrument_id: &InstrumentId) -> anyhow::Result<Ustr> {
    anyhow::ensure!(
        instrument_id.venue == *DERIVE_VENUE,
        "instrument ID `{instrument_id}` is not for venue {}",
        DERIVE_VENUE.as_str(),
    );
    Ok(Ustr::from(instrument_id.symbol.as_str()))
}

/// Deserializes a Derive decimal, rounding fractional scales above 28 digits.
///
/// # Errors
///
/// Returns an error when the value is not a valid decimal after Derive scale
/// normalization.
pub fn deserialize_derive_decimal<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(DeriveDecimalVisitor)
}

/// Deserializes an optional Derive decimal, rounding fractional scales above 28 digits.
///
/// # Errors
///
/// Returns an error when the value is not a valid decimal after Derive scale
/// normalization.
pub fn deserialize_optional_derive_decimal<'de, D>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(OptionalDeriveDecimalVisitor)
}

struct DeriveDecimalVisitor;

impl Visitor<'_> for DeriveDecimalVisitor {
    type Value = Decimal;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a Derive decimal number as string, integer, float, or null")
    }

    fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
        if value.is_empty() {
            return Ok(Decimal::ZERO);
        }
        parse_derive_decimal_str(value).map_err(E::custom)
    }

    fn visit_string<E: DeError>(self, value: String) -> Result<Self::Value, E> {
        self.visit_str(&value)
    }

    fn visit_i64<E: DeError>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Decimal::from(value))
    }

    fn visit_u64<E: DeError>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Decimal::from(value))
    }

    fn visit_i128<E: DeError>(self, value: i128) -> Result<Self::Value, E> {
        Ok(Decimal::from(value))
    }

    fn visit_u128<E: DeError>(self, value: u128) -> Result<Self::Value, E> {
        Ok(Decimal::from(value))
    }

    fn visit_f64<E: DeError>(self, value: f64) -> Result<Self::Value, E> {
        if value.is_nan() || value.is_infinite() {
            return Err(E::invalid_value(Unexpected::Float(value), &self));
        }
        Decimal::try_from(value).map_err(E::custom)
    }

    fn visit_unit<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(Decimal::ZERO)
    }

    fn visit_none<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(Decimal::ZERO)
    }
}

struct OptionalDeriveDecimalVisitor;

impl Visitor<'_> for OptionalDeriveDecimalVisitor {
    type Value = Option<Decimal>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("null or a Derive decimal number as string, integer, or float")
    }

    fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
        if value.is_empty() {
            return Ok(None);
        }
        parse_derive_decimal_str(value).map(Some).map_err(E::custom)
    }

    fn visit_string<E: DeError>(self, value: String) -> Result<Self::Value, E> {
        self.visit_str(&value)
    }

    fn visit_i64<E: DeError>(self, value: i64) -> Result<Self::Value, E> {
        DeriveDecimalVisitor.visit_i64(value).map(Some)
    }

    fn visit_u64<E: DeError>(self, value: u64) -> Result<Self::Value, E> {
        DeriveDecimalVisitor.visit_u64(value).map(Some)
    }

    fn visit_i128<E: DeError>(self, value: i128) -> Result<Self::Value, E> {
        DeriveDecimalVisitor.visit_i128(value).map(Some)
    }

    fn visit_u128<E: DeError>(self, value: u128) -> Result<Self::Value, E> {
        DeriveDecimalVisitor.visit_u128(value).map(Some)
    }

    fn visit_f64<E: DeError>(self, value: f64) -> Result<Self::Value, E> {
        DeriveDecimalVisitor.visit_f64(value).map(Some)
    }

    fn visit_unit<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
}

fn parse_derive_decimal_str(value: &str) -> Result<Decimal, String> {
    let parsed = if value.contains('e') || value.contains('E') {
        Decimal::from_scientific(value)
    } else {
        Decimal::from_str(value)
    };

    match parsed {
        Ok(decimal) => Ok(decimal),
        Err(e) => {
            for scale in (0..=DERIVE_DECIMAL_MAX_SCALE).rev() {
                let clamped =
                    decimal_string_clamped_to_scale(value, scale).ok_or_else(|| e.to_string())?;

                if let Ok(decimal) = Decimal::from_str(&clamped) {
                    return Ok(decimal);
                }
            }
            Err(e.to_string())
        }
    }
}

fn decimal_string_clamped_to_scale(value: &str, max_scale: usize) -> Option<String> {
    let (coefficient, exponent) = match value.find(['e', 'E']) {
        Some(index) => {
            let exponent = value[index + 1..].parse::<i32>().ok()?;
            (&value[..index], exponent)
        }
        None => (value, 0),
    };

    let (sign, unsigned) = match coefficient.as_bytes().first()? {
        b'+' => ("", &coefficient[1..]),
        b'-' => ("-", &coefficient[1..]),
        _ => ("", coefficient),
    };
    let (integer, fractional) = decimal_components(unsigned)?;
    let digits = format!("{integer}{fractional}");
    let point = i32::try_from(integer.len()).ok()?.checked_add(exponent)?;

    let (integer, fractional) = if point <= 0 {
        let zero_count = usize::try_from(-point).ok()?;
        (
            "0".to_string(),
            format!("{}{digits}", "0".repeat(zero_count)),
        )
    } else {
        let point = usize::try_from(point).ok()?;
        if point >= digits.len() {
            (
                format!("{}{}", digits, "0".repeat(point - digits.len())),
                String::new(),
            )
        } else {
            (digits[..point].to_string(), digits[point..].to_string())
        }
    };

    let (integer, fractional) = round_decimal_components(integer, fractional, max_scale);
    let sign = if sign == "-" && decimal_digits_are_zero(&integer, &fractional) {
        ""
    } else {
        sign
    };

    if fractional.is_empty() {
        Some(format!("{sign}{integer}"))
    } else {
        Some(format!("{sign}{integer}.{fractional}"))
    }
}

fn decimal_components(value: &str) -> Option<(&str, &str)> {
    let mut split = value.split('.');
    let integer = split.next()?;
    let fractional = split.next().unwrap_or("");
    if split.next().is_some()
        || (integer.is_empty() && fractional.is_empty())
        || !integer.chars().all(|c| c.is_ascii_digit())
        || !fractional.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some((integer, fractional))
}

fn round_decimal_components(
    mut integer: String,
    fractional: String,
    max_scale: usize,
) -> (String, String) {
    if fractional.len() <= max_scale {
        return (integer, fractional);
    }

    let mut rounded = fractional.as_bytes()[..max_scale].to_vec();
    if fractional.as_bytes()[max_scale] >= b'5' {
        increment_decimal_digits(&mut integer, &mut rounded);
    }

    (
        integer,
        String::from_utf8(rounded).expect("decimal digits are ASCII"),
    )
}

fn increment_decimal_digits(integer: &mut String, fractional: &mut [u8]) {
    for digit in fractional.iter_mut().rev() {
        if *digit < b'9' {
            *digit += 1;
            return;
        }
        *digit = b'0';
    }

    let mut integer_digits = integer.as_bytes().to_vec();
    for digit in integer_digits.iter_mut().rev() {
        if *digit < b'9' {
            *digit += 1;
            *integer = String::from_utf8(integer_digits).expect("decimal digits are ASCII");
            return;
        }
        *digit = b'0';
    }
    integer_digits.insert(0, b'1');
    *integer = String::from_utf8(integer_digits).expect("decimal digits are ASCII");
}

fn decimal_digits_are_zero(integer: &str, fractional: &str) -> bool {
    integer
        .bytes()
        .chain(fractional.bytes())
        .all(|digit| digit == b'0')
}

/// Maps a Nautilus order side to the Derive direction string.
///
/// # Errors
///
/// Returns an error for ambiguous Nautilus order sides
/// ([`OrderSide::NoOrderSide`]).
pub fn order_side_to_derive(side: OrderSide) -> anyhow::Result<DeriveOrderSide> {
    match side {
        OrderSide::Buy => Ok(DeriveOrderSide::Buy),
        OrderSide::Sell => Ok(DeriveOrderSide::Sell),
        OrderSide::NoOrderSide => anyhow::bail!("unsupported order side for Derive: {side:?}"),
    }
}

/// Maps a Nautilus order type to the Derive order type string.
///
/// # Errors
///
/// Returns an error for order types Derive does not accept.
pub fn order_type_to_derive(order_type: OrderType) -> anyhow::Result<DeriveOrderType> {
    match order_type {
        OrderType::Limit => Ok(DeriveOrderType::Limit),
        OrderType::Market => Ok(DeriveOrderType::Market),
        other => anyhow::bail!("unsupported order type for Derive: {other:?}"),
    }
}

/// Maps a supported Nautilus trigger order type to the child Derive order type.
///
/// # Errors
///
/// Returns an error for order types not supported by Derive trigger orders.
pub fn trigger_order_type_to_derive(order_type: OrderType) -> anyhow::Result<DeriveOrderType> {
    match order_type {
        OrderType::StopMarket | OrderType::MarketIfTouched => Ok(DeriveOrderType::Market),
        OrderType::StopLimit | OrderType::LimitIfTouched => Ok(DeriveOrderType::Limit),
        other => anyhow::bail!(
            "unsupported trigger order type for Derive: {other:?}; supported types are StopMarket, StopLimit, MarketIfTouched, and LimitIfTouched"
        ),
    }
}

/// Maps a Nautilus trigger order type to Derive's stop-loss/take-profit flag.
///
/// # Errors
///
/// Returns an error for order types not supported by Derive trigger orders.
pub fn trigger_type_to_derive(order_type: OrderType) -> anyhow::Result<DeriveTriggerType> {
    match order_type {
        OrderType::StopMarket | OrderType::StopLimit => Ok(DeriveTriggerType::Stoploss),
        OrderType::MarketIfTouched | OrderType::LimitIfTouched => Ok(DeriveTriggerType::Takeprofit),
        other => anyhow::bail!(
            "unsupported trigger order type for Derive: {other:?}; supported types are StopMarket, StopLimit, MarketIfTouched, and LimitIfTouched"
        ),
    }
}

/// Maps Nautilus trigger price source to Derive.
///
/// # Errors
///
/// Returns an error unless the trigger source maps to mark price, which is the
/// only source Derive currently accepts for trigger orders.
pub fn trigger_price_type_to_derive(
    trigger_type: Option<TriggerType>,
) -> anyhow::Result<DeriveTriggerPriceType> {
    match trigger_type {
        Some(TriggerType::Default | TriggerType::MarkPrice) => Ok(DeriveTriggerPriceType::Mark),
        Some(TriggerType::IndexPrice) => anyhow::bail!(
            "unsupported trigger price type for Derive: IndexPrice; Derive currently accepts only MarkPrice for trigger orders"
        ),
        Some(other) => anyhow::bail!(
            "unsupported trigger price type for Derive: {other:?}; Derive trigger orders support only MarkPrice"
        ),
        None => anyhow::bail!(
            "missing trigger price type for Derive trigger order; Derive trigger orders support only MarkPrice"
        ),
    }
}

/// Maps a Nautilus time-in-force flag to the Derive TIF.
///
/// # Errors
///
/// Returns an error for time-in-force flags Derive does not accept.
pub fn time_in_force_to_derive(
    tif: TimeInForce,
    post_only: bool,
) -> anyhow::Result<DeriveTimeInForce> {
    match tif {
        TimeInForce::Gtc if post_only => Ok(DeriveTimeInForce::PostOnly),
        TimeInForce::Ioc | TimeInForce::Fok if post_only => anyhow::bail!(
            "post-only Derive orders only support GTC time in force; received {tif:?}"
        ),
        TimeInForce::Gtc => Ok(DeriveTimeInForce::Gtc),
        TimeInForce::Ioc => Ok(DeriveTimeInForce::Ioc),
        TimeInForce::Fok => Ok(DeriveTimeInForce::Fok),
        other => anyhow::bail!("unsupported time in force for Derive: {other:?}"),
    }
}

/// Maps a Derive order side back to Nautilus.
#[must_use]
pub fn derive_order_side_to_nautilus(side: DeriveOrderSide) -> OrderSide {
    match side {
        DeriveOrderSide::Buy => OrderSide::Buy,
        DeriveOrderSide::Sell => OrderSide::Sell,
    }
}

/// Maps a Derive order type back to Nautilus.
#[must_use]
pub fn derive_order_type_to_nautilus(order_type: DeriveOrderType) -> OrderType {
    match order_type {
        DeriveOrderType::Limit => OrderType::Limit,
        DeriveOrderType::Market => OrderType::Market,
    }
}

/// Maps a Derive trigger order record back to the Nautilus order type.
#[must_use]
pub fn derive_order_type_to_nautilus_for_order(
    order_type: DeriveOrderType,
    trigger_type: Option<DeriveTriggerType>,
) -> OrderType {
    match (order_type, trigger_type) {
        (DeriveOrderType::Market, Some(DeriveTriggerType::Stoploss)) => OrderType::StopMarket,
        (DeriveOrderType::Limit, Some(DeriveTriggerType::Stoploss)) => OrderType::StopLimit,
        (DeriveOrderType::Market, Some(DeriveTriggerType::Takeprofit)) => {
            OrderType::MarketIfTouched
        }
        (DeriveOrderType::Limit, Some(DeriveTriggerType::Takeprofit)) => OrderType::LimitIfTouched,
        (order_type, None) => derive_order_type_to_nautilus(order_type),
    }
}

/// Maps a Derive trigger price source back to Nautilus.
#[must_use]
pub const fn derive_trigger_price_type_to_nautilus(
    trigger_price_type: DeriveTriggerPriceType,
) -> TriggerType {
    match trigger_price_type {
        DeriveTriggerPriceType::Mark => TriggerType::MarkPrice,
        DeriveTriggerPriceType::Index => TriggerType::IndexPrice,
    }
}

/// Maps a Derive TIF back to Nautilus.
#[must_use]
pub fn derive_tif_to_nautilus(tif: DeriveTimeInForce) -> TimeInForce {
    match tif {
        DeriveTimeInForce::Gtc | DeriveTimeInForce::PostOnly => TimeInForce::Gtc,
        DeriveTimeInForce::Ioc => TimeInForce::Ioc,
        DeriveTimeInForce::Fok => TimeInForce::Fok,
    }
}

/// Maps a Derive order status to the Nautilus equivalent, given the current
/// filled quantity.
#[must_use]
pub fn derive_status_to_nautilus(
    status: DeriveOrderStatus,
    filled_qty: Decimal,
    quantity: Decimal,
) -> OrderStatus {
    match status {
        DeriveOrderStatus::Open => {
            if filled_qty > Decimal::ZERO && filled_qty < quantity {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::Accepted
            }
        }
        DeriveOrderStatus::Filled => OrderStatus::Filled,
        DeriveOrderStatus::Rejected => OrderStatus::Rejected,
        DeriveOrderStatus::Cancelled => OrderStatus::Canceled,
        DeriveOrderStatus::Expired => OrderStatus::Expired,
        DeriveOrderStatus::Untriggered | DeriveOrderStatus::AlgoActive => OrderStatus::Accepted,
    }
}

/// Returns whether a Derive rejection means a post-only order crossed the market.
#[must_use]
pub fn derive_rejection_due_post_only(code: Option<i64>, reason: &str) -> bool {
    match code {
        Some(DERIVE_POST_ONLY_CROSS_MARKET_ERROR_CODE) => true,
        Some(_) => false,
        None => reason
            .to_ascii_lowercase()
            .contains(DERIVE_POST_ONLY_CROSS_MARKET_MESSAGE),
    }
}

/// Parses a Derive instrument definition into a Nautilus instrument.
///
/// # Errors
///
/// Returns an error when a Derive instrument is missing required details or
/// contains invalid price, quantity, or timestamp fields.
pub fn parse_derive_instrument_any(
    instrument: &DeriveInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<InstrumentAny>> {
    match instrument.instrument_type {
        DeriveInstrumentType::Perp => parse_perp_instrument(instrument, ts_init).map(Some),
        DeriveInstrumentType::Option => parse_option_instrument(instrument, ts_init).map(Some),
        DeriveInstrumentType::Erc20 => parse_spot_instrument(instrument, ts_init).map(Some),
    }
}

fn parse_perp_instrument(
    instrument: &DeriveInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    instrument
        .perp_details
        .as_ref()
        .context("missing perp_details for Derive perp instrument")?;

    let instrument_id = format_instrument_id(instrument.instrument_name.as_str());
    let raw_symbol = Symbol::new(instrument.instrument_name.as_str());
    let base_currency = Currency::get_or_create_crypto(instrument.base_currency.as_str());
    let quote_currency = Currency::get_or_create_crypto(instrument.quote_currency.as_str());
    let settlement_currency = quote_currency;
    let price_increment = price_from_decimal(instrument.tick_size, "tick_size")?;
    let size_increment = quantity_from_decimal(instrument.amount_step, "amount_step")?;
    let multiplier = quantity_from_decimal(Decimal::ONE, "multiplier")?;
    let max_quantity = quantity_from_decimal(instrument.maximum_amount, "maximum_amount")?;
    let min_quantity = quantity_from_decimal(instrument.minimum_amount, "minimum_amount")?;
    let info = derive_instrument_info(instrument)?;

    let perp = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        Some(multiplier),
        Some(size_increment),
        Some(max_quantity),
        Some(min_quantity),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(instrument.maker_fee_rate),
        Some(instrument.taker_fee_rate),
        None,
        Some(info),
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(perp))
}

fn parse_option_instrument(
    instrument: &DeriveInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let details = instrument
        .option_details
        .as_ref()
        .context("missing option_details for Derive option instrument")?;

    let instrument_id = format_instrument_id(instrument.instrument_name.as_str());
    let raw_symbol = Symbol::new(instrument.instrument_name.as_str());
    let underlying = Currency::get_or_create_crypto(instrument.base_currency.as_str());
    let quote_currency = Currency::get_or_create_crypto(instrument.quote_currency.as_str());
    let settlement_currency = quote_currency;
    let option_kind = parse_option_kind(details.option_type);
    let strike_price = price_from_decimal(details.strike, "option_details.strike")?;
    let activation_ns =
        timestamp_millis_to_nanos(instrument.scheduled_activation, "scheduled_activation")?;
    let expiration_ns = timestamp_seconds_to_nanos(details.expiry, "option_details.expiry")?;
    let price_increment = price_from_decimal(instrument.tick_size, "tick_size")?;
    let size_increment = quantity_from_decimal(instrument.amount_step, "amount_step")?;
    let multiplier = quantity_from_decimal(Decimal::ONE, "multiplier")?;
    let max_quantity = quantity_from_decimal(instrument.maximum_amount, "maximum_amount")?;
    let min_quantity = quantity_from_decimal(instrument.minimum_amount, "minimum_amount")?;
    let info = derive_instrument_info(instrument)?;

    let option = CryptoOption::new(
        instrument_id,
        raw_symbol,
        underlying,
        quote_currency,
        settlement_currency,
        false,
        option_kind,
        strike_price,
        activation_ns,
        expiration_ns,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        Some(multiplier),
        Some(size_increment),
        Some(max_quantity),
        Some(min_quantity),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(instrument.maker_fee_rate),
        Some(instrument.taker_fee_rate),
        None,
        Some(info),
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CryptoOption(option))
}

fn parse_spot_instrument(
    instrument: &DeriveInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = format_instrument_id(instrument.instrument_name.as_str());
    let raw_symbol = Symbol::new(instrument.instrument_name.as_str());
    let base_currency = Currency::get_or_create_crypto(instrument.base_currency.as_str());
    let quote_currency = Currency::get_or_create_crypto(instrument.quote_currency.as_str());
    let price_increment = price_from_decimal(instrument.tick_size, "tick_size")?;
    let size_increment = quantity_from_decimal(instrument.amount_step, "amount_step")?;
    let multiplier = quantity_from_decimal(Decimal::ONE, "multiplier")?;
    let max_quantity = quantity_from_decimal(instrument.maximum_amount, "maximum_amount")?;
    let min_quantity = quantity_from_decimal(instrument.minimum_amount, "minimum_amount")?;
    let info = derive_instrument_info(instrument)?;

    let pair = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        Some(multiplier),
        Some(size_increment),
        Some(max_quantity),
        Some(min_quantity),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(instrument.maker_fee_rate),
        Some(instrument.taker_fee_rate),
        None,
        Some(info),
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(pair))
}

fn parse_option_kind(kind: DeriveOptionKind) -> OptionKind {
    match kind {
        DeriveOptionKind::Call => OptionKind::Call,
        DeriveOptionKind::Put => OptionKind::Put,
    }
}

// Serializes the raw DeriveInstrument into the Nautilus `info` slot so
// downstream consumers can read venue fields (base_asset_address,
// base_asset_sub_id, base_fee, mark_price_fee_rate_cap, option_details,
// perp_details, etc.) that the core instrument model does not expose.
fn derive_instrument_info(instrument: &DeriveInstrument) -> anyhow::Result<Params> {
    let value = serde_json::to_value(instrument)
        .context("failed to serialize DeriveInstrument for info field")?;
    let object = value
        .as_object()
        .context("DeriveInstrument did not serialize to a JSON object")?
        .clone();
    Ok(Params::from_index_map(object.into_iter().collect()))
}

fn price_from_decimal(value: Decimal, field: &str) -> anyhow::Result<Price> {
    Price::from_decimal(value).with_context(|| format!("invalid Derive {field}"))
}

fn quantity_from_decimal(value: Decimal, field: &str) -> anyhow::Result<Quantity> {
    Quantity::from_decimal(value).with_context(|| format!("invalid Derive {field}"))
}

fn timestamp_seconds_to_nanos(value: i64, field: &str) -> anyhow::Result<UnixNanos> {
    timestamp_to_nanos(value, NANOSECONDS_IN_SECOND, field)
}

fn timestamp_millis_to_nanos(value: i64, field: &str) -> anyhow::Result<UnixNanos> {
    timestamp_to_nanos(value, NANOSECONDS_IN_MILLISECOND, field)
}

fn timestamp_to_nanos(value: i64, multiplier: u64, field: &str) -> anyhow::Result<UnixNanos> {
    let value = u64::try_from(value).with_context(|| format!("negative Derive {field}"))?;
    let nanos = value
        .checked_mul(multiplier)
        .with_context(|| format!("Derive {field} overflows nanoseconds"))?;
    Ok(UnixNanos::from(nanos))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::{OptionKind, OrderStatus, OrderType, TriggerType},
        identifiers::InstrumentId,
        instruments::{Instrument, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde::Deserialize;
    use serde_json::{Value, json};

    use super::*;

    fn data_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
    }

    fn load_json(filename: &str) -> Value {
        let content = std::fs::read_to_string(data_path().join(filename))
            .unwrap_or_else(|_| panic!("failed to read {filename}"));
        serde_json::from_str(&content).expect("invalid json")
    }

    fn perp_fixture() -> DeriveInstrument {
        serde_json::from_value(load_json("perps/instrument_eth.json")).unwrap()
    }

    fn option_fixture() -> DeriveInstrument {
        serde_json::from_value(load_json("options/instrument_eth.json")).unwrap()
    }

    fn spot_fixture() -> DeriveInstrument {
        serde_json::from_value(load_json("spot/instrument_eth.json")).unwrap()
    }

    #[derive(Deserialize)]
    struct DeriveDecimalProbe {
        #[serde(deserialize_with = "deserialize_derive_decimal")]
        rounded: Decimal,
        #[serde(deserialize_with = "deserialize_optional_derive_decimal")]
        optional: Option<Decimal>,
        #[serde(deserialize_with = "deserialize_derive_decimal")]
        carry: Decimal,
        #[serde(deserialize_with = "deserialize_derive_decimal")]
        negative_zero: Decimal,
    }

    #[rstest]
    fn test_deserialize_derive_decimal_rounds_high_scale_scientific_values() {
        let carry_input = "999999999999999999999999999995e-29";
        assert!(Decimal::from_scientific(carry_input).is_err());

        let probe: DeriveDecimalProbe = serde_json::from_value(json!({
            "rounded": "1.234567890123456789012345678912345e-1",
            "optional": "51.234567890123456789012345678912345e-1",
            "carry": carry_input,
            "negative_zero": "-4e-29"
        }))
        .unwrap();

        assert_eq!(probe.rounded.to_string(), "0.1234567890123456789012345679",);
        assert_eq!(
            probe.optional.as_ref().map(ToString::to_string),
            Some("5.1234567890123456789012345679".into()),
        );
        assert_eq!(probe.carry.to_string(), "10.000000000000000000000000000",);
        assert_eq!(
            probe.negative_zero.to_string(),
            "0.0000000000000000000000000000"
        );
    }

    #[rstest]
    #[case(OrderType::StopMarket, DeriveOrderType::Market)]
    #[case(OrderType::MarketIfTouched, DeriveOrderType::Market)]
    #[case(OrderType::StopLimit, DeriveOrderType::Limit)]
    #[case(OrderType::LimitIfTouched, DeriveOrderType::Limit)]
    fn test_trigger_order_type_to_derive(
        #[case] order_type: OrderType,
        #[case] expected: DeriveOrderType,
    ) {
        assert_eq!(trigger_order_type_to_derive(order_type).unwrap(), expected);
    }

    #[rstest]
    fn test_trigger_order_type_to_derive_rejects_unsupported() {
        let err = trigger_order_type_to_derive(OrderType::TrailingStopMarket)
            .expect_err("trailing stops must be rejected");

        assert!(
            err.to_string()
                .contains("unsupported trigger order type for Derive"),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    #[case(OrderType::StopMarket, DeriveTriggerType::Stoploss)]
    #[case(OrderType::StopLimit, DeriveTriggerType::Stoploss)]
    #[case(OrderType::MarketIfTouched, DeriveTriggerType::Takeprofit)]
    #[case(OrderType::LimitIfTouched, DeriveTriggerType::Takeprofit)]
    fn test_trigger_type_to_derive(
        #[case] order_type: OrderType,
        #[case] expected: DeriveTriggerType,
    ) {
        assert_eq!(trigger_type_to_derive(order_type).unwrap(), expected);
    }

    #[rstest]
    fn test_trigger_price_type_to_derive_accepts_only_mark_price() {
        assert_eq!(
            trigger_price_type_to_derive(Some(TriggerType::MarkPrice)).unwrap(),
            DeriveTriggerPriceType::Mark,
        );
        assert_eq!(
            trigger_price_type_to_derive(Some(TriggerType::Default)).unwrap(),
            DeriveTriggerPriceType::Mark,
        );

        for trigger_type in [
            TriggerType::IndexPrice,
            TriggerType::LastPrice,
            TriggerType::BidAsk,
            TriggerType::NoTrigger,
        ] {
            let err = trigger_price_type_to_derive(Some(trigger_type))
                .expect_err("unsupported trigger price type must fail");
            assert!(
                err.to_string().contains("unsupported trigger price type"),
                "unexpected error for {trigger_type:?}: {err}",
            );
        }
    }

    #[rstest]
    #[case(
        DeriveOrderType::Market,
        Some(DeriveTriggerType::Stoploss),
        OrderType::StopMarket
    )]
    #[case(
        DeriveOrderType::Limit,
        Some(DeriveTriggerType::Stoploss),
        OrderType::StopLimit
    )]
    #[case(
        DeriveOrderType::Market,
        Some(DeriveTriggerType::Takeprofit),
        OrderType::MarketIfTouched
    )]
    #[case(
        DeriveOrderType::Limit,
        Some(DeriveTriggerType::Takeprofit),
        OrderType::LimitIfTouched
    )]
    #[case(DeriveOrderType::Limit, None, OrderType::Limit)]
    fn test_derive_order_type_to_nautilus_for_order(
        #[case] order_type: DeriveOrderType,
        #[case] trigger_type: Option<DeriveTriggerType>,
        #[case] expected: OrderType,
    ) {
        assert_eq!(
            derive_order_type_to_nautilus_for_order(order_type, trigger_type),
            expected,
        );
    }

    #[rstest]
    fn test_derive_status_to_nautilus_maps_untriggered_to_accepted() {
        assert_eq!(
            derive_status_to_nautilus(DeriveOrderStatus::Untriggered, dec!(0), dec!(1)),
            OrderStatus::Accepted,
        );
    }

    #[rstest]
    #[case(
        Some(DERIVE_POST_ONLY_CROSS_MARKET_ERROR_CODE),
        "Post only order cannot cross the market",
        true
    )]
    #[case(
        Some(DERIVE_POST_ONLY_CROSS_MARKET_ERROR_CODE),
        "post only order cannot cross the market",
        true
    )]
    #[case(None, "Post only order cannot cross the market", true)]
    #[case(Some(-32602), "Post only order cannot cross the market", false)]
    #[case(Some(DERIVE_POST_ONLY_CROSS_MARKET_ERROR_CODE), "Invalid params", true)]
    fn test_derive_rejection_due_post_only(
        #[case] code: Option<i64>,
        #[case] reason: &str,
        #[case] expected: bool,
    ) {
        assert_eq!(derive_rejection_due_post_only(code, reason), expected);
    }

    #[rstest]
    fn test_parse_perp_instrument() {
        let instrument = parse_derive_instrument_any(&perp_fixture(), UnixNanos::from(123))
            .unwrap()
            .unwrap();

        let InstrumentAny::CryptoPerpetual(perp) = instrument else {
            panic!("expected CryptoPerpetual");
        };

        assert_eq!(perp.id(), InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(perp.raw_symbol().as_str(), "ETH-PERP");
        assert_eq!(perp.base_currency(), Some(Currency::ETH()));
        assert_eq!(perp.quote_currency(), Currency::USDC());
        assert_eq!(perp.settlement_currency(), Currency::USDC());
        assert_eq!(perp.price_increment(), Price::from("0.01"));
        assert_eq!(perp.size_increment(), Quantity::from("0.001"));
        assert_eq!(perp.max_quantity(), Some(Quantity::from("1000")));
        assert_eq!(perp.min_quantity(), Some(Quantity::from("0.001")));
        assert_eq!(perp.maker_fee(), dec!(0.0001));
        assert_eq!(perp.taker_fee(), dec!(0.0005));
        assert!(!perp.is_inverse());

        // `info` mirrors the raw venue payload so downstream consumers can read
        // fields the core model does not expose (asset address, sub-id, perp
        // funding details, etc.).
        let info = perp.info.as_ref().expect("info populated");
        assert_eq!(info.get_str("instrument_name"), Some("ETH-PERP"));
        assert_eq!(info.get_str("instrument_type"), Some("perp"));
        assert_eq!(info.get_str("base_asset_sub_id"), Some("0"));
        assert!(info.get("perp_details").is_some_and(|v| v.is_object()));
    }

    #[rstest]
    fn test_parse_option_instrument() {
        let instrument = parse_derive_instrument_any(&option_fixture(), UnixNanos::from(456))
            .unwrap()
            .unwrap();

        let InstrumentAny::CryptoOption(option) = instrument else {
            panic!("expected CryptoOption");
        };

        assert_eq!(
            option.id(),
            InstrumentId::from("ETH-20260627-3500-C.DERIVE")
        );
        assert_eq!(option.raw_symbol().as_str(), "ETH-20260627-3500-C");
        assert_eq!(option.base_currency(), Some(Currency::ETH()));
        assert_eq!(option.quote_currency(), Currency::USDC());
        assert_eq!(option.settlement_currency(), Currency::USDC());
        assert_eq!(option.option_kind(), Some(OptionKind::Call));
        assert_eq!(option.strike_price(), Some(Price::from("3500")));
        assert_eq!(
            option.activation_ns(),
            Some(UnixNanos::from(1_700_000_000_000_000_000)),
        );
        assert_eq!(
            option.expiration_ns(),
            Some(UnixNanos::from(1_782_000_000_000_000_000)),
        );
        assert_eq!(option.price_increment(), Price::from("1"));
        assert_eq!(option.size_increment(), Quantity::from("0.01"));
        assert_eq!(option.max_quantity(), Some(Quantity::from("100")));
        assert_eq!(option.min_quantity(), Some(Quantity::from("0.01")));
        assert_eq!(option.taker_fee(), dec!(0.001));

        let info = option.info.as_ref().expect("info populated");
        assert_eq!(info.get_str("instrument_name"), Some("ETH-20260627-3500-C"));
        assert_eq!(info.get_str("instrument_type"), Some("option"));
        let option_details = info.get("option_details").expect("option_details present");
        assert_eq!(
            option_details.get("option_type").and_then(|v| v.as_str()),
            Some("C")
        );
        assert_eq!(
            option_details.get("strike").and_then(|v| v.as_str()),
            Some("3500")
        );
    }

    #[rstest]
    fn test_symbol_instrument_id_mapping() {
        let instrument_id = format_instrument_id("ETH-20260627-3500-C");
        let venue_symbol = format_venue_symbol(&instrument_id).unwrap();

        assert_eq!(
            instrument_id,
            InstrumentId::from("ETH-20260627-3500-C.DERIVE")
        );
        assert_eq!(venue_symbol, "ETH-20260627-3500-C");
    }

    #[rstest]
    fn test_format_venue_symbol_rejects_non_derive_venue() {
        let instrument_id = InstrumentId::from("ETH-PERP.BINANCE");

        let err = format_venue_symbol(&instrument_id).expect_err("must reject non-Derive venue");

        assert!(err.to_string().contains("not for venue DERIVE"));
    }

    #[rstest]
    fn test_parse_spot_instrument() {
        let instrument = parse_derive_instrument_any(&spot_fixture(), UnixNanos::from(789))
            .unwrap()
            .unwrap();

        let InstrumentAny::CurrencyPair(pair) = instrument else {
            panic!("expected CurrencyPair");
        };

        assert_eq!(pair.id(), InstrumentId::from("ETH-USDC.DERIVE"));
        assert_eq!(pair.raw_symbol().as_str(), "ETH-USDC");
        assert_eq!(pair.base_currency(), Some(Currency::ETH()));
        assert_eq!(pair.quote_currency(), Currency::USDC());
        assert_eq!(pair.price_increment(), Price::from("0.1"));
        assert_eq!(pair.size_increment(), Quantity::from("0.01"));
        assert_eq!(pair.max_quantity(), Some(Quantity::from("10000")));
        assert_eq!(pair.min_quantity(), Some(Quantity::from("0.1")));
        assert_eq!(pair.maker_fee(), dec!(0));
        assert_eq!(pair.taker_fee(), dec!(0));

        let info = pair.info.as_ref().expect("info populated");
        assert_eq!(info.get_str("instrument_name"), Some("ETH-USDC"));
        assert_eq!(info.get_str("instrument_type"), Some("erc20"));
        assert_eq!(info.get_str("base_asset_sub_id"), Some("0"));
        assert_eq!(
            info.get_str("base_asset_address"),
            Some("0x41675b7746AE0E464f2594d258CF399c392A179C"),
        );
    }

    #[rstest]
    fn test_parse_spot_instrument_maps_fee_slots_distinctly() {
        // The shipped spot fixtures have maker_fee == taker_fee, so the
        // round-trip test above cannot catch a swap between the slots. Pin
        // the mapping with distinct values.
        let mut instrument = spot_fixture();
        instrument.maker_fee_rate = dec!(0.0001);
        instrument.taker_fee_rate = dec!(0.0005);

        let parsed = parse_derive_instrument_any(&instrument, UnixNanos::from(0))
            .unwrap()
            .unwrap();
        let InstrumentAny::CurrencyPair(pair) = parsed else {
            panic!("expected CurrencyPair");
        };

        assert_eq!(pair.maker_fee(), dec!(0.0001));
        assert_eq!(pair.taker_fee(), dec!(0.0005));
    }

    #[rstest]
    fn test_parse_perp_instrument_rejects_missing_perp_details() {
        let mut instrument = perp_fixture();
        instrument.perp_details = None;

        let err = parse_derive_instrument_any(&instrument, UnixNanos::from(123))
            .expect_err("must reject missing perp details");

        assert!(err.to_string().contains("missing perp_details"));
    }

    #[rstest]
    fn test_parse_option_instrument_rejects_missing_option_details() {
        let mut instrument = option_fixture();
        instrument.option_details = None;

        let err = parse_derive_instrument_any(&instrument, UnixNanos::from(123))
            .expect_err("must reject missing option details");

        assert!(err.to_string().contains("missing option_details"));
    }

    #[rstest]
    fn test_parse_option_instrument_rejects_negative_activation() {
        let mut instrument = option_fixture();
        instrument.scheduled_activation = -1;

        let err = parse_derive_instrument_any(&instrument, UnixNanos::from(123))
            .expect_err("must reject negative activation timestamp");

        assert!(
            err.to_string()
                .contains("negative Derive scheduled_activation")
        );
    }

    #[rstest]
    fn test_parse_option_instrument_rejects_negative_expiry() {
        let mut instrument = option_fixture();
        instrument.option_details.as_mut().unwrap().expiry = -1;

        let err = parse_derive_instrument_any(&instrument, UnixNanos::from(123))
            .expect_err("must reject negative expiry timestamp");

        assert!(
            err.to_string()
                .contains("negative Derive option_details.expiry")
        );
    }
}
