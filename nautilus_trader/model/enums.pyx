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

"""
Defines the enums of the trading domain model.

Account Type
------------
Represents a trading account type.

>>> from nautilus_trader.model.enums import AccountType
>>> AccountType.CASH
<AccountType.CASH: 1>
>>> AccountType.MARGIN
<AccountType.MARGIN: 2>
>>> AccountType.BETTING
<AccountType.BETTING: 3>

Aggregation Source
------------------
Represents where a bar was aggregated in relation to the platform.

>>> from nautilus_trader.model.enums import AggregationSource
>>> AggregationSource.EXTERNAL  # Bar was aggregated externally to the platform
<AggregationSource.EXTERNAL: 1>
>>> AggregationSource.INTERNAL  # Bar was aggregated internally within the platform
<AggregationSource.INTERNAL: 2>

Aggregssor Side
---------------
Represents the order side of the aggressor (liquidity taker) for a particular trade.

>>> from nautilus_trader.model.enums import AggressorSide
>>> AggressorSide.BUY
<AggressorSide.BUY: 1>
>>> AggressorSide.SELL
<AggressorSide.SELL: 2>

Asset Class
-----------
Represents a group of investment vehicles with similar properties and risk profiles.

>>> from nautilus_trader.model.enums import AssetClass
>>> AssetClass.FX
<AssetClass.FX: 1>
>>> AssetClass.EQUITY
<AssetClass.EQUITY: 2>
>>> AssetClass.COMMODITY
<AssetClass.COMMODITY: 3>
>>> AssetClass.METAL
<AssetClass.METAL: 4>
>>> AssetClass.ENERGY
<AssetClass.ENERGY: 5>
>>> AssetClass.BOND
<AssetClass.BOND: 6>
>>> AssetClass.INDEX
<AssetClass.INDEX: 7>
>>> AssetClass.CRYPTO
<AssetClass.CRYPTO: 8>
>>> AssetClass.BETTING
<AssetClass.BETTING: 9>

Asset Type
----------
Represents a group of financial product types.

>>> from nautilus_trader.model.enums import AssetType
>>> AssetType.SPOT
<AssetType.SPOT: 1>
>>> AssetType.SWAP
<AssetType.SWAP: 2>
>>> AssetType.FUTURE
<AssetType.FUTURE: 3>
>>> AssetType.FORWARD
<AssetType.FORWARD: 4>
>>> AssetType.CFD
<AssetType.CFD: 5>
>>> AssetType.OPTION
<AssetType.OPTION: 6>
>>> AssetType.WARRANT
<AssetType.WARRANT: 7>

Bar Aggregation
---------------
Represents a method of aggregating an OHLCV bar.

>>> from nautilus_trader.model.enums import BarAggregation
>>> BarAggregation.TICK
<BarAggregation.TICK: 1>
>>> BarAggregation.TICK_IMBALANCE
<BarAggregation.TICK_IMBALANCE: 2>
>>> BarAggregation.TICK_RUNS
<BarAggregation.TICK_RUNS: 3>
>>> BarAggregation.VOLUME
<BarAggregation.VOLUME: 4>
>>> BarAggregation.VOLUME_IMBALANCE
<BarAggregation.VOLUME_IMBALANCE: 5>
>>> BarAggregation.VOLUME_RUNS
<BarAggregation.VOLUME_RUNS: 6>
>>> BarAggregation.VALUE
<BarAggregation.VALUE: 7>
>>> BarAggregation.VALUE_IMBALANCE
<BarAggregation.VALUE_IMBALANCE: 8>
>>> BarAggregation.VALUE_RUNS
<BarAggregation.VALUE_RUNS: 9>
>>> BarAggregation.SECOND
<BarAggregation.SECOND: 10>
>>> BarAggregation.MINUTE
<BarAggregation.MINUTE: 11>
>>> BarAggregation.HOUR
<BarAggregation.HOUR: 12>
>>> BarAggregation.DAY
<BarAggregation.DAY: 13>

"""

from nautilus_trader.model.c_enums.account_type import AccountType
from nautilus_trader.model.c_enums.account_type import AccountTypeParser
from nautilus_trader.model.c_enums.aggregation_source import AggregationSource
from nautilus_trader.model.c_enums.aggregation_source import AggregationSourceParser
from nautilus_trader.model.c_enums.aggressor_side import AggressorSide
from nautilus_trader.model.c_enums.aggressor_side import AggressorSideParser
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


__all__ = [
    "AccountType",
    "AccountTypeParser",
    "AggregationSource",
    "AggregationSourceParser",
    "AggressorSide",
    "AggressorSideParser",
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
]
