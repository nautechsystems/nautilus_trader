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

//! Python conversion utilities for Interactive Brokers types.

use ibapi::contracts::{ComboLegOpenClose, Contract, ContractDetails, SecurityType};
use nautilus_core::python::{to_pytype_err, to_pyvalue_err};
use pyo3::{
    prelude::*,
    types::{PyBytes, PyDict, PyList},
};

use crate::common::contracts::parse_contract_from_json;

/// Convert a Python object to a JSON value.
pub fn py_to_json_value(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    // Try to call .json() first (NautilusConfig)
    if let Ok(json_bytes) = obj.call_method0("json") {
        if let Ok(bytes) = json_bytes.clone().cast_into::<PyBytes>() {
            let json_str = std::str::from_utf8(bytes.as_bytes())
                .map_err(|e| to_pyvalue_err(format!("Invalid UTF-8 in json output: {e}")))?;

            let value: serde_json::Value = serde_json::from_str(json_str)
                .map_err(|e| to_pyvalue_err(format!("Invalid JSON: {e}")))?;

            return Ok(value);
        }
    }

    // Try to treat as dict
    if let Ok(dict) = obj.clone().cast_into::<pyo3::types::PyDict>() {
        // Convert dict to JSON value using Python's json module
        let json_mod = obj.py().import("json")?;
        let json_str_obj = json_mod.call_method1("dumps", (dict,))?;
        let json_str = json_str_obj.extract::<String>()?;

        let value: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| to_pyvalue_err(format!("Invalid JSON from dict: {e}")))?;

        return Ok(value);
    }

    Err(to_pytype_err("Expected object with .json() or dict"))
}

/// Convert a Python object (IBContract or dict) to a Rust Contract.
///
/// # Arguments
///
/// * `obj` - The Python object to convert. Can be an `IBContract` instance (which has a `.json()` method)
///           or a dictionary.
///
/// # Returns
///
/// Returns the parsed Rust `Contract`.
///
/// # Errors
///
/// Returns a PyValueError if conversion fails.
pub fn py_to_contract(obj: &Bound<'_, PyAny>) -> PyResult<Contract> {
    let value = py_to_json_value(obj)?;
    parse_contract_from_json(&value)
        .map_err(|e| to_pyvalue_err(format!("Failed to parse contract: {e}")))
}

/// Convert a Python list of objects to a vector of Rust Contracts.
pub fn py_list_to_contracts(obj: &Bound<'_, PyAny>) -> PyResult<Vec<Contract>> {
    let list = obj.clone().cast_into::<pyo3::types::PyList>()?;
    let mut contracts = Vec::with_capacity(list.len());
    for item in list.iter() {
        contracts.push(py_to_contract(&item)?);
    }
    Ok(contracts)
}

/// Convert a Python list of objects to a vector of JSON values.
pub fn py_list_to_json_values(obj: &Bound<'_, PyAny>) -> PyResult<Vec<serde_json::Value>> {
    let list = obj.clone().cast_into::<pyo3::types::PyList>()?;
    let mut values = Vec::with_capacity(list.len());
    for item in list.iter() {
        values.push(py_to_json_value(&item)?);
    }
    Ok(values)
}

fn security_type_to_ib_str(security_type: &SecurityType) -> &str {
    match security_type {
        SecurityType::Stock => "STK",
        SecurityType::Option => "OPT",
        SecurityType::Future => "FUT",
        SecurityType::ContinuousFuture => "CONTFUT",
        SecurityType::FuturesOption => "FOP",
        SecurityType::ForexPair => "CASH",
        SecurityType::Crypto => "CRYPTO",
        SecurityType::Index => "IND",
        SecurityType::CFD => "CFD",
        SecurityType::Commodity => "CMDTY",
        SecurityType::Bond => "BOND",
        SecurityType::Spread => "BAG",
        SecurityType::Warrant => "WAR",
        SecurityType::News => "NEWS",
        SecurityType::MutualFund => "FUND",
        SecurityType::Other(_) => "",
    }
}

fn combo_leg_open_close_to_i32(open_close: ComboLegOpenClose) -> i32 {
    match open_close {
        ComboLegOpenClose::Same => 0,
        ComboLegOpenClose::Open => 1,
        ComboLegOpenClose::Close => 2,
        ComboLegOpenClose::Unknown => 3,
    }
}

pub fn contract_to_pydict<'py>(
    py: Python<'py>,
    contract: &Contract,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("secType", security_type_to_ib_str(&contract.security_type))?;
    dict.set_item("conId", contract.contract_id)?;
    dict.set_item("exchange", contract.exchange.as_str())?;
    dict.set_item("primaryExchange", contract.primary_exchange.as_str())?;
    dict.set_item("symbol", contract.symbol.as_str())?;
    dict.set_item("localSymbol", contract.local_symbol.as_str())?;
    dict.set_item("currency", contract.currency.as_str())?;
    dict.set_item("tradingClass", &contract.trading_class)?;
    dict.set_item(
        "lastTradeDateOrContractMonth",
        &contract.last_trade_date_or_contract_month,
    )?;
    dict.set_item(
        "lastTradeDate",
        contract
            .last_trade_date
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
    )?;
    dict.set_item("multiplier", &contract.multiplier)?;
    dict.set_item("strike", contract.strike)?;
    dict.set_item("right", &contract.right)?;
    dict.set_item("includeExpired", contract.include_expired)?;
    dict.set_item("secIdType", &contract.security_id_type)?;
    dict.set_item("secId", &contract.security_id)?;
    dict.set_item("description", &contract.description)?;
    dict.set_item("issuerId", &contract.issuer_id)?;
    dict.set_item("comboLegsDescrip", &contract.combo_legs_description)?;

    if !contract.combo_legs.is_empty() {
        let combo_legs = PyList::empty(py);
        for leg in &contract.combo_legs {
            let leg_dict = PyDict::new(py);
            leg_dict.set_item("conId", leg.contract_id)?;
            leg_dict.set_item("ratio", leg.ratio)?;
            leg_dict.set_item("action", &leg.action)?;
            leg_dict.set_item("exchange", &leg.exchange)?;
            leg_dict.set_item("openClose", combo_leg_open_close_to_i32(leg.open_close))?;
            leg_dict.set_item("shortSaleSlot", leg.short_sale_slot)?;
            leg_dict.set_item("designatedLocation", &leg.designated_location)?;
            leg_dict.set_item("exemptCode", leg.exempt_code)?;
            combo_legs.append(leg_dict)?;
        }
        dict.set_item("comboLegs", combo_legs)?;
    }

    if let Some(delta_neutral) = &contract.delta_neutral_contract {
        let delta_dict = PyDict::new(py);
        delta_dict.set_item("conId", delta_neutral.contract_id)?;
        delta_dict.set_item("delta", delta_neutral.delta)?;
        delta_dict.set_item("price", delta_neutral.price)?;
        dict.set_item("deltaNeutralContract", delta_dict)?;
    }

    Ok(dict)
}

pub fn contract_details_to_pyobject(
    py: Python<'_>,
    details: &ContractDetails,
) -> PyResult<Py<PyAny>> {
    let common = py.import("nautilus_trader.adapters.interactive_brokers.common")?;
    let dict_to_contract_details = common.getattr("dict_to_contract_details")?;
    let details_dict = PyDict::new(py);

    details_dict.set_item("contract", contract_to_pydict(py, &details.contract)?)?;
    details_dict.set_item("marketName", &details.market_name)?;
    details_dict.set_item("minTick", details.min_tick)?;
    details_dict.set_item("orderTypes", details.order_types.join(","))?;
    details_dict.set_item("validExchanges", details.valid_exchanges.join(","))?;
    details_dict.set_item("priceMagnifier", details.price_magnifier)?;
    details_dict.set_item("underConId", details.under_contract_id)?;
    details_dict.set_item("longName", &details.long_name)?;
    details_dict.set_item("contractMonth", &details.contract_month)?;
    details_dict.set_item("industry", &details.industry)?;
    details_dict.set_item("category", &details.category)?;
    details_dict.set_item("subcategory", &details.subcategory)?;
    details_dict.set_item("timeZoneId", &details.time_zone_id)?;
    details_dict.set_item("tradingHours", details.trading_hours.join(";"))?;
    details_dict.set_item("liquidHours", details.liquid_hours.join(";"))?;
    details_dict.set_item("evRule", &details.ev_rule)?;
    details_dict.set_item("evMultiplier", details.ev_multiplier)?;
    details_dict.set_item("aggGroup", details.agg_group)?;
    details_dict.set_item("underSymbol", &details.under_symbol)?;
    details_dict.set_item("underSecType", &details.under_security_type)?;
    details_dict.set_item("marketRuleIds", details.market_rule_ids.join(","))?;
    details_dict.set_item("realExpirationDate", &details.real_expiration_date)?;
    details_dict.set_item("lastTradeTime", &details.last_trade_time)?;
    details_dict.set_item("stockType", &details.stock_type)?;
    details_dict.set_item("cusip", &details.cusip)?;
    details_dict.set_item("ratings", &details.ratings)?;
    details_dict.set_item("descAppend", &details.desc_append)?;
    details_dict.set_item("bondType", &details.bond_type)?;
    details_dict.set_item("couponType", &details.coupon_type)?;
    details_dict.set_item("callable", details.callable)?;
    details_dict.set_item("putable", details.putable)?;
    details_dict.set_item("coupon", details.coupon)?;
    details_dict.set_item("convertible", details.convertible)?;
    details_dict.set_item("maturity", &details.maturity)?;
    details_dict.set_item("issueDate", &details.issue_date)?;
    details_dict.set_item("nextOptionDate", &details.next_option_date)?;
    details_dict.set_item("nextOptionType", &details.next_option_type)?;
    details_dict.set_item("nextOptionPartial", details.next_option_partial)?;
    details_dict.set_item("notes", &details.notes)?;
    details_dict.set_item("minSize", details.min_size.to_string())?;
    details_dict.set_item("sizeIncrement", details.size_increment.to_string())?;
    details_dict.set_item(
        "suggestedSizeIncrement",
        details.suggested_size_increment.to_string(),
    )?;
    details_dict.set_item("fundName", &details.fund_name)?;
    details_dict.set_item("fundFamily", &details.fund_family)?;
    details_dict.set_item("fundType", &details.fund_type)?;
    details_dict.set_item("fundFrontLoad", &details.fund_front_load)?;
    details_dict.set_item("fundBackLoad", &details.fund_back_load)?;
    details_dict.set_item(
        "fundBackLoadTimeInterval",
        &details.fund_back_load_time_interval,
    )?;
    details_dict.set_item("fundManagementFee", &details.fund_management_fee)?;
    details_dict.set_item("fundClosed", details.fund_closed)?;
    details_dict.set_item(
        "fundClosedForNewInvestors",
        details.fund_closed_for_new_investors,
    )?;
    details_dict.set_item("fundClosedForNewMoney", details.fund_closed_for_new_money)?;
    details_dict.set_item("fundNotifyAmount", &details.fund_notify_amount)?;
    details_dict.set_item(
        "fundMinimumInitialPurchase",
        &details.fund_minimum_initial_purchase,
    )?;
    details_dict.set_item(
        "fundSubsequentMinimumPurchase",
        &details.fund_subsequent_minimum_purchase,
    )?;
    details_dict.set_item("fundBlueSkyStates", &details.fund_blue_sky_states)?;
    details_dict.set_item("fundBlueSkyTerritories", &details.fund_blue_sky_territories)?;

    if !details.sec_id_list.is_empty() {
        let sec_id_list = PyDict::new(py);
        for item in &details.sec_id_list {
            sec_id_list.set_item(&item.tag, &item.value)?;
        }
        details_dict.set_item("secIdList", sec_id_list)?;
    }

    let result = dict_to_contract_details.call1((details_dict,))?;
    Ok(result.unbind())
}
