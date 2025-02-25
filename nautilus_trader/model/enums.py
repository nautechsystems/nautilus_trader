# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from enum import Enum
from enum import unique
from typing import TYPE_CHECKING

from nautilus_trader.core.rust.model import AccountType
from nautilus_trader.core.rust.model import AggregationSource
from nautilus_trader.core.rust.model import AggressorSide
from nautilus_trader.core.rust.model import AssetClass
from nautilus_trader.core.rust.model import BookAction
from nautilus_trader.core.rust.model import BookType
from nautilus_trader.core.rust.model import ContingencyType
from nautilus_trader.core.rust.model import CurrencyType
from nautilus_trader.core.rust.model import InstrumentClass
from nautilus_trader.core.rust.model import InstrumentCloseType
from nautilus_trader.core.rust.model import LiquiditySide
from nautilus_trader.core.rust.model import MarketStatus
from nautilus_trader.core.rust.model import MarketStatusAction
from nautilus_trader.core.rust.model import OmsType
from nautilus_trader.core.rust.model import OptionKind
from nautilus_trader.core.rust.model import OrderSide
from nautilus_trader.core.rust.model import OrderStatus
from nautilus_trader.core.rust.model import OrderType
from nautilus_trader.core.rust.model import PositionSide
from nautilus_trader.core.rust.model import PriceType
from nautilus_trader.core.rust.model import RecordFlag
from nautilus_trader.core.rust.model import TimeInForce
from nautilus_trader.core.rust.model import TradingState
from nautilus_trader.core.rust.model import TrailingOffsetType
from nautilus_trader.core.rust.model import TriggerType
from nautilus_trader.model.data import BarAggregation
from nautilus_trader.model.functions import account_type_from_str
from nautilus_trader.model.functions import account_type_to_str
from nautilus_trader.model.functions import aggregation_source_from_str
from nautilus_trader.model.functions import aggregation_source_to_str
from nautilus_trader.model.functions import aggressor_side_from_str
from nautilus_trader.model.functions import aggressor_side_to_str
from nautilus_trader.model.functions import asset_class_from_str
from nautilus_trader.model.functions import asset_class_to_str
from nautilus_trader.model.functions import bar_aggregation_from_str
from nautilus_trader.model.functions import bar_aggregation_to_str
from nautilus_trader.model.functions import book_action_from_str
from nautilus_trader.model.functions import book_action_to_str
from nautilus_trader.model.functions import book_type_from_str
from nautilus_trader.model.functions import book_type_to_str
from nautilus_trader.model.functions import contingency_type_from_str
from nautilus_trader.model.functions import contingency_type_to_str
from nautilus_trader.model.functions import currency_type_from_str
from nautilus_trader.model.functions import currency_type_to_str
from nautilus_trader.model.functions import instrument_class_from_str
from nautilus_trader.model.functions import instrument_class_to_str
from nautilus_trader.model.functions import instrument_close_type_from_str
from nautilus_trader.model.functions import instrument_close_type_to_str
from nautilus_trader.model.functions import liquidity_side_from_str
from nautilus_trader.model.functions import liquidity_side_to_str
from nautilus_trader.model.functions import market_status_action_from_str
from nautilus_trader.model.functions import market_status_action_to_str
from nautilus_trader.model.functions import market_status_from_str
from nautilus_trader.model.functions import market_status_to_str
from nautilus_trader.model.functions import oms_type_from_str
from nautilus_trader.model.functions import oms_type_to_str
from nautilus_trader.model.functions import option_kind_from_str
from nautilus_trader.model.functions import option_kind_to_str
from nautilus_trader.model.functions import order_side_from_str
from nautilus_trader.model.functions import order_side_to_str
from nautilus_trader.model.functions import order_status_from_str
from nautilus_trader.model.functions import order_status_to_str
from nautilus_trader.model.functions import order_type_from_str
from nautilus_trader.model.functions import order_type_to_str
from nautilus_trader.model.functions import position_side_from_str
from nautilus_trader.model.functions import position_side_to_str
from nautilus_trader.model.functions import price_type_from_str
from nautilus_trader.model.functions import price_type_to_str
from nautilus_trader.model.functions import record_flag_from_str
from nautilus_trader.model.functions import record_flag_to_str
from nautilus_trader.model.functions import time_in_force_from_str
from nautilus_trader.model.functions import time_in_force_to_str
from nautilus_trader.model.functions import trading_state_from_str
from nautilus_trader.model.functions import trading_state_to_str
from nautilus_trader.model.functions import trailing_offset_type_from_str
from nautilus_trader.model.functions import trailing_offset_type_to_str
from nautilus_trader.model.functions import trigger_type_from_str
from nautilus_trader.model.functions import trigger_type_to_str


__all__ = [
    "AccountType",
    "AggregationSource",
    "AggressorSide",
    "AssetClass",
    "BarAggregation",
    "BookAction",
    "BookType",
    "ContingencyType",
    "CurrencyType",
    "InstrumentClass",
    "InstrumentCloseType",
    "LiquiditySide",
    "MarketStatus",
    "MarketStatusAction",
    "OmsType",
    "OptionKind",
    "OrderSide",
    "OrderStatus",
    "OrderType",
    "PositionSide",
    "PriceType",
    "RecordFlag",
    "TimeInForce",
    "TradingState",
    "TrailingOffsetType",
    "TriggerType",
    "account_type_from_str",
    "account_type_to_str",
    "aggregation_source_from_str",
    "aggregation_source_to_str",
    "aggressor_side_from_str",
    "aggressor_side_to_str",
    "asset_class_from_str",
    "asset_class_to_str",
    "bar_aggregation_from_str",
    "bar_aggregation_to_str",
    "book_action_from_str",
    "book_action_to_str",
    "book_type_from_str",
    "book_type_to_str",
    "contingency_type_from_str",
    "contingency_type_to_str",
    "currency_type_from_str",
    "currency_type_to_str",
    "instrument_class_from_str",
    "instrument_class_to_str",
    "instrument_close_type_from_str",
    "instrument_close_type_to_str",
    "liquidity_side_from_str",
    "liquidity_side_to_str",
    "market_status_action_from_str",
    "market_status_action_to_str",
    "market_status_from_str",
    "market_status_to_str",
    "oms_type_from_str",
    "oms_type_to_str",
    "option_kind_from_str",
    "option_kind_to_str",
    "order_side_from_str",
    "order_side_to_str",
    "order_status_from_str",
    "order_status_to_str",
    "order_type_from_str",
    "order_type_to_str",
    "position_side_from_str",
    "position_side_to_str",
    "price_type_from_str",
    "price_type_to_str",
    "record_flag_from_str",
    "record_flag_to_str",
    "time_in_force_from_str",
    "time_in_force_to_str",
    "trading_state_from_str",
    "trading_state_to_str",
    "trailing_offset_type_from_str",
    "trailing_offset_type_to_str",
    "trigger_type_from_str",
    "trigger_type_to_str",
]

# mypy: disable-error-code=no-redef

if TYPE_CHECKING:

    @unique
    class AccountType(Enum):
        CASH = 1
        MARGIN = 2
        BETTING = 3

    @unique
    class AggregationSource(Enum):
        EXTERNAL = 1
        INTERNAL = 2

    @unique
    class AggressorSide(Enum):
        NO_AGGRESSOR = 0
        BUYER = 1
        SELLER = 2

    @unique
    class AssetClass(Enum):
        FX = 1
        EQUITY = 2
        COMMODITY = 3
        DEBT = 4
        INDEX = 5
        CRYPTOCURRENCY = 6
        ALTERNATIVE = 7

    @unique
    class BookAction(Enum):
        ADD = 1
        UPDATE = 2
        DELETE = 3
        CLEAR = 4

    @unique
    class BookType(Enum):
        L1_MBP = 1
        L2_MBP = 2
        L3_MBO = 3

    @unique
    class ContingencyType(Enum):
        NO_CONTINGENCY = 0
        OCO = 1
        OTO = 2
        OUO = 3

    @unique
    class CurrencyType(Enum):
        CRYPTO = 1
        FIAT = 2
        COMMODITY_BACKED = 3

    @unique
    class InstrumentClass(Enum):
        SPOT = 1
        SWAP = 2
        FUTURE = 3
        FUTURES_SPREAD = 4
        FORWARD = 5
        CFD = 6
        BOND = 7
        OPTION = 8
        OPTION_SPREAD = 9
        WARRANT = 10
        SPORTS_BETTING = 11
        BINARY_OPTION = 12

    @unique
    class InstrumentCloseType(Enum):
        END_OF_SESSION = 1
        CONTRACT_EXPIRED = 2

    @unique
    class LiquiditySide(Enum):
        NO_LIQUIDITY_SIDE = 0
        MAKER = 1
        TAKER = 2

    @unique
    class MarketStatus(Enum):
        OPEN = 1
        CLOSED = 2
        PAUSED = 3
        SUSPENDED = 5
        NOT_AVAILABLE = 6

    @unique
    class MarketStatusAction(Enum):
        NONE = 0
        PRE_OPEN = 1
        PRE_CROSS = 2
        QUOTING = 3
        CROSS = 4
        ROTATION = 5
        NEW_PRICE_INDICATION = 6
        TRADING = 7
        HALT = 8
        PAUSE = 9
        SUSPEND = 10
        PRE_CLOSE = 11
        CLOSE = 12
        POST_CLOSE = 13
        SHORT_SELL_RESTRICTION_CHANGE = 14
        NOT_AVAILABLE_FOR_TRADING = 15

    @unique
    class OmsType(Enum):
        UNSPECIFIED = 0
        NETTING = 1
        HEDGING = 2

    @unique
    class OptionKind(Enum):
        CALL = 1
        PUT = 2

    @unique
    class OrderSide(Enum):
        NO_ORDER_SIDE = 0
        BUY = 1
        SELL = 2

    @unique
    class OrderStatus(Enum):
        INITIALIZED = 1
        DENIED = 2
        EMULATED = 3
        RELEASED = 4
        SUBMITTED = 5
        ACCEPTED = 6
        REJECTED = 7
        CANCELED = 8
        EXPIRED = 9
        TRIGGERED = 10
        PENDING_UPDATE = 11
        PENDING_CANCEL = 12
        PARTIALLY_FILLED = 13
        FILLED = 14

    @unique
    class OrderType(Enum):
        MARKET = 1
        LIMIT = 2
        STOP_MARKET = 3
        STOP_LIMIT = 4
        MARKET_TO_LIMIT = 5
        MARKET_IF_TOUCHED = 6
        LIMIT_IF_TOUCHED = 7
        TRAILING_STOP_MARKET = 8
        TRAILING_STOP_LIMIT = 9

    @unique
    class PositionSide(Enum):
        NO_POSITION_SIDE = 0
        FLAT = 1
        LONG = 2
        SHORT = 3

    @unique
    class PriceType(Enum):
        BID = 1
        ASK = 2
        MID = 3
        LAST = 4
        MARK = 5

    @unique
    class RecordFlag(Enum):
        F_LAST = 128
        F_TOB = 64
        F_SNAPSHOT = 32
        F_MBP = 16
        RESERVED_2 = 8
        RESERVED_1 = 4

    @unique
    class TimeInForce(Enum):
        GTC = 1
        IOC = 2
        FOK = 3
        GTD = 4
        DAY = 5
        AT_THE_OPEN = 6
        AT_THE_CLOSE = 7

    @unique
    class TradingState(Enum):
        ACTIVE = 1
        HALTED = 2
        REDUCING = 3

    @unique
    class TrailingOffsetType(Enum):
        NO_TRAILING_OFFSET = 0
        PRICE = 1
        BASIS_POINTS = 2
        TICKS = 3
        PRICE_TIER = 4

    @unique
    class TriggerType(Enum):
        NO_TRIGGER = 0
        DEFAULT = 1
        BID_ASK = 2
        LAST_PRICE = 3
        DOUBLE_LAST = 4
        DOUBLE_BID_ASK = 5
        LAST_OR_BID_ASK = 6
        MID_POINT = 7
        MARK_PRICE = 8
        INDEX_PRICE = 9
