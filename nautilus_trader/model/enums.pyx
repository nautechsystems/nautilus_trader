# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

"""Defines the enums of the trading domain model."""

from nautilus_trader.core.rust.c_enums.bar_aggregation import BarAggregation
from nautilus_trader.core.rust.model import AccountType
from nautilus_trader.core.rust.model import AggregationSource
from nautilus_trader.core.rust.model import AggressorSide
from nautilus_trader.core.rust.model import AssetClass
from nautilus_trader.core.rust.model import AssetType
from nautilus_trader.core.rust.model import BookAction
from nautilus_trader.core.rust.model import BookType
from nautilus_trader.core.rust.model import ContingencyType
from nautilus_trader.core.rust.model import CurrencyType
from nautilus_trader.core.rust.model import DepthType
from nautilus_trader.core.rust.model import InstrumentCloseType
from nautilus_trader.core.rust.model import LiquiditySide
from nautilus_trader.core.rust.model import MarketStatus
from nautilus_trader.core.rust.model import OmsType
from nautilus_trader.core.rust.model import OptionKind
from nautilus_trader.core.rust.model import OrderSide
from nautilus_trader.core.rust.model import OrderStatus
from nautilus_trader.core.rust.model import OrderType
from nautilus_trader.core.rust.model import PositionSide
from nautilus_trader.core.rust.model import PriceType
from nautilus_trader.core.rust.model import TimeInForce
from nautilus_trader.core.rust.model import TradingState
from nautilus_trader.core.rust.model import TrailingOffsetType
from nautilus_trader.core.rust.model import TriggerType

from nautilus_trader.core.rust.enums cimport account_type_from_str
from nautilus_trader.core.rust.enums cimport account_type_to_str
from nautilus_trader.core.rust.enums cimport aggregation_source_from_str
from nautilus_trader.core.rust.enums cimport aggregation_source_to_str
from nautilus_trader.core.rust.enums cimport aggressor_side_from_str
from nautilus_trader.core.rust.enums cimport aggressor_side_to_str
from nautilus_trader.core.rust.enums cimport asset_class_from_str
from nautilus_trader.core.rust.enums cimport asset_class_to_str
from nautilus_trader.core.rust.enums cimport asset_type_from_str
from nautilus_trader.core.rust.enums cimport asset_type_to_str
from nautilus_trader.core.rust.enums cimport bar_aggregation_from_str
from nautilus_trader.core.rust.enums cimport bar_aggregation_to_str
from nautilus_trader.core.rust.enums cimport book_action_from_str
from nautilus_trader.core.rust.enums cimport book_action_to_str
from nautilus_trader.core.rust.enums cimport book_type_from_str
from nautilus_trader.core.rust.enums cimport book_type_to_str
from nautilus_trader.core.rust.enums cimport contingency_type_from_str
from nautilus_trader.core.rust.enums cimport contingency_type_to_str
from nautilus_trader.core.rust.enums cimport currency_type_from_str
from nautilus_trader.core.rust.enums cimport currency_type_to_str
from nautilus_trader.core.rust.enums cimport depth_type_from_str
from nautilus_trader.core.rust.enums cimport depth_type_to_str
from nautilus_trader.core.rust.enums cimport instrument_close_type_from_str
from nautilus_trader.core.rust.enums cimport instrument_close_type_to_str
from nautilus_trader.core.rust.enums cimport liquidity_side_from_str
from nautilus_trader.core.rust.enums cimport liquidity_side_to_str
from nautilus_trader.core.rust.enums cimport market_status_from_str
from nautilus_trader.core.rust.enums cimport market_status_to_str
from nautilus_trader.core.rust.enums cimport oms_type_from_str
from nautilus_trader.core.rust.enums cimport oms_type_to_str
from nautilus_trader.core.rust.enums cimport option_kind_from_str
from nautilus_trader.core.rust.enums cimport option_kind_to_str
from nautilus_trader.core.rust.enums cimport order_side_from_str
from nautilus_trader.core.rust.enums cimport order_side_to_str
from nautilus_trader.core.rust.enums cimport order_status_from_str
from nautilus_trader.core.rust.enums cimport order_status_to_str
from nautilus_trader.core.rust.enums cimport order_type_from_str
from nautilus_trader.core.rust.enums cimport order_type_to_str
from nautilus_trader.core.rust.enums cimport position_side_from_str
from nautilus_trader.core.rust.enums cimport position_side_to_str
from nautilus_trader.core.rust.enums cimport price_type_from_str
from nautilus_trader.core.rust.enums cimport price_type_to_str
from nautilus_trader.core.rust.enums cimport time_in_force_from_str
from nautilus_trader.core.rust.enums cimport time_in_force_to_str
from nautilus_trader.core.rust.enums cimport trading_state_from_str
from nautilus_trader.core.rust.enums cimport trading_state_to_str
from nautilus_trader.core.rust.enums cimport trailing_offset_type_from_str
from nautilus_trader.core.rust.enums cimport trailing_offset_type_to_str
from nautilus_trader.core.rust.enums cimport trigger_type_from_str
from nautilus_trader.core.rust.enums cimport trigger_type_to_str


__all__ = [
    "AccountType",
    "AggregationSource",
    "AggressorSide",
    "AssetClass",
    "AssetType",
    "BarAggregation",
    "BookAction",
    "BookType",
    "ContingencyType",
    "CurrencyType",
    "DepthType",
    "InstrumentCloseType",
    "LiquiditySide",
    "MarketStatus",
    "OmsType",
    "OptionKind",
    "OrderSide",
    "OrderStatus",
    "OrderType",
    "PositionSide",
    "PriceType",
    "TimeInForce",
    "TriggerType",
    "TradingState",
    "TrailingOffsetType",
    "account_type_to_str",
    "account_type_from_str",
    "aggregation_source_to_str",
    "aggregation_source_from_str",
    "aggressor_side_to_str",
    "aggressor_side_from_str",
    "asset_class_to_str",
    "asset_class_from_str",
    "asset_type_to_str",
    "asset_type_from_str",
    "bar_aggregation_to_str",
    "bar_aggregation_from_str",
    "book_action_to_str",
    "book_action_from_str",
    "book_type_to_str",
    "book_type_from_str",
    "contingency_type_to_str",
    "contingency_type_from_str",
    "currency_type_to_str",
    "currency_type_from_str",
    "depth_type_to_str",
    "depth_type_from_str",
    "instrument_close_type_to_str",
    "instrument_close_type_from_str",
    "liquidity_side_to_str",
    "liquidity_side_from_str",
    "market_status_to_str",
    "market_status_from_str",
    "oms_type_to_str",
    "oms_type_from_str",
    "option_kind_to_str",
    "option_kind_from_str",
    "order_side_to_str",
    "order_side_from_str",
    "order_status_to_str",
    "order_status_from_str",
    "order_type_to_str",
    "order_type_from_str",
    "position_side_to_str",
    "position_side_from_str",
    "price_type_to_str",
    "price_type_from_str",
    "time_in_force_to_str",
    "time_in_force_from_str",
    "trading_state_to_str",
    "trading_state_from_str",
    "trailing_offset_type_to_str",
    "trailing_offset_type_from_str",
    "trigger_type_to_str",
    "trigger_type_from_str",
]
