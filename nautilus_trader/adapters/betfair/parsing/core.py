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

from typing import Optional

import fsspec
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming.mcm import MCM
from betfair_parser.spec.streaming.mcm import MarketDefinition
from betfair_parser.util import iter_stream

from nautilus_trader.adapters.betfair.parsing.streaming import PARSE_TYPES
from nautilus_trader.adapters.betfair.parsing.streaming import market_change_to_updates
from nautilus_trader.core.datetime import millis_to_nanos


class BetfairParser:
    """
    Stateful parser that keeps market definition.
    """

    def __init__(self) -> None:
        self.market_definitions: dict[str, MarketDefinition] = {}

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
            mc_updates = market_change_to_updates(mc, ts_event, ts_init)
            updates.extend(mc_updates)
        return updates


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
