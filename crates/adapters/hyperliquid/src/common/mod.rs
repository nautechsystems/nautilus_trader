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

pub mod consts;
pub mod credential;
pub mod enums;
pub mod models;
pub mod parse;
pub mod risk;
pub mod types;

pub use enums::{HyperliquidOrderStatus, hyperliquid_status_to_order_status};
pub use models::{
    ConversionError, HyperliquidAccountEvent, HyperliquidAccountState, HyperliquidBalance,
    HyperliquidDataConverter, HyperliquidInstrumentCache, HyperliquidInstrumentInfo,
    HyperliquidPositionData, HyperliquidTradeKey, LatencyModel, parse_position_status_report,
};
pub use parse::{
    deserialize_decimal_from_str, deserialize_optional_decimal_from_str, ensure_min_notional,
    normalize_order, normalize_price, normalize_quantity, round_down_to_step, round_down_to_tick,
    serialize_decimal_as_str, serialize_optional_decimal_as_str,
};
pub use risk::{
    RiskLimits, RiskViolation, calculate_leverage, validate_limit_order, validate_one_way_mode,
    validate_reduce_only,
};
