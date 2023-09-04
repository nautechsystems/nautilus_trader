// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use pyo3::{prelude::*, PyResult, Python};

pub mod currencies;
pub mod data;
pub mod enums;
pub mod events;
pub mod identifiers;
pub mod instruments;
pub mod macros;
pub mod orderbook;
pub mod orders;
pub mod position;
pub mod python;
pub mod types;

/// Loaded as nautilus_pyo3.model
#[pymodule]
pub fn model(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<data::bar::BarSpecification>()?;
    m.add_class::<data::bar::BarType>()?;
    m.add_class::<data::bar::Bar>()?;
    m.add_class::<data::order::BookOrder>()?;
    m.add_class::<data::delta::OrderBookDelta>()?;
    m.add_class::<data::quote::QuoteTick>()?;
    m.add_class::<data::trade::TradeTick>()?;
    m.add_class::<enums::AccountType>()?;
    m.add_class::<enums::AggregationSource>()?;
    m.add_class::<enums::BarAggregation>()?;
    m.add_class::<enums::PriceType>()?;
    m.add_class::<enums::OrderSide>()?;
    m.add_class::<enums::PositionSide>()?;
    m.add_class::<identifiers::account_id::AccountId>()?;
    m.add_class::<identifiers::client_id::ClientId>()?;
    m.add_class::<identifiers::client_order_id::ClientOrderId>()?;
    m.add_class::<identifiers::component_id::ComponentId>()?;
    m.add_class::<identifiers::exec_algorithm_id::ExecAlgorithmId>()?;
    m.add_class::<identifiers::instrument_id::InstrumentId>()?;
    m.add_class::<identifiers::order_list_id::OrderListId>()?;
    m.add_class::<identifiers::position_id::PositionId>()?;
    m.add_class::<identifiers::strategy_id::StrategyId>()?;
    m.add_class::<identifiers::symbol::Symbol>()?;
    m.add_class::<identifiers::trade_id::TradeId>()?;
    m.add_class::<identifiers::trader_id::TraderId>()?;
    m.add_class::<identifiers::venue::Venue>()?;
    m.add_class::<identifiers::venue_order_id::VenueOrderId>()?;
    m.add_class::<orders::limit::LimitOrder>()?;
    m.add_class::<orders::limit_if_touched::LimitIfTouchedOrder>()?;
    m.add_class::<orders::market::MarketOrder>()?;
    m.add_class::<orders::market_to_limit::MarketToLimitOrder>()?;
    m.add_class::<orders::stop_limit::StopLimitOrder>()?;
    m.add_class::<orders::stop_market::StopMarketOrder>()?;
    m.add_class::<orders::trailing_stop_limit::TrailingStopLimitOrder>()?;
    m.add_class::<orders::trailing_stop_market::TrailingStopMarketOrder>()?;
    m.add_class::<types::currency::Currency>()?;
    m.add_class::<types::money::Money>()?;
    m.add_class::<types::price::Price>()?;
    m.add_class::<types::quantity::Quantity>()?;
    Ok(())
}
