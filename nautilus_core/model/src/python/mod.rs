// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python bindings from `pyo3`.

use pyo3::prelude::*;

pub mod account;
pub mod common;
pub mod data;
pub mod enums;
pub mod events;
pub mod identifiers;
pub mod instruments;
pub mod macros;
pub mod orderbook;
pub mod orders;
pub mod position;
pub mod types;

/// Loaded as nautilus_pyo3.model
#[pymodule]
pub fn model(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Types
    m.add_class::<crate::types::currency::Currency>()?;
    m.add_class::<crate::types::money::Money>()?;
    m.add_class::<crate::types::price::Price>()?;
    m.add_class::<crate::types::quantity::Quantity>()?;
    m.add_class::<crate::types::balance::AccountBalance>()?;
    m.add_class::<crate::types::balance::MarginBalance>()?;
    // Data
    m.add_function(wrap_pyfunction!(data::drop_cvec_pycapsule, m)?)?;
    m.add_class::<crate::data::DataType>()?;
    m.add_class::<crate::data::bar::BarSpecification>()?;
    m.add_class::<crate::data::bar::BarType>()?;
    m.add_class::<crate::data::bar::Bar>()?;
    m.add_class::<crate::data::order::BookOrder>()?;
    m.add_class::<crate::data::delta::OrderBookDelta>()?;
    m.add_class::<crate::data::deltas::OrderBookDeltas>()?;
    m.add_class::<crate::data::depth::OrderBookDepth10>()?;
    m.add_class::<crate::data::greeks::BlackScholesGreeksResult>()?;
    m.add_class::<crate::data::greeks::ImplyVolAndGreeksResult>()?;
    m.add_class::<crate::data::quote::QuoteTick>()?;
    m.add_class::<crate::data::status::InstrumentStatus>()?;
    m.add_class::<crate::data::trade::TradeTick>()?;
    m.add_function(wrap_pyfunction!(
        crate::python::data::greeks::py_black_scholes_greeks,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        crate::python::data::greeks::py_imply_vol,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        crate::python::data::greeks::py_imply_vol_and_greeks,
        m
    )?)?;
    // Enums
    m.add_class::<crate::enums::AccountType>()?;
    m.add_class::<crate::enums::AggregationSource>()?;
    m.add_class::<crate::enums::AggressorSide>()?;
    m.add_class::<crate::enums::AssetClass>()?;
    m.add_class::<crate::enums::InstrumentClass>()?;
    m.add_class::<crate::enums::BarAggregation>()?;
    m.add_class::<crate::enums::BookAction>()?;
    m.add_class::<crate::enums::BookType>()?;
    m.add_class::<crate::enums::ContingencyType>()?;
    m.add_class::<crate::enums::CurrencyType>()?;
    m.add_class::<crate::enums::InstrumentCloseType>()?;
    m.add_class::<crate::enums::LiquiditySide>()?;
    m.add_class::<crate::enums::MarketStatus>()?;
    m.add_class::<crate::enums::MarketStatusAction>()?;
    m.add_class::<crate::enums::OmsType>()?;
    m.add_class::<crate::enums::OptionKind>()?;
    m.add_class::<crate::enums::OrderSide>()?;
    m.add_class::<crate::enums::OrderStatus>()?;
    m.add_class::<crate::enums::OrderType>()?;
    m.add_class::<crate::enums::PositionSide>()?;
    m.add_class::<crate::enums::PriceType>()?;
    m.add_class::<crate::enums::TimeInForce>()?;
    m.add_class::<crate::enums::TradingState>()?;
    m.add_class::<crate::enums::TrailingOffsetType>()?;
    m.add_class::<crate::enums::TriggerType>()?;
    // Identifiers
    m.add_class::<crate::identifiers::AccountId>()?;
    m.add_class::<crate::identifiers::ClientId>()?;
    m.add_class::<crate::identifiers::ClientOrderId>()?;
    m.add_class::<crate::identifiers::ComponentId>()?;
    m.add_class::<crate::identifiers::ExecAlgorithmId>()?;
    m.add_class::<crate::identifiers::InstrumentId>()?;
    m.add_class::<crate::identifiers::OrderListId>()?;
    m.add_class::<crate::identifiers::PositionId>()?;
    m.add_class::<crate::identifiers::StrategyId>()?;
    m.add_class::<crate::identifiers::Symbol>()?;
    m.add_class::<crate::identifiers::TradeId>()?;
    m.add_class::<crate::identifiers::TraderId>()?;
    m.add_class::<crate::identifiers::Venue>()?;
    m.add_class::<crate::identifiers::VenueOrderId>()?;
    // Orders
    m.add_class::<crate::orders::LimitOrder>()?;
    m.add_class::<crate::orders::LimitIfTouchedOrder>()?;
    m.add_class::<crate::orders::MarketOrder>()?;
    m.add_class::<crate::orders::MarketToLimitOrder>()?;
    m.add_class::<crate::orders::StopLimitOrder>()?;
    m.add_class::<crate::orders::StopMarketOrder>()?;
    m.add_class::<crate::orders::TrailingStopLimitOrder>()?;
    m.add_class::<crate::orders::TrailingStopMarketOrder>()?;
    // Position
    m.add_class::<crate::position::Position>()?;
    // Instruments
    m.add_class::<crate::instruments::BettingInstrument>()?;
    m.add_class::<crate::instruments::BinaryOption>()?;
    m.add_class::<crate::instruments::CryptoFuture>()?;
    m.add_class::<crate::instruments::CryptoPerpetual>()?;
    m.add_class::<crate::instruments::CurrencyPair>()?;
    m.add_class::<crate::instruments::Equity>()?;
    m.add_class::<crate::instruments::FuturesContract>()?;
    m.add_class::<crate::instruments::FuturesSpread>()?;
    m.add_class::<crate::instruments::OptionsContract>()?;
    m.add_class::<crate::instruments::OptionsSpread>()?;
    m.add_class::<crate::instruments::SyntheticInstrument>()?;
    // Order book
    m.add_class::<crate::orderbook::book::OrderBook>()?;
    m.add_class::<crate::orderbook::level::BookLevel>()?;
    m.add_function(wrap_pyfunction!(
        crate::python::orderbook::book::py_update_book_with_quote_tick,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        crate::python::orderbook::book::py_update_book_with_trade_tick,
        m
    )?)?;
    // Events
    m.add_class::<crate::events::AccountState>()?;
    m.add_class::<crate::events::OrderDenied>()?;
    m.add_class::<crate::events::OrderFilled>()?;
    m.add_class::<crate::events::OrderInitialized>()?;
    m.add_class::<crate::events::OrderRejected>()?;
    m.add_class::<crate::events::OrderTriggered>()?;
    m.add_class::<crate::events::OrderSubmitted>()?;
    m.add_class::<crate::events::OrderEmulated>()?;
    m.add_class::<crate::events::OrderReleased>()?;
    m.add_class::<crate::events::OrderUpdated>()?;
    m.add_class::<crate::events::OrderPendingUpdate>()?;
    m.add_class::<crate::events::OrderPendingCancel>()?;
    m.add_class::<crate::events::OrderModifyRejected>()?;
    m.add_class::<crate::events::OrderAccepted>()?;
    m.add_class::<crate::events::OrderCancelRejected>()?;
    m.add_class::<crate::events::OrderCanceled>()?;
    m.add_class::<crate::events::OrderExpired>()?;
    m.add_class::<crate::events::OrderSnapshot>()?;
    m.add_class::<crate::events::PositionSnapshot>()?;
    // Accounts
    m.add_class::<crate::accounts::CashAccount>()?;
    m.add_class::<crate::accounts::MarginAccount>()?;
    m.add_function(wrap_pyfunction!(
        crate::python::account::transformer::cash_account_from_account_events,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        crate::python::account::transformer::margin_account_from_account_events,
        m
    )?)?;
    Ok(())
}
