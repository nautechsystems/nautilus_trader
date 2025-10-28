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

import sys

import msgspec
import pytest
from betfair_parser.spec.betting import MarketCatalogue
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import MarketChange

from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.adapters.betfair.providers import load_markets
from nautilus_trader.adapters.betfair.providers import load_markets_metadata
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.adapters.betfair.providers import parse_market_catalog
from nautilus_trader.common.component import LiveClock
from nautilus_trader.model.enums import MarketStatusAction
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
class TestBetfairInstrumentProvider:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.clock = LiveClock()
        self.client = BetfairTestStubs.betfair_client(loop=self.loop)
        self.provider = BetfairInstrumentProvider(
            client=self.client,
            config=BetfairInstrumentProviderConfig(account_currency="GBP"),
        )
        self.parser = BetfairParser(currency="GBP")

        yield

    @pytest.mark.asyncio()
    async def test_load_markets(self):
        markets = await load_markets(self.client)
        assert len(markets) == 13227

        markets = await load_markets(self.client, event_type_names=["Basketball"])
        assert len(markets) == 302

        markets = await load_markets(self.client, event_type_names=["Tennis"])
        assert len(markets) == 1958

        # TODO: Fix symbology
        markets = await load_markets(self.client, market_ids=["1.177125728"])
        assert len(markets) == 1

    @pytest.mark.asyncio()
    async def test_load_markets_metadata(self):
        markets = await load_markets(self.client, event_type_names=["Basketball"])
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
            for instrument in make_instruments(metadata, currency="GBP", ts_event=0, ts_init=0)
        ]

        # Assert
        assert len(instruments) == 30412

    @pytest.mark.asyncio()
    async def test_load_all(self):
        await self.provider.load_all_async({"event_type_names": ["Tennis"]})
        assert len(self.provider.list_all()) == 4711

    @pytest.mark.asyncio()
    async def test_list_all(self):
        await self.provider.load_all_async({"event_type_names": ["Basketball"]})
        instruments = self.provider.list_all()
        assert len(instruments) == 23908

    def test_market_update_runner_removed(self) -> None:
        # Arrange
        raw = BetfairStreaming.market_definition_runner_removed()
        update = msgspec.json.decode(raw, type=MCM)

        mc: MarketChange = update.mc[0]
        market_def = mc.market_definition
        market_def = msgspec.structs.replace(market_def, market_id=mc.id)
        instruments = make_instruments(
            market_def,
            currency="GBP",
            ts_event=0,
            ts_init=0,
        )
        self.provider.add_bulk(instruments)

        # Act
        results = []
        for data in self.parser.parse(update):
            results.append(data)

        # Assert
        result = [r.action for r in results[8:16]]
        expected = [MarketStatusAction.PRE_OPEN] * 7 + [MarketStatusAction.CLOSE]
        assert result == expected

    def test_list_market_catalogue_parsing(self):
        # Arrange
        raw = BetfairResponses.list_market_catalogue()
        market_catalogue = msgspec.json.decode(msgspec.json.encode(raw), type=MarketCatalogue)

        # Act
        instruments = make_instruments(
            market_catalogue,
            currency="GBP",
            ts_event=0,
            ts_init=0,
        )

        # Assert
        result = [ins.id.value for ins in instruments]
        expected = [
            "1-221718403-20075720-None.BETFAIR",
            "1-221718403-10733147-None.BETFAIR",
            "1-221718403-38666189-None.BETFAIR",
            "1-221718403-11781146-None.BETFAIR",
            "1-221718403-36709273-None.BETFAIR",
            "1-221718403-51130740-None.BETFAIR",
            "1-221718403-63132709-None.BETFAIR",
            "1-221718403-18508590-None.BETFAIR",
            "1-221718403-41921465-None.BETFAIR",
        ]

        assert result == expected
