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

use anyhow::Context;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND, params::Params};
use nautilus_model::{
    enums::{OptionKind, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType},
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoOption, CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::{Value, value::RawValue};
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

const DERIVE_POST_ONLY_CROSS_MARKET_MESSAGE: &str = "post only order cannot cross the market";

/// JSON-RPC error code returned when a post-only order crosses the market.
pub const DERIVE_POST_ONLY_CROSS_MARKET_ERROR_CODE: i64 = 11008;

/// Converts a Derive venue symbol to a Nautilus instrument ID.
///
/// # Errors
///
/// Returns an error when the venue symbol is empty or contains only whitespace.
pub fn format_instrument_id(venue_symbol: impl AsRef<str>) -> anyhow::Result<InstrumentId> {
    let symbol =
        Symbol::new_checked(venue_symbol.as_ref()).context("invalid Derive instrument_name")?;
    Ok(InstrumentId::new(symbol, *DERIVE_VENUE))
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
    Ok(instrument_id.symbol.inner())
}

/// Deserializes a JSON array into `Vec<T>`, salvaging the decodable elements
/// (see [`salvage_elements`]).
///
/// # Errors
///
/// Returns an error when the value is not a JSON array.
pub fn deserialize_salvaged_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Ok(salvage_rows(Vec::<Box<RawValue>>::deserialize(
        deserializer,
    )?))
}

/// Decodes each element of a JSON array into `T`, logging and skipping
/// elements that fail to decode instead of failing the whole collection.
///
/// Venue enum sets drift over time, so one unmodeled trade or account row
/// must degrade to a logged skip rather than discard its siblings (the
/// Hyperliquid dust-conversion incident shape). Reserved for rows where a
/// missed element is recoverable (fills backfill via reconciliation); order
/// and position arrays feeding mass status stay strict because absence there
/// is read as state. The log carries only the decode error (which names the
/// failing field and value); private rows hold signatures and wallet
/// addresses, so the raw payload stays out of the logs.
pub fn salvage_elements<T: DeserializeOwned>(values: Vec<Value>) -> Vec<T> {
    salvage_decoded(values.into_iter().map(serde_json::from_value))
}

pub(crate) fn salvage_rows<T: DeserializeOwned>(values: Vec<Box<RawValue>>) -> Vec<T> {
    salvage_decoded(
        values
            .into_iter()
            .map(|value| serde_json::from_str(value.get())),
    )
}

fn salvage_decoded<T>(values: impl Iterator<Item = serde_json::Result<T>>) -> Vec<T> {
    let context = std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .unwrap_or("element");
    let mut elements = Vec::with_capacity(values.size_hint().0);
    for value in values {
        match value {
            Ok(element) => elements.push(element),
            Err(e) => log::warn!("Skipping undecodable {context} element: {e}"),
        }
    }

    elements
}

pub(crate) fn deserialize_decimal<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Decimal, D::Error> {
    nautilus_core::serialization::deserialize_optional_decimal_token(deserializer)
        .map(|value| value.unwrap_or(Decimal::ZERO))
}

/// Maps a Nautilus order side to the Derive direction string.
pub fn order_side_to_derive(side: OrderSide) -> DeriveOrderSide {
    match side {
        OrderSide::Buy => DeriveOrderSide::Buy,
        OrderSide::Sell => DeriveOrderSide::Sell,
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
///
/// # Errors
///
/// Returns an error for an unmodeled order type.
pub fn derive_order_type_to_nautilus(order_type: DeriveOrderType) -> anyhow::Result<OrderType> {
    match order_type {
        DeriveOrderType::Limit => Ok(OrderType::Limit),
        DeriveOrderType::Market => Ok(OrderType::Market),
        DeriveOrderType::Unknown => anyhow::bail!("unmodeled Derive order type"),
    }
}

/// Maps a Derive trigger order record back to the Nautilus order type.
///
/// # Errors
///
/// Returns an error for an unmodeled order or trigger type.
pub fn derive_order_type_to_nautilus_for_order(
    order_type: DeriveOrderType,
    trigger_type: Option<DeriveTriggerType>,
) -> anyhow::Result<OrderType> {
    match (order_type, trigger_type) {
        (DeriveOrderType::Market, Some(DeriveTriggerType::Stoploss)) => Ok(OrderType::StopMarket),
        (DeriveOrderType::Limit, Some(DeriveTriggerType::Stoploss)) => Ok(OrderType::StopLimit),
        (DeriveOrderType::Market, Some(DeriveTriggerType::Takeprofit)) => {
            Ok(OrderType::MarketIfTouched)
        }
        (DeriveOrderType::Limit, Some(DeriveTriggerType::Takeprofit)) => {
            Ok(OrderType::LimitIfTouched)
        }
        (_, Some(DeriveTriggerType::Unknown)) => anyhow::bail!("unmodeled Derive trigger type"),
        (order_type, _) => derive_order_type_to_nautilus(order_type),
    }
}

/// Maps a Derive trigger price source back to Nautilus.
///
/// # Errors
///
/// Returns an error for an unmodeled trigger price source.
pub fn derive_trigger_price_type_to_nautilus(
    trigger_price_type: DeriveTriggerPriceType,
) -> anyhow::Result<TriggerType> {
    match trigger_price_type {
        DeriveTriggerPriceType::Mark => Ok(TriggerType::MarkPrice),
        DeriveTriggerPriceType::Index => Ok(TriggerType::IndexPrice),
        DeriveTriggerPriceType::Unknown => anyhow::bail!("unmodeled Derive trigger price type"),
    }
}

/// Maps a Derive TIF back to Nautilus.
///
/// # Errors
///
/// Returns an error for an unmodeled time-in-force flag.
pub fn derive_tif_to_nautilus(tif: DeriveTimeInForce) -> anyhow::Result<TimeInForce> {
    match tif {
        DeriveTimeInForce::Gtc | DeriveTimeInForce::PostOnly => Ok(TimeInForce::Gtc),
        DeriveTimeInForce::Ioc => Ok(TimeInForce::Ioc),
        DeriveTimeInForce::Fok => Ok(TimeInForce::Fok),
        DeriveTimeInForce::Unknown => anyhow::bail!("unmodeled Derive time in force"),
    }
}

/// Maps a Derive order status to the Nautilus equivalent, given the current
/// filled quantity.
///
/// # Errors
///
/// Returns an error for an unmodeled order status.
pub fn derive_status_to_nautilus(
    status: DeriveOrderStatus,
    filled_qty: Decimal,
    quantity: Decimal,
) -> anyhow::Result<OrderStatus> {
    let status = match status {
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
        DeriveOrderStatus::Unknown => anyhow::bail!("unmodeled Derive order status"),
    };

    Ok(status)
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
/// Perpetuals are normalized to USDC quote and settlement: the wire quotes
/// perps in `"USD"` index terms, while all Derive collateral, fees, and PnL
/// settle in USDC, so Money currencies must match the account balances. The
/// raw wire values remain in the instrument `info` payload.
///
/// Derive can fill taker orders below `minimum_amount`, so that venue value
/// remains in `info` rather than becoming an unconditional `min_quantity`.
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
        DeriveInstrumentType::Unknown => {
            log::warn!(
                "Skipping Derive instrument {} with unmodeled instrument type",
                instrument.instrument_name,
            );
            Ok(None)
        }
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

    let instrument_id = format_instrument_id(instrument.instrument_name)?;
    let raw_symbol = instrument_id.symbol;
    let base_currency = Currency::get_or_create_crypto(instrument.base_currency);
    // Wire says "USD" but Derive settles everything in USDC
    let quote_currency = Currency::USDC();
    let settlement_currency = quote_currency;
    let price_increment = price_from_decimal(instrument.tick_size, "tick_size")?;
    let size_increment = quantity_from_decimal(instrument.amount_step, "amount_step")?;
    let multiplier = quantity_from_decimal(Decimal::ONE, "multiplier")?;
    let max_quantity = quantity_from_decimal(instrument.maximum_amount, "maximum_amount")?;
    let info = derive_instrument_info(instrument)?;

    let perp = CryptoPerpetual::builder()
        .instrument_id(instrument_id)
        .raw_symbol(raw_symbol)
        .base_currency(base_currency)
        .quote_currency(quote_currency)
        .settlement_currency(settlement_currency)
        .is_inverse(false)
        .price_precision(price_increment.precision)
        .size_precision(size_increment.precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .multiplier(multiplier)
        .lot_size(size_increment)
        .max_quantity(max_quantity)
        .info(info)
        .ts_event(ts_init)
        .ts_init(ts_init)
        .build()?;

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

    let instrument_id = format_instrument_id(instrument.instrument_name)?;
    let raw_symbol = instrument_id.symbol;
    let underlying = Currency::get_or_create_crypto(instrument.base_currency);
    let quote_currency = Currency::get_or_create_crypto(instrument.quote_currency);
    let settlement_currency = quote_currency;
    let option_kind = parse_option_kind(details.option_type);
    let strike_price = price_from_decimal(details.strike, "option_details.strike")?;
    let activation_ns =
        timestamp_seconds_to_nanos(instrument.scheduled_activation, "scheduled_activation")?;
    let expiration_ns = timestamp_seconds_to_nanos(details.expiry, "option_details.expiry")?;
    let price_increment = price_from_decimal(instrument.tick_size, "tick_size")?;
    let size_increment = quantity_from_decimal(instrument.amount_step, "amount_step")?;
    let multiplier = quantity_from_decimal(Decimal::ONE, "multiplier")?;
    let max_quantity = quantity_from_decimal(instrument.maximum_amount, "maximum_amount")?;
    let info = derive_instrument_info(instrument)?;

    let option = CryptoOption::builder()
        .instrument_id(instrument_id)
        .raw_symbol(raw_symbol)
        .underlying(underlying)
        .quote_currency(quote_currency)
        .settlement_currency(settlement_currency)
        .is_inverse(false)
        .option_kind(option_kind)
        .strike_price(strike_price)
        .activation_ns(activation_ns)
        .expiration_ns(expiration_ns)
        .price_precision(price_increment.precision)
        .size_precision(size_increment.precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .multiplier(multiplier)
        .lot_size(size_increment)
        .max_quantity(max_quantity)
        .info(info)
        .ts_event(ts_init)
        .ts_init(ts_init)
        .build()?;

    Ok(InstrumentAny::CryptoOption(option))
}

fn parse_spot_instrument(
    instrument: &DeriveInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = format_instrument_id(instrument.instrument_name)?;
    let raw_symbol = instrument_id.symbol;
    let base_currency = Currency::get_or_create_crypto(instrument.base_currency);
    let quote_currency = Currency::get_or_create_crypto(instrument.quote_currency);
    let price_increment = price_from_decimal(instrument.tick_size, "tick_size")?;
    let size_increment = quantity_from_decimal(instrument.amount_step, "amount_step")?;
    let multiplier = quantity_from_decimal(Decimal::ONE, "multiplier")?;
    let max_quantity = quantity_from_decimal(instrument.maximum_amount, "maximum_amount")?;
    let info = derive_instrument_info(instrument)?;

    let pair = CurrencyPair::builder()
        .instrument_id(instrument_id)
        .raw_symbol(raw_symbol)
        .base_currency(base_currency)
        .quote_currency(quote_currency)
        .price_precision(price_increment.precision)
        .size_precision(size_increment.precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .multiplier(multiplier)
        .lot_size(size_increment)
        .max_quantity(max_quantity)
        .info(info)
        .ts_event(ts_init)
        .ts_init(ts_init)
        .build()?;

    Ok(InstrumentAny::CurrencyPair(pair))
}

fn parse_option_kind(kind: DeriveOptionKind) -> OptionKind {
    match kind {
        DeriveOptionKind::Call => OptionKind::Call,
        DeriveOptionKind::Put => OptionKind::Put,
    }
}

// Reconstruct metadata only for definitions created without a venue response
fn derive_instrument_info(instrument: &DeriveInstrument) -> anyhow::Result<Params> {
    if let Some(raw) = &instrument.raw {
        return Ok(raw.clone());
    }

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

fn timestamp_to_nanos(value: i64, multiplier: u64, field: &str) -> anyhow::Result<UnixNanos> {
    let value = u64::try_from(value).with_context(|| format!("negative Derive {field}"))?;
    let nanos = value
        .checked_mul(multiplier)
        .with_context(|| format!("Derive {field} overflows nanoseconds"))?;
    Ok(UnixNanos::from(nanos))
}

pub(crate) const STRATEGY_REASON_MAX_CHARS: usize = 256;

pub(crate) fn strategy_rejection_reason(message: &str) -> String {
    let mut output = String::new();
    let mut markup = false;
    let mut length = 0;

    for character in message.chars() {
        let previous_size = output.len();

        match character {
            '<' => markup = true,
            '>' => markup = false,
            _ if markup => {}
            _ if character.is_whitespace() => {
                if !output.is_empty() && !output.ends_with(' ') {
                    output.push(' ');
                }
            }
            _ if character.is_control() => {}
            _ => output.push(character),
        }

        length += usize::from(output.len() != previous_size);
        if length >= STRATEGY_REASON_MAX_CHARS {
            break;
        }
    }

    let output = output.trim_end();
    if output.is_empty() {
        "Order command rejected".to_string()
    } else {
        output.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nautilus_core::{UnixNanos, serialization::ToMsgPack};
    use nautilus_model::{
        enums::{OptionKind, OrderStatus, OrderType, TriggerType},
        identifiers::InstrumentId,
        instruments::{Instrument, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde::Serialize;
    use serde_json::{Value, json};

    use super::*;

    #[rstest]
    fn test_json_numeric_params_keep_messagepack_scalar_encoding() {
        #[derive(Serialize)]
        #[serde(transparent)]
        struct NumericParams(Params);
        impl ToMsgPack for NumericParams {}

        let mut params = Params::new();
        params.insert("integer".to_owned(), json!(42));
        params.insert("fraction".to_owned(), json!(0.125));
        let encoded = NumericParams(params).to_msgpack_bytes().unwrap();

        assert_eq!(
            encoded.as_ref(),
            &[
                0x82, 0xa7, b'i', b'n', b't', b'e', b'g', b'e', b'r', 42, 0xa8, b'f', b'r', b'a',
                b'c', b't', b'i', b'o', b'n', 0xcb, 0x3f, 0xc0, 0, 0, 0, 0, 0, 0,
            ],
        );
    }

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
            derive_order_type_to_nautilus_for_order(order_type, trigger_type).unwrap(),
            expected,
        );
    }

    #[rstest]
    fn test_unknown_wire_variants_reject_domain_conversion() {
        assert!(derive_order_type_to_nautilus(DeriveOrderType::Unknown).is_err());
        assert!(
            derive_order_type_to_nautilus_for_order(
                DeriveOrderType::Limit,
                Some(DeriveTriggerType::Unknown)
            )
            .is_err()
        );
        assert!(derive_tif_to_nautilus(DeriveTimeInForce::Unknown).is_err());
        assert!(derive_trigger_price_type_to_nautilus(DeriveTriggerPriceType::Unknown).is_err());
        assert!(derive_status_to_nautilus(DeriveOrderStatus::Unknown, dec!(0), dec!(1)).is_err());
    }

    #[rstest]
    fn test_salvage_elements_skips_undecodable_rows() {
        let values = vec![json!(1), json!("not a number"), json!(2)];

        let salvaged: Vec<i64> = salvage_elements(values);

        assert_eq!(salvaged, vec![1, 2]);
    }

    #[rstest]
    fn test_derive_status_to_nautilus_maps_untriggered_to_accepted() {
        assert_eq!(
            derive_status_to_nautilus(DeriveOrderStatus::Untriggered, dec!(0), dec!(1)).unwrap(),
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
    #[case("perps/instrument_eth.json")]
    #[case("options/instrument_eth.json")]
    #[case("spot/instrument_eth.json")]
    fn test_instrument_parser_rejects_blank_symbol_without_panic(
        #[case] filename: &str,
        #[values("", " ", "\t\n")] symbol: &str,
    ) {
        let mut instrument: DeriveInstrument = serde_json::from_value(load_json(filename)).unwrap();
        instrument.instrument_name = symbol.into();
        let result = std::panic::catch_unwind(|| {
            parse_derive_instrument_any(&instrument, UnixNanos::from(17))
        });

        assert!(result.is_ok(), "untrusted symbols must not panic");
        assert!(result.unwrap().is_err());
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
        // Fixture carries the live wire quote "USD"; parser normalizes to USDC
        assert_eq!(perp.quote_currency(), Currency::USDC());
        assert_eq!(perp.settlement_currency(), Currency::USDC());
        assert_eq!(perp.price_increment(), Price::from("0.01"));
        assert_eq!(perp.size_increment(), Quantity::from("0.001"));
        assert_eq!(perp.max_quantity(), Some(Quantity::from("10000")));
        assert_eq!(perp.min_quantity(), None);
        assert!(!perp.is_inverse());

        // `info` mirrors the raw venue payload so downstream consumers can read
        // fields the core model does not expose (asset address, sub-id, perp
        // funding details, etc.).
        let info = perp.info.as_ref().expect("info populated");
        assert_eq!(info.get_str("instrument_name"), Some("ETH-PERP"));
        assert_eq!(info.get_str("instrument_type"), Some("perp"));
        assert_eq!(info.get_str("base_asset_sub_id"), Some("0"));
        // Normalization must not rewrite the raw venue payload.
        assert_eq!(info.get_str("quote_currency"), Some("USD"));
        assert_eq!(info.get_str("minimum_amount"), Some("0.1"));
        assert!(info.get("perp_details").is_some_and(|v| v.is_object()));
    }

    #[rstest]
    fn test_parse_perp_instrument_money_flows_settle_in_usdc() {
        // Linear notional and PnL come out in cost_currency (= quote), which
        // must match the USDC-only account
        let instrument = parse_derive_instrument_any(&perp_fixture(), UnixNanos::from(123))
            .unwrap()
            .unwrap();

        let InstrumentAny::CryptoPerpetual(perp) = instrument else {
            panic!("expected CryptoPerpetual");
        };

        let notional =
            perp.calculate_notional_value(Quantity::from("2"), Price::from("3000.00"), None);

        assert!(!perp.is_quanto());
        assert_eq!(perp.cost_currency(), Currency::USDC());
        assert_eq!(notional.currency, Currency::USDC());
        assert_eq!(notional.as_decimal(), dec!(6000));
    }

    #[rstest]
    fn test_parse_perp_instrument_pins_usdc_for_any_wire_quote() {
        // The USDC pin is unconditional, not gated on the wire saying "USD".
        let mut instrument = perp_fixture();
        instrument.quote_currency = "XUSD".into();

        let parsed = parse_derive_instrument_any(&instrument, UnixNanos::from(123))
            .unwrap()
            .unwrap();

        let InstrumentAny::CryptoPerpetual(perp) = parsed else {
            panic!("expected CryptoPerpetual");
        };

        assert_eq!(perp.quote_currency(), Currency::USDC());
        assert_eq!(perp.settlement_currency(), Currency::USDC());
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
            InstrumentId::from("ETH-20261225-3500-C.DERIVE")
        );
        assert_eq!(option.raw_symbol().as_str(), "ETH-20261225-3500-C");
        assert_eq!(option.base_currency(), Some(Currency::ETH()));
        assert_eq!(option.quote_currency(), Currency::USDC());
        assert_eq!(option.settlement_currency(), Currency::USDC());
        assert_eq!(option.option_kind(), Some(OptionKind::Call));
        assert_eq!(option.strike_price(), Some(Price::from("3500")));
        assert_eq!(
            option.activation_ns(),
            Some(UnixNanos::from(1_774_598_400_000_000_000)),
        );
        assert_eq!(
            option.expiration_ns(),
            Some(UnixNanos::from(1_798_185_600_000_000_000)),
        );
        assert_eq!(option.price_increment(), Price::from("0.1"));
        assert_eq!(option.size_increment(), Quantity::from("0.01"));
        assert_eq!(option.max_quantity(), Some(Quantity::from("10000")));
        assert_eq!(option.min_quantity(), None);

        let info = option.info.as_ref().expect("info populated");
        assert_eq!(info.get_str("instrument_name"), Some("ETH-20261225-3500-C"));
        assert_eq!(info.get_str("instrument_type"), Some("option"));
        assert_eq!(info.get_str("minimum_amount"), Some("0.1"));
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
        let instrument_id = format_instrument_id("ETH-20260627-3500-C").unwrap();
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
        assert_eq!(pair.min_quantity(), None);

        let info = pair.info.as_ref().expect("info populated");
        assert_eq!(info.get_str("instrument_name"), Some("ETH-USDC"));
        assert_eq!(info.get_str("instrument_type"), Some("erc20"));
        assert_eq!(info.get_str("minimum_amount"), Some("0.1"));
        assert_eq!(info.get_str("base_asset_sub_id"), Some("0"));
        assert_eq!(
            info.get_str("base_asset_address"),
            Some("0x41675b7746AE0E464f2594d258CF399c392A179C"),
        );
    }

    #[rstest]
    fn test_parse_instrument_without_response_uses_typed_metadata() {
        let mut instrument = perp_fixture();
        instrument.raw = None;
        instrument.minimum_amount = dec!(0.03);

        let parsed = parse_derive_instrument_any(&instrument, UnixNanos::from(123))
            .unwrap()
            .unwrap();
        let info = parsed.info().unwrap();

        assert_eq!(info.get_str("instrument_name"), Some("ETH-PERP"));
        assert_eq!(info.get_str("quote_currency"), Some("USD"));
        assert_eq!(info.get_str("minimum_amount"), Some("0.03"));
        assert_eq!(info.get("raw"), None);
    }

    #[rstest]
    #[case::perp("perps/instrument_eth.json")]
    #[case::option("options/instrument_eth.json")]
    #[case::spot("spot/instrument_eth_mainnet.json")]
    fn test_parse_instrument_preserves_complete_response(#[case] filename: &str) {
        let mut response = load_json(filename);
        response["base_fee"] = json!(0.125);
        response["pro_rata_fraction"] = json!("0.8");
        response["fifo_min_allocation"] = json!("10");
        response["pro_rata_amount_step"] = json!("1");
        response["erc20_details"] = json!({
            "decimals": 18,
            "underlying_erc20_address": "0x15CEcd5190A43C7798dD2058308781D0662e678E",
            "borrow_index": "1.000000000000000001",
            "supply_index": "1.000000000000000002",
        });
        response["additional_data"] = json!([null, true, {"value": "0.000000000000000001"}]);
        for field in ["option_details", "perp_details"] {
            if let Some(details) = response[field].as_object_mut() {
                details.insert("additional_data".to_string(), json!({"value": 42}));
            }
        }

        let instrument: DeriveInstrument = serde_json::from_value(response.clone()).unwrap();

        let parsed = parse_derive_instrument_any(&instrument, UnixNanos::from(123))
            .unwrap()
            .unwrap();

        assert_eq!(
            serde_json::to_value(parsed.info().unwrap()).unwrap(),
            response
        );
    }

    #[rstest]
    #[case::perp("perps/instrument_eth.json")]
    #[case::option("options/instrument_eth.json")]
    #[case::spot("spot/instrument_eth.json")]
    fn test_parse_instrument_preserves_trading_parameters(#[case] filename: &str) {
        let mut response = load_json(filename);
        response["tick_size"] = json!("0.005");
        response["amount_step"] = json!("0.0002");
        response["maximum_amount"] = json!("1234.5678");
        response["minimum_amount"] = json!("0.03");
        let instrument: DeriveInstrument = serde_json::from_value(response).unwrap();

        let parsed = parse_derive_instrument_any(&instrument, UnixNanos::from(987))
            .unwrap()
            .unwrap();

        assert_eq!(parsed.price_increment(), Price::from("0.005"));
        assert_eq!(parsed.size_increment(), Quantity::from("0.0002"));
        assert_eq!(parsed.price_precision(), 3);
        assert_eq!(parsed.size_precision(), 4);
        assert_eq!(parsed.multiplier(), Quantity::from("1"));
        assert_eq!(parsed.lot_size(), Some(Quantity::from("0.0002")));
        assert_eq!(parsed.max_quantity(), Some(Quantity::from("1234.5678")));
        assert_eq!(parsed.min_quantity(), None);
        assert_eq!(parsed.ts_event(), UnixNanos::from(987));
        assert_eq!(parsed.ts_init(), UnixNanos::from(987));
        assert_eq!(
            parsed.info().unwrap().get_str("minimum_amount"),
            Some("0.03")
        );
    }

    #[rstest]
    #[case::perp(DeriveInstrumentType::Perp)]
    #[case::option(DeriveInstrumentType::Option)]
    #[case::spot(DeriveInstrumentType::Erc20)]
    fn test_parse_instrument_rejects_non_positive_tick_size(
        #[case] instrument_type: DeriveInstrumentType,
    ) {
        let mut instrument = match instrument_type {
            DeriveInstrumentType::Perp => perp_fixture(),
            DeriveInstrumentType::Option => option_fixture(),
            DeriveInstrumentType::Erc20 => spot_fixture(),
            DeriveInstrumentType::Unknown => unreachable!(),
        };

        instrument.tick_size = Decimal::ZERO;

        let err = parse_derive_instrument_any(&instrument, UnixNanos::from(123))
            .expect_err("must reject non-positive tick size");
        let message = err.to_string();

        assert!(message.contains("price_increment"), "{message}");
        assert!(message.contains("not positive"), "{message}");
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
    fn test_parse_derive_instrument_any_skips_unknown_instrument_type() {
        let mut instrument = perp_fixture();
        instrument.instrument_type = DeriveInstrumentType::Unknown;

        let parsed = parse_derive_instrument_any(&instrument, UnixNanos::from(123))
            .expect("unknown instrument type must not error");

        assert!(parsed.is_none());
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
