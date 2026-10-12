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

//! Conversion of CoinAPI REST responses into Nautilus instruments and market data.
//!
//! Price and size increments come from CoinAPI `price_precision` and `size_precision`. These
//! are provider data resolutions, not venue trading increments. Market data values finer than
//! the instrument precision are rejected, except for bounded numeric tails in OHLCV
//! volume, as on the WebSocket path.

use std::{collections::HashMap, fmt::Display, str::FromStr};

use jiff::{Timestamp, civil::Date, tz::TimeZone};
use nautilus_core::{UnixNanos, correctness::check_valid_string_utf8};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, QuoteTick, TradeTick},
    enums::{AssetClass, BookType, OrderSide},
    identifiers::{InstrumentId, Symbol as RawSymbol, TradeId},
    instruments::{
        CryptoFuture, CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny, PerpetualContract,
    },
    orderbook::OrderBook,
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::models::{Book, Ohlcv, Quote, Symbol, Trade};
use crate::{
    common::symbol::{CoinApiSymbolType, Contract, SymbolAssets, SymbolError, VenueSymbol},
    websocket::parse::{parse_aggressor_side, price, quantity, timestamp, volume},
};

/// A native instrument with the full CoinAPI `symbol_id` for its transport requests and
/// subscriptions.
#[derive(Debug, Clone, PartialEq)]
pub struct CoinApiInstrument {
    pub instrument: InstrumentAny,
    pub symbol_id: Ustr,
}

/// A CoinAPI symbol that cannot become a complete native instrument.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstrumentError {
    #[error("CoinAPI symbol {symbol_id}: {source}")]
    Symbol {
        symbol_id: Ustr,
        source: SymbolError,
    },
    #[error("CoinAPI symbol {symbol_id}: invalid {field}: {reason}")]
    Field {
        symbol_id: Ustr,
        field: &'static str,
        reason: String,
    },
}

/// Parses the spot, perpetual, and futures symbols in a metadata response.
///
/// Other CoinAPI symbol types are skipped. Symbols of a parsed type that cannot be mapped or
/// validated are returned as errors alongside the instruments. Valid symbols that resolve to the
/// same native instrument ID are all rejected, because their transport symbol would be ambiguous.
#[must_use]
pub fn parse_instruments(
    symbols: &[Symbol],
    ts_init: UnixNanos,
) -> (Vec<CoinApiInstrument>, Vec<InstrumentError>) {
    let mut parsed = Vec::new();
    let mut errors = Vec::new();

    for symbol in symbols {
        if CoinApiSymbolType::from_str(&symbol.symbol_type).is_err() {
            continue;
        }

        match parse_instrument(symbol, ts_init) {
            Ok(instrument) => parsed.push(instrument),
            Err(e) => errors.push(e),
        }
    }

    let mut counts = HashMap::<InstrumentId, usize>::new();
    for instrument in &parsed {
        *counts.entry(instrument.instrument.id()).or_default() += 1;
    }

    let mut instruments = Vec::with_capacity(parsed.len());
    for instrument in parsed {
        let instrument_id = instrument.instrument.id();
        if counts[&instrument_id] > 1 {
            errors.push(InstrumentError::Symbol {
                symbol_id: instrument.symbol_id,
                source: SymbolError::Ambiguous(instrument_id),
            });

            continue;
        }

        instruments.push(instrument);
    }

    (instruments, errors)
}

/// Parses one CoinAPI symbol into a native spot, perpetual, or futures instrument.
///
/// # Errors
///
/// Returns an error if the symbol type is unsupported, the native mapping is unsupported or
/// unresolved, or a required identity, precision, contract, or date field is missing or invalid.
pub fn parse_instrument(
    symbol: &Symbol,
    ts_init: UnixNanos,
) -> Result<CoinApiInstrument, InstrumentError> {
    let symbol_id = symbol.symbol_id;

    let field = |field: &'static str, reason: &dyn Display| InstrumentError::Field {
        symbol_id,
        field,
        reason: reason.to_string(),
    };

    let required = |name: &'static str, value: Option<Ustr>| {
        let value = value
            .filter(|value| !value.is_empty())
            .ok_or_else(|| field(name, &"missing"))?;
        check_valid_string_utf8(value, name).map_err(|e| field(name, &e))?;
        Ok(value)
    };

    let symbol_type = CoinApiSymbolType::from_str(&symbol.symbol_type).map_err(|_| {
        field(
            "symbol_type",
            &format!("unsupported {}", symbol.symbol_type),
        )
    })?;

    let venue_symbol = VenueSymbol {
        exchange_id: symbol.exchange_id,
        symbol_type,
        symbol: required("symbol_id_exchange", symbol.symbol_id_exchange)?,
    };

    let assets = SymbolAssets {
        base: required("asset_id_base", symbol.asset_id_base)?,
        quote: required("asset_id_quote", symbol.asset_id_quote)?,
        base_exchange: required("asset_id_base_exchange", symbol.asset_id_base_exchange)?,
        quote_exchange: required("asset_id_quote_exchange", symbol.asset_id_quote_exchange)?,
    };

    let delivery = match symbol_type {
        CoinApiSymbolType::Futures => Some(
            symbol
                .future_delivery_time
                .ok_or_else(|| field("future_delivery_time", &"missing"))?,
        ),
        CoinApiSymbolType::Spot | CoinApiSymbolType::Perpetual => None,
    };

    let native = venue_symbol
        .to_native(
            &assets,
            delivery.map(|time| time.to_zoned(TimeZone::UTC).date()),
        )
        .map_err(|source| InstrumentError::Symbol { symbol_id, source })?;

    let price_increment = decimal(symbol.price_precision)
        .and_then(|value| Price::from_decimal(value).map_err(|e| e.to_string()))
        .map_err(|reason| field("price_precision", &reason))?;
    let size_increment = decimal(symbol.size_precision)
        .and_then(|value| Quantity::from_decimal(value).map_err(|e| e.to_string()))
        .map_err(|reason| field("size_precision", &reason))?;

    let instrument_id = native.instrument_id;
    let raw_symbol = RawSymbol::new(venue_symbol.symbol);
    let quote = Currency::get_or_create_crypto(assets.quote);

    let instrument = match (symbol_type, native.contract) {
        (CoinApiSymbolType::Spot, _) => CurrencyPair::builder()
            .instrument_id(instrument_id)
            .raw_symbol(raw_symbol)
            .base_currency(Currency::get_or_create_crypto(assets.base))
            .quote_currency(quote)
            .price_precision(price_increment.precision)
            .size_precision(size_increment.precision)
            .price_increment(price_increment)
            .size_increment(size_increment)
            .ts_event(ts_init)
            .ts_init(ts_init)
            .build()
            .map(InstrumentAny::CurrencyPair),
        (CoinApiSymbolType::Perpetual, contract) => {
            let multiplier =
                multiplier(symbol).map_err(|reason| field("future_contract_unit", &reason))?;

            match perpetual_asset_class(symbol).map_err(|(name, reason)| field(name, &reason))? {
                Some(asset_class) => PerpetualContract::builder()
                    .instrument_id(instrument_id)
                    .raw_symbol(raw_symbol)
                    .underlying(assets.base)
                    .asset_class(asset_class)
                    .quote_currency(quote)
                    .settlement_currency(quote)
                    .is_inverse(false)
                    .price_precision(price_increment.precision)
                    .size_precision(size_increment.precision)
                    .price_increment(price_increment)
                    .size_increment(size_increment)
                    .multiplier(multiplier)
                    .ts_event(ts_init)
                    .ts_init(ts_init)
                    .build()
                    .map(InstrumentAny::PerpetualContract),
                None => {
                    let base = Currency::get_or_create_crypto(assets.base);

                    CryptoPerpetual::builder()
                        .instrument_id(instrument_id)
                        .raw_symbol(raw_symbol)
                        .base_currency(base)
                        .quote_currency(quote)
                        .settlement_currency(settlement(contract, base, quote))
                        .is_inverse(contract == Contract::Inverse)
                        .price_precision(price_increment.precision)
                        .size_precision(size_increment.precision)
                        .price_increment(price_increment)
                        .size_increment(size_increment)
                        .multiplier(multiplier)
                        .ts_event(ts_init)
                        .ts_init(ts_init)
                        .build()
                        .map(InstrumentAny::CryptoPerpetual)
                }
            }
        }
        (CoinApiSymbolType::Futures, contract) => {
            let activation = symbol
                .data_start
                .as_deref()
                .ok_or_else(|| "missing".to_string())
                .and_then(|value| Date::from_str(value).map_err(|e| e.to_string()))
                .and_then(|date| {
                    date.to_zoned(TimeZone::UTC)
                        .map_err(|e| e.to_string())
                        .and_then(|zoned| unix_nanos(zoned.timestamp()))
                })
                .map_err(|reason| field("data_start", &reason))?;

            let expiration = delivery
                .map_or_else(|| Err("missing".to_string()), unix_nanos)
                .map_err(|reason| field("future_delivery_time", &reason))?;

            if activation >= expiration {
                return Err(field("data_start", &"not before future_delivery_time"));
            }

            let base = Currency::get_or_create_crypto(assets.base);

            CryptoFuture::builder()
                .instrument_id(instrument_id)
                .raw_symbol(raw_symbol)
                .underlying(base)
                .quote_currency(quote)
                .settlement_currency(settlement(contract, base, quote))
                .is_inverse(contract == Contract::Inverse)
                .activation_ns(activation)
                .expiration_ns(expiration)
                .price_precision(price_increment.precision)
                .size_precision(size_increment.precision)
                .price_increment(price_increment)
                .size_increment(size_increment)
                .multiplier(
                    multiplier(symbol).map_err(|reason| field("future_contract_unit", &reason))?,
                )
                .ts_event(ts_init)
                .ts_init(ts_init)
                .build()
                .map(InstrumentAny::CryptoFuture)
        }
    }
    .map_err(|e| field("definition", &e))?;

    Ok(CoinApiInstrument {
        instrument,
        symbol_id,
    })
}

/// Converts a current order book into an L2 market-by-price book stamped with `time_exchange`.
///
/// # Errors
///
/// Returns an error if a level is finer than the instrument precision or the timestamp is invalid;
/// a single invalid level rejects the whole snapshot.
pub fn parse_order_book(book: &Book, instrument: &InstrumentAny) -> anyhow::Result<OrderBook> {
    let ts_event = timestamp(book.time_exchange, "time_exchange")?;
    let mut order_book = OrderBook::new(instrument.id(), BookType::L2_MBP);

    for (side, levels) in [(OrderSide::Buy, &book.bids), (OrderSide::Sell, &book.asks)] {
        for level in levels {
            let order = BookOrder::new(
                side,
                price(level.price, instrument, "price")?,
                quantity(level.size, instrument, "size")?,
                0,
            );
            order_book.add(order, 0, 0, ts_event);
        }
    }

    Ok(order_book)
}

/// Converts a historical best bid and ask, taking both `ts_event` and `ts_init` from
/// `time_exchange`.
///
/// # Errors
///
/// Returns an error if a value is finer than the instrument precision or a timestamp is invalid.
pub fn parse_quote_tick(quote: &Quote, instrument: &InstrumentAny) -> anyhow::Result<QuoteTick> {
    let ts_event = timestamp(quote.time_exchange, "time_exchange")?;
    QuoteTick::new_checked(
        instrument.id(),
        price(quote.bid_price, instrument, "bid_price")?,
        price(quote.ask_price, instrument, "ask_price")?,
        quantity(quote.bid_size, instrument, "bid_size")?,
        quantity(quote.ask_size, instrument, "ask_size")?,
        ts_event,
        ts_event,
    )
}

/// Converts a historical trade, taking both `ts_event` and `ts_init` from `time_exchange`.
///
/// Historical records are initialized at their event time so the data engine, which bounds
/// responses by `ts_init`, keeps every record the exchange-time window selected.
///
/// # Errors
///
/// Returns an error if a value is finer than the instrument precision, the size is not positive, or
/// a timestamp or the trade identifier is invalid.
pub fn parse_trade_tick(trade: &Trade, instrument: &InstrumentAny) -> anyhow::Result<TradeTick> {
    let ts_event = timestamp(trade.time_exchange, "time_exchange")?;
    TradeTick::new_checked(
        instrument.id(),
        price(trade.price, instrument, "price")?,
        quantity(trade.size, instrument, "size")?,
        parse_aggressor_side(trade.taker_side),
        TradeId::new_checked(&trade.uuid)?,
        ts_event,
        ts_event,
    )
}

/// Converts a historical OHLCV period into a bar of the requested type, taking both `ts_event`
/// and `ts_init` from `time_period_end`.
///
/// Rounds volume to the instrument's size precision when the adjustment is at most both
/// `1e-15` times the reported volume and `1e-6` times one unit at that precision.
///
/// # Errors
///
/// Returns an error if a price is finer than the instrument precision, volume needs rounding beyond
/// the noise tolerance, the prices are inconsistent, or a timestamp is invalid.
pub fn parse_bar(
    ohlcv: &Ohlcv,
    bar_type: BarType,
    instrument: &InstrumentAny,
) -> anyhow::Result<Bar> {
    let ts_event = timestamp(ohlcv.time_period_end, "time_period_end")?;
    Bar::new_checked(
        bar_type,
        price(ohlcv.price_open, instrument, "price_open")?,
        price(ohlcv.price_high, instrument, "price_high")?,
        price(ohlcv.price_low, instrument, "price_low")?,
        price(ohlcv.price_close, instrument, "price_close")?,
        volume(ohlcv.volume_traded, instrument)?,
        ts_event,
        ts_event,
    )
}

fn settlement(contract: Contract, base: Currency, quote: Currency) -> Currency {
    match contract {
        Contract::Inverse => base,
        Contract::Linear | Contract::Spot => quote,
    }
}

// Binance USD-M lists commodity and equity perpetuals beside crypto perpetuals under the same
// CoinAPI symbol type, and only the forwarded venue contract type distinguishes them.
fn perpetual_asset_class(symbol: &Symbol) -> Result<Option<AssetClass>, (&'static str, String)> {
    if symbol.exchange_id != "BINANCEFTS" {
        return Ok(None);
    }

    let raw_kvp = symbol.raw_kvp.as_ref();
    match raw_kvp.and_then(|raw_kvp| raw_kvp.contract_type).as_deref() {
        Some("PERPETUAL") => Ok(None),
        Some("TRADIFI_PERPETUAL") => {
            match raw_kvp
                .and_then(|raw_kvp| raw_kvp.underlying_type)
                .as_deref()
            {
                Some("EQUITY" | "CN_EQUITY" | "KR_EQUITY" | "HK_EQUITY" | "PREMARKET") => {
                    Ok(Some(AssetClass::Equity))
                }
                Some("COMMODITY") => Ok(Some(AssetClass::Commodity)),
                other => Err(("raw_kvp.underlyingType", format!("unsupported {other:?}"))),
            }
        }
        other => Err(("raw_kvp.contractType", format!("unsupported {other:?}"))),
    }
}

fn multiplier(symbol: &Symbol) -> Result<Quantity, String> {
    decimal(symbol.future_contract_unit)
        .and_then(|value| Quantity::from_decimal(value).map_err(|e| e.to_string()))
}

// CoinAPI pads resolutions with trailing zeros beyond the domain precision limit
fn decimal(value: Option<Decimal>) -> Result<Decimal, String> {
    match value {
        None => Err("missing".to_string()),
        Some(value) if value <= Decimal::ZERO => Err(format!("{value} is not positive")),
        Some(value) => Ok(value.normalize()),
    }
}

pub(crate) fn unix_nanos(timestamp: Timestamp) -> Result<UnixNanos, String> {
    u64::try_from(timestamp.as_nanosecond())
        .map(UnixNanos::from)
        .map_err(|_| format!("{timestamp} precedes the UNIX epoch"))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::http::models::BookLevel;

    fn btc_usd() -> InstrumentAny {
        let symbols: Vec<Symbol> =
            serde_json::from_str(include_str!("../../test_data/symbols.json")).unwrap();
        let (instruments, _) = parse_instruments(&symbols, UnixNanos::default());
        instruments
            .into_iter()
            .find(|loaded| loaded.symbol_id == "COINBASE_SPOT_BTC_USD")
            .unwrap()
            .instrument
    }

    fn book() -> Book {
        serde_json::from_str(include_str!("../../test_data/book.json")).unwrap()
    }

    #[rstest]
    #[case("0.9707556800000001", "0.97075568")]
    #[case("7.087936829999999", "7.08793683")]
    #[case("1.2582318399999999", "1.25823184")]
    fn bar_normalizes_volume_noise(#[case] volume: &str, #[case] expected_volume: &str) {
        let mut records: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/ohlcv.json")).unwrap();
        let ohlcv = &mut records[0];
        ohlcv.price_close = Decimal::from_str_exact("85998.69").unwrap();
        ohlcv.volume_traded = Decimal::from_str_exact(volume).unwrap();
        let bar_type = BarType::from("BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL");

        let bar = parse_bar(ohlcv, bar_type, &btc_usd()).unwrap();

        assert_eq!(
            bar,
            Bar::new(
                bar_type,
                Price::from("85988.49"),
                Price::from("86007.23"),
                Price::from("85986.91"),
                Price::from("85998.69"),
                Quantity::from(expected_volume),
                UnixNanos::from(1_790_154_600_000_000_000),
                UnixNanos::from(1_790_154_600_000_000_000),
            )
        );
        assert_eq!(bar.volume.precision, 8);
    }

    #[rstest]
    fn bar_rejects_finer_price_despite_volume_noise() {
        let records: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/ohlcv.json")).unwrap();
        let bar_type = BarType::from("BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL");

        let error = parse_bar(&records[0], bar_type, &btc_usd()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "price_close 85998.695 is finer than instrument precision 2"
        );
    }

    #[rstest]
    fn order_book_rejects_whole_snapshot_for_one_finer_level() {
        let mut book = book();
        book.asks.push(BookLevel {
            id: None,
            price: Decimal::from_str_exact("83556.605").unwrap(),
            size: Decimal::ONE,
        });

        let error = parse_order_book(&book, &btc_usd()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "price 83556.605 is finer than instrument precision 2"
        );
    }
}
