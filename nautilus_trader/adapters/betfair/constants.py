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

import os
from enum import Enum

from betfair_parser.spec.streaming.mcm import MarketStatus as BetfairMarketStatus

from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.objects import Price


BETFAIR_PRICE_PRECISION = 2
BETFAIR_QUANTITY_PRECISION = 2
BETFAIR_BOOK_TYPE = BookType.L2_MBP

CLOSE_PRICE_WINNER = Price(1.0, precision=BETFAIR_PRICE_PRECISION)
CLOSE_PRICE_LOSER = Price(0.0, precision=BETFAIR_PRICE_PRECISION)


MARKET_STATUS_MAPPING: dict[tuple[BetfairMarketStatus, bool], MarketStatus] = {
    (BetfairMarketStatus.OPEN, False): MarketStatus.PRE_OPEN,
    (BetfairMarketStatus.OPEN, True): MarketStatus.OPEN,
    (BetfairMarketStatus.SUSPENDED, False): MarketStatus.PAUSE,
    (BetfairMarketStatus.SUSPENDED, True): MarketStatus.PAUSE,
    (BetfairMarketStatus.CLOSED, False): MarketStatus.CLOSED,
    (BetfairMarketStatus.CLOSED, True): MarketStatus.CLOSED,
}

STRICT_MARKET_DATA_HANDLING = os.environ.get("BETFAIR_STRICT_MARKET_DATA_HANDLING", "1")


class MarketDataKind(Enum):
    """MarketDataKind"""

    ALL = "ALL"
    BEST = "BEST"
    DISPLAY = "DISPLAY"


EVENT_TYPE_TO_NAME = {
    "1": "Soccer",
    "2": "Tennis",
    "3": "Golf",
    "4": "Cricket",
    "5": "Rugby Union",
    "1477": "Rugby League",
    "6": "Boxing",
    "7": "Horse Racing",
    "8": "Motor Sport",
    "27454571": "Esports",
    "10": "Special Bets",
    "998917": "Volleyball",
    "11": "Cycling",
    "2152880": "Gaelic Games",
    "3988": "Athletics",
    "6422": "Snooker",
    "7511": "Baseball",
    "6231": "Financial Bets",
    "6423": "American Football",
    "7522": "Basketball",
    "7524": "Ice Hockey",
    "61420": "Australian Rules",
    "468328": "Handball",
    "3503": "Darts",
    "26420387": "Mixed Martial Arts",
    "4339": "Greyhound Racing",
    "2378961": "Politics",
}


class HistoricalSportType(Enum):
    """
    Represents a `Betfair` historical sport type.
    """

    HORSE_RACING = "Horse Racing"
    SOCCER = "Soccer"
    TENNIS = "Tennis"
    CRICKET = "Cricket"
    GOLF = "Golf"
    GREYHOUND_RACING = "Greyhound Racing"
    OTHER_SPORTS = "Other Sports"
