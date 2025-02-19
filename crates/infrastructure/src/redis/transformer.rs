// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::{AccountAny, CashAccount, MarginAccount},
    enums::{AccountType, AssetClass, CurrencyType, OptionKind},
    events::AccountState,
    identifiers::{AccountId, InstrumentId, Symbol},
    instruments::{
        BettingInstrument, BinaryOption, CryptoFuture, CryptoPerpetual, CurrencyPair, Equity,
        FuturesContract, FuturesSpread, InstrumentAny, OptionContract, OptionSpread,
        SyntheticInstrument,
    },
    orders::OrderAny,
    position::Position,
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use serde_json::{Map, Value};
use ustr::Ustr;
pub struct Transformer;

impl Transformer {
    pub fn currency_from_value(value: Value, code: &Ustr) -> anyhow::Result<Currency> {
        let c_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid currency map"),
        };

        let currency = Currency::new(
            *code,
            c_map["precision"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Invalid precision"))?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Precision out of range"))?,
            c_map["iso4217"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Invalid iso4217"))?
                .try_into()
                .map_err(|_| anyhow::anyhow!("ISO4217 out of range"))?,
            c_map["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid name"))?
                .into(),
            CurrencyType::from_str(
                c_map["currency_type"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid currency_type"))?,
            )?,
        );

        Ok(currency)
    }

    pub fn account_from_value(value: Value) -> anyhow::Result<AccountAny> {
        let a_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid account map"),
        };

        let account_id = AccountId::new(a_map["account_id"].as_str().unwrap());
        let account_type = AccountType::from_str(a_map["account_type"].as_str().unwrap())?;

        let balances = a_map["balances"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| {
                let free = Money::from_str(b["free"].as_str().unwrap()).unwrap();
                let locked = Money::from_str(b["locked"].as_str().unwrap()).unwrap();
                let total = Money::from_str(b["total"].as_str().unwrap()).unwrap();

                AccountBalance::new_checked(total, locked, free)
            })
            .collect::<Result<Vec<AccountBalance>, _>>()?;

        let margins = a_map["margins"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing margins array"))?
            .iter()
            .map(|m| -> anyhow::Result<MarginBalance> {
                let initial = Money::from_str(
                    m["initial"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Missing initial margin"))?,
                )
                .map_err(|e| anyhow::anyhow!("Invalid initial margin: {}", e))?;

                let maintenance = Money::from_str(
                    m["maintenance"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Missing maintenance margin"))?,
                )
                .map_err(|e| anyhow::anyhow!("Invalid maintenance margin: {}", e))?;

                let instrument_id = InstrumentId::from_str(
                    m["instrument_id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Missing instrument_id"))?,
                )?;

                Ok(MarginBalance::new(initial, maintenance, instrument_id))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let is_reported = a_map["reported"].as_bool().unwrap();
        let event_id = a_map["event_id"].as_str().unwrap().to_string();
        let ts_event =
            UnixNanos::from_str(&a_map["ts_event"].as_i64().map(|n| n.to_string()).unwrap())?;
        let ts_init =
            UnixNanos::from_str(&a_map["ts_init"].as_i64().map(|n| n.to_string()).unwrap())?;
        let base_currency = a_map["base_currency"]
            .as_str()
            .map(Currency::from_str)
            .transpose()?;

        let state = AccountState::new(
            account_id,
            account_type,
            balances,
            margins,
            is_reported,
            event_id.into(),
            ts_event,
            ts_init,
            base_currency,
        );

        match account_type {
            AccountType::Cash => Ok(AccountAny::Cash(CashAccount::new(state, true))),
            AccountType::Margin => Ok(AccountAny::Margin(MarginAccount::new(state, true))),
            _ => anyhow::bail!("Betting Account Not Supported"),
        }
    }

    pub fn position_from_value(value: Value) -> anyhow::Result<Position> {
        let _ = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid position map"),
        };

        todo!()
    }

    pub fn order_from_value(_: Value) -> anyhow::Result<OrderAny> {
        todo!()
    }

    pub fn synthetic_from_value(value: Value) -> anyhow::Result<SyntheticInstrument> {
        let s_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid synthetic map"),
        };

        let symbol = Symbol::from_str_unchecked(
            s_map["symbol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing symbol field"))?,
        );

        let price_precision: u8 = s_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let formula = s_map["formula"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing formula field"))?;

        // check this: TODO
        let ts_event = UnixNanos::from_str(
            s_map["ts_event"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing ts_event field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            s_map["ts_init"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing ts_init field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let synthetic: SyntheticInstrument = SyntheticInstrument::new(
            symbol,
            price_precision,
            vec![],
            formula.to_string(),
            ts_event,
            ts_init,
        );

        Ok(synthetic)
    }

    pub fn instrument_from_value(value: Value) -> anyhow::Result<InstrumentAny> {
        let i_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid instrument map"),
        };

        let instrument_type = i_map["type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid instrument type"))?;

        // TODO: verify all with python
        let instrument = match instrument_type {
            "Betting" => InstrumentAny::Betting(Self::betting_from_value(i_map)?),
            "BinaryOption" => InstrumentAny::BinaryOption(Self::binary_option_from_value(i_map)?),
            "CryptoFuture" => InstrumentAny::CryptoFuture(Self::crypto_future_from_value(i_map)?),
            "CryptoPerpetual" => {
                InstrumentAny::CryptoPerpetual(Self::crypto_perpetual_from_value(i_map)?)
            }
            "CurrencyPair" => InstrumentAny::CurrencyPair(Self::currency_pair_from_value(i_map)?),
            "Equity" => InstrumentAny::Equity(Self::equity_from_value(i_map)?),
            "FuturesContract" => {
                InstrumentAny::FuturesContract(Self::futures_contract_from_value(i_map)?)
            }
            "FuturesSpread" => {
                InstrumentAny::FuturesSpread(Self::futures_spread_from_value(i_map)?)
            }
            "OptionContract" => {
                InstrumentAny::OptionContract(Self::option_contract_from_value(i_map)?)
            }
            "OptionSpread" => InstrumentAny::OptionSpread(Self::option_spread_from_value(i_map)?),
            _ => anyhow::bail!("Invalid instrument type"),
        };

        Ok(instrument)
    }

    pub fn betting_from_value(i_map: Map<String, Value>) -> anyhow::Result<BettingInstrument> {
        // id: InstrumentId,
        let id = i_map["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing id field"))?;
        let id = InstrumentId::from_str(id)?;

        // raw_symbol: Symbol,
        let raw_symbol = i_map["raw_symbol"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?;
        let raw_symbol = Symbol::from_str_unchecked(raw_symbol);

        // event_type_id: u64,
        let event_type_id = i_map["event_type_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing event_type_id field"))?;

        // event_type_name: Ustr,
        let event_type_name = i_map["event_type_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing event_type_name field"))?;
        let event_type_name = Ustr::from_str(event_type_name)?;

        // competition_id: u64,
        let competition_id = i_map["competition_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing competition_id field"))?;

        // competition_name: Ustr,
        let competition_name = i_map["competition_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing competition_name field"))?;
        let competition_name = Ustr::from_str(competition_name)?;

        // event_id: u64,
        let event_id = i_map["event_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?;

        // event_name: Ustr,
        let event_name = i_map["event_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing event_name field"))?;
        let event_name = Ustr::from_str(event_name)?;

        // event_country_code: Ustr,
        let event_country_code = i_map["event_country_code"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing event_country_code field"))?;
        let event_country_code = Ustr::from_str(event_country_code)?;

        // event_open_date: UnixNanos,
        let event_open_date = i_map["event_open_date"]
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".to_string());
        let event_open_date = UnixNanos::from_str(&event_open_date)
            .map_err(|e| anyhow::anyhow!("Invalid event_open_date: {}", e))?;

        // betting_type: Ustr,
        let betting_type = i_map["betting_type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing betting_type field"))?;
        let betting_type = Ustr::from_str(betting_type)?;

        // market_id: Ustr,
        let market_id = i_map["market_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing market_id field"))?;
        let market_id = Ustr::from_str(market_id)?;

        // market_name: Ustr,
        let market_name = i_map["market_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing market_name field"))?;
        let market_name = Ustr::from_str(market_name)?;

        // market_type: Ustr,
        let market_type = i_map["market_type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing market_type field"))?;
        let market_type = Ustr::from_str(market_type)?;

        // market_start_time: UnixNanos,
        let market_start_time = i_map["market_start_time"]
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".to_string());
        let market_start_time = UnixNanos::from_str(&market_start_time)
            .map_err(|e| anyhow::anyhow!("Invalid market_start_time: {}", e))?;

        // selection_id: u64,
        let selection_id = i_map["selection_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing selection_id field"))?;

        // selection_name: Ustr,
        let selection_name = i_map["selection_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing selection_name field"))?;
        let selection_name = Ustr::from_str(selection_name)?;

        // selection_handicap: f64,
        let selection_handicap = i_map["selection_handicap"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("Missing selection_handicap field"))?;

        // currency: Currency,
        let currency = i_map["currency"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?;
        let currency = Currency::from_str(currency)?;

        // price_precision: u8,
        let price_precision = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?;
        let price_precision: u8 = price_precision
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;
        // size_precision: u8,
        let size_precision = i_map["size_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing size_precision field"))?;
        let size_precision: u8 = size_precision
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;

        // price_increment: Price,
        let price_increment = i_map["price_increment"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?;
        let price_increment = Price::from_str(price_increment).unwrap();

        // size_increment: Quantity,
        let size_increment = i_map["size_increment"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?;
        let size_increment = Quantity::from_str(size_increment)
            .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;

        // max_quantity: Option<Quantity>,
        let max_quantity = i_map["max_quantity"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing max_quantity field"))?;
        let max_quantity = Quantity::from_str(max_quantity)
            .map_err(|e| anyhow::anyhow!("Invalid max_quantity: {}", e))?;

        // min_quantity: Option<Quantity>,
        let min_quantity = i_map["min_quantity"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing min_quantity field"))?;
        let min_quantity = Quantity::from_str(min_quantity)
            .map_err(|e| anyhow::anyhow!("Invalid min_quantity: {}", e))?;

        // max_notional: Option<Money>,
        // TODO: For Later: make this Optional
        let max_notional = i_map["max_notional"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing max_notional field"))?;
        let max_notional = Money::from_str(max_notional)
            .map_err(|e| anyhow::anyhow!("Invalid max_notional: {}", e))?;

        // min_notional: Option<Money>,
        // TODO: For Later: make this Optional
        let min_notional = i_map["min_notional"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing min_notional field"))?;
        let min_notional = Money::from_str(min_notional)
            .map_err(|e| anyhow::anyhow!("Invalid min_notional: {}", e))?;

        // max_price: Option<Price>,
        // TODO: For Later: make this Optional
        let max_price = i_map["max_price"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing max_price field"))?;
        let max_price = Price::from_str(max_price).unwrap();

        // min_price: Option<Price>,
        let min_price = i_map["min_price"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing min_price field"))?;
        let min_price = Price::from_str(min_price).unwrap();

        // margin_init: Option<Decimal>,
        let margin_init = i_map["margin_init"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing margin_init field"))?;
        let margin_init = Decimal::from_str(margin_init)
            .map_err(|e| anyhow::anyhow!("Invalid margin_init: {}", e))?;

        // margin_maint: Option<Decimal>,
        let margin_maint = i_map["margin_maint"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing margin_maint field"))?;
        let margin_maint = Decimal::from_str(margin_maint)
            .map_err(|e| anyhow::anyhow!("Invalid margin_maint: {}", e))?;

        // maker_fee: Option<Decimal>,
        // TODO: For Later: make this Optional
        let maker_fee = i_map["maker_fee"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing maker_fee field"))?;
        let maker_fee = Decimal::from_str(maker_fee)
            .map_err(|e| anyhow::anyhow!("Invalid maker_fee: {}", e))?;

        // taker_fee: Option<Decimal>,
        // TODO: For Later: make this Optional
        let taker_fee = i_map["taker_fee"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing taker_fee field"))?;
        let taker_fee = Decimal::from_str(taker_fee)
            .map_err(|e| anyhow::anyhow!("Invalid taker_fee: {}", e))?;
        // ts_event: UnixNanos,
        let ts_event = i_map["ts_event"]
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".to_string());
        let ts_event = UnixNanos::from_str(&ts_event)
            .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        // ts_init: UnixNanos,
        let ts_init = i_map["ts_init"]
            .as_i64()
            .map(|n| n.to_string())
            .ok_or_else(|| anyhow::anyhow!("Missing ts_init field"))?;
        let ts_init =
            UnixNanos::from_str(&ts_init).map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = BettingInstrument::new(
            id,
            raw_symbol,
            event_type_id,
            event_type_name,
            competition_id,
            competition_name,
            event_id,
            event_name,
            event_country_code,
            event_open_date,
            betting_type,
            market_id,
            market_name,
            market_type,
            market_start_time,
            selection_id,
            selection_name,
            selection_handicap,
            currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            Some(max_quantity),
            Some(min_quantity),
            Some(max_notional),
            Some(min_notional),
            Some(max_price),
            Some(min_price),
            Some(margin_init),
            Some(margin_maint),
            Some(maker_fee),
            Some(taker_fee),
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }

    pub fn binary_option_from_value(i_map: Map<String, Value>) -> anyhow::Result<BinaryOption> {
        let id = InstrumentId::from_str(
            i_map["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map["raw_symbol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let asset_class = AssetClass::from_str(
            i_map["asset_class"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;

        let currency = Currency::from_str(
            i_map["currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;

        let activation_ns = UnixNanos::from_str(
            &i_map["activation_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;

        let expiration_ns = UnixNanos::from_str(
            &i_map["expiration_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;

        let price_precision: u8 = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let size_precision: u8 = i_map["size_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing size_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map["price_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .unwrap();

        let size_increment = Quantity::from_str(
            i_map["size_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;

        let margin_init = i_map["margin_init"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let margin_maint = i_map["margin_maint"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let maker_fee = i_map["maker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let taker_fee = i_map["taker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let outcome = i_map["outcome"]
            .as_str()
            .map(|value| Ustr::from_str(value).unwrap());

        let description = i_map["description"]
            .as_str()
            .map(|value| Ustr::from_str(value).unwrap());

        let max_quantity = i_map["max_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let min_quantity = i_map["min_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_notional = i_map["max_notional"]
            .as_str()
            .map(|value| Money::from_str(value).unwrap());

        let min_notional = i_map["min_notional"]
            .as_str()
            .map(|value| Money::from_str(value).unwrap());

        let max_price = i_map["max_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());
        let min_price = i_map["min_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let ts_event = i_map["ts_event"]
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".to_string());
        let ts_event = UnixNanos::from_str(&ts_event)
            .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = i_map["ts_init"]
            .as_i64()
            .map(|n| n.to_string())
            .ok_or_else(|| anyhow::anyhow!("Missing ts_init field"))?;
        let ts_init =
            UnixNanos::from_str(&ts_init).map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = BinaryOption::new(
            id,
            raw_symbol,
            asset_class,
            currency,
            activation_ns,
            expiration_ns,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            outcome,
            description,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }

    pub fn crypto_future_from_value(i_map: Map<String, Value>) -> anyhow::Result<CryptoFuture> {
        let id = InstrumentId::from_str(
            i_map["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map["raw_symbol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let underlying = Currency::from_str(
            i_map["underlying"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;

        let quote_currency = Currency::from_str(
            i_map["quote_currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing quote_currency field"))?,
        )?;

        let settlement_currency = Currency::from_str(
            i_map["settlement_currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing settlement_currency field"))?,
        )?;

        let is_inverse = i_map["is_inverse"]
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("Missing is_inverse field"))?;

        // TODO
        let activation_ns = UnixNanos::from_str(
            &i_map["activation_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;

        let expiration_ns = UnixNanos::from_str(
            &i_map["expiration_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;

        let price_precision: u8 = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let size_precision: u8 = i_map["size_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing size_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map["price_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .unwrap();

        let size_increment = Quantity::from_str(
            i_map["size_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;

        let multiplier = i_map["multiplier"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());
        let lot_size = i_map["lot_size"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_quantity = i_map["max_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());
        let min_quantity = i_map["min_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_notional = i_map["max_notional"]
            .as_str()
            .map(|value| Money::from_str(value).unwrap());
        let min_notional = i_map["min_notional"]
            .as_str()
            .map(|value| Money::from_str(value).unwrap());

        let max_price = i_map["max_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());
        let min_price = i_map["min_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let margin_init = i_map["margin_init"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());
        let margin_maint = i_map["margin_maint"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let maker_fee = i_map["maker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());
        let taker_fee = i_map["taker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let ts_event = UnixNanos::from_str(
            &i_map["ts_event"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map["ts_init"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = CryptoFuture::new(
            id,
            raw_symbol,
            underlying,
            quote_currency,
            settlement_currency,
            is_inverse,
            activation_ns,
            expiration_ns,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }

    pub fn crypto_perpetual_from_value(
        i_map: Map<String, Value>,
    ) -> anyhow::Result<CryptoPerpetual> {
        let id = InstrumentId::from_str(
            i_map["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map["raw_symbol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let base_currency = Currency::from_str(
            i_map["base_currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing base_currency field"))?,
        )?;

        let quote_currency = Currency::from_str(
            i_map["quote_currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing quote_currency field"))?,
        )?;

        let settlement_currency = Currency::from_str(
            i_map["settlement_currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing settlement_currency field"))?,
        )?;

        let is_inverse = i_map["is_inverse"]
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("Missing is_inverse field"))?;

        let price_precision: u8 = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let size_precision: u8 = i_map["size_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing size_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map["price_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .unwrap();

        let size_increment = Quantity::from_str(
            i_map["size_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;

        let multiplier = i_map["multiplier"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let lot_size = i_map["lot_size"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_quantity = i_map["max_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let min_quantity = i_map["min_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_notional = i_map["max_notional"]
            .as_str()
            .map(|value| Money::from_str(value).unwrap());

        let min_notional = i_map["min_notional"]
            .as_str()
            .map(|value| Money::from_str(value).unwrap());

        let max_price = i_map["max_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let min_price = i_map["min_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let margin_init = i_map["margin_init"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let margin_maint = i_map["margin_maint"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let maker_fee = i_map["maker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let taker_fee = i_map["taker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let ts_event = UnixNanos::from_str(
            &i_map["ts_event"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map["ts_init"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = CryptoPerpetual::new(
            id,
            raw_symbol,
            base_currency,
            quote_currency,
            settlement_currency,
            is_inverse,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }

    pub fn futures_contract_from_value(
        i_map: Map<String, Value>,
    ) -> anyhow::Result<FuturesContract> {
        let id = InstrumentId::from_str(
            i_map["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map["raw_symbol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let asset_class = AssetClass::from_str(
            i_map["asset_class"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;

        let exchange = i_map["exchange"]
            .as_str()
            .map(|value| Ustr::from_str(value).unwrap());

        let underlying = Ustr::from_str(
            i_map["underlying"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;

        let activation_ns = UnixNanos::from_str(
            &i_map["activation_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;

        let expiration_ns = UnixNanos::from_str(
            &i_map["expiration_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;

        let currency = Currency::from_str(
            i_map["currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;

        let price_precision: u8 = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map["price_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;

        let multiplier = Quantity::from_str(
            i_map["multiplier"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing multiplier field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))?;

        let lot_size = Quantity::from_str(
            i_map["lot_size"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing lot_size field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))?;

        let max_quantity = i_map["max_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let min_quantity = i_map["min_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_price = i_map["max_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let min_price = i_map["min_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let margin_init = i_map["margin_init"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let margin_maint = i_map["margin_maint"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let maker_fee = i_map["maker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let taker_fee = i_map["taker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let ts_event = UnixNanos::from_str(
            &i_map["ts_event"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map["ts_init"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = FuturesContract::new(
            id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            activation_ns,
            expiration_ns,
            currency,
            price_precision,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }

    pub fn futures_spread_from_value(i_map: Map<String, Value>) -> anyhow::Result<FuturesSpread> {
        let id = InstrumentId::from_str(
            i_map["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map["raw_symbol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let asset_class = AssetClass::from_str(
            i_map["asset_class"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;

        let exchange = i_map["exchange"]
            .as_str()
            .map(|value| Ustr::from_str(value).unwrap());

        let underlying = Ustr::from_str(
            i_map["underlying"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;

        let strategy_type = Ustr::from_str(
            i_map["strategy_type"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_type field"))?,
        )?;

        let activation_ns = UnixNanos::from_str(
            &i_map["activation_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;

        let expiration_ns = UnixNanos::from_str(
            &i_map["expiration_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;

        let currency = Currency::from_str(
            i_map["currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;

        let price_precision: u8 = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map["price_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;

        let multiplier = Quantity::from_str(
            i_map["multiplier"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing multiplier field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))?;

        let lot_size = Quantity::from_str(
            i_map["lot_size"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing lot_size field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))?;

        let max_quantity = i_map["max_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let min_quantity = i_map["min_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_price = i_map["max_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let min_price = i_map["min_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let margin_init = i_map["margin_init"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let margin_maint = i_map["margin_maint"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let maker_fee = i_map["maker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let taker_fee = i_map["taker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let ts_event = UnixNanos::from_str(
            &i_map["ts_event"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map["ts_init"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = FuturesSpread::new(
            id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            strategy_type,
            activation_ns,
            expiration_ns,
            currency,
            price_precision,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }

    pub fn option_contract_from_value(i_map: Map<String, Value>) -> anyhow::Result<OptionContract> {
        let id = InstrumentId::from_str(
            i_map["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map["raw_symbol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let asset_class = AssetClass::from_str(
            i_map["asset_class"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;

        let exchange = i_map["exchange"]
            .as_str()
            .map(|value| Ustr::from_str(value).unwrap());

        let underlying = Ustr::from_str(
            i_map["underlying"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;

        let option_kind = OptionKind::from_str(
            i_map["option_kind"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing option_kind field"))?,
        )?;

        let strike_price = Price::from_str(
            i_map["strike_price"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing strike_price field"))?,
        )
        .unwrap();

        let currency = Currency::from_str(
            i_map["currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;

        let activation_ns = UnixNanos::from_str(
            &i_map["activation_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;

        let expiration_ns = UnixNanos::from_str(
            &i_map["expiration_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;

        let price_precision: u8 = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map["price_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;

        let multiplier = Quantity::from_str(
            i_map["multiplier"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing multiplier field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))?;

        let lot_size = Quantity::from_str(
            i_map["lot_size"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing lot_size field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))?;

        let max_quantity = i_map["max_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let min_quantity = i_map["min_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_price = i_map["max_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let min_price = i_map["min_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let margin_init = i_map["margin_init"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let margin_maint = i_map["margin_maint"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let maker_fee = i_map["maker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let taker_fee = i_map["taker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let ts_event = UnixNanos::from_str(
            &i_map["ts_event"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map["ts_init"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = OptionContract::new(
            id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            option_kind,
            strike_price,
            currency,
            activation_ns,
            expiration_ns,
            price_precision,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }

    pub fn option_spread_from_value(i_map: Map<String, Value>) -> anyhow::Result<OptionSpread> {
        // id: InstrumentId,
        let id = InstrumentId::from_str(
            i_map["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        // raw_symbol: Symbol,
        let raw_symbol = Symbol::from_str_unchecked(
            i_map["raw_symbol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        // asset_class: AssetClass,
        let asset_class = AssetClass::from_str(
            i_map["asset_class"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;

        // exchange: Option<Ustr>,
        let exchange = i_map["exchange"]
            .as_str()
            .map(|value| Ustr::from_str(value).unwrap());

        // underlying: Ustr,
        let underlying = Ustr::from_str(
            i_map["underlying"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;
        // strategy_type: Ustr,
        let strategy_type = Ustr::from_str(
            i_map["strategy_type"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_type field"))?,
        )?;

        // activation_ns: UnixNanos,
        let activation_ns = UnixNanos::from_str(
            &i_map["activation_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;
        // expiration_ns: UnixNanos,
        let expiration_ns = UnixNanos::from_str(
            &i_map["expiration_ns"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;
        // currency: Currency,
        let currency = Currency::from_str(
            i_map["currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;
        // price_precision: u8,
        let price_precision: u8 = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;
        // price_increment: Price,
        let price_increment = Price::from_str(
            i_map["price_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .unwrap();
        // multiplier: Quantity,
        let multiplier = Quantity::from_str(
            i_map["multiplier"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing multiplier field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))?;
        // lot_size: Quantity,
        let lot_size = Quantity::from_str(
            i_map["lot_size"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing lot_size field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))?;
        // max_quantity: Option<Quantity>,
        let max_quantity = i_map["max_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());
        // min_quantity: Option<Quantity>,
        let min_quantity = i_map["min_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());
        // max_price: Option<Price>,
        let max_price = i_map["max_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());
        // min_price: Option<Price>,
        let min_price = i_map["min_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());
        // margin_init: Option<Decimal>,
        let margin_init = i_map["margin_init"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());
        // margin_maint: Option<Decimal>,
        let margin_maint = i_map["margin_maint"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());
        // maker_fee: Option<Decimal>,
        let maker_fee = i_map["maker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());
        // taker_fee: Option<Decimal>,
        let taker_fee = i_map["taker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());
        // ts_event: UnixNanos,
        let ts_event = UnixNanos::from_str(
            &i_map["ts_event"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;
        // ts_init: UnixNanos,
        let ts_init = UnixNanos::from_str(
            &i_map["ts_init"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = OptionSpread::new(
            id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            strategy_type,
            activation_ns,
            expiration_ns,
            currency,
            price_precision,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }

    pub fn equity_from_value(i_map: Map<String, Value>) -> anyhow::Result<Equity> {
        let id = InstrumentId::from_str(
            i_map["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map["raw_symbol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let isin = i_map["isin"]
            .as_str()
            .map(|value| Ustr::from_str(value).unwrap());

        let currency = Currency::from_str(
            i_map["currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;

        let price_precision: u8 = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map["price_increment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .unwrap();

        let lot_size = i_map["lot_size"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_quantity = i_map["max_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let min_quantity = i_map["min_quantity"]
            .as_str()
            .map(|value| Quantity::from_str(value).unwrap());

        let max_price = i_map["max_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let min_price = i_map["min_price"]
            .as_str()
            .map(|value| Price::from_str(value).unwrap());

        let margin_init = i_map["margin_init"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let margin_maint = i_map["margin_maint"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let maker_fee = i_map["maker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let taker_fee = i_map["taker_fee"]
            .as_str()
            .map(|value| Decimal::from_str(value).unwrap());

        let ts_event = UnixNanos::from_str(
            &i_map["ts_event"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map["ts_init"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = Equity::new(
            id,
            raw_symbol,
            isin,
            currency,
            price_precision,
            price_increment,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }

    pub fn currency_pair_from_value(i_map: Map<String, Value>) -> anyhow::Result<CurrencyPair> {
        let id = i_map["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing id field"))?;
        let id = InstrumentId::from_str(id)?;

        let raw_symbol = i_map["raw_symbol"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?;
        let raw_symbol = Symbol::from_str_unchecked(raw_symbol);

        let base_currency = i_map["base_currency"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing base_currency field"))?;
        let base_currency = Currency::from_str(base_currency)?;

        let quote_currency = i_map["quote_currency"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing quote_currency field"))?;
        let quote_currency = Currency::from_str(quote_currency)?;

        let price_precision = i_map["price_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid price_precision"))?;
        let price_precision: u8 = price_precision
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let size_precision = i_map["size_precision"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid size_precision"))?;
        let size_precision: u8 = size_precision
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;

        let price_increment = i_map["price_increment"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?;
        let price_increment = Price::from_str(price_increment).unwrap();

        let size_increment = i_map["size_increment"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?;
        let size_increment = Quantity::from_str(size_increment)
            .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;

        let lot_size = i_map["lot_size"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing lot_size field"))?;
        let lot_size =
            Quantity::from_str(lot_size).map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))?;

        let max_quantity = i_map["max_quantity"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing max_quantity field"))?;
        let max_quantity = Quantity::from_str(max_quantity)
            .map_err(|e| anyhow::anyhow!("Invalid max_quantity: {}", e))?;

        let min_quantity = i_map["min_quantity"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing min_quantity field"))?;
        let min_quantity = Quantity::from_str(min_quantity)
            .map_err(|e| anyhow::anyhow!("Invalid min_quantity: {}", e))?;

        let max_notional = i_map["max_notional"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing max_notional field"))?;
        let max_notional = Money::from_str(max_notional)
            .map_err(|e| anyhow::anyhow!("Invalid max_notional: {}", e))?;

        let min_notional = i_map["min_notional"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing min_notional field"))?;
        let min_notional = Money::from_str(min_notional)
            .map_err(|e| anyhow::anyhow!("Invalid min_notional: {}", e))?;

        let max_price = i_map["max_price"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing max_price field"))?;
        let max_price = Price::from_str(max_price).unwrap();

        let min_price = i_map["min_price"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing min_price field"))?;
        let min_price = Price::from_str(min_price).unwrap();

        let margin_init = i_map["margin_init"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing margin_init field"))?;
        let margin_init = Decimal::from_str(margin_init)
            .map_err(|e| anyhow::anyhow!("Invalid margin_init: {}", e))?;

        let margin_maint = i_map["margin_maint"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing margin_maint field"))?;
        let margin_maint = Decimal::from_str(margin_maint)
            .map_err(|e| anyhow::anyhow!("Invalid margin_maint: {}", e))?;

        let maker_fee = i_map["maker_fee"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing maker_fee field"))?;
        let maker_fee = Decimal::from_str(maker_fee)
            .map_err(|e| anyhow::anyhow!("Invalid maker_fee: {}", e))?;

        let taker_fee = i_map["taker_fee"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing taker_fee field"))?;
        let taker_fee = Decimal::from_str(taker_fee)
            .map_err(|e| anyhow::anyhow!("Invalid taker_fee: {}", e))?;

        let ts_event = i_map["ts_event"]
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".to_string());
        let ts_event = UnixNanos::from_str(&ts_event)
            .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = i_map["ts_init"]
            .as_i64()
            .map(|n| n.to_string())
            .ok_or_else(|| anyhow::anyhow!("Missing ts_init field"))?;
        let ts_init =
            UnixNanos::from_str(&ts_init).map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = CurrencyPair::new(
            id,
            raw_symbol,
            base_currency,
            quote_currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            Some(lot_size),
            Some(max_quantity),
            Some(min_quantity),
            Some(max_notional),
            Some(min_notional),
            Some(max_price),
            Some(min_price),
            Some(margin_init),
            Some(margin_maint),
            Some(maker_fee),
            Some(taker_fee),
            ts_event,
            ts_init,
        );

        Ok(instrument)
    }
}
