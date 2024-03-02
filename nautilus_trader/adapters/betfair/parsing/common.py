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
import hashlib
from functools import lru_cache

import msgspec
from betfair_parser.spec.common import Handicap
from betfair_parser.spec.common import MarketId
from betfair_parser.spec.common import OrderSide as BetSide
from betfair_parser.spec.common import SelectionId

from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.betting import make_symbol
from nautilus_trader.model.instruments.betting import null_handicap


def hash_market_trade(timestamp: int, price: float, volume: float) -> str:
    data = (timestamp, price, volume)
    return hashlib.shake_256(msgspec.json.encode(data)).hexdigest(18)


@lru_cache
def betfair_instrument_id(
    market_id: MarketId,
    selection_id: SelectionId,
    selection_handicap: Handicap | None,
) -> InstrumentId:
    """
    Create an instrument ID from betfair fields.

    >>> betfair_instrument_id(market_id="1.201070830", selection_id=123456, selection_handicap=None)
    InstrumentId('1.201070830-123456-None.BETFAIR')

    """
    PyCondition.not_empty(market_id, "market_id")
    symbol = make_symbol(market_id, selection_id, selection_handicap or null_handicap())
    return InstrumentId(symbol=symbol, venue=BETFAIR_VENUE)


def instrument_id_betfair_ids(
    instrument_id: InstrumentId,
) -> tuple[MarketId, SelectionId, Handicap | None]:
    parts = instrument_id.symbol.value.split("-", maxsplit=2)
    return (
        MarketId(parts[0]),
        SelectionId(parts[1]),
        Handicap(parts[2]) if parts[2] != "None" else None,
    )


def chunk(list_like, n):
    """
    Yield successive n-sized chunks from l.
    """
    for i in range(0, len(list_like), n):
        yield list_like[i : i + n]


def order_side_to_bet_side(side: OrderSide) -> BetSide:
    if side == OrderSide.BUY:
        return BetSide.LAY
    elif side == OrderSide.SELL:
        return BetSide.BACK
    else:
        raise RuntimeError(f"Unknown side: {side}")


def bet_side_to_order_side(side: BetSide) -> OrderSide:
    if side == BetSide.LAY:
        return OrderSide.BUY
    elif side == BetSide.BACK:
        return OrderSide.SELL
    else:
        raise RuntimeError(f"Unknown side: {side}")
