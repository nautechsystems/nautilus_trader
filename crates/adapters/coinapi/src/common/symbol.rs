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

//! Mapping between CoinAPI exchange symbols and native venue instrument IDs.
//!
//! Each supported CoinAPI exchange and product maps to the symbol convention of the
//! corresponding native adapter. The CoinAPI exchange symbol must match the form built from
//! its exchange asset codes and delivery date; any other form is unresolved rather than guessed.

use jiff::civil::Date;
use nautilus_model::identifiers::{InstrumentId, Symbol, Venue};
use ustr::Ustr;

/// A CoinAPI symbol type with a native instrument mapping.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, strum::AsRefStr, strum::Display, strum::EnumString,
)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinApiSymbolType {
    Spot,
    Perpetual,
    Futures,
}

/// Contract semantics fixed by the native venue product convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contract {
    /// Spot pair without contract terms.
    Spot,
    /// Quote-settled contract.
    Linear,
    /// Base-settled contract.
    Inverse,
}

/// Asset codes that a CoinAPI exchange symbol must agree with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolAssets {
    /// CoinAPI normalized base asset.
    pub base: Ustr,
    /// CoinAPI normalized quote asset.
    pub quote: Ustr,
    /// Venue base asset code.
    pub base_exchange: Ustr,
    /// Venue quote asset code.
    pub quote_exchange: Ustr,
}

/// A native instrument identity resolved from a CoinAPI exchange symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeSymbol {
    pub instrument_id: InstrumentId,
    pub contract: Contract,
}

/// A CoinAPI exchange, symbol type, and exchange symbol.
///
/// This identifies the venue product, not the full CoinAPI `symbol_id`, which can carry a
/// provider suffix and must be retained separately for transport lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VenueSymbol {
    pub exchange_id: Ustr,
    pub symbol_type: CoinApiSymbolType,
    pub symbol: Ustr,
}

/// A failure to map between CoinAPI and native instrument identity.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SymbolError {
    #[error("CoinAPI {exchange_id} {symbol_type} symbols have no native mapping")]
    Unsupported {
        exchange_id: Ustr,
        symbol_type: CoinApiSymbolType,
    },
    #[error(
        "CoinAPI {exchange_id} {symbol_type} symbol {symbol} does not match its native form for the given assets and delivery date"
    )]
    Unresolved {
        exchange_id: Ustr,
        symbol_type: CoinApiSymbolType,
        symbol: Ustr,
    },
    #[error("instrument ID {0} has no CoinAPI mapping")]
    UnsupportedInstrument(InstrumentId),
    #[error("instrument ID {0} matches more than one CoinAPI product")]
    Ambiguous(InstrumentId),
}

const MONTH_CODES: [char; 12] = ['F', 'G', 'H', 'J', 'K', 'M', 'N', 'Q', 'U', 'V', 'X', 'Z'];

impl VenueSymbol {
    /// Resolves the native instrument ID and contract semantics.
    ///
    /// `delivery` is the UTC delivery date and is required for futures.
    ///
    /// # Errors
    ///
    /// Returns an error if the exchange and symbol type have no native mapping, or when the
    /// exchange symbol does not match the native form for `assets` and `delivery`.
    pub fn to_native(
        &self,
        assets: &SymbolAssets,
        delivery: Option<Date>,
    ) -> Result<NativeSymbol, SymbolError> {
        let symbol = self.symbol.as_str();
        let pair = format!("{}{}", assets.base_exchange, assets.quote_exchange);
        let dashed = format!("{}-{}", assets.base_exchange, assets.quote_exchange);
        let yymmdd = delivery.map(|date| date.strftime("%y%m%d").to_string());
        let matches =
            |expected: Option<String>| expected.is_some_and(|expected| expected == symbol);

        let resolved =
            match (self.exchange_id.as_str(), self.symbol_type) {
                ("BINANCE", CoinApiSymbolType::Spot) => {
                    (symbol == pair).then(|| (symbol.to_string(), "BINANCE", Contract::Spot))
                }
                ("BINANCEFTS", CoinApiSymbolType::Perpetual) => (symbol == pair)
                    .then(|| (format!("{symbol}-PERP"), "BINANCE", Contract::Linear)),
                ("BINANCEFTS", CoinApiSymbolType::Futures) => {
                    matches(yymmdd.map(|date| format!("{pair}_{date}")))
                        .then(|| (symbol.to_string(), "BINANCE", Contract::Linear))
                }
                ("BYBITSPOT", CoinApiSymbolType::Spot) => {
                    (symbol == pair).then(|| (format!("{symbol}-SPOT"), "BYBIT", Contract::Spot))
                }
                ("BYBIT", CoinApiSymbolType::Perpetual) => (symbol == pair)
                    .then(|| (format!("{symbol}-LINEAR"), "BYBIT", Contract::Linear)),
                ("BYBIT", CoinApiSymbolType::Futures) => matches(delivery.map(|date| {
                    let date = date.strftime("%d%b%y").to_string().to_ascii_uppercase();
                    format!("{pair}-{date}")
                }))
                .then(|| (format!("{symbol}-LINEAR"), "BYBIT", Contract::Linear)),
                ("BYBITINVERSE", CoinApiSymbolType::Perpetual) => (symbol == pair)
                    .then(|| (format!("{symbol}-INVERSE"), "BYBIT", Contract::Inverse)),
                ("BYBITINVERSE", CoinApiSymbolType::Futures) => matches(delivery.map(|date| {
                    let month = MONTH_CODES[date.month() as usize - 1];
                    format!("{pair}{month}{}", date.strftime("%y"))
                }))
                .then(|| (format!("{symbol}-INVERSE"), "BYBIT", Contract::Inverse)),
                ("OKEX", CoinApiSymbolType::Spot) => {
                    (symbol == dashed).then(|| (symbol.to_string(), "OKX", Contract::Spot))
                }
                ("OKEX", CoinApiSymbolType::Perpetual) => (symbol == format!("{dashed}-SWAP"))
                    .then(|| (symbol.to_string(), "OKX", okx_contract(assets))),
                ("OKEX", CoinApiSymbolType::Futures) => {
                    matches(yymmdd.map(|date| format!("{dashed}-{date}")))
                        .then(|| (symbol.to_string(), "OKX", okx_contract(assets)))
                }
                ("COINBASE", CoinApiSymbolType::Spot) => {
                    (symbol == dashed).then(|| (symbol.to_string(), "COINBASE", Contract::Spot))
                }
                ("KRAKEN", CoinApiSymbolType::Spot) => kraken_spot_native(symbol, assets)
                    .map(|native| (native, "KRAKEN", Contract::Spot)),
                ("KRAKENFTS", CoinApiSymbolType::Perpetual) => {
                    [("PI", Contract::Inverse), ("PF", Contract::Linear)]
                        .into_iter()
                        .find(|(prefix, _)| symbol == format!("{prefix}_{pair}"))
                        .map(|(_, contract)| (symbol.to_string(), "KRAKEN", contract))
                }
                _ => {
                    return Err(SymbolError::Unsupported {
                        exchange_id: self.exchange_id,
                        symbol_type: self.symbol_type,
                    });
                }
            };

        let unresolved = || SymbolError::Unresolved {
            exchange_id: self.exchange_id,
            symbol_type: self.symbol_type,
            symbol: self.symbol,
        };

        let (native, venue, contract) = resolved.ok_or_else(unresolved)?;
        let symbol = Symbol::new_checked(native).map_err(|_| unresolved())?;
        let instrument_id = InstrumentId::new(symbol, Venue::new(venue));

        // A native ID that does not map back to this product would be ambiguous
        if Self::from_native(instrument_id) != Ok(*self) {
            return Err(unresolved());
        }

        Ok(NativeSymbol {
            instrument_id,
            contract,
        })
    }

    /// Resolves the CoinAPI exchange, symbol type, and exchange symbol for a native ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the ID has no supported native form, or when its form is shared by more
    /// than one CoinAPI product.
    pub fn from_native(instrument_id: InstrumentId) -> Result<Self, SymbolError> {
        let symbol = instrument_id.symbol.as_str();
        let resolved = match instrument_id.venue.as_str() {
            "BINANCE" if is_binance_usd_delivery(symbol) => {
                return Err(SymbolError::Ambiguous(instrument_id));
            }
            "BINANCE" => binance_venue_symbol(symbol),
            "BYBIT" => bybit_venue_symbol(symbol),
            "OKX" => okx_venue_symbol(symbol),
            "COINBASE" => coinbase_venue_symbol(symbol),
            "KRAKEN" => kraken_venue_symbol(symbol),
            _ => None,
        };

        resolved.ok_or(SymbolError::UnsupportedInstrument(instrument_id))
    }

    fn new(exchange_id: &str, symbol_type: CoinApiSymbolType, symbol: &str) -> Self {
        Self {
            exchange_id: Ustr::from(exchange_id),
            symbol_type,
            symbol: Ustr::from(symbol),
        }
    }
}

// COIN-M delivery symbols share the USD-M form and carry a USD quote
fn is_binance_usd_delivery(symbol: &str) -> bool {
    symbol
        .split_once('_')
        .is_some_and(|(stem, date)| stem.ends_with("USD") && is_digits(date, 6))
}

fn binance_venue_symbol(symbol: &str) -> Option<VenueSymbol> {
    if let Some(stem) = symbol.strip_suffix("-PERP") {
        return is_alphanumeric(stem)
            .then(|| VenueSymbol::new("BINANCEFTS", CoinApiSymbolType::Perpetual, stem));
    }

    if let Some((stem, date)) = symbol.split_once('_') {
        return (is_alphanumeric(stem) && is_digits(date, 6))
            .then(|| VenueSymbol::new("BINANCEFTS", CoinApiSymbolType::Futures, symbol));
    }

    is_alphanumeric(symbol).then(|| VenueSymbol::new("BINANCE", CoinApiSymbolType::Spot, symbol))
}

fn bybit_venue_symbol(symbol: &str) -> Option<VenueSymbol> {
    let (raw, suffix) = symbol.rsplit_once('-')?;
    let (exchange_id, symbol_type) = match (suffix, raw.split_once('-')) {
        ("SPOT", None) if is_alphanumeric(raw) => ("BYBITSPOT", CoinApiSymbolType::Spot),
        ("LINEAR", None) if is_alphanumeric(raw) => ("BYBIT", CoinApiSymbolType::Perpetual),
        ("LINEAR", Some((stem, date))) if is_alphanumeric(stem) && is_bybit_linear_date(date) => {
            ("BYBIT", CoinApiSymbolType::Futures)
        }
        ("INVERSE", None) if is_bybit_inverse_future(raw) => {
            ("BYBITINVERSE", CoinApiSymbolType::Futures)
        }
        ("INVERSE", None) if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_uppercase()) => {
            ("BYBITINVERSE", CoinApiSymbolType::Perpetual)
        }
        _ => return None,
    };

    Some(VenueSymbol::new(exchange_id, symbol_type, raw))
}

fn okx_venue_symbol(symbol: &str) -> Option<VenueSymbol> {
    let parts: Vec<&str> = symbol.split('-').collect();
    if !parts.iter().all(|part| is_alphanumeric(part)) {
        return None;
    }

    let symbol_type = match parts.as_slice() {
        [_, _] => CoinApiSymbolType::Spot,
        [_, _, "SWAP"] => CoinApiSymbolType::Perpetual,
        [_, _, date] if is_digits(date, 6) => CoinApiSymbolType::Futures,
        _ => return None,
    };

    Some(VenueSymbol::new("OKEX", symbol_type, symbol))
}

fn coinbase_venue_symbol(symbol: &str) -> Option<VenueSymbol> {
    let (base, quote) = symbol.split_once('-')?;
    (is_alphanumeric(base) && is_alphanumeric(quote))
        .then(|| VenueSymbol::new("COINBASE", CoinApiSymbolType::Spot, symbol))
}

fn kraken_venue_symbol(symbol: &str) -> Option<VenueSymbol> {
    if let Some((base, quote)) = symbol.split_once('/') {
        let base = kraken_spot_venue_code(base)?;
        let quote = kraken_spot_venue_code(quote)?;
        return Some(VenueSymbol::new(
            "KRAKEN",
            CoinApiSymbolType::Spot,
            &format!("{base}/{quote}"),
        ));
    }

    let (prefix, rest) = symbol.split_at_checked(3)?;
    (matches!(prefix, "PI_" | "PF_") && is_alphanumeric(rest))
        .then(|| VenueSymbol::new("KRAKENFTS", CoinApiSymbolType::Perpetual, symbol))
}

fn okx_contract(assets: &SymbolAssets) -> Contract {
    if assets.quote_exchange == "USD" {
        Contract::Inverse
    } else {
        Contract::Linear
    }
}

// Native Kraken spot IDs rename XBT and XDG, so venue symbols already using BTC or DOGE
// would share their native ID and cannot round-trip.
fn kraken_spot_native(symbol: &str, assets: &SymbolAssets) -> Option<String> {
    let (base, quote) = symbol.split_once('/')?;
    let base = kraken_spot_native_code(base)?;
    let quote = kraken_spot_native_code(quote)?;
    (base == assets.base && quote == assets.quote).then(|| format!("{base}/{quote}"))
}

fn kraken_spot_native_code(code: &str) -> Option<&str> {
    match code {
        "XBT" => Some("BTC"),
        "XDG" => Some("DOGE"),
        "BTC" | "DOGE" => None,
        code if is_alphanumeric(code) => Some(code),
        _ => None,
    }
}

fn kraken_spot_venue_code(code: &str) -> Option<&str> {
    match code {
        "BTC" => Some("XBT"),
        "DOGE" => Some("XDG"),
        "XBT" | "XDG" => None,
        code if is_alphanumeric(code) => Some(code),
        _ => None,
    }
}

fn is_alphanumeric(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn is_digits(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_digit())
}

fn is_bybit_linear_date(value: &str) -> bool {
    Date::strptime("%d%b%y", value)
        .is_ok_and(|date| date.strftime("%d%b%y").to_string().to_ascii_uppercase() == value)
}

fn is_bybit_inverse_future(raw: &str) -> bool {
    let Some((stem, code)) = raw.split_at_checked(raw.len().saturating_sub(3)) else {
        return false;
    };

    let mut chars = code.chars();

    !stem.is_empty()
        && stem.bytes().all(|b| b.is_ascii_uppercase())
        && chars
            .next()
            .is_some_and(|month| MONTH_CODES.contains(&month))
        && is_digits(chars.as_str(), 2)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn assets(base: &str, quote: &str, base_exchange: &str, quote_exchange: &str) -> SymbolAssets {
        SymbolAssets {
            base: Ustr::from(base),
            quote: Ustr::from(quote),
            base_exchange: Ustr::from(base_exchange),
            quote_exchange: Ustr::from(quote_exchange),
        }
    }

    fn venue_symbol(
        exchange_id: &str,
        symbol_type: CoinApiSymbolType,
        symbol: &str,
    ) -> VenueSymbol {
        VenueSymbol {
            exchange_id: Ustr::from(exchange_id),
            symbol_type,
            symbol: Ustr::from(symbol),
        }
    }

    #[rstest]
    #[case("BINANCE", CoinApiSymbolType::Spot, "BTCUSDT", ("BTC", "USDT", "BTC", "USDT"), None, "BTCUSDT.BINANCE", Contract::Spot)]
    #[case("BINANCEFTS", CoinApiSymbolType::Perpetual, "ETHUSDC", ("ETH", "USDC", "ETH", "USDC"), None, "ETHUSDC-PERP.BINANCE", Contract::Linear)]
    #[case("BINANCEFTS", CoinApiSymbolType::Futures, "BTCUSDT_261225", ("BTC", "USDT", "BTC", "USDT"), Some("2026-12-25"), "BTCUSDT_261225.BINANCE", Contract::Linear)]
    #[case("BYBITSPOT", CoinApiSymbolType::Spot, "SOLUSDC", ("SOL", "USDC", "SOL", "USDC"), None, "SOLUSDC-SPOT.BYBIT", Contract::Spot)]
    #[case("BYBIT", CoinApiSymbolType::Perpetual, "XRPUSDT", ("XRP", "USDT", "XRP", "USDT"), None, "XRPUSDT-LINEAR.BYBIT", Contract::Linear)]
    #[case("BYBIT", CoinApiSymbolType::Futures, "BTCUSDT-09OCT26", ("BTC", "USDT", "BTC", "USDT"), Some("2026-10-09"), "BTCUSDT-09OCT26-LINEAR.BYBIT", Contract::Linear)]
    #[case("BYBITINVERSE", CoinApiSymbolType::Perpetual, "ETHUSD", ("ETH", "USD", "ETH", "USD"), None, "ETHUSD-INVERSE.BYBIT", Contract::Inverse)]
    #[case("BYBITINVERSE", CoinApiSymbolType::Futures, "BTCUSDH27", ("BTC", "USD", "BTC", "USD"), Some("2027-03-26"), "BTCUSDH27-INVERSE.BYBIT", Contract::Inverse)]
    #[case("OKEX", CoinApiSymbolType::Spot, "ETH-USDC", ("ETH", "USDC", "ETH", "USDC"), None, "ETH-USDC.OKX", Contract::Spot)]
    #[case("OKEX", CoinApiSymbolType::Perpetual, "BTC-USDT-SWAP", ("BTC", "USDT", "BTC", "USDT"), None, "BTC-USDT-SWAP.OKX", Contract::Linear)]
    #[case("OKEX", CoinApiSymbolType::Perpetual, "ETH-USD-SWAP", ("ETH", "USD", "ETH", "USD"), None, "ETH-USD-SWAP.OKX", Contract::Inverse)]
    #[case("OKEX", CoinApiSymbolType::Futures, "BTC-USD-270326", ("BTC", "USD", "BTC", "USD"), Some("2027-03-26"), "BTC-USD-270326.OKX", Contract::Inverse)]
    #[case("OKEX", CoinApiSymbolType::Futures, "SOL-USDT-261225", ("SOL", "USDT", "SOL", "USDT"), Some("2026-12-25"), "SOL-USDT-261225.OKX", Contract::Linear)]
    #[case("COINBASE", CoinApiSymbolType::Spot, "ETH-USD", ("ETH", "USD", "ETH", "USD"), None, "ETH-USD.COINBASE", Contract::Spot)]
    #[case("KRAKEN", CoinApiSymbolType::Spot, "XBT/USDC", ("BTC", "USDC", "XXBT", "USDC"), None, "BTC/USDC.KRAKEN", Contract::Spot)]
    #[case("KRAKEN", CoinApiSymbolType::Spot, "XDG/EUR", ("DOGE", "EUR", "XXDG", "ZEUR"), None, "DOGE/EUR.KRAKEN", Contract::Spot)]
    #[case("KRAKEN", CoinApiSymbolType::Spot, "ETH/USD", ("ETH", "USD", "XETH", "ZUSD"), None, "ETH/USD.KRAKEN", Contract::Spot)]
    #[case("KRAKENFTS", CoinApiSymbolType::Perpetual, "PI_XBTUSD", ("BTC", "USD", "XBT", "USD"), None, "PI_XBTUSD.KRAKEN", Contract::Inverse)]
    #[case("KRAKENFTS", CoinApiSymbolType::Perpetual, "PF_ETHUSD", ("ETH", "USD", "ETH", "USD"), None, "PF_ETHUSD.KRAKEN", Contract::Linear)]
    fn test_native_round_trip(
        #[case] exchange_id: &str,
        #[case] symbol_type: CoinApiSymbolType,
        #[case] symbol: &str,
        #[case] codes: (&str, &str, &str, &str),
        #[case] delivery: Option<&str>,
        #[case] expected_id: &str,
        #[case] expected_contract: Contract,
    ) {
        let venue_symbol = venue_symbol(exchange_id, symbol_type, symbol);
        let assets = assets(codes.0, codes.1, codes.2, codes.3);
        let delivery = delivery.map(|date| date.parse().unwrap());

        let native = venue_symbol.to_native(&assets, delivery).unwrap();
        let reverse = VenueSymbol::from_native(native.instrument_id).unwrap();

        assert_eq!(
            native,
            NativeSymbol {
                instrument_id: InstrumentId::from(expected_id),
                contract: expected_contract,
            }
        );
        assert_eq!(reverse, venue_symbol);
    }

    #[rstest]
    #[case::binance_pair_mismatch("BINANCE", CoinApiSymbolType::Spot, "BTCUSDC", ("BTC", "USDT", "BTC", "USDT"), None)]
    #[case::binance_future_date("BINANCEFTS", CoinApiSymbolType::Futures, "BTCUSDT_261225", ("BTC", "USDT", "BTC", "USDT"), Some("2026-09-25"))]
    #[case::binance_future_missing_date("BINANCEFTS", CoinApiSymbolType::Futures, "BTCUSDT_261225", ("BTC", "USDT", "BTC", "USDT"), None)]
    #[case::binance_usd_delivery("BINANCEFTS", CoinApiSymbolType::Futures, "BTCUSD_261225", ("BTC", "USD", "BTC", "USD"), Some("2026-12-25"))]
    #[case::binance_perp_as_future("BINANCEFTS", CoinApiSymbolType::Futures, "BTCUSDT", ("BTC", "USDT", "BTC", "USDT"), Some("2026-12-25"))]
    #[case::bybit_usdc_perp("BYBIT", CoinApiSymbolType::Perpetual, "BTCPERP", ("BTC", "USDC", "BTC", "USDC"), None)]
    #[case::bybit_future_date("BYBIT", CoinApiSymbolType::Futures, "BTCUSDT-30OCT26", ("BTC", "USDT", "BTC", "USDT"), Some("2026-10-09"))]
    #[case::bybit_inverse_month("BYBITINVERSE", CoinApiSymbolType::Futures, "BTCUSDZ26", ("BTC", "USD", "BTC", "USD"), Some("2026-09-25"))]
    #[case::okx_usd_margined("OKEX", CoinApiSymbolType::Futures, "BTC-USD_UM-261225", ("BTC", "USD", "BTC", "USD"), Some("2026-12-25"))]
    #[case::okx_pseudo_perp("OKEX", CoinApiSymbolType::Futures, "BTC-USD_UM_XPERP-310404", ("BTC", "USD", "BTC", "USD"), Some("2031-04-04"))]
    #[case::coinbase_quote_mismatch("COINBASE", CoinApiSymbolType::Spot, "BTC-USD", ("BTC", "USDT", "BTC", "USDT"), None)]
    #[case::kraken_literal_btc("KRAKEN", CoinApiSymbolType::Spot, "BTC/USD", ("BTC", "USD", "XXBT", "ZUSD"), None)]
    #[case::kraken_asset_mismatch("KRAKEN", CoinApiSymbolType::Spot, "XBT/USD", ("BTC", "USDT", "XXBT", "USDT"), None)]
    #[case::kraken_futures_prefix("KRAKENFTS", CoinApiSymbolType::Perpetual, "PV_XBTUSD", ("BTC", "USD", "XBT", "USD"), None)]
    #[case::kraken_futures_case("KRAKENFTS", CoinApiSymbolType::Perpetual, "pi_xbtusd", ("BTC", "USD", "XBT", "USD"), None)]
    fn test_unresolved_forms_are_rejected(
        #[case] exchange_id: &str,
        #[case] symbol_type: CoinApiSymbolType,
        #[case] symbol: &str,
        #[case] codes: (&str, &str, &str, &str),
        #[case] delivery: Option<&str>,
    ) {
        let venue_symbol = venue_symbol(exchange_id, symbol_type, symbol);
        let assets = assets(codes.0, codes.1, codes.2, codes.3);
        let delivery = delivery.map(|date| date.parse().unwrap());

        let result = venue_symbol.to_native(&assets, delivery);

        assert_eq!(
            result,
            Err(SymbolError::Unresolved {
                exchange_id: Ustr::from(exchange_id),
                symbol_type,
                symbol: Ustr::from(symbol),
            })
        );
    }

    #[rstest]
    #[case("BINANCEFTSC", CoinApiSymbolType::Perpetual, "BTCUSD_PERP")]
    #[case("BINANCEFTSC", CoinApiSymbolType::Futures, "BTCUSD_261225")]
    #[case("COINBASE", CoinApiSymbolType::Perpetual, "BTC-PERP")]
    #[case("KRAKENFTS", CoinApiSymbolType::Spot, "XBT/USD")]
    #[case("KRAKENFTS", CoinApiSymbolType::Futures, "FF_XBTUSD_260925")]
    #[case("BITSTAMP", CoinApiSymbolType::Spot, "btcusd")]
    fn test_unsupported_products_are_rejected(
        #[case] exchange_id: &str,
        #[case] symbol_type: CoinApiSymbolType,
        #[case] symbol: &str,
    ) {
        let venue_symbol = venue_symbol(exchange_id, symbol_type, symbol);
        let assets = assets("BTC", "USD", "BTC", "USD");

        let result = venue_symbol.to_native(&assets, None);

        assert_eq!(
            result,
            Err(SymbolError::Unsupported {
                exchange_id: Ustr::from(exchange_id),
                symbol_type,
            })
        );
    }

    #[rstest]
    #[case("BTCUSD_PERP.BINANCE")]
    #[case("BTCUSDT_2612.BINANCE")]
    #[case("BTC-USDT.BINANCE")]
    #[case("BTCUSDT.BYBIT")]
    #[case("BTCUSDT-OPTION.BYBIT")]
    #[case("BTC-30OCT2026-LINEAR.BYBIT")]
    #[case("BTCUSDT-30oct26-LINEAR.BYBIT")]
    #[case("BTCUSDZ6-INVERSE.BYBIT")]
    #[case("BTC€D-INVERSE.BYBIT")]
    #[case("BTC-USD-261225-90000-C.OKX")]
    #[case("BTC-USD_UM-261225.OKX")]
    #[case("BIP-20DEC30-CDE.COINBASE")]
    #[case("XBT/USD.KRAKEN")]
    #[case("PV_XBTUSD.KRAKEN")]
    #[case("FF_XBTUSD.KRAKEN")]
    #[case("FF_XBTUSD_260925.KRAKEN")]
    #[case("FI_XBTUSD_261225.KRAKEN")]
    #[case("BTCUSDT.BITSTAMP")]
    fn test_unsupported_instrument_ids_are_rejected(#[case] instrument_id: &str) {
        let instrument_id = InstrumentId::from(instrument_id);

        let result = VenueSymbol::from_native(instrument_id);

        assert_eq!(
            result,
            Err(SymbolError::UnsupportedInstrument(instrument_id))
        );
    }

    #[rstest]
    fn test_binance_usd_delivery_is_ambiguous() {
        let instrument_id = InstrumentId::from("BTCUSD_261225.BINANCE");

        let result = VenueSymbol::from_native(instrument_id);

        assert_eq!(result, Err(SymbolError::Ambiguous(instrument_id)));
    }
}
