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

use databento::dbn;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::{
        CurrencyPair, Equity, FuturesContract, FuturesSpread, InstrumentAny, OptionContract,
        OptionSpread,
    },
    types::{Currency, Quantity},
};
use ustr::Ustr;

use super::{
    expiration::{DatabentoDecodeConfig, corrected_option_expiration},
    primitives::{
        decode_lot_size, decode_multiplier, decode_optional_timestamp, decode_price,
        decode_price_increment, decode_timestamp, decode_underlying, parse_cfi_iso10926,
        parse_currency_or_usd_default, parse_option_kind,
    },
};

/// # Errors
///
/// Returns an error if decoding the `InstrumentDefMsg` fails.
///
/// Returns `Ok(None)` for instrument classes with no Nautilus equivalent (`'I'` Index,
/// `'B'` Bond, or any future class) and FX spots that cannot be mapped to known currencies.
pub fn decode_instrument_def_msg(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
    decode_config: Option<&DatabentoDecodeConfig>,
) -> anyhow::Result<Option<InstrumentAny>> {
    match msg.instrument_class as u8 as char {
        'K' => Ok(Some(InstrumentAny::Equity(decode_equity(
            msg,
            instrument_id,
            ts_init,
        )?))),
        'X' => {
            Ok(decode_currency_pair(msg, instrument_id, ts_init)?.map(InstrumentAny::CurrencyPair))
        }
        'F' => Ok(Some(InstrumentAny::FuturesContract(
            decode_futures_contract(msg, instrument_id, ts_init)?,
        ))),
        'S' => Ok(Some(InstrumentAny::FuturesSpread(decode_futures_spread(
            msg,
            instrument_id,
            ts_init,
        )?))),
        'C' | 'P' => Ok(Some(InstrumentAny::OptionContract(decode_option_contract(
            msg,
            instrument_id,
            ts_init,
            decode_config,
        )?))),
        'T' | 'M' => Ok(Some(InstrumentAny::OptionSpread(decode_option_spread(
            msg,
            instrument_id,
            ts_init,
            decode_config,
        )?))),
        other => {
            let label = match other {
                'I' => "'I' (Index)".to_string(),
                'B' => "'B' (Bond)".to_string(),
                _ => format!("'{other}'"),
            };
            log::warn!("Skipping unsupported `instrument_class` {label} for {instrument_id}",);
            Ok(None)
        }
    }
}

fn decode_currency_pair(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<Option<CurrencyPair>> {
    let raw_symbol_str = msg.raw_symbol()?;
    let raw_symbol = Symbol::from(raw_symbol_str);
    let Some((base_currency, quote_currency)) = parse_fx_pair(
        raw_symbol_str,
        msg.asset().unwrap_or_default(),
        msg.currency().unwrap_or_default(),
    ) else {
        log::warn!(
            "Skipping FX spot {instrument_id}: could not parse currencies from raw_symbol='{raw_symbol_str}'"
        );
        return Ok(None);
    };
    let price_increment = decode_price_increment(msg.min_price_increment, quote_currency.precision);
    let size_increment = decode_lot_size(msg.min_lot_size_round_lot);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty)?;
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv);
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(Some(
        CurrencyPair::builder()
            .instrument_id(instrument_id)
            .raw_symbol(raw_symbol)
            .base_currency(base_currency)
            .quote_currency(quote_currency)
            .price_precision(price_increment.precision)
            .size_precision(size_increment.precision)
            .price_increment(price_increment)
            .size_increment(size_increment)
            .multiplier(multiplier)
            .lot_size(lot_size)
            .ts_event(ts_event)
            .ts_init(ts_init)
            .build()?,
    ))
}

fn parse_fx_pair(raw_symbol: &str, asset: &str, currency: &str) -> Option<(Currency, Currency)> {
    parse_fx_pair_from_symbol(raw_symbol)
        .or_else(|| parse_fx_pair_from_symbol(asset))
        .or_else(|| parse_fx_pair_from_asset_currency(asset, currency))
}

fn parse_fx_pair_from_symbol(value: &str) -> Option<(Currency, Currency)> {
    let normalized = value
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();

    if normalized.len() != 6 {
        return None;
    }

    let base = Currency::try_from_str(&normalized[..3])?;
    let quote = Currency::try_from_str(&normalized[3..])?;
    Some((base, quote))
}

fn parse_fx_pair_from_asset_currency(asset: &str, currency: &str) -> Option<(Currency, Currency)> {
    let base = Currency::try_from_str(asset.trim().to_ascii_uppercase().as_str())?;
    let quote = Currency::try_from_str(currency.trim().to_ascii_uppercase().as_str())?;
    Some((base, quote))
}

/// Decodes a Databento instrument definition message into an `Equity` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `Equity` fails.
///
/// # Panics
///
/// Panics if the constructed instrument fails validation.
pub fn decode_equity(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<Equity> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(Equity::builder()
        .instrument_id(instrument_id)
        .raw_symbol(instrument_id.symbol)
        // No ISIN available yet
        .currency(currency)
        .price_precision(price_increment.precision)
        .price_increment(price_increment)
        .lot_size(lot_size)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()
        .unwrap())
}

/// Decodes a Databento instrument definition message into a `FuturesContract` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `FuturesContract` fails.
pub fn decode_futures_contract(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<FuturesContract> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = decode_underlying(msg.asset()?, &instrument_id.symbol);
    let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?);
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty)?;
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(FuturesContract::builder()
        .instrument_id(instrument_id)
        .raw_symbol(instrument_id.symbol)
        .asset_class(asset_class.unwrap_or(AssetClass::Commodity))
        .exchange(exchange)
        .underlying(underlying)
        .activation_ns(decode_optional_timestamp(msg.activation).unwrap_or_default())
        .expiration_ns(decode_timestamp(msg.expiration, "expiration")?)
        .currency(currency)
        .price_precision(price_increment.precision)
        .price_increment(price_increment)
        .multiplier(multiplier)
        .lot_size(lot_size)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()?)
}

/// Decodes a Databento instrument definition message into a `FuturesSpread` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `FuturesSpread` fails.
pub fn decode_futures_spread(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<FuturesSpread> {
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = decode_underlying(msg.asset()?, &instrument_id.symbol);
    let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?);
    let strategy_type = Ustr::from(msg.secsubtype()?);
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty)?;
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(FuturesSpread::builder()
        .instrument_id(instrument_id)
        .raw_symbol(instrument_id.symbol)
        .asset_class(asset_class.unwrap_or(AssetClass::Commodity))
        .exchange(exchange)
        .underlying(underlying)
        .strategy_type(strategy_type)
        .activation_ns(decode_optional_timestamp(msg.activation).unwrap_or_default())
        .expiration_ns(decode_timestamp(msg.expiration, "expiration")?)
        .currency(currency)
        .price_precision(price_increment.precision)
        .price_increment(price_increment)
        .multiplier(multiplier)
        .lot_size(lot_size)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()?)
}

/// Decodes a Databento instrument definition message into an `OptionContract` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `OptionContract` fails.
pub fn decode_option_contract(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
    decode_config: Option<&DatabentoDecodeConfig>,
) -> anyhow::Result<OptionContract> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let strike_price_currency = parse_currency_or_usd_default(msg.strike_price_currency());
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = decode_underlying(msg.underlying()?, &instrument_id.symbol);
    let asset_class_opt = if instrument_id.venue.as_str() == "OPRA" {
        Some(AssetClass::Equity)
    } else {
        let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?);
        asset_class
    };
    let option_kind = parse_option_kind(msg.instrument_class)?;
    let strike_price = decode_price(
        msg.strike_price,
        strike_price_currency.precision,
        "strike_price",
    )?;
    let dataset = msg.hd.publisher().ok().map(|p| p.dataset());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier =
        decode_option_multiplier(dataset, msg.contract_multiplier, msg.unit_of_measure_qty)?;
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let expiration = corrected_option_expiration(
        decode_timestamp(msg.expiration, "expiration")?,
        underlying,
        dataset,
        decode_config,
    );
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(OptionContract::builder()
        .instrument_id(instrument_id)
        .raw_symbol(instrument_id.symbol)
        .asset_class(asset_class_opt.unwrap_or(AssetClass::Commodity))
        .exchange(exchange)
        .underlying(underlying)
        .option_kind(option_kind)
        .strike_price(strike_price)
        .currency(currency)
        .activation_ns(decode_optional_timestamp(msg.activation).unwrap_or_default())
        .expiration_ns(expiration)
        .price_precision(price_increment.precision)
        .price_increment(price_increment)
        .multiplier(multiplier)
        .lot_size(lot_size)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()?)
}

fn decode_option_multiplier(
    dataset: Option<dbn::Dataset>,
    contract_multiplier: i32,
    unit_of_measure_qty: i64,
) -> anyhow::Result<Quantity> {
    match (dataset, contract_multiplier) {
        (Some(dbn::Dataset::OpraPillar), 0 | i32::MAX) => decode_multiplier(unit_of_measure_qty),
        (Some(dbn::Dataset::OpraPillar), value) if value < 0 => {
            anyhow::bail!("Invalid negative `contract_multiplier`: {value}")
        }
        (Some(dbn::Dataset::OpraPillar), value) => {
            Ok(Quantity::from_mantissa_exponent(value as u64, 0, 0))
        }
        _ => decode_multiplier(unit_of_measure_qty),
    }
}

/// Decodes a Databento instrument definition message into an `OptionSpread` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `OptionSpread` fails.
pub fn decode_option_spread(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
    decode_config: Option<&DatabentoDecodeConfig>,
) -> anyhow::Result<OptionSpread> {
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = decode_underlying(msg.underlying()?, &instrument_id.symbol);
    let asset_class_opt = if instrument_id.venue.as_str() == "OPRA" {
        Some(AssetClass::Equity)
    } else {
        let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?);
        asset_class
    };
    let strategy_type = Ustr::from(msg.secsubtype()?);
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty)?;
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let expiration = corrected_option_expiration(
        decode_timestamp(msg.expiration, "expiration")?,
        underlying,
        msg.hd.publisher().ok().map(|p| p.dataset()),
        decode_config,
    );
    let ts_event = msg.ts_recv.into(); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(OptionSpread::builder()
        .instrument_id(instrument_id)
        .raw_symbol(instrument_id.symbol)
        .asset_class(asset_class_opt.unwrap_or(AssetClass::Commodity))
        .exchange(exchange)
        .underlying(underlying)
        .strategy_type(strategy_type)
        .activation_ns(decode_optional_timestamp(msg.activation).unwrap_or_default())
        .expiration_ns(expiration)
        .currency(currency)
        .price_precision(price_increment.precision)
        .price_increment(price_increment)
        .multiplier(multiplier)
        .lot_size(lot_size)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()?)
}
