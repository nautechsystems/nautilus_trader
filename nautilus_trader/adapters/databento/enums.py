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

from enum import Enum
from enum import unique


@unique
class DatabentoInstrumentClass(Enum):
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
    ADDED = 1
    DELETED = 2
