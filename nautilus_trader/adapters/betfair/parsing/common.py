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

from functools import lru_cache

from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.betting import make_symbol


def hash_market_trade(timestamp: int, price: float, volume: float):
    return f"{str(timestamp)[:-6]}{price}{volume!s}"


@lru_cache
def betfair_instrument_id(
    market_id: str,
    selection_id: float,
    selection_handicap: float | None,
) -> InstrumentId:
    """
    Create an instrument ID from betfair fields.

    >>> betfair_instrument_id(market_id="1.201070830", selection_id="123456", selection_handicap=None)
    InstrumentId('1.201070830-123456-None.BETFAIR')

    """
    PyCondition.not_empty(market_id, "market_id")
    symbol = make_symbol(market_id, selection_id, selection_handicap)
    return InstrumentId(symbol=symbol, venue=BETFAIR_VENUE)


def chunk(list_like, n):
    """
    Yield successive n-sized chunks from l.
    """
    for i in range(0, len(list_like), n):
        yield list_like[i : i + n]
