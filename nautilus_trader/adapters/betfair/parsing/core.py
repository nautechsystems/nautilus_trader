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
import typing
from typing import Optional

import fsspec
import msgspec
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import MarketDefinition
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.parsing.streaming import PARSE_TYPES
from nautilus_trader.adapters.betfair.parsing.streaming import market_change_to_updates
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import BettingInstrument


class BetfairParser:
    """
    Stateful parser that keeps market definition.
    """

    def __init__(self) -> None:
        self.market_definitions: dict[str, MarketDefinition] = {}
        self.traded_volumes: dict[InstrumentId, dict[float, float]] = {}

    def parse(self, mcm: MCM, ts_init: Optional[int] = None) -> list[PARSE_TYPES]:
        if isinstance(mcm, (Status, Connection, OCM)):
            return []
        if mcm.is_heartbeat:
            return []
        updates = []
        ts_event = millis_to_nanos(mcm.pt)
        ts_init = ts_init or ts_event
        for mc in mcm.mc:
            if mc.market_definition is not None:
                self.market_definitions[mc.id] = mc.market_definition
            mc_updates = market_change_to_updates(mc, self.traded_volumes, ts_event, ts_init)
            updates.extend(mc_updates)
        return updates


def iter_stream(file_like: typing.BinaryIO):
    for line in file_like:
        yield stream_decode(line)
        # try:
        #     data = stream_decode(line)
        # except (msgspec.DecodeError, msgspec.ValidationError) as e:
        #     print("ERR", e)
        #     print(msgspec.json.decode(line))
        #     raise e
        # yield data


def parse_betfair_file(uri: str):  # noqa
    """
    Parse a file of streaming data.

    Parameters
    ----------
        uri: fsspec-compatible URI.

    """
    parser = BetfairParser()
    with fsspec.open(uri, compression="infer") as f:
        for mcm in iter_stream(f):
            yield from parser.parse(mcm)


def betting_instruments_from_file(uri: str) -> list[BettingInstrument]:
    from nautilus_trader.adapters.betfair.providers import make_instruments

    instruments: list[BettingInstrument] = []

    with fsspec.open(uri, compression="infer") as f:
        for mcm in iter_stream(f):
            for mc in mcm.mc:
                if mc.market_definition:
                    market_def = msgspec.structs.replace(mc.market_definition, market_id=mc.id)
                    mc = msgspec.structs.replace(mc, market_definition=market_def)
                    instruments = make_instruments(mc.market_definition, currency="GBP")
                    instruments.extend(instruments)
    return list(set(instruments))
