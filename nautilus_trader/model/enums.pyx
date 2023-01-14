# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.data.bar_aggregation import BarAggregation
from nautilus_trader.model.enums_c import account_type_from_str
from nautilus_trader.model.enums_c import account_type_to_str
from nautilus_trader.model.enums_c import aggregation_source_from_str
from nautilus_trader.model.enums_c import aggregation_source_to_str
from nautilus_trader.model.enums_c import aggressor_side_from_str
from nautilus_trader.model.enums_c import aggressor_side_to_str
from nautilus_trader.model.enums_c import asset_class_from_str
from nautilus_trader.model.enums_c import asset_class_to_str
from nautilus_trader.model.enums_c import asset_type_from_str
from nautilus_trader.model.enums_c import asset_type_to_str
from nautilus_trader.model.enums_c import bar_aggregation_from_str
from nautilus_trader.model.enums_c import bar_aggregation_to_str
from nautilus_trader.model.enums_c import book_action_from_str
from nautilus_trader.model.enums_c import book_action_to_str
from nautilus_trader.model.enums_c import book_type_from_str
from nautilus_trader.model.enums_c import book_type_to_str
from nautilus_trader.model.enums_c import contingency_type_from_str
from nautilus_trader.model.enums_c import contingency_type_to_str
from nautilus_trader.model.enums_c import currency_type_from_str
from nautilus_trader.model.enums_c import currency_type_to_str
from nautilus_trader.model.enums_c import depth_type_from_str
from nautilus_trader.model.enums_c import depth_type_to_str
from nautilus_trader.model.enums_c import instrument_close_type_from_str
from nautilus_trader.model.enums_c import instrument_close_type_to_str
from nautilus_trader.model.enums_c import liquidity_side_from_str
from nautilus_trader.model.enums_c import liquidity_side_to_str
from nautilus_trader.model.enums_c import market_status_from_str
from nautilus_trader.model.enums_c import market_status_to_str
from nautilus_trader.model.enums_c import oms_type_from_str
from nautilus_trader.model.enums_c import oms_type_to_str
from nautilus_trader.model.enums_c import option_kind_from_str
from nautilus_trader.model.enums_c import option_kind_to_str
from nautilus_trader.model.enums_c import order_side_from_str
from nautilus_trader.model.enums_c import order_side_to_str
from nautilus_trader.model.enums_c import order_status_from_str
from nautilus_trader.model.enums_c import order_status_to_str
from nautilus_trader.model.enums_c import order_type_from_str
from nautilus_trader.model.enums_c import order_type_to_str
from nautilus_trader.model.enums_c import position_side_from_str
from nautilus_trader.model.enums_c import position_side_to_str
from nautilus_trader.model.enums_c import price_type_from_str
from nautilus_trader.model.enums_c import price_type_to_str
from nautilus_trader.model.enums_c import time_in_force_from_str
from nautilus_trader.model.enums_c import time_in_force_to_str
from nautilus_trader.model.enums_c import trading_state_from_str
from nautilus_trader.model.enums_c import trading_state_to_str
from nautilus_trader.model.enums_c import trailing_offset_type_from_str
from nautilus_trader.model.enums_c import trailing_offset_type_to_str
from nautilus_trader.model.enums_c import trigger_type_from_str
from nautilus_trader.model.enums_c import trigger_type_to_str


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
    "TradingState",
    "TrailingOffsetType",
    "TriggerType",
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
