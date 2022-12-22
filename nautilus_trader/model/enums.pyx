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

from nautilus_trader.core.rust.enums import AggressorSide
from nautilus_trader.core.rust.model import AccountType
from nautilus_trader.core.rust.model import AggregationSource
from nautilus_trader.model.c_enums.asset_class import AssetClass
from nautilus_trader.model.c_enums.asset_class import AssetClassParser
from nautilus_trader.model.c_enums.asset_type import AssetType
from nautilus_trader.model.c_enums.asset_type import AssetTypeParser
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregationParser
from nautilus_trader.model.c_enums.book_action import BookAction
from nautilus_trader.model.c_enums.book_action import BookActionParser
from nautilus_trader.model.c_enums.book_type import BookType
from nautilus_trader.model.c_enums.book_type import BookTypeParser
from nautilus_trader.model.c_enums.contingency_type import ContingencyType
from nautilus_trader.model.c_enums.contingency_type import ContingencyTypeParser
from nautilus_trader.model.c_enums.currency_type import CurrencyType
from nautilus_trader.model.c_enums.currency_type import CurrencyTypeParser
from nautilus_trader.model.c_enums.depth_type import DepthType
from nautilus_trader.model.c_enums.depth_type import DepthTypeParser
from nautilus_trader.model.c_enums.instrument_close_type import InstrumentCloseType
from nautilus_trader.model.c_enums.instrument_close_type import InstrumentCloseTypeParser
from nautilus_trader.model.c_enums.instrument_status import InstrumentStatus
from nautilus_trader.model.c_enums.instrument_status import InstrumentStatusParser
from nautilus_trader.model.c_enums.liquidity_side import LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side import LiquiditySideParser
from nautilus_trader.model.c_enums.oms_type import OMSType
from nautilus_trader.model.c_enums.oms_type import OMSTypeParser
from nautilus_trader.model.c_enums.option_kind import OptionKind
from nautilus_trader.model.c_enums.option_kind import OptionKindParser
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


__all__ = [
    "AccountType",
    "AggregationSource",
    "AggressorSide",
    "AssetClass",
    "AssetClassParser",
    "AssetType",
    "AssetTypeParser",
    "BarAggregation",
    "BarAggregationParser",
    "ContingencyType",
    "ContingencyTypeParser",
    "CurrencyType",
    "CurrencyTypeParser",
    "DepthType",
    "DepthTypeParser",
    "InstrumentCloseType",
    "InstrumentCloseTypeParser",
    "InstrumentStatus",
    "InstrumentStatusParser",
    "LiquiditySide",
    "LiquiditySideParser",
    "OMSType",
    "OMSTypeParser",
    "OptionKind",
    "OptionKindParser",
    "OrderSide",
    "OrderSideParser",
    "OrderStatus",
    "OrderStatusParser",
    "OrderType",
    "OrderTypeParser",
    "BookType",
    "BookTypeParser",
    "BookAction",
    "BookActionParser",
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
]
