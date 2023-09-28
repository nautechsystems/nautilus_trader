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

import asyncio
import sys

import msgspec
import pytest
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import MarketChange

from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.adapters.betfair.providers import load_markets
from nautilus_trader.adapters.betfair.providers import load_markets_metadata
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.adapters.betfair.providers import parse_market_catalog
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
class TestBetfairInstrumentProvider:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.logger = Logger(clock=self.clock, bypass=True)
        self.client = BetfairTestStubs.betfair_client(loop=self.loop, logger=self.logger)
        self.provider = BetfairInstrumentProvider(
            client=self.client,
            logger=TestComponentStubs.logger(),
            config=BetfairInstrumentProviderConfig(),
        )

    @pytest.mark.asyncio()
    async def test_load_markets(self):
        markets = await load_markets(self.client, market_filter={})
        assert len(markets) == 13227

        markets = await load_markets(self.client, market_filter={"event_type_name": "Basketball"})
        assert len(markets) == 302

        markets = await load_markets(self.client, market_filter={"event_type_name": "Tennis"})
        assert len(markets) == 1958

        markets = await load_markets(self.client, market_filter={"market_id": "1.177125728"})
        assert len(markets) == 1

    @pytest.mark.asyncio()
    async def test_load_markets_metadata(self):
        markets = await load_markets(self.client, market_filter={"event_type_name": "Basketball"})
        market_metadata = await load_markets_metadata(client=self.client, markets=markets)
        assert len(market_metadata) == 169

    @pytest.mark.asyncio()
    async def test_make_instruments(self):
        # Arrange
        list_market_catalogue_data = [
            m
            for m in parse_market_catalog(
                BetfairResponses.betting_list_market_catalogue()["result"],
            )
            if m.event_type.name == "Basketball"
        ]

        # Act
        instruments = [
            instrument
            for metadata in list_market_catalogue_data
            for instrument in make_instruments(metadata, currency="GBP")
        ]

        # Assert
        assert len(instruments) == 30412

    @pytest.mark.asyncio()
    async def test_load_all(self):
        await self.provider.load_all_async({"event_type_name": "Tennis"})
        assert len(self.provider.list_all()) == 4711

    @pytest.mark.asyncio()
    async def test_list_all(self):
        await self.provider.load_all_async(market_filter={"event_type_name": "Basketball"})
        instruments = self.provider.list_all()
        assert len(instruments) == 23908

    # @pytest.mark.asyncio()
    # async def test_search_instruments(self):
    #     await self.provider.load_all_async(market_filter={"event_type_name": "Basketball"})
    #     instruments = self.provider.search_instruments(
    #         instrument_filter={"market_type": "MATCH_ODDS"},
    #     )
    #     assert len(instruments) == 104

    # @pytest.mark.asyncio()
    # async def test_get_betting_instrument(self):
    #     await self.provider.load_all_async(market_filter={"market_id": ["1.180678317"]})
    #     kw = {
    #         "market_id": "1.180678317",
    #         "selection_id": "11313157",
    #         "handicap": "0.0",
    #     }
    #     instrument = self.provider.get_betting_instrument(**kw)
    #     assert instrument.market_id == "1.180678317"
    #
    #     # Test throwing warning
    #     kw["handicap"] = "-1000"
    #     instrument = self.provider.get_betting_instrument(**kw)
    #     assert instrument is None
    #
    #     # Test already in self._subscribed_instruments
    #     instrument = self.provider.get_betting_instrument(**kw)
    #     assert instrument is None

    def test_market_update_runner_removed(self) -> None:
        # Arrange
        raw = BetfairStreaming.market_definition_runner_removed()
        update = msgspec.json.decode(raw, type=MCM)

        mc: MarketChange = update.mc[0]
        market_def = mc.market_definition
        market_def = msgspec.structs.replace(market_def, market_id=mc.id)
        instruments = make_instruments(market_def, currency="GBP")
        self.provider.add_bulk(instruments)

        # Act
        results = []
        parser = BetfairParser()
        for data in parser.parse(update):
            results.append(data)

        # Assert
        result = [r.status for r in results[:8]]
        expected = [MarketStatus.PRE_OPEN] * 7 + [MarketStatus.CLOSED]
        assert result == expected
