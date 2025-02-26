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

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::{AccountAny, CashAccount, MarginAccount},
    enums::{
        AccountType, AssetClass, ContingencyType, CurrencyType, LiquiditySide, OptionKind,
        OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEmulated, OrderEventAny, OrderExpired, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSubmitted, OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, Symbol, TradeId, TraderId, VenueOrderId,
    },
    instruments::{
        BettingInstrument, BinaryOption, CryptoFuture, CryptoPerpetual, CurrencyPair, Equity,
        FuturesContract, FuturesSpread, InstrumentAny, OptionContract, OptionSpread,
        SyntheticInstrument,
    },
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
            c_map
                .get("precision")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Invalid precision"))?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Precision out of range"))?,
            c_map
                .get("iso4217")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Invalid iso4217"))?
                .try_into()
                .map_err(|_| anyhow::anyhow!("ISO4217 out of range"))?,
            c_map
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Invalid name"))?
                .into(),
            CurrencyType::from_str(
                c_map
                    .get("currency_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Invalid currency_type"))?,
            )?,
        );

        Ok(currency)
    }

    pub fn instrument_from_value(value: Value) -> anyhow::Result<InstrumentAny> {
        let i_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid instrument map"),
        };

        let instrument_type = i_map
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid instrument type"))?;

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
        let id = InstrumentId::from_str(
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;
        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );
        let event_type_id = i_map
            .get("event_type_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing event_type_id field"))?;
        let event_type_name = Ustr::from_str(
            i_map
                .get("event_type_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_type_name field"))?,
        )?;
        let competition_id = i_map
            .get("competition_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing competition_id field"))?;
        let competition_name = Ustr::from_str(
            i_map
                .get("competition_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing competition_name field"))?,
        )?;
        let event_id = i_map
            .get("event_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?;
        let event_name = Ustr::from_str(
            i_map
                .get("event_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_name field"))?,
        )?;
        let event_country_code = Ustr::from_str(
            i_map
                .get("event_country_code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_country_code field"))?,
        )?;
        let event_open_date = UnixNanos::from_str(
            i_map
                .get("event_open_date")
                .and_then(|v| v.as_str())
                .unwrap_or("0"),
        )
        .map_err(|e| anyhow::anyhow!("Invalid event_open_date: {}", e))?;
        let betting_type = Ustr::from_str(
            i_map
                .get("betting_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing betting_type field"))?,
        )?;
        let market_id = Ustr::from_str(
            i_map
                .get("market_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing market_id field"))?,
        )?;
        let market_name = Ustr::from_str(
            i_map
                .get("market_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing market_name field"))?,
        )?;
        let market_type = Ustr::from_str(
            i_map
                .get("market_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing market_type field"))?,
        )?;
        let market_start_time = UnixNanos::from_str(
            i_map
                .get("market_start_time")
                .and_then(|v| v.as_str())
                .unwrap_or("0"),
        )
        .map_err(|e| anyhow::anyhow!("Invalid market_start_time: {}", e))?;
        let selection_id = i_map
            .get("selection_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing selection_id field"))?;
        let selection_name = Ustr::from_str(
            i_map
                .get("selection_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing selection_name field"))?,
        )?;
        let selection_handicap = i_map
            .get("selection_handicap")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing selection_handicap field"))?;
        let currency = Currency::from_str(
            i_map
                .get("currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;
        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;
        let size_precision: u8 = i_map
            .get("size_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing size_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;
        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;
        let size_increment = Quantity::from_str(
            i_map
                .get("size_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;
        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| {
                            tracing::error!("Invalid max_quantity: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid min_quantity: {}", e))
                        .ok()
                },
            );
        let max_notional = i_map
            .get("max_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_notional field");
                    None
                },
                |value| {
                    Money::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid max_notional: {}", e))
                        .ok()
                },
            );
        let min_notional = i_map
            .get("min_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_notional field");
                    None
                },
                |value| {
                    Money::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid min_notional: {}", e))
                        .ok()
                },
            );
        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid max_price: {}", e))
                    .ok()
            },
        );
        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid min_price: {}", e))
                    .ok()
            },
        );
        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid margin_init: {}", e))
                        .ok()
                },
            );
        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid margin_maint: {}", e))
                        .ok()
                },
            );
        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid maker_fee: {}", e))
                    .ok()
            },
        );
        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid taker_fee: {}", e))
                    .ok()
            },
        );
        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;
        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

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

    pub fn binary_option_from_value(i_map: Map<String, Value>) -> anyhow::Result<BinaryOption> {
        let id = InstrumentId::from_str(
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;
        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );
        let asset_class = AssetClass::from_str(
            i_map
                .get("asset_class")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;
        let currency = Currency::from_str(
            i_map
                .get("currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;
        let activation_ns = UnixNanos::from_str(
            &i_map
                .get("activation_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;
        let expiration_ns = UnixNanos::from_str(
            &i_map
                .get("expiration_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;
        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;
        let size_precision: u8 = i_map
            .get("size_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing size_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;
        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;
        let size_increment = Quantity::from_str(
            i_map
                .get("size_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;
        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid margin_init: {}", e))
                        .ok()
                },
            );
        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid margin_maint: {}", e))
                        .ok()
                },
            );
        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid maker_fee: {}", e))
                    .ok()
            },
        );
        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid taker_fee: {}", e))
                    .ok()
            },
        );
        let outcome = i_map.get("outcome").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing outcome field");
                None
            },
            |value| {
                Ustr::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid outcome: {}", e))
                    .ok()
            },
        );
        let description = i_map
            .get("description")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing description field");
                    None
                },
                |value| {
                    Ustr::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid description: {}", e))
                        .ok()
                },
            );
        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid max_quantity: {}", e))
                        .ok()
                },
            );
        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid min_quantity: {}", e))
                        .ok()
                },
            );
        let max_notional = i_map
            .get("max_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_notional field");
                    None
                },
                |value| {
                    Money::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid max_notional: {}", e))
                        .ok()
                },
            );
        let min_notional = i_map
            .get("min_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_notional field");
                    None
                },
                |value| {
                    Money::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid min_notional: {}", e))
                        .ok()
                },
            );
        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid max_price: {}", e))
                    .ok()
            },
        );
        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid min_price: {}", e))
                    .ok()
            },
        );
        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;
        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

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
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;
        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );
        let underlying = Currency::from_str(
            i_map
                .get("underlying")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;
        let quote_currency = Currency::from_str(
            i_map
                .get("quote_currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing quote_currency field"))?,
        )?;
        let settlement_currency = Currency::from_str(
            i_map
                .get("settlement_currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing settlement_currency field"))?,
        )?;
        let is_inverse = i_map
            .get("is_inverse")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing is_inverse field"))?;

        let activation_ns = UnixNanos::from_str(
            &i_map
                .get("activation_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;
        let expiration_ns = UnixNanos::from_str(
            &i_map
                .get("expiration_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;
        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;
        let size_precision: u8 = i_map
            .get("size_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing size_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;
        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;
        let size_increment = Quantity::from_str(
            i_map
                .get("size_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;
        let multiplier = i_map
            .get("multiplier")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing multiplier field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))
                        .ok()
                },
            );
        let lot_size = i_map.get("lot_size").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing lot_size field");
                None
            },
            |value| {
                Quantity::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))
                    .ok()
            },
        );
        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid max_quantity: {}", e))
                        .ok()
                },
            );
        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid min_quantity: {}", e))
                        .ok()
                },
            );
        let max_notional = i_map
            .get("max_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_notional field");
                    None
                },
                |value| {
                    Money::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid max_notional: {}", e))
                        .ok()
                },
            );
        let min_notional = i_map
            .get("min_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_notional field");
                    None
                },
                |value| {
                    Money::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid min_notional: {}", e))
                        .ok()
                },
            );
        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid max_price: {}", e))
                    .ok()
            },
        );
        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid min_price: {}", e))
                    .ok()
            },
        );
        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid margin_init: {}", e))
                        .ok()
                },
            );
        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid margin_maint: {}", e))
                        .ok()
                },
            );
        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid maker_fee: {}", e))
                    .ok()
            },
        );
        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid taker_fee: {}", e))
                    .ok()
            },
        );
        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;
        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
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
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;
        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );
        let base_currency = Currency::from_str(
            i_map
                .get("base_currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing base_currency field"))?,
        )?;
        let quote_currency = Currency::from_str(
            i_map
                .get("quote_currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing quote_currency field"))?,
        )?;
        let settlement_currency = Currency::from_str(
            i_map
                .get("settlement_currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing settlement_currency field"))?,
        )?;
        let is_inverse = i_map
            .get("is_inverse")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing is_inverse field"))?;
        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;
        let size_precision: u8 = i_map
            .get("size_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing size_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;
        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;
        let size_increment = Quantity::from_str(
            i_map
                .get("size_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;
        let multiplier = i_map
            .get("multiplier")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing multiplier field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))
                        .ok()
                },
            );
        let lot_size = i_map.get("lot_size").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing lot_size field");
                None
            },
            |value| {
                Quantity::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))
                    .ok()
            },
        );
        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid max_quantity: {}", e))
                        .ok()
                },
            );
        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| {
                    Quantity::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid min_quantity: {}", e))
                        .ok()
                },
            );
        let max_notional = i_map
            .get("max_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_notional field");
                    None
                },
                |value| {
                    Money::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid max_notional: {}", e))
                        .ok()
                },
            );
        let min_notional = i_map
            .get("min_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_notional field");
                    None
                },
                |value| {
                    Money::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid min_notional: {}", e))
                        .ok()
                },
            );
        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid max_price: {}", e))
                    .ok()
            },
        );
        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid min_price: {}", e))
                    .ok()
            },
        );
        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid margin_init: {}", e))
                        .ok()
                },
            );
        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid margin_maint: {}", e))
                        .ok()
                },
            );
        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid maker_fee: {}", e))
                    .ok()
            },
        );
        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid taker_fee: {}", e))
                    .ok()
            },
        );
        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;
        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
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

    pub fn currency_pair_from_value(i_map: Map<String, Value>) -> anyhow::Result<CurrencyPair> {
        let id = InstrumentId::from_str(
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;
        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );
        let base_currency = Currency::from_str(
            i_map
                .get("base_currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing base_currency field"))?,
        )?;
        let quote_currency = Currency::from_str(
            i_map
                .get("quote_currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing quote_currency field"))?,
        )?;
        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid price_precision"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;
        let size_precision: u8 = i_map
            .get("size_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid size_precision"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Size precision out of range for u8"))?;
        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;
        let size_increment = Quantity::from_str(
            i_map
                .get("size_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing size_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid size_increment: {}", e))?;
        let lot_size = i_map.get("lot_size").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing lot_size field");
                None
            },
            |value| Quantity::from_str(value).ok(),
        );
        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );
        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );
        let max_notional = i_map
            .get("max_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_notional field");
                    None
                },
                |value| Money::from_str(value).ok(),
            );
        let min_notional = i_map
            .get("min_notional")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_notional field");
                    None
                },
                |value| Money::from_str(value).ok(),
            );
        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );
        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );
        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );
        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );
        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );
        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );
        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;
        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let instrument = CurrencyPair::new(
            id,
            raw_symbol,
            base_currency,
            quote_currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
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

    pub fn equity_from_value(i_map: Map<String, Value>) -> anyhow::Result<Equity> {
        let id = InstrumentId::from_str(
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;
        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );
        let isin = i_map.get("isin").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing isin field");
                None
            },
            |value| Ustr::from_str(value).ok(),
        );
        let currency = Currency::from_str(
            i_map
                .get("currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;
        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;
        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;
        let lot_size = i_map.get("lot_size").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing lot_size field");
                None
            },
            |value| Quantity::from_str(value).ok(),
        );
        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );
        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );
        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );
        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );
        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );
        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );
        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );
        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );
        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;
        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
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

    pub fn futures_contract_from_value(
        i_map: Map<String, Value>,
    ) -> anyhow::Result<FuturesContract> {
        let id = InstrumentId::from_str(
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let asset_class = AssetClass::from_str(
            i_map
                .get("asset_class")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;

        let exchange = i_map.get("exchange").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing exchange field");
                None
            },
            |value| Ustr::from_str(value).ok(),
        );

        let underlying = Ustr::from_str(
            i_map
                .get("underlying")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;

        let activation_ns = UnixNanos::from_str(
            &i_map
                .get("activation_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;

        let expiration_ns = UnixNanos::from_str(
            &i_map
                .get("expiration_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;

        let currency = Currency::from_str(
            i_map
                .get("currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;

        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;

        let multiplier = Quantity::from_str(
            i_map
                .get("multiplier")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing multiplier field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))?;

        let lot_size = Quantity::from_str(
            i_map
                .get("lot_size")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing lot_size field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))?;

        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );

        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );

        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );

        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );

        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );

        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );

        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );

        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );

        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
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
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let asset_class = AssetClass::from_str(
            i_map
                .get("asset_class")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;

        let exchange = i_map.get("exchange").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing exchange field");
                None
            },
            |value| Ustr::from_str(value).ok(),
        );

        let underlying = Ustr::from_str(
            i_map
                .get("underlying")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;

        let strategy_type = Ustr::from_str(
            i_map
                .get("strategy_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_type field"))?,
        )?;

        let activation_ns = UnixNanos::from_str(
            &i_map
                .get("activation_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;

        let expiration_ns = UnixNanos::from_str(
            &i_map
                .get("expiration_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;

        let currency = Currency::from_str(
            i_map
                .get("currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;

        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;

        let multiplier = Quantity::from_str(
            i_map
                .get("multiplier")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing multiplier field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))?;

        let lot_size = Quantity::from_str(
            i_map
                .get("lot_size")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing lot_size field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))?;

        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );

        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );

        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );

        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );

        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );

        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );

        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );

        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );

        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
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
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let asset_class = AssetClass::from_str(
            i_map
                .get("asset_class")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;

        let exchange = i_map.get("exchange").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing exchange field");
                None
            },
            |value| Ustr::from_str(value).ok(),
        );

        let underlying = Ustr::from_str(
            i_map
                .get("underlying")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;

        let option_kind = OptionKind::from_str(
            i_map
                .get("option_kind")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing option_kind field"))?,
        )?;

        let strike_price = Price::from_str(
            i_map
                .get("strike_price")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strike_price field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid strike_price: {}", e))?;

        let currency = Currency::from_str(
            i_map
                .get("currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;

        let activation_ns = UnixNanos::from_str(
            &i_map
                .get("activation_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;

        let expiration_ns = UnixNanos::from_str(
            &i_map
                .get("expiration_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;

        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;

        let multiplier = Quantity::from_str(
            i_map
                .get("multiplier")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing multiplier field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))?;

        let lot_size = Quantity::from_str(
            i_map
                .get("lot_size")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing lot_size field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))?;

        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );

        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );

        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );

        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );

        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );

        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );

        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );

        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );

        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
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
        let id = InstrumentId::from_str(
            i_map
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id field"))?,
        )?;

        let raw_symbol = Symbol::from_str_unchecked(
            i_map
                .get("raw_symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing raw_symbol field"))?,
        );

        let asset_class = AssetClass::from_str(
            i_map
                .get("asset_class")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing asset_class field"))?,
        )?;

        let exchange = i_map.get("exchange").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing exchange field");
                None
            },
            |value| Ustr::from_str(value).ok(),
        );

        let underlying = Ustr::from_str(
            i_map
                .get("underlying")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing underlying field"))?,
        )?;

        let strategy_type = Ustr::from_str(
            i_map
                .get("strategy_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_type field"))?,
        )?;

        let activation_ns = UnixNanos::from_str(
            &i_map
                .get("activation_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid activation_ns: {}", e))?;

        let expiration_ns = UnixNanos::from_str(
            &i_map
                .get("expiration_ns")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid expiration_ns: {}", e))?;

        let currency = Currency::from_str(
            i_map
                .get("currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )?;

        let price_precision: u8 = i_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let price_increment = Price::from_str(
            i_map
                .get("price_increment")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing price_increment field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid price_increment: {}", e))?;

        let multiplier = Quantity::from_str(
            i_map
                .get("multiplier")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing multiplier field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid multiplier: {}", e))?;

        let lot_size = Quantity::from_str(
            i_map
                .get("lot_size")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing lot_size field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid lot_size: {}", e))?;

        let max_quantity = i_map
            .get("max_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing max_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );

        let min_quantity = i_map
            .get("min_quantity")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing min_quantity field");
                    None
                },
                |value| Quantity::from_str(value).ok(),
            );

        let max_price = i_map.get("max_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing max_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );

        let min_price = i_map.get("min_price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing min_price field");
                None
            },
            |value| Price::from_str(value).ok(),
        );

        let margin_init = i_map
            .get("margin_init")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_init field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );

        let margin_maint = i_map
            .get("margin_maint")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing margin_maint field");
                    None
                },
                |value| Decimal::from_str(value).ok(),
            );

        let maker_fee = i_map.get("maker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing maker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );

        let taker_fee = i_map.get("taker_fee").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing taker_fee field");
                None
            },
            |value| Decimal::from_str(value).ok(),
        );

        let ts_event = UnixNanos::from_str(
            &i_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &i_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
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

    pub fn synthetic_from_value(value: Value) -> anyhow::Result<SyntheticInstrument> {
        let s_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid synthetic map"),
        };

        let symbol = Symbol::from_str_unchecked(
            s_map
                .get("symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing symbol field"))?,
        );

        let price_precision: u8 = s_map
            .get("price_precision")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing price_precision field"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Price precision out of range for u8"))?;

        let formula = s_map
            .get("formula")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing formula field"))?;

        let ts_event = UnixNanos::from_str(
            &s_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &s_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
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

    pub fn account_from_value(value: Value) -> anyhow::Result<AccountAny> {
        let a_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid account map"),
        };

        let account_id = AccountId::new_checked(
            a_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id"))?,
        )?;
        let account_type = AccountType::from_str(
            a_map
                .get("account_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_type"))?,
        )?;
        let balances = a_map
            .get("balances")
            .and_then(|v| v.as_array())
            .map(|array| {
                array
                    .iter()
                    .filter_map(|b| {
                        let result: Result<AccountBalance, anyhow::Error> = (|| {
                            let currency = Currency::from_str(
                                b["currency"]
                                    .as_str()
                                    .ok_or_else(|| anyhow::anyhow!("Missing currency"))?,
                            )?;
                            let free = Money::new_checked(
                                b["free"]
                                    .as_str()
                                    .ok_or_else(|| anyhow::anyhow!("Missing free"))?
                                    .parse::<f64>()
                                    .map_err(|e| anyhow::anyhow!("Invalid free: {}", e))?,
                                currency,
                            )
                            .map_err(|e| anyhow::anyhow!("Invalid free: {}", e))?;
                            let locked = Money::new_checked(
                                b["locked"]
                                    .as_str()
                                    .ok_or_else(|| anyhow::anyhow!("Missing locked"))?
                                    .parse::<f64>()
                                    .map_err(|e| anyhow::anyhow!("Invalid locked: {}", e))?,
                                currency,
                            )
                            .map_err(|e| anyhow::anyhow!("Invalid locked: {}", e))?;
                            let total = Money::new_checked(
                                b["total"]
                                    .as_str()
                                    .ok_or_else(|| anyhow::anyhow!("Missing total"))?
                                    .parse::<f64>()
                                    .map_err(|e| anyhow::anyhow!("Invalid total: {}", e))?,
                                currency,
                            )
                            .map_err(|e| anyhow::anyhow!("Invalid total: {}", e))?;

                            AccountBalance::new_checked(total, locked, free)
                        })(
                        );

                        match result {
                            Ok(balance) => Some(balance),
                            Err(e) => {
                                tracing::error!("Failed to parse balance: {}", e);
                                None
                            }
                        }
                    })
                    .collect::<Vec<AccountBalance>>()
            })
            .unwrap_or_default();

        let margins = a_map
            .get("margins")
            .and_then(|v| v.as_array())
            .map(|array| {
                array
                    .iter()
                    .filter_map(|m| {
                        let result: Result<MarginBalance, anyhow::Error> = (|| {
                            let initial = Money::from_str(
                                m["initial"]
                                    .as_str()
                                    .ok_or_else(|| anyhow::anyhow!("Missing initial margin"))?,
                            )
                            .map_err(|e| anyhow::anyhow!("Invalid initial margin: {}", e))?;

                            let maintenance =
                                Money::from_str(m["maintenance"].as_str().ok_or_else(|| {
                                    anyhow::anyhow!("Missing maintenance margin")
                                })?)
                                .map_err(|e| {
                                    anyhow::anyhow!("Invalid maintenance margin: {}", e)
                                })?;

                            let instrument_id = InstrumentId::from_str(
                                m["instrument_id"]
                                    .as_str()
                                    .ok_or_else(|| anyhow::anyhow!("Missing instrument_id"))?,
                            )?;

                            Ok(MarginBalance::new(initial, maintenance, instrument_id))
                        })(
                        );

                        match result {
                            Ok(margin) => Some(margin),
                            Err(e) => {
                                tracing::error!("Failed to parse margin: {}", e);
                                None
                            }
                        }
                    })
                    .collect::<Vec<MarginBalance>>()
            })
            .unwrap_or_default();

        let is_reported = a_map
            .get("reported")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reported"))?;
        let event_id = a_map
            .get("event_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing event_id"))?
            .to_string();
        let ts_event = UnixNanos::from_str(
            &a_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing ts_event"))?,
        )?;
        let ts_init = UnixNanos::from_str(
            &a_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing ts_init"))?,
        )?;
        let base_currency = a_map
            .get("base_currency")
            .and_then(|v| v.as_str())
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

    pub fn order_event_any_from_value(value: Value) -> anyhow::Result<OrderEventAny> {
        let event_type = match &value {
            Value::Object(map) => map
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing type field"))?,
            _ => anyhow::bail!("Invalid order event any map"),
        };

        let event = match event_type {
            "OrderInitialized" => {
                OrderEventAny::Initialized(Self::order_initialized_from_value(value)?)
            }
            "OrderDenied" => OrderEventAny::Denied(Self::order_denied_from_value(value)?),
            "OrderEmulated" => OrderEventAny::Emulated(Self::order_emulated_from_value(value)?),
            "OrderReleased" => OrderEventAny::Released(Self::order_released_from_value(value)?),
            "OrderSubmitted" => OrderEventAny::Submitted(Self::order_submitted_from_value(value)?),
            "OrderAccepted" => OrderEventAny::Accepted(Self::order_accepted_from_value(value)?),
            "OrderRejected" => OrderEventAny::Rejected(Self::order_rejected_from_value(value)?),
            "OrderCanceled" => OrderEventAny::Canceled(Self::order_canceled_from_value(value)?),
            "OrderExpired" => OrderEventAny::Expired(Self::order_expired_from_value(value)?),
            "OrderTriggered" => OrderEventAny::Triggered(Self::order_triggered_from_value(value)?),
            "OrderPendingUpdate" => {
                OrderEventAny::PendingUpdate(Self::order_pending_update_from_value(value)?)
            }
            "OrderPendingCancel" => {
                OrderEventAny::PendingCancel(Self::order_pending_cancel_from_value(value)?)
            }
            "OrderModifyRejected" => {
                OrderEventAny::ModifyRejected(Self::order_modify_rejected_from_value(value)?)
            }
            "OrderCancelRejected" => {
                OrderEventAny::CancelRejected(Self::order_cancel_rejected_from_value(value)?)
            }
            "OrderUpdated" => OrderEventAny::Updated(Self::order_updated_from_value(value)?),
            "OrderFilled" => OrderEventAny::Filled(Self::order_filled_from_value(value)?),
            _ => anyhow::bail!("Invalid event type"),
        };

        Ok(event)
    }

    pub fn order_initialized_from_value(value: Value) -> anyhow::Result<OrderInitialized> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order initialized map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;

        let order_side = OrderSide::from_str(
            o_map
                .get("order_side")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing order_side field"))?,
        )?;

        let order_type = OrderType::from_str(
            o_map
                .get("order_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing order_type field"))?,
        )?;

        let quantity = Quantity::from_str(
            o_map
                .get("quantity")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing quantity field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid quantity: {}", e))?;

        let time_in_force = TimeInForce::from_str(
            o_map
                .get("time_in_force")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing time_in_force field"))?,
        )?;

        let post_only = o_map
            .get("post_only")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing post_only field"))?;

        let reduce_only = o_map
            .get("reduce_only")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reduce_only field"))?;

        let quote_quantity = o_map
            .get("quote_quantity")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing quote_quantity field"))?;

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let options = o_map.get("options").and_then(|v| v.as_object());

        let price = options
            .and_then(|opts| opts.get("price"))
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing price in options");
                    None
                },
                |value| match Price::from_str(value) {
                    Ok(price) => Some(price),
                    Err(e) => {
                        tracing::error!("Invalid price: {}", e);
                        None
                    }
                },
            );

        let expire_time = options
            .and_then(|opts| opts.get("expire_time_ns"))
            .and_then(|v| v.as_i64())
            .map_or_else(
                || {
                    tracing::error!("Missing expire_time_ns in options");
                    None
                },
                |ns| match UnixNanos::from_str(&ns.to_string()) {
                    Ok(time) => Some(time),
                    Err(e) => {
                        tracing::error!("Invalid expire_time: {}", e);
                        None
                    }
                },
            );

        let display_qty = options
            .and_then(|opts| opts.get("display_qty"))
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing display_qty in options");
                    None
                },
                |value| match Quantity::from_str(value) {
                    Ok(qty) => Some(qty),
                    Err(e) => {
                        tracing::error!("Invalid display_qty: {}", e);
                        None
                    }
                },
            );

        let trigger_price = o_map
            .get("trigger_price")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing trigger_price field");
                    None
                },
                |value| {
                    Price::from_str(value)
                        .map_err(|e| {
                            tracing::error!("Invalid trigger_price: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let trigger_type = o_map
            .get("trigger_type")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing trigger_type field");
                    None
                },
                |value| {
                    TriggerType::from_str(value)
                        .inspect_err(|e| {
                            tracing::error!("Invalid trigger_type: {}", e);
                        })
                        .ok()
                },
            );
        let limit_offset = o_map
            .get("limit_offset")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing limit_offset field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| {
                            tracing::error!("Invalid limit_offset: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let trailing_offset = o_map
            .get("trailing_offset")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing trailing_offset field");
                    None
                },
                |value| {
                    Decimal::from_str(value)
                        .map_err(|e| {
                            tracing::error!("Invalid trailing_offset: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let trailing_offset_type = o_map
            .get("trailing_offset_type")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing trailing_offset_type field");
                    None
                },
                |value| {
                    TrailingOffsetType::from_str(value)
                        .inspect_err(|e| {
                            tracing::error!("Invalid trailing_offset_type: {}", e);
                        })
                        .ok()
                },
            );
        let emulation_trigger = o_map
            .get("emulation_trigger")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing emulation_trigger field");
                    None
                },
                |value| {
                    TriggerType::from_str(value)
                        .inspect_err(|e| {
                            tracing::error!("Invalid emulation_trigger: {}", e);
                        })
                        .ok()
                },
            );
        let trigger_instrument_id = o_map
            .get("trigger_instrument_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing trigger_instrument_id field");
                    None
                },
                |value| {
                    InstrumentId::from_str(value)
                        .map_err(|e| {
                            tracing::error!("Invalid trigger_instrument_id: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let contingency_type = o_map
            .get("contingency_type")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing contingency_type field");
                    None
                },
                |value| {
                    ContingencyType::from_str(value)
                        .inspect_err(|e| {
                            tracing::error!("Invalid contingency_type: {}", e);
                        })
                        .ok()
                },
            );
        let order_list_id = o_map
            .get("order_list_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing order_list_id field");
                    None
                },
                |value| {
                    OrderListId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid order_list_id: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let linked_order_ids = o_map
            .get("linked_order_ids")
            .and_then(|v| v.as_array())
            .map(|array| {
                array
                    .iter()
                    .filter_map(|v| match v.as_str() {
                        None => {
                            tracing::error!("Linked order ID is not a string");
                            None
                        }
                        Some(str_val) => match ClientOrderId::new_checked(str_val) {
                            Ok(order_id) => Some(order_id),
                            Err(e) => {
                                tracing::error!("Invalid linked order ID format: {}", e);
                                None
                            }
                        },
                    })
                    .collect::<Vec<_>>()
            });
        let parent_order_id = o_map
            .get("parent_order_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing parent_order_id field");
                    None
                },
                |value| {
                    ClientOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid parent_order_id: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let exec_algorithm_id = o_map
            .get("exec_algorithm_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing exec_algorithm_id field");
                    None
                },
                |value| {
                    ExecAlgorithmId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid exec_algorithm_id: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let exec_algorithm_params = o_map
            .get("exec_algorithm_params")
            .and_then(|v| v.as_object())
            .map_or_else(
                || {
                    tracing::error!("Missing or invalid exec_algorithm_params field");
                    None
                },
                |obj| {
                    let params: Option<IndexMap<Ustr, Ustr>> = Some(
                        obj.iter()
                            .filter_map(|(k, v)| {
                                let key = match Ustr::from_str(k) {
                                    Ok(key) => key,
                                    Err(e) => {
                                        tracing::error!("Invalid exec_algorithm_params key: {}", e);
                                        return None;
                                    }
                                };

                                let value = match v.as_str() {
                                    Some(str_val) => match Ustr::from_str(str_val) {
                                        Ok(val) => val,
                                        Err(e) => {
                                            tracing::error!(
                                                "Invalid exec_algorithm_params value: {}",
                                                e
                                            );
                                            return None;
                                        }
                                    },
                                    None => {
                                        tracing::error!(
                                            "exec_algorithm_params value is not a string"
                                        );
                                        return None;
                                    }
                                };

                                Some((key, value))
                            })
                            .collect(),
                    );

                    if params.as_ref().is_none_or(|p| p.is_empty()) {
                        None
                    } else {
                        params
                    }
                },
            );
        let exec_spawn_id = o_map
            .get("exec_spawn_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing exec_spawn_id field");
                    None
                },
                |value| {
                    ClientOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid exec_spawn_id: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let tags = o_map.get("tags").and_then(|v| v.as_array()).map(|array| {
            array
                .iter()
                .filter_map(|v| match v.as_str() {
                    None => {
                        tracing::error!("Tag is not a string");
                        None
                    }
                    Some(str_val) => match Ustr::from_str(str_val) {
                        Ok(tag) => Some(tag),
                        Err(e) => {
                            tracing::error!("Invalid tag: {}", e);
                            None
                        }
                    },
                })
                .collect::<Vec<_>>()
        });
        let order_initialized = OrderInitialized::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            order_type,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            quote_quantity,
            reconciliation,
            event_id,
            ts_event,
            ts_init,
            price,
            trigger_price,
            trigger_type,
            limit_offset,
            trailing_offset,
            trailing_offset_type,
            expire_time,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
        );

        Ok(order_initialized)
    }

    pub fn order_denied_from_value(value: Value) -> anyhow::Result<OrderDenied> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;

        let reason = Ustr::from_str(
            o_map
                .get("reason")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing reason field"))?,
        )?;

        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let order_denied = OrderDenied::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            event_id,
            ts_event,
            ts_init,
        );

        Ok(order_denied)
    }

    pub fn order_emulated_from_value(value: Value) -> anyhow::Result<OrderEmulated> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;

        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let order_emulated = OrderEmulated::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event,
            ts_init,
        );

        Ok(order_emulated)
    }

    pub fn order_released_from_value(value: Value) -> anyhow::Result<OrderReleased> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;

        let released_price = Price::from_str(
            o_map
                .get("released_price")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing released_price field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid released_price: {}", e))?;

        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let order_released = OrderReleased::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            released_price,
            event_id,
            ts_event,
            ts_init,
        );

        Ok(order_released)
    }

    pub fn order_submitted_from_value(value: Value) -> anyhow::Result<OrderSubmitted> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )?;

        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let order_submitted = OrderSubmitted::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
        );

        Ok(order_submitted)
    }

    pub fn order_accepted_from_value(value: Value) -> anyhow::Result<OrderAccepted> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;

        let venue_order_id = VenueOrderId::new_checked(
            o_map
                .get("venue_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing venue_order_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )?;

        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let order_accepted = OrderAccepted::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        );

        Ok(order_accepted)
    }

    pub fn order_rejected_from_value(value: Value) -> anyhow::Result<OrderRejected> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )?;

        let reason = Ustr::from_str(
            o_map
                .get("reason")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing reason field"))?,
        )?;

        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let order_rejected = OrderRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            reason,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        );

        Ok(order_rejected)
    }

    pub fn order_canceled_from_value(value: Value) -> anyhow::Result<OrderCanceled> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;
        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )
        .map(Some)
        .unwrap_or(None);

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let venue_order_id = o_map
            .get("venue_order_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing venue_order_id field");
                    None
                },
                |value| {
                    VenueOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid venue_order_id: {}", e);
                            e
                        })
                        .ok()
                },
            );

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let order_canceled = OrderCanceled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        );

        Ok(order_canceled)
    }

    pub fn order_expired_from_value(value: Value) -> anyhow::Result<OrderExpired> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;
        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )
        .map(Some)
        .unwrap_or(None);

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let venue_order_id = o_map
            .get("venue_order_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing venue_order_id field");
                    None
                },
                |value| {
                    VenueOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid venue_order_id: {}", e);
                            e
                        })
                        .ok()
                },
            );

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let order_expired = OrderExpired::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        );

        Ok(order_expired)
    }

    pub fn order_triggered_from_value(value: Value) -> anyhow::Result<OrderTriggered> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;
        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )
        .map(Some)
        .unwrap_or(None);

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let venue_order_id = o_map
            .get("venue_order_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing venue_order_id field");
                    None
                },
                |value| {
                    VenueOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid venue_order_id: {}", e);
                            e
                        })
                        .ok()
                },
            );
        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let order_triggered = OrderTriggered::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        );

        Ok(order_triggered)
    }

    pub fn order_pending_update_from_value(value: Value) -> anyhow::Result<OrderPendingUpdate> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;
        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let venue_order_id = o_map
            .get("venue_order_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing venue_order_id field");
                    None
                },
                |value| {
                    VenueOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid venue_order_id: {}", e);
                            e
                        })
                        .ok()
                },
            );

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let order_pending_update = OrderPendingUpdate::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
        );

        Ok(order_pending_update)
    }

    pub fn order_pending_cancel_from_value(value: Value) -> anyhow::Result<OrderPendingCancel> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;
        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let venue_order_id = o_map
            .get("venue_order_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing venue_order_id field");
                    None
                },
                |value| {
                    VenueOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid venue_order_id: {}", e);
                            e
                        })
                        .ok()
                },
            );

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let order_pending_cancel = OrderPendingCancel::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
        );

        Ok(order_pending_cancel)
    }

    pub fn order_modify_rejected_from_value(value: Value) -> anyhow::Result<OrderModifyRejected> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;
        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )
        .map(Some)
        .unwrap_or(None);

        let reason = Ustr::from_str(
            o_map
                .get("reason")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing reason field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let venue_order_id = o_map
            .get("venue_order_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing venue_order_id field");
                    None
                },
                |value| {
                    VenueOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid venue_order_id: {}", e);
                            e
                        })
                        .ok()
                },
            );

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let order_modify_rejected = OrderModifyRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        );

        Ok(order_modify_rejected)
    }

    pub fn order_cancel_rejected_from_value(value: Value) -> anyhow::Result<OrderCancelRejected> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;
        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )
        .map(Some)
        .unwrap_or(None);

        let reason = Ustr::from_str(
            o_map
                .get("reason")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing reason field"))?,
        )?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let venue_order_id = o_map
            .get("venue_order_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing venue_order_id field");
                    None
                },
                |value| {
                    VenueOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid venue_order_id: {}", e);
                            e
                        })
                        .ok()
                },
            );

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let order_cancel_rejected = OrderCancelRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        );

        Ok(order_cancel_rejected)
    }

    pub fn order_updated_from_value(value: Value) -> anyhow::Result<OrderUpdated> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order denied map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;

        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;

        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;
        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;

        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )
        .map(Some)
        .unwrap_or(None);

        let quantity = Quantity::from_str(
            o_map
                .get("quantity")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing quantity field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid quantity: {}", e))?;

        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

        let venue_order_id = o_map
            .get("venue_order_id")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing venue_order_id field");
                    None
                },
                |value| {
                    VenueOrderId::new_checked(value)
                        .map_err(|e| {
                            tracing::error!("Invalid venue_order_id: {}", e);
                            e
                        })
                        .ok()
                },
            );

        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

        let price = o_map.get("price").and_then(|v| v.as_str()).map_or_else(
            || {
                tracing::error!("Missing price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| anyhow::anyhow!("Invalid price: {}", e))
                    .ok()
            },
        );

        let trigger_price = o_map
            .get("trigger_price")
            .and_then(|v| v.as_str())
            .map_or_else(
                || {
                    tracing::error!("Missing trigger_price field");
                    None
                },
                |value| {
                    Price::from_str(value)
                        .map_err(|e| anyhow::anyhow!("Invalid trigger_price: {}", e))
                        .ok()
                },
            );

        let order_updated = OrderUpdated::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            quantity,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
            price,
            trigger_price,
        );

        Ok(order_updated)
    }

    pub fn order_filled_from_value(value: Value) -> anyhow::Result<OrderFilled> {
        let o_map = match value {
            Value::Object(map) => map,
            _ => anyhow::bail!("Invalid order filled map"),
        };

        let trader_id = TraderId::new_checked(
            o_map
                .get("trader_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
        )?;

        let strategy_id = StrategyId::new_checked(
            o_map
                .get("strategy_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
        )?;
        let instrument_id = InstrumentId::from_str(
            o_map
                .get("instrument_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
        )?;
        let client_order_id = ClientOrderId::new_checked(
            o_map
                .get("client_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
        )?;
        let venue_order_id = VenueOrderId::new_checked(
            o_map
                .get("venue_order_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing venue_order_id field"))?,
        )?;
        let account_id = AccountId::new_checked(
            o_map
                .get("account_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing account_id field"))?,
        )?;
        let trade_id = TradeId::new_checked(
            o_map
                .get("trade_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing trade_id field"))?,
        )?;
        let order_side = OrderSide::from_str(
            o_map
                .get("order_side")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing order_side field"))?,
        )?;
        let order_type = OrderType::from_str(
            o_map
                .get("order_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing order_type field"))?,
        )?;
        let last_qty = Quantity::from_str(
            o_map
                .get("last_qty")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing last_qty field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid last_qty: {}", e))?;
        let last_px = Price::from_str(
            o_map
                .get("last_px")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing last_px field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid last_px: {}", e))?;
        let currency = Currency::from_str(
            o_map
                .get("currency")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing currency field"))?,
        )
        .map_err(|e| anyhow::anyhow!("Invalid currency: {}", e))?;
        let liquidity_side = LiquiditySide::from_str(
            o_map
                .get("liquidity_side")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing liquidity_side field"))?,
        )?;
        let event_id = UUID4::from_str(
            o_map
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
        )?;
        let ts_event = UnixNanos::from_str(
            &o_map
                .get("ts_event")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;
        let ts_init = UnixNanos::from_str(
            &o_map
                .get("ts_init")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;
        let reconciliation = o_map
            .get("reconciliation")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;
        let position_id = match o_map.get("position_id").and_then(|v| v.as_str()) {
            Some(value) => match PositionId::new_checked(value) {
                Ok(id) => Some(id),
                Err(e) => {
                    tracing::error!("Invalid position_id: {}", e);
                    None
                }
            },
            None => {
                tracing::error!("Missing position_id field");
                None
            }
        };
        let commission = match o_map.get("commission").and_then(|v| v.as_str()) {
            Some(value) => match Money::from_str(value) {
                Ok(money) => Some(money),
                Err(e) => {
                    tracing::error!("Invalid commission: {}", e);
                    None
                }
            },
            None => {
                tracing::error!("Missing commission field");
                None
            }
        };
        let order_filled = OrderFilled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            trade_id,
            order_side,
            order_type,
            last_qty,
            last_px,
            currency,
            liquidity_side,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            position_id,
            commission,
        );

        Ok(order_filled)
    }
}
