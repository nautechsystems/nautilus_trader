# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

    class AccountType:
        CASH: int = 1
        MARGIN: int = 2
        BETTING: int = 3

    class AggregationSource:
        EXTERNAL: int = 1
        INTERNAL: int = 2

    class AggressorSide:
        NO_AGGRESSOR: int = 0
        BUYER: int = 1
        SELLER: int = 2

    class AssetClass:
        FX: int = 1
        EQUITY: int = 2
        COMMODITY: int = 3
        DEBT: int = 4
        INDEX: int = 5
        CRYPTOCURRENCY: int = 6
        ALTERNATIVE: int = 7

    class BookAction:
        ADD: int = 1
        UPDATE: int = 2
        DELETE: int = 3
        CLEAR: int = 4

    class BookType:
        L1_MBP: int = 1
        L2_MBP: int = 2
        L3_MBO: int = 3

    class ContingencyType:
        NO_CONTINGENCY: int = 0
        OCO: int = 1
        OTO: int = 2
        OUO: int = 3

    class CurrencyType:
        CRYPTO: int = 1
        FIAT: int = 2
        COMMODITY_BACKED: int = 3

    class InstrumentClass:
        SPOT: int = 1
        SWAP: int = 2
        FUTURE: int = 3
        FUTURE_SPREAD: int = 4
        FORWARD: int = 5
        CFD: int = 6
        BOND: int = 7
        OPTION: int = 8
        OPTION_SPREAD: int = 9
        WARRANT: int = 10
        SPORTS_BETTING: int = 11
        BINARY_OPTION: int = 12

    class InstrumentCloseType:
        END_OF_SESSION: int = 1
        CONTRACT_EXPIRED: int = 2

    class LiquiditySide:
        NO_LIQUIDITY_SIDE: int = 0
        MAKER: int = 1
        TAKER: int = 2

    class MarketStatus:
        OPEN: int = 1
        CLOSED: int = 2
        PAUSED: int = 3
        SUSPENDED: int = 5
        NOT_AVAILABLE: int = 6

    class MarketStatusAction:
        NONE: int = 0
        PRE_OPEN: int = 1
        PRE_CROSS: int = 2
        QUOTING: int = 3
        CROSS: int = 4
        ROTATION: int = 5
        NEW_PRICE_INDICATION: int = 6
        TRADING: int = 7
        HALT: int = 8
        PAUSE: int = 9
        SUSPEND: int = 10
        PRE_CLOSE: int = 11
        CLOSE: int = 12
        POST_CLOSE: int = 13
        SHORT_SELL_RESTRICTION_CHANGE: int = 14
        NOT_AVAILABLE_FOR_TRADING: int = 15

    class OmsType:
        UNSPECIFIED: int = 0
        NETTING: int = 1
        HEDGING: int = 2

    class OptionKind:
        CALL: int = 1
        PUT: int = 2

    class OrderSide:
        NO_ORDER_SIDE: int = 0
        BUY: int = 1
        SELL: int = 2

    class OrderStatus:
        INITIALIZED: int = 1
        DENIED: int = 2
        EMULATED: int = 3
        RELEASED: int = 4
        SUBMITTED: int = 5
        ACCEPTED: int = 6
        REJECTED: int = 7
        CANCELED: int = 8
        EXPIRED: int = 9
        TRIGGERED: int = 10
        PENDING_UPDATE: int = 11
        PENDING_CANCEL: int = 12
        PARTIALLY_FILLED: int = 13
        FILLED: int = 14

    class OrderType:
        MARKET: int = 1
        LIMIT: int = 2
        STOP_MARKET: int = 3
        STOP_LIMIT: int = 4
        MARKET_TO_LIMIT: int = 5
        MARKET_IF_TOUCHED: int = 6
        LIMIT_IF_TOUCHED: int = 7
        TRAILING_STOP_MARKET: int = 8
        TRAILING_STOP_LIMIT: int = 9

    class PositionSide:
        NO_POSITION_SIDE: int = 0
        FLAT: int = 1
        LONG: int = 2
        SHORT: int = 3

    class PriceType:
        BID: int = 1
        ASK: int = 2
        MID: int = 3
        LAST: int = 4

    class RecordFlag:
        F_LAST: int = 128
        F_TOB: int = 64
        F_SNAPSHOT: int = 32
        F_MBP: int = 16
        RESERVED_2: int = 8
        RESERVED_1: int = 4

    class TimeInForce:
        GTC: int = 1
        IOC: int = 2
        FOK: int = 3
        GTD: int = 4
        DAY: int = 5
        AT_THE_OPEN: int = 6
        AT_THE_CLOSE: int = 7

    class TradingState:
        ACTIVE: int = 1
        HALTED: int = 2
        REDUCING: int = 3

    class TrailingOffsetType:
        NO_TRAILING_OFFSET: int = 0
        PRICE: int = 1
        BASIS_POINTS: int = 2
        TICKS: int = 3
        PRICE_TIER: int = 4

    class TriggerType:
        NO_TRIGGER: int = 0
        DEFAULT: int = 1
        BID_ASK: int = 2
        LAST_TRADE: int = 3
        DOUBLE_LAST: int = 4
        DOUBLE_BID_ASK: int = 5
        LAST_OR_BID_ASK: int = 6
        MID_POINT: int = 7
        MARK_PRICE: int = 8
        INDEX_PRICE: int = 9
