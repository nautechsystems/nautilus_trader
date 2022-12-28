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
from nautilus_trader.core.rust.model import LiquiditySide
from nautilus_trader.core.rust.model import OmsType
from nautilus_trader.core.rust.model import OptionKind
from nautilus_trader.model.c_enums.instrument_close_type import InstrumentCloseType
from nautilus_trader.model.c_enums.instrument_close_type import InstrumentCloseTypeParser
from nautilus_trader.model.c_enums.instrument_status import InstrumentStatus
from nautilus_trader.model.c_enums.instrument_status import InstrumentStatusParser
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.order_side import OrderSideParser
from nautilus_trader.model.c_enums.order_status import OrderStatus
from nautilus_trader.model.c_enums.order_status import OrderStatusParser
from nautilus_trader.model.c_enums.order_type import OrderType
from nautilus_trader.model.c_enums.order_type import OrderTypeParser
from nautilus_trader.model.c_enums.position_side import PositionSide
from nautilus_trader.model.c_enums.position_side import PositionSideParser
from nautilus_trader.model.c_enums.price_type import PriceType
from nautilus_trader.model.c_enums.price_type import PriceTypeParser
from nautilus_trader.model.c_enums.time_in_force import TimeInForce
from nautilus_trader.model.c_enums.time_in_force import TimeInForceParser
from nautilus_trader.model.c_enums.trading_state import TradingState
from nautilus_trader.model.c_enums.trading_state import TradingStateParser
from nautilus_trader.model.c_enums.trailing_offset_type import TrailingOffsetType
from nautilus_trader.model.c_enums.trailing_offset_type import TrailingOffsetTypeParser
from nautilus_trader.model.c_enums.trigger_type import TriggerType
from nautilus_trader.model.c_enums.trigger_type import TriggerTypeParser
from nautilus_trader.model.c_enums.venue_status import VenueStatus
from nautilus_trader.model.c_enums.venue_status import VenueStatusParser

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
from nautilus_trader.core.rust.enums cimport liquidity_side_from_str
from nautilus_trader.core.rust.enums cimport liquidity_side_to_str
from nautilus_trader.core.rust.enums cimport oms_type_from_str
from nautilus_trader.core.rust.enums cimport oms_type_to_str
from nautilus_trader.core.rust.enums cimport option_kind_from_str
from nautilus_trader.core.rust.enums cimport option_kind_to_str


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
    "InstrumentCloseTypeParser",
    "InstrumentStatus",
    "InstrumentStatusParser",
    "LiquiditySide",
    "OmsType",
    "OptionKind",
    "OrderSide",
    "OrderSideParser",
    "OrderStatus",
    "OrderStatusParser",
    "OrderType",
    "OrderTypeParser",
    "PositionSide",
    "PositionSideParser",
    "PriceType",
    "PriceTypeParser",
    "TimeInForce",
    "TimeInForceParser",
    "TriggerType",
    "TriggerTypeParser",
    "TradingState",
    "TradingStateParser",
    "TrailingOffsetType",
    "TrailingOffsetTypeParser",
    "VenueStatus",
    "VenueStatusParser",
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
    "liquidity_side_to_str",
    "liquidity_side_from_str",
    "oms_type_to_str",
    "oms_type_from_str",
    "option_kind_to_str",
    "option_kind_from_str",
]
