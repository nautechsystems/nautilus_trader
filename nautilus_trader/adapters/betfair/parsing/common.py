# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import NamedTuple

import msgspec
from betfair_parser.spec.common import Handicap
from betfair_parser.spec.common import MarketId
from betfair_parser.spec.common import OrderSide as BetSide
from betfair_parser.spec.common import SelectionId
from betfair_parser.spec.common import Size

from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.instruments.betting import make_symbol
from nautilus_trader.model.instruments.betting import null_handicap
from nautilus_trader.model.objects import Quantity


@lru_cache
def betfair_instrument_id(
    market_id: MarketId,
    selection_id: SelectionId,
    selection_handicap: Handicap | None,
) -> InstrumentId:
    """
    Create an instrument ID from betfair fields.

    >>> betfair_instrument_id(market_id="1.201070830", selection_id=123456, selection_handicap=None)
    InstrumentId('1-201070830-123456-None.BETFAIR')

    """
    PyCondition.not_empty(market_id, "market_id")
    symbol = make_symbol(market_id, selection_id, selection_handicap or null_handicap())
    return InstrumentId(symbol=symbol, venue=BETFAIR_VENUE)


def instrument_id_betfair_ids(
    instrument_id: InstrumentId,
) -> tuple[MarketId, SelectionId, Handicap | None]:
    """
    Extract the Betfair market ID, selection ID, and handicap from an instrument ID.

    >>> instrument_id_betfair_ids(InstrumentId.from_str("1-201070830-123456-None.BETFAIR"))
    ('1-201070830', 123456, None)
    >>> instrument_id_betfair_ids(InstrumentId.from_str("1-201070830-123456--2.5.BETFAIR"))
    ('1-201070830', 123456, -2.5)

    """
    # Using maxsplit=3 handles negative handicaps (e.g., --2.5 becomes -2.5)
    parts = instrument_id.symbol.value.split("-", maxsplit=3)
    return (
        MarketId(f"{parts[0]}-{parts[1]}"),
        SelectionId(parts[2]),
        Handicap(parts[3]) if parts[3] != "None" else None,
    )


def market_id_from_instrument_id(instrument_id: InstrumentId) -> MarketId:
    """
    Extract the Betfair market ID from an instrument ID.

    The market ID is returned in Betfair's native format with a dot separator.

    >>> market_id_from_instrument_id(InstrumentId.from_str("1-201070830-123456-None.BETFAIR"))
    '1.201070830'
    >>> market_id_from_instrument_id(InstrumentId.from_str("1-201070830-123456--2.5.BETFAIR"))
    '1.201070830'

    """
    # Symbol: "{market_prefix}-{market_suffix}-..." - only need first two parts
    parts = instrument_id.symbol.value.split("-", maxsplit=2)
    return MarketId(f"{parts[0]}.{parts[1]}")


def merge_instrument_fields(
    old: BettingInstrument,
    new: BettingInstrument,
    logger,
) -> BettingInstrument:
    old_dict = old.to_dict(old)
    new_dict = new.to_dict(new)
    for key, value in new_dict.items():
        if key in ("type", "id"):
            continue
        if key == "info":
            # Merge info dicts so stream fields (e.g. version) are preserved
            if isinstance(value, dict) and isinstance(old_dict.get(key), dict):
                merged_info = {**old_dict[key], **value}
                if merged_info != old_dict[key]:
                    old_dict[key] = merged_info
            continue
        if value != old_dict[key] and value:
            old_value = old_dict[key]
            logger.debug(f"Got updated field for {old.id}: {key=} {value=} {old_value=}")
            old_dict[key] = value

    return BettingInstrument.from_dict(old_dict)


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


def min_fill_size(time_in_force) -> Size | None:
    if time_in_force == TimeInForce.IOC:
        return 0
    else:
        return None


def hash_market_trade(timestamp: int, price: float, volume: float) -> str:
    data = (timestamp, price, volume)
    return hashlib.shake_256(msgspec.json.encode(data)).hexdigest(18)


class FillQtyResult(NamedTuple):
    fill_qty: Quantity
    total_matched_qty: Quantity
