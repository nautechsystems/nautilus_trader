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

from typing import Final

from betfair_parser.spec.betting import MarketStatus as BetfairMarketStatus

from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price


BETFAIR: Final[str] = "BETFAIR"
BETFAIR_VENUE: Final[Venue] = Venue(BETFAIR)
BETFAIR_CLIENT_ID: Final[ClientId] = ClientId(BETFAIR)

BETFAIR_PRICE_PRECISION: Final[int] = 2
BETFAIR_QUANTITY_PRECISION: Final[int] = 2
BETFAIR_BOOK_TYPE: Final[BookType] = BookType.L2_MBP

CLOSE_PRICE_WINNER: Final[Price] = Price(1.0, precision=BETFAIR_PRICE_PRECISION)
CLOSE_PRICE_LOSER: Final[Price] = Price(0.0, precision=BETFAIR_PRICE_PRECISION)

MARKET_STATUS_MAPPING: Final[dict[tuple[BetfairMarketStatus, bool], MarketStatusAction]] = {
    (BetfairMarketStatus.INACTIVE, False): MarketStatusAction.CLOSE,
    (BetfairMarketStatus.OPEN, False): MarketStatusAction.PRE_OPEN,
    (BetfairMarketStatus.OPEN, True): MarketStatusAction.TRADING,
    (BetfairMarketStatus.SUSPENDED, False): MarketStatusAction.PAUSE,
    (BetfairMarketStatus.SUSPENDED, True): MarketStatusAction.PAUSE,
    (BetfairMarketStatus.CLOSED, False): MarketStatusAction.CLOSE,
    (BetfairMarketStatus.CLOSED, True): MarketStatusAction.CLOSE,
}
