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
from typing import Optional

import msgspec
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import STREAM_DECODER

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.persistence.external.readers import LinePreprocessor
from nautilus_trader.persistence.external.readers import TextReader


def hash_market_trade(timestamp: int, price: float, volume: float):
    return f"{str(timestamp)[:-6]}{price}{str(volume)}"


def make_symbol(
    market_id: str,
    selection_id: str,
    selection_handicap: Optional[str],
) -> Symbol:
    """
    Make symbol

    >>> make_symbol(market_id="1.201070830", selection_id="123456", selection_handicap=None)
    Symbol('1.201070830|123456|None')
    """

    def _clean(s):
        return str(s).replace(" ", "").replace(":", "")

    value: str = "|".join(
        [_clean(k) for k in (market_id, selection_id, selection_handicap)],
    )
    assert len(value) <= 32, f"Symbol too long ({len(value)}): '{value}'"
    return Symbol(value)


@lru_cache
def betfair_instrument_id(
    market_id: str,
    runner_id: str,
    runner_handicap: Optional[str],
) -> InstrumentId:
    """
    Create an instrument ID from betfair fields

    >>> betfair_instrument_id(market_id="1.201070830", selection_id="123456", selection_handicap=None)
    InstrumentId('1.201070830|123456|None.BETFAIR')

    """
    PyCondition.not_empty(market_id, "market_id")
    symbol = make_symbol(market_id, runner_id, runner_handicap)
    return InstrumentId(symbol=symbol, venue=BETFAIR_VENUE)


def flatten_tree(y: dict, **filters):
    """
    Flatten a nested dict into a list of dicts with each nested level combined
    into a single dict.
    """
    results = []
    ignore_keys = ("type", "children")

    def flatten(dict_like, depth: Optional[int] = None):
        def _filter(k, v):
            if isinstance(v, str):
                return k == v
            elif isinstance(v, (tuple, list)):
                return k in v
            else:
                raise TypeError

        depth = depth or 0
        node_type = dict_like["type"].lower()
        data = {f"{node_type}_{k}": v for k, v in dict_like.items() if k not in ignore_keys}
        if "children" in dict_like:
            for child in dict_like["children"]:
                for child_data in flatten(child, depth=depth + 1):
                    if depth == 0:
                        if all(_filter(child_data[k], v) for k, v in filters.items()):
                            results.append(child_data)
                    else:
                        yield {**data, **child_data}
        else:
            yield data

    list(flatten(y))
    return results


def chunk(list_like, n):
    """
    Yield successive n-sized chunks from l.
    """
    for i in range(0, len(list_like), n):
        yield list_like[i : i + n]


def historical_instrument_provider_loader(instrument_provider, line):
    from nautilus_trader.adapters.betfair.providers import make_instruments

    if instrument_provider is None:
        return

    mcm = msgspec.json.decode(line, type=MCM)
    # Find instruments in data
    for mc in mcm.mc:
        if mc.marketDefinition:
            mc.marketDefinition.marketId = mc.id
            instruments = make_instruments(mc.marketDefinition, currency="GBP")
            instrument_provider.add_bulk(instruments)

    # By this point we should always have some instruments loaded from historical data
    if not instrument_provider.list_all():
        # TODO - Need to add historical search
        raise Exception("No instruments found")


def make_betfair_reader(
    instrument_provider: Optional[InstrumentProvider] = None,
    line_preprocessor: Optional[LinePreprocessor] = None,
) -> TextReader:
    from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
    from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider

    instrument_provider = instrument_provider or BetfairInstrumentProvider.from_instruments([])
    parser = BetfairParser()

    def parse_line(line):
        yield from parser.parse(STREAM_DECODER.decode(line))

    return TextReader(
        # Use the standard `on_market_update` betfair parser that the adapter uses
        line_preprocessor=line_preprocessor,
        line_parser=parse_line,
        instrument_provider_update=historical_instrument_provider_loader,
        instrument_provider=instrument_provider,
    )
