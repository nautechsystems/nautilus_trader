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

from collections.abc import Generator
from os import PathLike
from typing import BinaryIO

import fsspec
import msgspec
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import MarketDefinition
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.data_types import BetfairSequenceCompleted
from nautilus_trader.adapters.betfair.parsing.streaming import PARSE_TYPES
from nautilus_trader.adapters.betfair.parsing.streaming import market_change_to_updates
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money


class BetfairParser:
    """
    Stateful parser that keeps market definition.
    """

    def __init__(self, currency: str) -> None:
        self.currency = Currency.from_str(currency)
        self.market_definitions: dict[str, MarketDefinition] = {}
        self.traded_volumes: dict[InstrumentId, dict[float, float]] = {}

    def parse(
        self,
        mcm: MCM,
        ts_init: int | None = None,
        min_notional: Money | None = None,
    ) -> list[PARSE_TYPES]:
        if isinstance(mcm, Status | Connection | OCM):
            return []
        if mcm.is_heartbeat:
            return []
        updates = []
        ts_event = millis_to_nanos(mcm.pt)
        ts_init = ts_init or ts_event
        for mc in mcm.mc:
            if mc.market_definition is not None:
                market_def = msgspec.structs.replace(mc.market_definition, market_id=mc.id)
                self.market_definitions[mc.id] = market_def
                instruments = make_instruments(
                    market_def,
                    currency=self.currency.code,
                    ts_event=ts_event,
                    ts_init=ts_init,
                    min_notional=min_notional,
                )
                updates.extend(instruments)
            mc_updates = market_change_to_updates(mc, self.traded_volumes, ts_event, ts_init)
            updates.extend(mc_updates)

        updates.append(BetfairSequenceCompleted(ts_event=ts_event, ts_init=ts_init))

        return updates


def iter_stream(file_like: BinaryIO):
    for line in file_like:
        yield stream_decode(line)


def parse_betfair_file(
    uri: PathLike[str] | str,
    currency: str,
    min_notional: Money | None = None,
) -> Generator[list[PARSE_TYPES], None, None]:
    """
    Parse a file of streaming data.

    Parameters
    ----------
    uri : PathLike[str] | str
        The fsspec-compatible URI.
    currency : str
        The Betfair account currency.
    min_notional : Money
        The minimum notional value for instrument definitions.

    """
    parser = BetfairParser(currency=currency)
    with fsspec.open(uri, compression="infer") as f:
        for mcm in iter_stream(f):
            yield from parser.parse(mcm, min_notional=min_notional)


def betting_instruments_from_file(
    uri: PathLike[str] | str,
    currency: str,
    ts_event: int,
    ts_init: int,
    min_notional: Money | None = None,
) -> list[BettingInstrument]:
    from nautilus_trader.adapters.betfair.providers import make_instruments

    instruments: list[BettingInstrument] = []

    with fsspec.open(uri, compression="infer") as f:
        for mcm in iter_stream(f):
            for mc in mcm.mc:
                if mc.market_definition:
                    market_def = msgspec.structs.replace(mc.market_definition, market_id=mc.id)
                    mc = msgspec.structs.replace(mc, market_definition=market_def)
                    instruments = make_instruments(
                        mc.market_definition,
                        currency=currency,
                        ts_event=ts_event,
                        ts_init=ts_init,
                        min_notional=min_notional,
                    )
                    instruments.extend(instruments)

    return list(set(instruments))
