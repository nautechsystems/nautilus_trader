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

//! Contract parsing utilities for Interactive Brokers adapter.

use ibapi::contracts::{
    Contract, Currency as IBCurrency, Exchange as IBExchange, SecurityType, Symbol,
};
use nautilus_core::Params;
use serde_json::Value;

/// Convert an IB contract into JSON metadata suitable for instrument `info["contract"]`.
#[must_use]
pub fn contract_to_json_value(contract: &Contract) -> Value {
    serde_json::json!({
        "secType": security_type_to_code(&contract.security_type),
        "conId": contract.contract_id,
        "exchange": contract.exchange.to_string(),
        "primaryExchange": contract.primary_exchange.to_string(),
        "symbol": contract.symbol.to_string(),
        "localSymbol": contract.local_symbol,
        "currency": contract.currency.to_string(),
        "tradingClass": contract.trading_class,
        "lastTradeDateOrContractMonth": contract.last_trade_date_or_contract_month,
        "multiplier": contract.multiplier,
        "strike": contract.strike,
        "right": contract.right,
        "includeExpired": contract.include_expired,
        "secIdType": contract.security_id_type,
        "secId": contract.security_id,
        "description": contract.description,
        "issuerId": contract.issuer_id,
        "comboLegsDescrip": contract.combo_legs_description,
    })
}

#[must_use]
pub fn contract_to_params(contract: &Contract) -> Params {
    let mut params = Params::new();

    if let Value::Object(map) = contract_to_json_value(contract) {
        for (key, value) in map {
            params.insert(key, value);
        }
    }

    params
}

fn security_type_to_code(security_type: &SecurityType) -> &str {
    match security_type {
        SecurityType::Stock => "STK",
        SecurityType::Option => "OPT",
        SecurityType::Future => "FUT",
        SecurityType::FuturesOption => "FOP",
        SecurityType::ForexPair => "CASH",
        SecurityType::Crypto => "CRYPTO",
        SecurityType::ContinuousFuture => "CONTFUT",
        SecurityType::Index => "IND",
        SecurityType::CFD => "CFD",
        SecurityType::Commodity => "CMDTY",
        SecurityType::Bond => "BOND",
        SecurityType::Warrant => "WAR",
        SecurityType::News => "NEWS",
        SecurityType::MutualFund => "FUND",
        SecurityType::Spread => "BAG",
        SecurityType::Other(other) => other.as_str(),
    }
}

/// Parse IB contract from JSON dictionary.
///
/// This function parses a JSON object (dictionary) representing an IBContract
/// and converts it to a rust-ibapi Contract struct.
///
/// # Arguments
///
/// * `json` - JSON value representing the contract dictionary
///
/// # Returns
///
/// Returns a Contract if parsing succeeds, or None if parsing fails.
///
/// # Errors
///
/// Returns an error if the JSON is not a valid object or if required fields are missing.
pub fn parse_contract_from_json(json: &Value) -> anyhow::Result<Contract> {
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Expected JSON object for contract"))?;

    // Helper to get string field with default
    let get_str = |key: &str| -> String {
        obj.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };

    // Helper to get i32 field with default
    let get_i32 = |key: &str| -> i32 {
        obj.get(key)
            .and_then(|v| v.as_i64())
            .map_or(0, |n| n as i32)
    };

    // Helper to get f64 field with default
    let get_f64 = |key: &str| -> f64 { obj.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0) };

    // Helper to get bool field with default
    let get_bool = |key: &str| -> bool { obj.get(key).and_then(|v| v.as_bool()).unwrap_or(false) };

    // Parse security type
    let sec_type_str = get_str("secType");
    let security_type = match sec_type_str.as_str() {
        "STK" | "stk" => SecurityType::Stock,
        "OPT" | "opt" => SecurityType::Option,
        "FUT" | "fut" => SecurityType::Future,
        "FOP" | "fop" => SecurityType::FuturesOption,
        "CASH" | "cash" => SecurityType::ForexPair,
        "CRYPTO" | "crypto" => SecurityType::Crypto,
        "IND" | "ind" => SecurityType::Index,
        "CFD" | "cfd" => SecurityType::CFD,
        "CMDTY" | "cmdty" => SecurityType::Commodity,
        "BOND" | "bond" => SecurityType::Bond,
        "BAG" | "bag" => SecurityType::Spread,
        "" => SecurityType::Stock, // Default to stock
        other => SecurityType::Other(other.to_string()),
    };

    Ok(Contract {
        contract_id: get_i32("conId"),
        symbol: Symbol::from(get_str("symbol")),
        security_type,
        last_trade_date_or_contract_month: get_str("lastTradeDateOrContractMonth"),
        strike: get_f64("strike"),
        right: get_str("right"),
        multiplier: get_str("multiplier"),
        exchange: IBExchange::from(get_str("exchange")),
        currency: IBCurrency::from(get_str("currency")),
        local_symbol: get_str("localSymbol"),
        primary_exchange: IBExchange::from(get_str("primaryExchange")),
        trading_class: get_str("tradingClass"),
        include_expired: get_bool("includeExpired"),
        security_id_type: get_str("secIdType"),
        security_id: get_str("secId"),
        last_trade_date: None,
        combo_legs_description: get_str("comboLegsDescrip"),
        combo_legs: Vec::new(),       // TODO: Parse combo_legs if needed
        delta_neutral_contract: None, // TODO: Parse delta_neutral_contract if needed
        issuer_id: get_str("issuerId"),
        description: get_str("description"),
    })
}

/// Parse multiple IB contracts from JSON array.
///
/// # Arguments
///
/// * `json_str` - JSON string containing an array of contract dictionaries
///
/// # Returns
///
/// Returns a vector of parsed contracts.
///
/// # Errors
///
/// Returns an error if the JSON string is invalid or if any contract fails to parse.
pub fn parse_contracts_from_json_array(json_str: &str) -> anyhow::Result<Vec<Contract>> {
    let value: Value = serde_json::from_str(json_str).context("Failed to parse JSON string")?;

    let array = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Expected JSON array for contracts"))?;

    let mut contracts = Vec::new();

    for (idx, item) in array.iter().enumerate() {
        match parse_contract_from_json(item) {
            Ok(contract) => contracts.push(contract),
            Err(e) => {
                tracing::warn!("Failed to parse contract at index {}: {}", idx, e);
            }
        }
    }

    Ok(contracts)
}

use anyhow::Context;
