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

use std::str::FromStr;

use anyhow::Context;
use ibapi::contracts::{
    Contract, Currency as IBCurrency, Exchange as IBExchange, OptionRight, SecurityIdType,
    SecurityType, Symbol,
};
use nautilus_core::Params;
use serde_json::Value;

use crate::common::enums::{IbOptionRight, IbSecurityType};

pub(crate) const KEY_BUILD_OPTIONS_CHAIN: &str = "build_options_chain";
pub(crate) const KEY_BUILD_FUTURES_CHAIN: &str = "build_futures_chain";
pub(crate) const KEY_MIN_EXPIRY_DAYS: &str = "min_expiry_days";
pub(crate) const KEY_MAX_EXPIRY_DAYS: &str = "max_expiry_days";
pub(crate) const KEY_OPTIONS_CHAIN_EXCHANGE: &str = "options_chain_exchange";
pub(crate) const KEY_OPTIONS_CHAIN_EXCHANGE_ALT: &str = "optionsChainExchange";

/// A configured IB contract with optional per-contract chain-building settings.
#[derive(Clone, Debug)]
pub struct ConfiguredContract {
    /// The IB contract to load.
    pub contract: Contract,
    /// Whether to build the options chain for this contract, overriding the global flag.
    pub build_options_chain: Option<bool>,
    /// Whether to build the futures chain for this contract, overriding the global flag.
    pub build_futures_chain: Option<bool>,
    /// Minimum expiry days for this contract's chains, overriding the global value.
    pub min_expiry_days: Option<u32>,
    /// Maximum expiry days for this contract's chains, overriding the global value.
    pub max_expiry_days: Option<u32>,
    /// Exchange override for options chain lookups.
    pub options_chain_exchange: Option<String>,
}

impl From<Contract> for ConfiguredContract {
    fn from(contract: Contract) -> Self {
        Self {
            contract,
            build_options_chain: None,
            build_futures_chain: None,
            min_expiry_days: None,
            max_expiry_days: None,
            options_chain_exchange: None,
        }
    }
}

impl ConfiguredContract {
    /// Convert to the JSON representation used in configuration files.
    #[must_use]
    pub fn to_json_value(&self) -> Value {
        let mut json = contract_to_json_value(&self.contract);

        if let Value::Object(map) = &mut json {
            for (key, value) in self.chain_entries() {
                map.insert(key.to_string(), value);
            }
        }

        json
    }

    /// Chain-building spec for `load_contract_spec`, when any per-contract setting is present.
    #[must_use]
    pub fn chain_spec_json(&self) -> Option<Value> {
        let entries = self.chain_entries();

        if entries.is_empty() {
            return None;
        }

        let mut map = serde_json::Map::new();
        for (key, value) in entries {
            map.insert(key.to_string(), value);
        }
        Some(Value::Object(map))
    }

    fn chain_entries(&self) -> Vec<(&'static str, Value)> {
        let mut entries = Vec::new();

        if let Some(value) = self.build_options_chain {
            entries.push((KEY_BUILD_OPTIONS_CHAIN, Value::from(value)));
        }

        if let Some(value) = self.build_futures_chain {
            entries.push((KEY_BUILD_FUTURES_CHAIN, Value::from(value)));
        }

        if let Some(value) = self.min_expiry_days {
            entries.push((KEY_MIN_EXPIRY_DAYS, Value::from(value)));
        }

        if let Some(value) = self.max_expiry_days {
            entries.push((KEY_MAX_EXPIRY_DAYS, Value::from(value)));
        }

        if let Some(value) = &self.options_chain_exchange {
            entries.push((KEY_OPTIONS_CHAIN_EXCHANGE, Value::from(value.as_str())));
        }

        entries
    }
}

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

fn security_type_to_code(security_type: &SecurityType) -> String {
    IbSecurityType::try_from(security_type).map_or_else(
        |_| security_type.to_string(),
        |security_type| security_type.to_string(),
    )
}

/// Parse IB contract from JSON dictionary.
///
/// This function parses a JSON object (dictionary) representing an IBContract
/// and converts it to a rust-ibapi Contract struct.
///
/// # Errors
///
/// Returns an error if the JSON is not a valid object or if required fields are missing.
pub fn parse_contract_from_json(json: &Value) -> anyhow::Result<Contract> {
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Expected JSON object for contract"))?;

    let get_str = |key: &str| -> String {
        obj.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };

    let get_i32 = |key: &str| -> i32 {
        obj.get(key)
            .and_then(|v| v.as_i64())
            .map_or(0, |n| n as i32)
    };

    let get_f64 = |key: &str| -> f64 { obj.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0) };

    let get_bool = |key: &str| -> bool { obj.get(key).and_then(|v| v.as_bool()).unwrap_or(false) };

    let parse_option_right = |key: &str| -> Option<OptionRight> {
        match IbOptionRight::from_str(&get_str(key)).ok()? {
            IbOptionRight::Call => Some(OptionRight::Call),
            IbOptionRight::Put => Some(OptionRight::Put),
        }
    };

    let parse_security_id_type = |key: &str| -> Option<SecurityIdType> {
        match get_str(key).to_ascii_uppercase().as_str() {
            "CUSIP" => Some(SecurityIdType::Cusip),
            "ISIN" => Some(SecurityIdType::Isin),
            "SEDOL" => Some(SecurityIdType::Sedol),
            "RIC" => Some(SecurityIdType::Ric),
            "FIGI" => Some(SecurityIdType::Figi),
            _ => None,
        }
    };

    // Parse security type
    let sec_type_str = get_str("secType");
    let security_type = if sec_type_str.is_empty() {
        SecurityType::Stock
    } else {
        IbSecurityType::from_str(&sec_type_str).map_or_else(
            |_| SecurityType::Other(sec_type_str.clone()),
            IbSecurityType::ibapi_security_type,
        )
    };

    Ok(Contract {
        contract_id: get_i32("conId"),
        symbol: Symbol::from(get_str("symbol")),
        security_type,
        last_trade_date_or_contract_month: get_str("lastTradeDateOrContractMonth"),
        strike: get_f64("strike"),
        right: parse_option_right("right"),
        multiplier: get_str("multiplier"),
        exchange: IBExchange::from(get_str("exchange")),
        currency: IBCurrency::from(get_str("currency")),
        local_symbol: get_str("localSymbol"),
        primary_exchange: IBExchange::from(get_str("primaryExchange")),
        trading_class: get_str("tradingClass"),
        include_expired: get_bool("includeExpired"),
        security_id_type: parse_security_id_type("secIdType"),
        security_id: get_str("secId"),
        last_trade_date: None,
        combo_legs_description: get_str("comboLegsDescrip"),
        combo_legs: Vec::new(),       // TODO: Parse combo_legs if needed
        delta_neutral_contract: None, // TODO: Parse delta_neutral_contract if needed
        issuer_id: get_str("issuerId"),
        description: get_str("description"),
    })
}

/// Parse a configured IB contract with strict field and enum validation.
///
/// # Errors
///
/// Returns an error when a known field has the wrong JSON type, a numeric field is out of
/// range, or an enum value is unknown.
pub fn parse_configured_contract_from_json(json: &Value) -> anyhow::Result<ConfiguredContract> {
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Expected JSON object for configured contract"))?;

    for key in [
        "symbol",
        "lastTradeDateOrContractMonth",
        "multiplier",
        "exchange",
        "currency",
        "localSymbol",
        "primaryExchange",
        "tradingClass",
        "secId",
        "issuerId",
        "description",
        "comboLegsDescrip",
    ] {
        if let Some(value) = obj.get(key)
            && !value.is_string()
        {
            anyhow::bail!("Configured contract field '{key}' must be a string");
        }
    }

    for key in ["strike"] {
        if let Some(value) = obj.get(key)
            && !value.is_number()
        {
            anyhow::bail!("Configured contract field '{key}' must be a number");
        }
    }

    if let Some(value) = obj.get("includeExpired")
        && !value.is_boolean()
    {
        anyhow::bail!("Configured contract field 'includeExpired' must be a boolean");
    }

    let mut contract_id = 0;

    if let Some(value) = obj.get("conId") {
        let raw_contract_id = value
            .as_i64()
            .context("Configured contract field 'conId' must be an integer")?;
        contract_id = i32::try_from(raw_contract_id)
            .context("Configured contract field 'conId' exceeds i32")?;
    }

    match obj.get("secType").and_then(Value::as_str) {
        Some(security_type) => {
            IbSecurityType::from_str(security_type)?;
        }
        None if contract_id > 0 => {} // IB resolves the contract by conId
        None => anyhow::bail!(
            "Configured contract requires 'secType' as a known IB security type string, or a positive 'conId'"
        ),
    }

    if let Some(value) = obj.get("right")
        && !value.is_null()
    {
        let right = value
            .as_str()
            .context("Configured contract field 'right' must be a string")?;
        IbOptionRight::from_str(right)?;
    }

    if let Some(value) = obj.get("secIdType")
        && !value.is_null()
    {
        let security_id_type = value
            .as_str()
            .context("Configured contract field 'secIdType' must be a string")?;

        if !matches!(
            security_id_type.to_ascii_uppercase().as_str(),
            "CUSIP" | "ISIN" | "SEDOL" | "RIC" | "FIGI"
        ) {
            anyhow::bail!("Unknown IB security ID type: {security_id_type}");
        }
    }

    let build_options_chain = configured_bool(obj, KEY_BUILD_OPTIONS_CHAIN)?;
    let build_futures_chain = configured_bool(obj, KEY_BUILD_FUTURES_CHAIN)?;
    let min_expiry_days = configured_u32(obj, KEY_MIN_EXPIRY_DAYS)?;
    let max_expiry_days = configured_u32(obj, KEY_MAX_EXPIRY_DAYS)?;
    let options_chain_exchange = match configured_string(obj, KEY_OPTIONS_CHAIN_EXCHANGE)? {
        Some(exchange) => Some(exchange),
        None => configured_string(obj, KEY_OPTIONS_CHAIN_EXCHANGE_ALT)?,
    };

    Ok(ConfiguredContract {
        contract: parse_contract_from_json(json)?,
        build_options_chain,
        build_futures_chain,
        min_expiry_days,
        max_expiry_days,
        options_chain_exchange,
    })
}

fn configured_bool(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> anyhow::Result<Option<bool>> {
    obj.get(key)
        .map(|value| {
            value
                .as_bool()
                .with_context(|| format!("Configured contract field '{key}' must be a boolean"))
        })
        .transpose()
}

fn configured_u32(obj: &serde_json::Map<String, Value>, key: &str) -> anyhow::Result<Option<u32>> {
    obj.get(key)
        .map(|value| {
            value
                .as_u64()
                .and_then(|days| u32::try_from(days).ok())
                .with_context(|| {
                    format!("Configured contract field '{key}' must be a non-negative integer")
                })
        })
        .transpose()
}

fn configured_string(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> anyhow::Result<Option<String>> {
    obj.get(key)
        .map(|value| {
            value
                .as_str()
                .map(ToString::to_string)
                .with_context(|| format!("Configured contract field '{key}' must be a string"))
        })
        .transpose()
}

/// Parse multiple IB contracts from JSON array.
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
