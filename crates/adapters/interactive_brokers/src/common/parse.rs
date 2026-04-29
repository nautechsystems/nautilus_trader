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

//! Parsing utilities for converting Interactive Brokers data to Nautilus types.

use std::{collections::HashMap, sync::LazyLock};

use ibapi::contracts::{Contract, Currency, Exchange, SecurityType, Symbol};
use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{InstrumentId, Symbol as NautilusSymbol, TradeId, Venue};

/// Generate a unique trade ID for Interactive Brokers trades.
///
/// This format matches the Python adapter: "{secs}-{price}-{size}"
///
/// # Arguments
///
/// * `ts_event` - Event timestamp in nanoseconds
/// * `price` - Trade price
/// * `size` - Trade size
pub fn generate_ib_trade_id(ts_event: UnixNanos, price: f64, size: f64) -> TradeId {
    let ts_secs = ts_event.as_i64() / 1_000_000_000;
    TradeId::new(format!("{ts_secs}-{price}-{size}"))
}

/// Convert an IB Contract to an InstrumentId using simplified symbology.
///
/// This implements IB_SIMPLIFIED symbology: clean, readable symbols.
/// For example:
/// - STK: "AAPL" -> "AAPL.SMART"
/// - CASH: "EUR.USD" -> "EUR/USD.IDEALPRO"
/// - FUT: "ESM23" -> "ESM23.GLOBEX"
/// - OPT: "AAPL230120C00150000" -> "AAPL230120C00150000.SMART"
/// - IND: "SPX" -> "^SPX.SMART"
///
/// # Arguments
///
/// * `contract` - The IB contract to convert
/// * `venue` - Optional venue override (defaults based on security type)
///
/// # Errors
///
/// Returns an error if the instrument ID cannot be constructed.
pub fn ib_contract_to_instrument_id_simplified(
    contract: &Contract,
    venue: Option<Venue>,
) -> anyhow::Result<InstrumentId> {
    let venue = venue.unwrap_or_else(|| {
        // For Index and Future, use contract exchange when set (e.g. ESTX50 -> EUREX, FESX -> EUREX).
        match contract.security_type {
            SecurityType::Index => {
                if !contract.exchange.as_str().is_empty() && contract.exchange.as_str() != "SMART" {
                    Venue::from(contract.exchange.as_str())
                } else {
                    Venue::from("SMART")
                }
            }
            SecurityType::Future => {
                if !contract.exchange.as_str().is_empty() && contract.exchange.as_str() != "SMART" {
                    Venue::from(contract.exchange.as_str())
                } else {
                    Venue::from("GLOBEX")
                }
            }
            SecurityType::ForexPair => Venue::from("IDEALPRO"),
            SecurityType::Crypto => Venue::from("PAXOS"),
            SecurityType::Stock => Venue::from("SMART"),
            SecurityType::Option | SecurityType::FuturesOption => {
                if !contract.exchange.as_str().is_empty() && contract.exchange.as_str() != "SMART" {
                    Venue::from(contract.exchange.as_str())
                } else {
                    Venue::from("SMART")
                }
            }
            SecurityType::CFD => Venue::from("SMART"),
            SecurityType::Commodity => Venue::from("SMART"),
            SecurityType::Bond => Venue::from("SMART"),
            _ => Venue::from("SMART"),
        }
    });

    let symbol = match contract.security_type {
        SecurityType::Stock => {
            // STK: Use localSymbol with spaces replaced by hyphens, fallback to symbol
            let symbol_str = if contract.local_symbol.is_empty() {
                contract.symbol.as_str().to_string()
            } else {
                contract.local_symbol.as_str().replace(' ', "-")
            };
            NautilusSymbol::from(symbol_str.as_str())
        }
        SecurityType::Index => {
            // IND: Prefix with ^
            let base = if contract.local_symbol.is_empty() {
                contract.symbol.as_str()
            } else {
                contract.local_symbol.as_str()
            };
            NautilusSymbol::from(format!("^{base}").as_str())
        }
        SecurityType::Option => {
            // OPT: Preserve OCC 6-character root padding when present.
            let symbol_str = if contract.local_symbol.is_empty() {
                format!(
                    "{} {} {} {}",
                    contract.right.as_str(),
                    contract.trading_class.as_str(),
                    contract.last_trade_date_or_contract_month.as_str(),
                    format_option_strike(contract.strike),
                )
            } else {
                normalize_option_symbol(contract.local_symbol.as_str())
            };
            NautilusSymbol::from(symbol_str.as_str())
        }
        SecurityType::ForexPair | SecurityType::Crypto => {
            // CASH/CRYPTO: Replace dots with slashes (e.g., "EUR.USD" -> "EUR/USD")
            let symbol_str = if contract.local_symbol.is_empty() {
                format!(
                    "{}/{}",
                    contract.symbol.as_str(),
                    contract.currency.as_str()
                )
            } else {
                contract.local_symbol.as_str().replace('.', "/")
            };
            NautilusSymbol::from(symbol_str.as_str())
        }
        SecurityType::Future => {
            // FUT: Use localSymbol if available; else symbol + trading_class + expiry (e.g. ESTX50 FESX 20240315).
            if contract.local_symbol.is_empty() {
                if !contract.trading_class.is_empty()
                    && !contract.last_trade_date_or_contract_month.is_empty()
                {
                    let symbol_str = format!(
                        "{} {} {}",
                        contract.symbol.as_str(),
                        contract.trading_class.as_str(),
                        contract.last_trade_date_or_contract_month.as_str()
                    );
                    NautilusSymbol::from(symbol_str.as_str())
                } else if !contract.last_trade_date_or_contract_month.is_empty() {
                    let expiry = contract.last_trade_date_or_contract_month.as_str();
                    let symbol_str = format!("{}{}", contract.symbol.as_str(), expiry);
                    NautilusSymbol::from(symbol_str.as_str())
                } else {
                    NautilusSymbol::from(contract.symbol.as_str())
                }
            } else {
                NautilusSymbol::from(contract.local_symbol.as_str())
            }
        }
        SecurityType::FuturesOption => {
            // FOP: Format like "ESM23 C4500" -> "ESM23C4500"
            if contract.local_symbol.is_empty() {
                // Fallback construction
                let expiry = contract.last_trade_date_or_contract_month.as_str();
                let right = if contract.right == "C" { "C" } else { "P" };
                let strike_str = format!("{}", contract.strike as i64);
                let symbol_str = format!(
                    "{}{} {}{}",
                    contract.symbol.as_str(),
                    expiry,
                    right,
                    strike_str
                );
                NautilusSymbol::from(symbol_str.as_str())
            } else {
                let cleaned = contract.local_symbol.as_str().replace(' ', "");
                NautilusSymbol::from(cleaned.as_str())
            }
        }
        SecurityType::CFD => {
            // CFD: If localSymbol matches EUR.USD pattern, convert to EUR/USD, else use symbol with spaces as hyphens
            if !contract.local_symbol.is_empty() && contract.local_symbol.contains('.') {
                let cash_like = contract.local_symbol.as_str().replace('.', "/");
                NautilusSymbol::from(cash_like.as_str())
            } else {
                let symbol_str = contract.symbol.as_str().replace(' ', "-");
                NautilusSymbol::from(symbol_str.as_str())
            }
        }
        SecurityType::Commodity => {
            // CMDTY: Replace spaces with hyphens
            let symbol_str = contract.symbol.as_str().replace(' ', "-");
            NautilusSymbol::from(symbol_str.as_str())
        }
        SecurityType::Bond => {
            // BOND: Use localSymbol or symbol
            let symbol_str = if contract.local_symbol.is_empty() {
                contract.symbol.as_str()
            } else {
                contract.local_symbol.as_str()
            };
            NautilusSymbol::from(symbol_str)
        }
        _ => {
            // Default: use localSymbol or symbol
            let symbol_str = if contract.local_symbol.is_empty() {
                contract.symbol.as_str()
            } else {
                contract.local_symbol.as_str()
            };
            NautilusSymbol::from(symbol_str)
        }
    };

    Ok(InstrumentId::new(symbol, venue))
}

/// Convert an IB Contract to an InstrumentId using raw symbology.
///
/// This implements IB_RAW symbology: preserves IB raw format with security type suffix.
/// For example:
/// - "AAPL=STK.SMART"
/// - "EUR.USD=CASH.IDEALPRO"
/// - "ESM23=FUT.GLOBEX"
///
/// # Arguments
///
/// * `contract` - The IB contract to convert
/// * `venue` - Optional venue override (defaults based on security type)
///
/// # Errors
///
/// Returns an error if the instrument ID cannot be constructed.
pub fn ib_contract_to_instrument_id_raw(
    contract: &Contract,
    venue: Option<Venue>,
) -> anyhow::Result<InstrumentId> {
    let venue = venue.unwrap_or_else(|| match contract.security_type {
        SecurityType::ForexPair => Venue::from("IDEALPRO"),
        SecurityType::Crypto => Venue::from("PAXOS"),
        SecurityType::Stock => Venue::from("SMART"),
        SecurityType::Option => Venue::from("SMART"),
        SecurityType::FuturesOption => Venue::from("SMART"),
        SecurityType::Future => Venue::from("GLOBEX"),
        SecurityType::Index => Venue::from("SMART"),
        SecurityType::CFD => Venue::from("SMART"),
        SecurityType::Commodity => Venue::from("SMART"),
        SecurityType::Bond => Venue::from("SMART"),
        _ => Venue::from("SMART"),
    });

    let local_symbol = if contract.local_symbol.is_empty() {
        contract.symbol.as_str()
    } else {
        contract.local_symbol.as_str()
    };

    let sec_type_str = match contract.security_type {
        SecurityType::Stock => "STK",
        SecurityType::Option => "OPT",
        SecurityType::Future => "FUT",
        SecurityType::FuturesOption => "FOP",
        SecurityType::ForexPair => "CASH",
        SecurityType::Crypto => "CRYPTO",
        SecurityType::Index => "IND",
        SecurityType::CFD => "CFD",
        SecurityType::Commodity => "CMDTY",
        SecurityType::Bond => "BOND",
        _ => "OTHER",
    };

    let symbol_str = format!("{local_symbol}={sec_type_str}");
    let symbol = NautilusSymbol::from(symbol_str.as_str());
    Ok(InstrumentId::new(symbol, venue))
}

/// Convert an IB Contract to an InstrumentId (simple version using contract fields).
///
/// This is a convenience wrapper that uses simplified symbology by default.
/// For more accurate mapping, use the instrument provider which has contract details.
///
/// # Errors
///
/// Returns an error if the instrument ID cannot be constructed.
pub fn ib_contract_to_instrument_id_simple(contract: &Contract) -> anyhow::Result<InstrumentId> {
    ib_contract_to_instrument_id_simplified(contract, None)
}

/// Venue to IB exchange mappings.
/// Maps MIC venue codes to lists of IB exchange codes used by Interactive Brokers.
pub static VENUE_MEMBERS: LazyLock<HashMap<&'static str, Vec<&'static str>>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();
        // ICE Endex
        map.insert("NDEX", vec!["ENDEX"]);
        // CME Group Exchanges
        map.insert("XCME", vec!["CME"]);
        map.insert("XCEC", vec!["CME"]);
        map.insert("XFXS", vec!["CME"]);
        // Chicago Board of Trade Segments
        map.insert("XCBT", vec!["CBOT"]);
        map.insert("CBCM", vec!["CBOT"]);
        // New York Mercantile Exchange Segments
        map.insert("XNYM", vec!["NYMEX"]);
        map.insert("NYUM", vec!["NYMEX"]);
        // ICE Futures US (formerly NYBOT)
        map.insert("IFUS", vec!["NYBOT"]);
        // GLBX, Name used by databento
        map.insert("GLBX", vec!["CBOT", "CME", "NYBOT", "NYMEX"]);
        // US Major Exchanges & Index Venues
        map.insert("XNAS", vec!["NASDAQ"]);
        map.insert("XNYS", vec!["NYSE"]);
        map.insert("ARCX", vec!["ARCA"]);
        map.insert("BATS", vec!["BATS"]);
        map.insert("IEXG", vec!["IEX"]);
        map.insert("XCBO", vec!["CBOE"]);
        map.insert("XCBF", vec!["CFE"]);
        // Canadian Exchanges
        map.insert("XTSE", vec!["TSX"]);
        // ICE Europe Exchanges
        map.insert("IFEU", vec!["ICEEU", "ICEEUSOFT", "IPE"]);
        // European Exchanges
        map.insert("XLON", vec!["LSE"]);
        map.insert("XPAR", vec!["SBF"]);
        map.insert("XETR", vec!["IBIS"]);
        map.insert("XEUR", vec!["DTB", "EUREX", "SOFFEX"]);
        map.insert("XAMS", vec!["AEB"]);
        map.insert("XBRU", vec!["EBS"]);
        map.insert("XBRD", vec!["BELFOX"]);
        map.insert("XLIS", vec!["BVLP"]);
        map.insert("XDUB", vec!["IRE"]);
        map.insert("XOSL", vec!["OSL"]);
        map.insert("XSWX", vec!["EBS", "SIX", "SWX"]);
        map.insert("XSVX", vec!["VRTX"]);
        map.insert("XMIL", vec!["BIT", "BVME", "IDEM"]);
        map.insert("XMAD", vec!["MDRD", "BME"]);
        map.insert("DXEX", vec!["BATEEN"]);
        map.insert("XWBO", vec!["WBAG"]);
        map.insert("XBUD", vec!["BUX"]);
        map.insert("XPRA", vec!["PRA"]);
        map.insert("XWAR", vec!["WSE"]);
        map.insert("XIST", vec!["ISE"]);
        // Nasdaq Nordic Exchanges
        map.insert("XSTO", vec!["SFB"]);
        map.insert("XCSE", vec!["KFB"]);
        map.insert("XHEL", vec!["HMB"]);
        map.insert("XICE", vec!["ISB"]);
        // Asia-Pacific Exchanges
        map.insert("XASX", vec!["ASX"]);
        map.insert("XHKG", vec!["SEHK"]);
        map.insert("XHKF", vec!["HKFE"]);
        map.insert("XSES", vec!["SGX"]);
        map.insert("XOSE", vec!["OSE.JPN"]);
        map.insert("XTKS", vec!["TSEJ", "TSE.JPN"]);
        map.insert("XKRX", vec!["KSE", "KRX"]);
        map.insert("XTAI", vec!["TASE", "TWSE"]);
        map.insert("XSHG", vec!["SEHKNTL", "SSE"]);
        map.insert("XSHE", vec!["SEHKSZSE"]);
        map.insert("XNSE", vec!["NSE"]);
        map.insert("XBOM", vec!["BSE"]);
        // Other Derivatives Exchanges
        map.insert("XSFE", vec!["SNFE"]);
        map.insert("XMEX", vec!["MEXDER"]);
        // African, Middle Eastern, South American Exchanges
        map.insert("XJSE", vec!["JSE"]);
        map.insert("XBOG", vec!["BVC"]);
        map.insert("XTAE", vec!["TASE"]);
        map.insert("BVMF", vec!["BVMF"]);
        map
    });

#[must_use]
pub fn possible_exchanges_for_venue(venue: &str) -> Vec<String> {
    if let Some(exchanges) = VENUE_MEMBERS.get(venue) {
        return exchanges
            .iter()
            .map(|exchange| (*exchange).to_string())
            .collect();
    }

    vec![venue.to_string()]
}

/// Venue lists for different asset classes
const VENUES_CASH: &[&str] = &["IDEALPRO"];
const VENUES_CRYPTO: &[&str] = &["PAXOS"];
const VENUES_OPT: &[&str] = &["SMART", "EUREX"];
const VENUES_FUT: &[&str] = &[
    "GLOBEX",
    "NYMEX",
    "NYBOT",
    "CBOT",
    "CME",
    "CFE",
    "ICE",
    "ECBOT",
    "CBOE",
    "CMECRYPTO",
    "NYMEXMETALS",
    "NYMEXNG",
    "NYMEXENERGY",
    "CMEPRECIOUS",
    "CMECURRENCY",
    "CMEINDEX",
    "CMEWEATHER",
    "CMEINTEREST",
    "CMEFLOOR",
    "CBOTFLOOR",
    "NYMEXFLOOR",
    "NYBOTFLOOR",
    "CFEFLOOR",
    "CMEOPTIONS",
    "CBOTOPTIONS",
    "NYMEXOPTIONS",
    "NYBOTOPTIONS",
];
const VENUES_CFD: &[&str] = &["SMART"];
const VENUES_CMDTY: &[&str] = &["IBCMDTY"];

fn venue_matches(venue_str: &str, venues: &[&str]) -> bool {
    venues.contains(&venue_str)
        || VENUE_MEMBERS
            .get(venue_str)
            .is_some_and(|exchanges| exchanges.iter().any(|exchange| venues.contains(exchange)))
}

/// Futures month codes mapping (F=Jan, G=Feb, H=Mar, J=Apr, K=May, M=Jun, N=Jul, Q=Aug, U=Sep, V=Oct, X=Nov, Z=Dec)
/// This constant is kept for potential future use in more complex parsing scenarios.
#[allow(dead_code)]
const FUTURES_MONTH_CODES: &[(char, &str)] = &[
    ('F', "01"),
    ('G', "02"),
    ('H', "03"),
    ('J', "04"),
    ('K', "05"),
    ('M', "06"),
    ('N', "07"),
    ('Q', "08"),
    ('U', "09"),
    ('V', "10"),
    ('X', "11"),
    ('Z', "12"),
];

/// Determine venue from contract using provider configuration.
///
/// This implements the same logic as Python's `determine_venue_from_contract`:
/// 1. Check symbol-specific venue mapping first (prefix matching)
/// 2. Use VENUE_MEMBERS mapping if convert_exchange_to_mic_venue is enabled
/// 3. Fall back to exchange
///
/// # Arguments
///
/// * `contract` - The IB contract
/// * `symbol_to_mic_venue` - Symbol prefix to venue mapping
/// * `convert_exchange_to_mic_venue` - Whether to convert exchange to MIC venue
///
/// # Returns
///
/// The determined venue as a string.
pub fn determine_venue_from_contract(
    contract: &Contract,
    symbol_to_mic_venue: &std::collections::HashMap<String, String>,
    convert_exchange_to_mic_venue: bool,
    valid_exchanges: Option<&str>,
) -> String {
    if matches!(contract.security_type, SecurityType::CFD) {
        return "IBCFD".to_string();
    }

    if matches!(contract.security_type, SecurityType::Commodity) {
        return "IBCMDTY".to_string();
    }

    if !symbol_to_mic_venue.is_empty() {
        let symbol = contract.symbol.as_str();
        for (symbol_prefix, symbol_venue) in symbol_to_mic_venue {
            if symbol.starts_with(symbol_prefix) {
                return symbol_venue.clone();
            }
        }
    }

    // Use the exchange from the contract (primaryExchange if exchange is SMART)
    let mut exchange = if contract.exchange.as_str() == "SMART"
        && !contract.primary_exchange.as_str().is_empty()
        && contract.primary_exchange.as_str() != "SMART"
    {
        contract.primary_exchange.as_str().to_string()
    } else {
        contract.exchange.as_str().to_string()
    };

    if exchange == "SMART"
        && let Some(valid_exchanges) = valid_exchanges
    {
        let parts: Vec<&str> = valid_exchanges
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();

        if let Some(chosen) = parts.iter().find(|part| **part != "SMART") {
            exchange = (*chosen).to_string();
        } else if let Some(first) = parts.first() {
            exchange = (*first).to_string();
        }
    }

    if convert_exchange_to_mic_venue {
        for (venue_member, exchanges) in VENUE_MEMBERS.iter() {
            if exchanges.iter().any(|candidate| *candidate == exchange) {
                return (*venue_member).to_string();
            }
        }
    }

    exchange
}

/// Convert a NautilusTrader `InstrumentId` to an Interactive Brokers `Contract`.
///
/// This function handles all instrument types:
/// - Stocks (STK)
/// - Options (OPT)
/// - Futures (FUT, CONTFUT)
/// - Futures Options (FOP)
/// - Forex (CASH)
/// - Crypto (CRYPTO)
/// - CFDs (CFD)
/// - Commodities (CMDTY)
/// - Indices (IND)
/// - Option Spreads (BAG) - requires contract details map
///
/// # Arguments
///
/// * `instrument_id` - The NautilusTrader instrument identifier
/// * `exchange` - An optional exchange string. If `None`, defaults to "SMART"
///
/// # Errors
///
/// Returns an error if the conversion fails (e.g., unsupported instrument type, invalid format).
pub fn instrument_id_to_ib_contract(
    instrument_id: InstrumentId,
    exchange: Option<&str>,
) -> anyhow::Result<Contract> {
    let venue_str = instrument_id.venue.to_string();
    let derived_exchange = VENUE_MEMBERS
        .get(venue_str.as_str())
        .and_then(|exchanges| exchanges.first().copied())
        .or_else(|| {
            if venue_matches(venue_str.as_str(), VENUES_CASH)
                || venue_matches(venue_str.as_str(), VENUES_CRYPTO)
                || venue_matches(venue_str.as_str(), VENUES_OPT)
                || venue_matches(venue_str.as_str(), VENUES_FUT)
            {
                Some(venue_str.as_str())
            } else {
                None
            }
        })
        .unwrap_or("SMART");
    let exchange_str = exchange.unwrap_or(derived_exchange);
    let symbol_str = instrument_id.symbol.as_str();

    if let Some(contract) = instrument_id_to_ib_contract_raw(&instrument_id, exchange) {
        return Ok(contract);
    }

    // Handle spreads (BAG contracts) - requires contract details, so we skip for now
    // This should be handled by the instrument provider which has access to contract details
    // if symbol_str.contains(":") {
    //     return create_bag_contract(instrument_id, exchange_str);
    // }

    // Handle Forex (CASH)
    if venue_matches(venue_str.as_str(), VENUES_CASH)
        && let Some(captures) = parse_cash_symbol(symbol_str)
    {
        return Ok(Contract {
            contract_id: 0,
            symbol: Symbol::from(&captures.base),
            security_type: SecurityType::ForexPair,
            exchange: Exchange::from(exchange_str),
            currency: Currency::from(&captures.quote),
            local_symbol: format!("{}.{}", captures.base, captures.quote),
            ..Default::default()
        });
    }

    // Handle Crypto
    if venue_matches(venue_str.as_str(), VENUES_CRYPTO)
        && let Some(captures) = parse_cash_symbol(symbol_str)
    {
        return Ok(Contract {
            contract_id: 0,
            symbol: Symbol::from(&captures.base),
            security_type: SecurityType::Crypto,
            exchange: Exchange::from(exchange_str),
            currency: Currency::from(&captures.quote),
            local_symbol: format!("{}.{}", captures.base, captures.quote),
            ..Default::default()
        });
    }

    // Handle Options (OPT)
    if venue_matches(venue_str.as_str(), VENUES_OPT) {
        if let Some(opt) = parse_option_symbol(symbol_str) {
            let local_symbol = format!(
                "{:6}{}{}{}{:08}",
                opt.symbol, opt.expiry, opt.right, opt.strike_integer, opt.strike_decimal
            );
            return Ok(Contract {
                contract_id: 0,
                symbol: Symbol::from(&opt.symbol),
                security_type: SecurityType::Option,
                exchange: Exchange::from(exchange_str),
                currency: Currency::from("USD"), // Will be resolved from contract details
                local_symbol,
                last_trade_date_or_contract_month: opt.expiry,
                strike: opt.strike_value,
                right: opt.right,
                ..Default::default()
            });
        }

        if let Some(opt) = parse_named_option_symbol(symbol_str) {
            return Ok(Contract {
                contract_id: 0,
                symbol: Symbol::from(&opt.trading_class),
                security_type: SecurityType::Option,
                exchange: Exchange::from(exchange_str),
                currency: Currency::from("USD"),
                trading_class: opt.trading_class,
                last_trade_date_or_contract_month: opt.expiry,
                strike: opt.strike_value,
                right: opt.right,
                ..Default::default()
            });
        }
    }

    // Handle Futures and Futures Options
    if venue_matches(venue_str.as_str(), VENUES_FUT) {
        // Check for continuous futures (underlying only, no expiry)
        // IB uses FUT with no expiry date to represent continuous futures
        if let Some(underlying) = parse_futures_underlying(symbol_str) {
            return Ok(Contract {
                contract_id: 0,
                symbol: Symbol::from(&underlying),
                security_type: SecurityType::ContinuousFuture,
                exchange: Exchange::from(exchange_str),
                currency: Currency::from("USD"), // Will be resolved from contract details
                ..Default::default()
            });
        }

        // Check for Futures Options (FOP)
        if let Some(local_symbol) = parse_futures_option_symbol(symbol_str) {
            return Ok(Contract {
                contract_id: 0,
                security_type: SecurityType::FuturesOption,
                exchange: Exchange::from(exchange_str),
                currency: Currency::from("USD"),
                local_symbol,
                ..Default::default()
            });
        }

        // Check for regular Futures (FUT)
        if let Some(fut) = parse_futures_symbol(symbol_str) {
            return Ok(Contract {
                contract_id: 0,
                security_type: SecurityType::Future,
                exchange: Exchange::from(exchange_str),
                currency: Currency::from("USD"),
                local_symbol: fut.local_symbol,
                ..Default::default()
            });
        }
    }

    // Handle CFDs
    if VENUES_CFD.contains(&venue_str.as_str()) {
        if let Some(captures) = parse_cfd_cash_symbol(symbol_str) {
            return Ok(Contract {
                contract_id: 0,
                symbol: Symbol::from(&captures.base),
                security_type: SecurityType::CFD,
                exchange: Exchange::from("SMART"),
                currency: Currency::from(&captures.quote),
                local_symbol: format!("{}.{}", captures.base, captures.quote),
                ..Default::default()
            });
        } else {
            // CFD with space-separated symbol
            let symbol_clean = symbol_str.replace('-', " ");
            return Ok(Contract {
                contract_id: 0,
                symbol: Symbol::from(&symbol_clean),
                security_type: SecurityType::CFD,
                exchange: Exchange::from("SMART"),
                currency: Currency::from("USD"),
                ..Default::default()
            });
        }
    }

    // Handle Commodities
    if VENUES_CMDTY.contains(&venue_str.as_str()) {
        let symbol_clean = symbol_str.replace('-', " ");
        return Ok(Contract {
            contract_id: 0,
            symbol: Symbol::from(&symbol_clean),
            security_type: SecurityType::Commodity,
            exchange: Exchange::from("SMART"),
            currency: Currency::from("USD"),
            ..Default::default()
        });
    }

    // Handle Indices (symbols starting with ^)
    if let Some(local_symbol) = symbol_str.strip_prefix('^') {
        return Ok(Contract {
            contract_id: 0,
            symbol: Symbol::from(local_symbol),
            security_type: SecurityType::Index,
            exchange: Exchange::from(exchange_str),
            currency: Currency::from("USD"),
            local_symbol: local_symbol.into(),
            ..Default::default()
        });
    }

    // Default to Stock (STK)
    let symbol_clean = symbol_str.replace('-', " ");
    Ok(Contract {
        contract_id: 0,
        symbol: Symbol::from(&symbol_clean),
        security_type: SecurityType::Stock,
        exchange: Exchange::from("SMART"),
        currency: Currency::from("USD"), // Will be resolved from contract details
        primary_exchange: Exchange::from(exchange_str),
        local_symbol: symbol_clean,
        ..Default::default()
    })
}

fn instrument_id_to_ib_contract_raw(
    instrument_id: &InstrumentId,
    exchange: Option<&str>,
) -> Option<Contract> {
    let (local_symbol, sec_type_code) = instrument_id.symbol.as_str().rsplit_once('=')?;

    let venue_exchange = instrument_id.venue.as_str().replace('/', ".");
    let exchange_str = exchange.unwrap_or(venue_exchange.as_str());
    let security_type = match sec_type_code {
        "STK" => SecurityType::Stock,
        "OPT" => SecurityType::Option,
        "FUT" => SecurityType::Future,
        "FOP" => SecurityType::FuturesOption,
        "CASH" => SecurityType::ForexPair,
        "CRYPTO" => SecurityType::Crypto,
        "CONTFUT" => SecurityType::ContinuousFuture,
        "IND" => SecurityType::Index,
        "CFD" => SecurityType::CFD,
        "CMDTY" => SecurityType::Commodity,
        "BOND" => SecurityType::Bond,
        _ => return None,
    };

    let contract = match security_type {
        SecurityType::Stock => Contract {
            contract_id: 0,
            security_type,
            exchange: Exchange::from("SMART"),
            primary_exchange: Exchange::from(exchange_str),
            local_symbol: local_symbol.to_string(),
            ..Default::default()
        },
        SecurityType::CFD | SecurityType::Commodity => Contract {
            contract_id: 0,
            security_type,
            exchange: Exchange::from("SMART"),
            local_symbol: local_symbol.to_string(),
            ..Default::default()
        },
        SecurityType::Index => Contract {
            contract_id: 0,
            security_type,
            exchange: Exchange::from(exchange_str),
            local_symbol: local_symbol.to_string(),
            ..Default::default()
        },
        _ => Contract {
            contract_id: 0,
            security_type,
            exchange: Exchange::from(exchange_str),
            local_symbol: local_symbol.to_string(),
            ..Default::default()
        },
    };

    Some(contract)
}

/// Currency pair captures
struct CurrencyPair {
    base: String,
    quote: String,
}

/// Parse cash/forex symbol like "EUR/USD"
fn parse_cash_symbol(symbol: &str) -> Option<CurrencyPair> {
    if let Some((base, quote)) = symbol.split_once('/')
        && base.len() == 3
        && quote.len() == 3
    {
        return Some(CurrencyPair {
            base: base.to_string(),
            quote: quote.to_string(),
        });
    }
    None
}

/// Parse CFD cash symbol like "EUR.USD"
fn parse_cfd_cash_symbol(symbol: &str) -> Option<CurrencyPair> {
    if let Some((base, quote)) = symbol.split_once('.')
        && base.len() == 3
        && quote.len() == 3
    {
        return Some(CurrencyPair {
            base: base.to_string(),
            quote: quote.to_string(),
        });
    }
    None
}

/// Option symbol captures
struct OptionSymbol {
    symbol: String,
    expiry: String,
    right: String,
    strike_integer: String,
    strike_decimal: String,
    strike_value: f64,
}

/// Parse option symbol like "AAPL230120C00150000" (6-char symbol, 6-char expiry YYMMDD, 1-char right, 8-char strike)
fn parse_option_symbol(symbol: &str) -> Option<OptionSymbol> {
    // Pattern: SYMBOL + YYMMDD + C/P + STRIKE (8 digits, could have decimal)
    // Minimum: 6 (symbol) + 6 (date) + 1 (right) + 8 (strike) = 21 chars
    if symbol.len() < 21 {
        return None;
    }

    // Try to match: 6-char symbol, 6-char date, 1-char right (C/P), remainder is strike
    let symbol_part = &symbol[..6.min(symbol.len())].trim();
    let remaining = &symbol[6.min(symbol.len())..];

    if remaining.len() < 15 {
        return None;
    }

    let expiry = &remaining[..6];
    let right_char = remaining.chars().nth(6)?;
    let right = if right_char == 'C' || right_char == 'c' {
        "C"
    } else if right_char == 'P' || right_char == 'p' {
        "P"
    } else {
        return None;
    };

    let strike_str = &remaining[7..];
    if strike_str.len() < 8 {
        return None;
    }

    // Strike is typically 8 digits with possible decimal
    let strike_value = if strike_str.contains('.') {
        strike_str.parse().ok()?
    } else {
        // 8-digit integer strike, divide by 1000 for typical option strikes
        let strike_int: i32 = strike_str.parse().ok()?;
        strike_int as f64 / 1000.0
    };

    let strike_integer = if strike_str.len() >= 8 {
        &strike_str[..strike_str.len().min(8)]
    } else {
        strike_str
    };
    let strike_decimal = if strike_str.len() > 8 {
        &strike_str[8..]
    } else {
        ""
    };

    Some(OptionSymbol {
        symbol: (*symbol_part).to_string(),
        expiry: expiry.to_string(),
        right: right.to_string(),
        strike_integer: strike_integer.to_string(),
        strike_decimal: strike_decimal.to_string(),
        strike_value,
    })
}

/// Named option symbol captures for formats like "C OESX 20260213 4775".
struct NamedOptionSymbol {
    trading_class: String,
    expiry: String,
    right: String,
    strike_value: f64,
}

fn parse_named_option_symbol(symbol: &str) -> Option<NamedOptionSymbol> {
    let parts: Vec<&str> = symbol.split_whitespace().collect();
    if !(parts.len() == 4 || parts.len() == 5) {
        return None;
    }

    let right = match parts[0] {
        "C" | "c" => "C",
        "P" | "p" => "P",
        _ => return None,
    };

    let expiry = parts[2];
    if expiry.len() != 8 || !expiry.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    Some(NamedOptionSymbol {
        trading_class: parts[1].to_string(),
        expiry: expiry.to_string(),
        right: right.to_string(),
        strike_value: parts[3].parse::<f64>().ok()?,
    })
}

fn normalize_option_symbol(local_symbol: &str) -> String {
    if local_symbol.len() >= 15 {
        let (root, suffix) = local_symbol.split_at(local_symbol.len() - 15);
        let is_occ_suffix = suffix[..6].chars().all(|c| c.is_ascii_digit())
            && matches!(suffix.chars().nth(6), Some('C' | 'P'))
            && suffix[7..].chars().all(|c| c.is_ascii_digit());

        if !root.is_empty() && root.len() <= 6 && is_occ_suffix {
            return format!("{:<6}{}", root.trim_end(), suffix);
        }
    }

    local_symbol.to_string()
}

fn format_option_strike(strike: f64) -> String {
    if strike.fract() == 0.0 {
        format!("{strike:.0}")
    } else {
        format!("{strike}")
    }
}

/// Futures symbol captures
struct FuturesSymbol {
    local_symbol: String,
}

/// Parse futures underlying (continuous) - just the symbol without expiry
fn parse_futures_underlying(symbol: &str) -> Option<String> {
    // If it's just 1-3 characters, it's likely an underlying
    if symbol.len() <= 3 && symbol.chars().all(|c| c.is_alphabetic()) {
        Some(symbol.to_string())
    } else {
        None
    }
}

fn is_futures_month_code(ch: char) -> bool {
    matches!(
        ch,
        'F' | 'G' | 'H' | 'J' | 'K' | 'M' | 'N' | 'Q' | 'U' | 'V' | 'X' | 'Z'
    )
}

fn parse_futures_month_and_year(symbol: &str) -> Option<(usize, char, String)> {
    for (month_pos, month_char) in symbol.char_indices().rev() {
        if !is_futures_month_code(month_char) {
            continue;
        }

        let remaining = &symbol[month_pos + month_char.len_utf8()..];
        if remaining.is_empty() || !remaining.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }

        let year = match remaining.len() {
            1 | 2 => remaining.to_string(),
            4 => remaining[remaining.len() - 2..].to_string(),
            _ => continue,
        };

        if month_pos == 0 {
            continue;
        }

        return Some((month_pos, month_char, year));
    }

    None
}

/// Parse futures symbol like "YMM6", "ESM23", or "ESM2023"
fn parse_futures_symbol(symbol: &str) -> Option<FuturesSymbol> {
    parse_futures_month_and_year(symbol).map(|_| FuturesSymbol {
        local_symbol: symbol.to_string(),
    })
}

/// Parse futures option symbol like "YMM6 C4500", "ESM23 C4500", or "ESM2023 C4500"
fn parse_futures_option_symbol(symbol: &str) -> Option<String> {
    let (futures_symbol, rest) = symbol.split_once(' ')?;
    let (month_pos, _, _) = parse_futures_month_and_year(futures_symbol)?;
    let _fut_symbol = &futures_symbol[..month_pos];

    // Parse right and strike
    let right_char = rest.chars().next()?;
    if right_char != 'C' && right_char != 'c' && right_char != 'P' && right_char != 'p' {
        return None;
    }

    let strike_str = &rest[1..];
    strike_str.parse::<f64>().ok()?;

    Some(symbol.to_string())
}

/// Check if an instrument ID represents a spread.
///
/// This checks if the symbol contains the spread format pattern: `(ratio)symbol_` or `((ratio))symbol_`
///
/// # Arguments
///
/// * `instrument_id` - The instrument ID to check
///
/// # Returns
///
/// Returns `true` if the instrument ID appears to be a spread.
#[must_use]
pub fn is_spread_instrument_id(instrument_id: &InstrumentId) -> bool {
    let symbol_str = instrument_id.symbol.as_str();
    // Check if symbol contains spread pattern: (ratio) or ((ratio))
    symbol_str.contains('(') && symbol_str.contains('_')
}

/// Parse a spread instrument ID back into leg tuples.
///
/// This implements the same logic as Python's `InstrumentId.to_list()`:
/// - Parses symbol string like `(1)SYMBOL1_((2))SYMBOL2`
/// - Positive ratios: `(ratio)SYMBOL`
/// - Negative ratios: `((abs(ratio)))SYMBOL`
/// - Returns sorted list of (instrument_id, ratio) tuples
///
/// # Arguments
///
/// * `instrument_id` - The spread instrument ID to parse
///
/// # Returns
///
/// Returns a vector of (instrument_id, ratio) tuples, sorted alphabetically by symbol.
///
/// # Errors
///
/// Returns an error if the symbol format is invalid.
pub fn parse_spread_instrument_id_to_legs(
    instrument_id: &InstrumentId,
) -> anyhow::Result<Vec<(InstrumentId, i32)>> {
    let symbol_str = instrument_id.symbol.as_str();
    let venue = instrument_id.venue;

    // Split by underscore to get individual components
    let components: Vec<&str> = symbol_str.split('_').collect();
    let mut result = Vec::new();

    // Pattern to match (ratio)symbol or ((ratio))symbol
    // Positive: (ratio)symbol
    // Negative: ((ratio))symbol
    for component in components {
        if component.is_empty() {
            continue;
        }

        // Check for negative ratio: ((ratio))symbol
        if let Some(rest) = component.strip_prefix("((")
            && let Some(pos) = rest.find("))")
        {
            let ratio_str = &rest[..pos];
            let symbol_value = &rest[pos + 2..];

            if let Ok(ratio) = ratio_str.parse::<i32>() {
                let leg_instrument_id =
                    InstrumentId::new(NautilusSymbol::from(symbol_value), venue);
                result.push((leg_instrument_id, -ratio));
                continue;
            }
        }

        // Check for positive ratio: (ratio)symbol
        if let Some(rest) = component.strip_prefix('(')
            && let Some(pos) = rest.find(')')
        {
            let ratio_str = &rest[..pos];
            let symbol_value = &rest[pos + 1..];

            if let Ok(ratio) = ratio_str.parse::<i32>() {
                let leg_instrument_id =
                    InstrumentId::new(NautilusSymbol::from(symbol_value), venue);
                result.push((leg_instrument_id, ratio));
                continue;
            }
        }

        anyhow::bail!("Invalid spread symbol format for component: {component}");
    }

    // Sort result alphabetically by symbol
    result.sort_by(|a, b| a.0.symbol.as_str().cmp(b.0.symbol.as_str()));

    Ok(result)
}

#[cfg(test)]
mod tests {
    use ibapi::contracts::{Contract, Currency, Exchange, SecurityType, Symbol};
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::{ib_contract_to_instrument_id_simplified, instrument_id_to_ib_contract};

    #[rstest]
    fn test_ib_contract_to_instrument_id_simplified_normalizes_occ_option_root() {
        let contract = Contract {
            symbol: Symbol::from("SPXW"),
            security_type: SecurityType::Option,
            exchange: Exchange::from("SMART"),
            currency: Currency::from("USD"),
            local_symbol: "SPXW260313P06630000".to_string(),
            last_trade_date_or_contract_month: "260313".to_string(),
            right: "P".to_string(),
            strike: 6630.0,
            ..Default::default()
        };

        let instrument_id = ib_contract_to_instrument_id_simplified(&contract, None).unwrap();

        assert_eq!(
            instrument_id,
            InstrumentId::from("SPXW  260313P06630000.SMART")
        );
    }

    #[rstest]
    fn test_ib_contract_to_instrument_id_simplified_formats_named_option_without_local_symbol() {
        let contract = Contract {
            symbol: Symbol::from("OESX"),
            security_type: SecurityType::Option,
            exchange: Exchange::from("EUREX"),
            currency: Currency::from("EUR"),
            trading_class: "OESX".to_string(),
            local_symbol: String::new(),
            last_trade_date_or_contract_month: "20260213".to_string(),
            right: "C".to_string(),
            strike: 4775.0,
            ..Default::default()
        };

        let instrument_id = ib_contract_to_instrument_id_simplified(&contract, None).unwrap();

        assert_eq!(
            instrument_id,
            InstrumentId::from("C OESX 20260213 4775.EUREX")
        );
    }

    #[rstest]
    fn test_instrument_id_to_ib_contract_parses_named_option_symbol() {
        let instrument_id = InstrumentId::from("C OESX 20260213 4775.EUREX");

        let contract = instrument_id_to_ib_contract(instrument_id, None).unwrap();

        assert_eq!(contract.security_type, SecurityType::Option);
        assert_eq!(contract.exchange.as_str(), "EUREX");
        assert_eq!(contract.symbol.as_str(), "OESX");
        assert_eq!(contract.trading_class.as_str(), "OESX");
        assert_eq!(
            contract.last_trade_date_or_contract_month.as_str(),
            "20260213"
        );
        assert_eq!(contract.right.as_str(), "C");
        assert_eq!(contract.strike, 4775.0);
    }

    #[rstest]
    fn test_instrument_id_to_ib_contract_maps_xcbt_to_cbot_exchange() {
        let instrument_id = InstrumentId::from("YMM6.XCBT");

        let contract = instrument_id_to_ib_contract(instrument_id, None).unwrap();

        assert_eq!(contract.security_type, SecurityType::Future);
        assert_eq!(contract.exchange.as_str(), "CBOT");
        assert_eq!(contract.local_symbol.as_str(), "YMM6");
        assert!(contract.symbol.as_str().is_empty());
        assert!(contract.last_trade_date_or_contract_month.is_empty());
    }

    #[rstest]
    fn test_instrument_id_to_ib_contract_parses_futures_option_with_month_code_in_symbol() {
        let instrument_id = InstrumentId::from("YMM6 C45000.XCBT");

        let contract = instrument_id_to_ib_contract(instrument_id, None).unwrap();

        assert_eq!(contract.security_type, SecurityType::FuturesOption);
        assert_eq!(contract.exchange.as_str(), "CBOT");
        assert_eq!(contract.local_symbol.as_str(), "YMM6 C45000");
    }

    #[rstest]
    fn test_instrument_id_to_ib_contract_uses_contfut_for_underlying() {
        let instrument_id = InstrumentId::from("ES.XCME");

        let contract = instrument_id_to_ib_contract(instrument_id, None).unwrap();

        assert_eq!(contract.security_type, SecurityType::ContinuousFuture);
        assert_eq!(contract.exchange.as_str(), "CME");
        assert_eq!(contract.symbol.as_str(), "ES");
    }

    #[rstest]
    fn test_instrument_id_to_ib_contract_parses_raw_stock_symbol() {
        let instrument_id = InstrumentId::from("AAPL=STK.NASDAQ");

        let contract = instrument_id_to_ib_contract(instrument_id, None).unwrap();

        assert_eq!(contract.security_type, SecurityType::Stock);
        assert_eq!(contract.exchange.as_str(), "SMART");
        assert_eq!(contract.primary_exchange.as_str(), "NASDAQ");
        assert_eq!(contract.local_symbol.as_str(), "AAPL");
        assert!(contract.symbol.as_str().is_empty());
    }

    #[rstest]
    fn test_instrument_id_to_ib_contract_parses_raw_forex_symbol() {
        let instrument_id = InstrumentId::from("EUR.USD=CASH.IDEALPRO");

        let contract = instrument_id_to_ib_contract(instrument_id, None).unwrap();

        assert_eq!(contract.security_type, SecurityType::ForexPair);
        assert_eq!(contract.exchange.as_str(), "IDEALPRO");
        assert_eq!(contract.local_symbol.as_str(), "EUR.USD");
        assert!(contract.symbol.as_str().is_empty());
    }

    #[rstest]
    fn test_instrument_id_to_ib_contract_raw_respects_exchange_override() {
        let instrument_id = InstrumentId::from("YMM6=FUT.XCBT");

        let contract = instrument_id_to_ib_contract(instrument_id, Some("CBOT")).unwrap();

        assert_eq!(contract.security_type, SecurityType::Future);
        assert_eq!(contract.exchange.as_str(), "CBOT");
        assert_eq!(contract.local_symbol.as_str(), "YMM6");
    }
}
