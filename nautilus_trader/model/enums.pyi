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
