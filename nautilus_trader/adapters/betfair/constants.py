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

from betfair_parser.spec.betting import MarketStatus as BetfairMarketStatus

from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price


BETFAIR_VENUE = Venue("BETFAIR")
BETFAIR_PRICE_PRECISION = 6
BETFAIR_QUANTITY_PRECISION = 6
BETFAIR_BOOK_TYPE = BookType.L2_MBP

CLOSE_PRICE_WINNER = Price(1.0, precision=BETFAIR_PRICE_PRECISION)
CLOSE_PRICE_LOSER = Price(0.0, precision=BETFAIR_PRICE_PRECISION)

MARKET_STATUS_MAPPING: dict[tuple[MarketStatus, bool], MarketStatus] = {
    (BetfairMarketStatus.INACTIVE, False): MarketStatus.CLOSED,
    (BetfairMarketStatus.OPEN, False): MarketStatus.PRE_OPEN,
    (BetfairMarketStatus.OPEN, True): MarketStatus.OPEN,
    (BetfairMarketStatus.SUSPENDED, False): MarketStatus.PAUSE,
    (BetfairMarketStatus.SUSPENDED, True): MarketStatus.PAUSE,
    (BetfairMarketStatus.CLOSED, False): MarketStatus.CLOSED,
    (BetfairMarketStatus.CLOSED, True): MarketStatus.CLOSED,
}
