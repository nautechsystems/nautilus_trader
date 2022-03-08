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

# isort:skip_file

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

from nautilus_trader.model.c_enums.account_type import AccountType                         # noqa F401 (being used)
from nautilus_trader.model.c_enums.account_type import AccountTypeParser                   # noqa F401 (being used)
from nautilus_trader.model.c_enums.aggregation_source import AggregationSource             # noqa F401 (being used)
from nautilus_trader.model.c_enums.aggregation_source import AggregationSourceParser       # noqa F401 (being used)
from nautilus_trader.model.c_enums.aggressor_side import AggressorSide                     # noqa F401 (being used)
from nautilus_trader.model.c_enums.aggressor_side import AggressorSideParser               # noqa F401 (being used)
from nautilus_trader.model.c_enums.asset_class import AssetClass                           # noqa F401 (being used)
from nautilus_trader.model.c_enums.asset_class import AssetClassParser                     # noqa F401 (being used)
from nautilus_trader.model.c_enums.asset_type import AssetType                             # noqa F401 (being used)
from nautilus_trader.model.c_enums.asset_type import AssetTypeParser                       # noqa F401 (being used)
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregation                   # noqa F401 (being used)
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregationParser             # noqa F401 (being used)
from nautilus_trader.model.c_enums.contingency_type import ContingencyType                 # noqa F401 (being used)
from nautilus_trader.model.c_enums.contingency_type import ContingencyTypeParser           # noqa F401 (being used)
from nautilus_trader.model.c_enums.currency_type import CurrencyType                       # noqa F401 (being used)
from nautilus_trader.model.c_enums.currency_type import CurrencyTypeParser                 # noqa F401 (being used)
from nautilus_trader.model.c_enums.depth_type import DepthType                             # noqa F401 (being used)
from nautilus_trader.model.c_enums.depth_type import DepthTypeParser                       # noqa F401 (being used)
from nautilus_trader.model.c_enums.instrument_close_type import InstrumentCloseType        # noqa F401 (being used)
from nautilus_trader.model.c_enums.instrument_close_type import InstrumentCloseTypeParser  # noqa F401 (being used)
from nautilus_trader.model.c_enums.instrument_status import InstrumentStatus               # noqa F401 (being used)
from nautilus_trader.model.c_enums.instrument_status import InstrumentStatusParser         # noqa F401 (being used)
from nautilus_trader.model.c_enums.liquidity_side import LiquiditySide                     # noqa F401 (being used)
from nautilus_trader.model.c_enums.liquidity_side import LiquiditySideParser               # noqa F401 (being used)
from nautilus_trader.model.c_enums.oms_type import OMSType                                 # noqa F401 (being used)
from nautilus_trader.model.c_enums.oms_type import OMSTypeParser                           # noqa F401 (being used)
from nautilus_trader.model.c_enums.option_kind import OptionKind                           # noqa F401 (being used)
from nautilus_trader.model.c_enums.option_kind import OptionKindParser                     # noqa F401 (being used)
from nautilus_trader.model.c_enums.order_side import OrderSide                             # noqa F401 (being used)
from nautilus_trader.model.c_enums.order_side import OrderSideParser                       # noqa F401 (being used)
from nautilus_trader.model.c_enums.order_status import OrderStatus                         # noqa F401 (being used)
from nautilus_trader.model.c_enums.order_status import OrderStatusParser                   # noqa F401 (being used)
from nautilus_trader.model.c_enums.order_type import OrderType                             # noqa F401 (being used)
from nautilus_trader.model.c_enums.order_type import OrderTypeParser                       # noqa F401 (being used)
from nautilus_trader.model.c_enums.book_type import BookType                               # noqa F401 (being used)
from nautilus_trader.model.c_enums.book_type import BookTypeParser                         # noqa F401 (being used)
from nautilus_trader.model.c_enums.book_action import BookAction                           # noqa F401 (being used)
from nautilus_trader.model.c_enums.book_action import BookActionParser                     # noqa F401 (being used)
from nautilus_trader.model.c_enums.position_side import PositionSide                       # noqa F401 (being used)
from nautilus_trader.model.c_enums.position_side import PositionSideParser                 # noqa F401 (being used)
from nautilus_trader.model.c_enums.price_type import PriceType                             # noqa F401 (being used)
from nautilus_trader.model.c_enums.price_type import PriceTypeParser                       # noqa F401 (being used)
from nautilus_trader.model.c_enums.time_in_force import TimeInForce                        # noqa F401 (being used)
from nautilus_trader.model.c_enums.time_in_force import TimeInForceParser                  # noqa F401 (being used)
from nautilus_trader.model.c_enums.trigger_type import TriggerType                         # noqa F401 (being used)
from nautilus_trader.model.c_enums.trigger_type import TriggerTypeParser                   # noqa F401 (being used)
from nautilus_trader.model.c_enums.trading_state import TradingState                       # noqa F401 (being used)
from nautilus_trader.model.c_enums.trading_state import TradingStateParser                 # noqa F401 (being used)
from nautilus_trader.model.c_enums.trailing_offset_type import TrailingOffsetType          # noqa F401 (being used)
from nautilus_trader.model.c_enums.trailing_offset_type import TrailingOffsetTypeParser    # noqa F401 (being used)
from nautilus_trader.model.c_enums.venue_status import VenueStatus                         # noqa F401 (being used)
from nautilus_trader.model.c_enums.venue_status import VenueStatusParser                   # noqa F401 (being used)
