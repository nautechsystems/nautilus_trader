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

from enum import Enum
from enum import IntFlag
from enum import unique


class DatabentoSchema(Enum):
    """
    Represents a Databento schema.
    """

    MBO = "mbo"
    MBP_1 = "mbp-1"
    MBP_10 = "mbp-10"
    TBBO = "tbbo"
    TRADES = "trades"
    OHLCV_1S = "ohlcv-1s"
    OHLCV_1M = "ohlcv-1m"
    OHLCV_1H = "ohlcv-1h"
    OHLCV_1D = "ohlcv-1d"
    OHLCV_EOD = "ohlcv-eod"
    DEFINITION = "definition"
    STATISTICS = "statistics"
    STATUS = "status"
    IMBALANCE = "imbalance"


class DatabentoRecordFlags(IntFlag):
    """
    Represents Databento record flags.

    F_LAST
        Last message in the packet from the venue for a given Databento `instrument_id`.
    F_SNAPSHOT
        Message sourced from a replay, such as a snapshot server.
    F_MBP
        Aggregated price level message, not an individual order.
    F_BAD_TS_RECV
        The `ts_recv` value is inaccurate (clock issues or reordering).

    Other bits are reserved and have no current meaning.

    """

    F_LAST = 128
    F_SNAPSHOT = 32
    F_MBP = 16
    F_BAD_TS_RECV = 8


@unique
class DatabentoInstrumentClass(Enum):
    """
    Represents a Databento instrument class.
    """

    BOND = "B"
    CALL = "C"
    FUTURE = "F"
    STOCK = "K"
    MIXED_SPREAD = "M"
    PUT = "P"
    FUTURE_SPREAD = "S"
    OPTION_SPREAD = "T"
    FX_SPOT = "X"


@unique
class DatabentoStatisticType(Enum):
    """
    Represents a Databento statistic type.
    """

    OPENING_PRICE = 1
    INDICATIVE_OPENING_PRICE = 2
    SETTLEMENT_PRICE = 3
    TRADING_SESSION_LOW_PRICE = 4
    TRADING_SESSION_HIGH_PRICE = 5
    CLEARED_VOLUME = 6
    LOWEST_OFFER = 7
    HIGHEST_BID = 8
    OPEN_INTEREST = 9
    FIXING_PRICE = 10
    CLOSE_PRICE = 11
    NET_CHANGE = 12


@unique
class DatabentoStatisticUpdateAction(Enum):
    """
    Represents a Databento statistic update action.
    """

    ADDED = 1
    DELETED = 2
