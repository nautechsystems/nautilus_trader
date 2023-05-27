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

use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[macro_use]
mod macros;

pub mod account_id;
pub mod client_id;
pub mod client_order_id;
pub mod component_id;
pub mod exec_algorithm_id;
pub mod instrument_id;
pub mod order_list_id;
pub mod position_id;
pub mod strategy_id;
pub mod symbol;
pub mod trade_id;
pub mod trader_id;
pub mod venue;
pub mod venue_order_id;

from_str_for_identifier!(account_id::AccountId);
from_str_for_identifier!(client_id::ClientId);
from_str_for_identifier!(client_order_id::ClientOrderId);
from_str_for_identifier!(component_id::ComponentId);
from_str_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
from_str_for_identifier!(order_list_id::OrderListId);
from_str_for_identifier!(position_id::PositionId);
from_str_for_identifier!(strategy_id::StrategyId);
from_str_for_identifier!(symbol::Symbol);
from_str_for_identifier!(trade_id::TradeId);
from_str_for_identifier!(trader_id::TraderId);
from_str_for_identifier!(venue::Venue);
from_str_for_identifier!(venue_order_id::VenueOrderId);

serialize_for_identifier!(account_id::AccountId);
serialize_for_identifier!(client_id::ClientId);
serialize_for_identifier!(client_order_id::ClientOrderId);
serialize_for_identifier!(component_id::ComponentId);
serialize_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
serialize_for_identifier!(order_list_id::OrderListId);
serialize_for_identifier!(position_id::PositionId);
serialize_for_identifier!(strategy_id::StrategyId);
serialize_for_identifier!(symbol::Symbol);
serialize_for_identifier!(trade_id::TradeId);
serialize_for_identifier!(trader_id::TraderId);
serialize_for_identifier!(venue::Venue);
serialize_for_identifier!(venue_order_id::VenueOrderId);
