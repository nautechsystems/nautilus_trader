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

import msgspec
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.persistence.external.readers import LinePreprocessor
from nautilus_trader.persistence.external.readers import TextReader


def historical_instrument_provider_loader(instrument_provider, line):
    from nautilus_trader.adapters.betfair.providers import make_instruments

    if instrument_provider is None:
        return

    mcm = msgspec.json.decode(line, type=MCM)
    # Find instruments in data
    for mc in mcm.mc:
        if mc.market_definition:
            market_def = msgspec.structs.replace(mc.market_definition, market_id=mc.id)
            mc = msgspec.structs.replace(mc, market_definition=market_def)
            instruments = make_instruments(mc.market_definition, currency="GBP")
            instrument_provider.add_bulk(instruments)

    # By this point we should always have some instruments loaded from historical data
    if not instrument_provider.list_all():
        # TODO - Need to add historical search
        raise Exception("No instruments found")


def make_betfair_reader(
    instrument_provider: Optional[InstrumentProvider] = None,
    line_preprocessor: Optional[LinePreprocessor] = None,
) -> TextReader:
    instrument_provider = instrument_provider or BetfairInstrumentProvider.from_instruments([])
    parser = BetfairParser()

    def parse_line(line):
        yield from parser.parse(stream_decode(line))

    return TextReader(
        # Use the standard `on_market_update` betfair parser that the adapter uses
        line_preprocessor=line_preprocessor,
        line_parser=parse_line,
        instrument_provider_update=historical_instrument_provider_loader,
        instrument_provider=instrument_provider,
    )
