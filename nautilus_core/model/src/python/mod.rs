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

use pyo3::prelude::*;

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
pub fn model(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    // Data
    m.add_function(wrap_pyfunction!(data::drop_cvec_pycapsule, m)?)?;
    m.add_class::<crate::data::bar::BarSpecification>()?;
    m.add_class::<crate::data::bar::BarType>()?;
    m.add_class::<crate::data::bar::Bar>()?;
    m.add_class::<crate::data::order::BookOrder>()?;
    m.add_class::<crate::data::delta::OrderBookDelta>()?;
    m.add_class::<crate::data::deltas::OrderBookDeltas>()?;
    m.add_class::<crate::data::depth::OrderBookDepth10>()?;
    m.add_class::<crate::data::quote::QuoteTick>()?;
    m.add_class::<crate::data::trade::TradeTick>()?;
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
    m.add_class::<crate::identifiers::account_id::AccountId>()?;
    m.add_class::<crate::identifiers::client_id::ClientId>()?;
    m.add_class::<crate::identifiers::client_order_id::ClientOrderId>()?;
    m.add_class::<crate::identifiers::component_id::ComponentId>()?;
    m.add_class::<crate::identifiers::exec_algorithm_id::ExecAlgorithmId>()?;
    m.add_class::<crate::identifiers::instrument_id::InstrumentId>()?;
    m.add_class::<crate::identifiers::order_list_id::OrderListId>()?;
    m.add_class::<crate::identifiers::position_id::PositionId>()?;
    m.add_class::<crate::identifiers::strategy_id::StrategyId>()?;
    m.add_class::<crate::identifiers::symbol::Symbol>()?;
    m.add_class::<crate::identifiers::trade_id::TradeId>()?;
    m.add_class::<crate::identifiers::trader_id::TraderId>()?;
    m.add_class::<crate::identifiers::venue::Venue>()?;
    m.add_class::<crate::identifiers::venue_order_id::VenueOrderId>()?;
    // Orders
    m.add_class::<crate::orders::limit::LimitOrder>()?;
    m.add_class::<crate::orders::limit_if_touched::LimitIfTouchedOrder>()?;
    m.add_class::<crate::orders::market::MarketOrder>()?;
    m.add_class::<crate::orders::market_to_limit::MarketToLimitOrder>()?;
    m.add_class::<crate::orders::stop_limit::StopLimitOrder>()?;
    m.add_class::<crate::orders::stop_market::StopMarketOrder>()?;
    m.add_class::<crate::orders::trailing_stop_limit::TrailingStopLimitOrder>()?;
    m.add_class::<crate::orders::trailing_stop_market::TrailingStopMarketOrder>()?;
    m.add_class::<crate::types::currency::Currency>()?;
    m.add_class::<crate::types::money::Money>()?;
    m.add_class::<crate::types::price::Price>()?;
    m.add_class::<crate::types::quantity::Quantity>()?;
    m.add_class::<crate::types::balance::AccountBalance>()?;
    m.add_class::<crate::types::balance::MarginBalance>()?;
    // Instruments
    m.add_class::<crate::instruments::crypto_future::CryptoFuture>()?;
    m.add_class::<crate::instruments::crypto_perpetual::CryptoPerpetual>()?;
    m.add_class::<crate::instruments::currency_pair::CurrencyPair>()?;
    m.add_class::<crate::instruments::equity::Equity>()?;
    m.add_class::<crate::instruments::futures_contract::FuturesContract>()?;
    m.add_class::<crate::instruments::futures_spread::FuturesSpread>()?;
    m.add_class::<crate::instruments::options_contract::OptionsContract>()?;
    m.add_class::<crate::instruments::options_spread::OptionsSpread>()?;
    m.add_class::<crate::instruments::synthetic::SyntheticInstrument>()?;
    // Order book
    m.add_class::<crate::orderbook::book_mbo::OrderBookMbo>()?;
    m.add_class::<crate::orderbook::book_mbp::OrderBookMbp>()?;
    m.add_class::<crate::orderbook::level::Level>()?;
    // Events - order
    m.add_class::<crate::events::order::denied::OrderDenied>()?;
    m.add_class::<crate::events::order::filled::OrderFilled>()?;
    m.add_class::<crate::events::order::initialized::OrderInitialized>()?;
    m.add_class::<crate::events::order::rejected::OrderRejected>()?;
    m.add_class::<crate::events::order::triggered::OrderTriggered>()?;
    m.add_class::<crate::events::order::submitted::OrderSubmitted>()?;
    m.add_class::<crate::events::order::emulated::OrderEmulated>()?;
    m.add_class::<crate::events::order::released::OrderReleased>()?;
    m.add_class::<crate::events::order::updated::OrderUpdated>()?;
    m.add_class::<crate::events::order::pending_update::OrderPendingUpdate>()?;
    m.add_class::<crate::events::order::pending_cancel::OrderPendingCancel>()?;
    m.add_class::<crate::events::order::modify_rejected::OrderModifyRejected>()?;
    m.add_class::<crate::events::order::accepted::OrderAccepted>()?;
    m.add_class::<crate::events::order::cancel_rejected::OrderCancelRejected>()?;
    m.add_class::<crate::events::order::canceled::OrderCanceled>()?;
    m.add_class::<crate::events::order::expired::OrderExpired>()?;
    // Events - account
    m.add_class::<crate::events::account::state::AccountState>()?;
    m.add_class::<crate::position::Position>()?;
    Ok(())
}
