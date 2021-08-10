# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.adapters.betfair.client import BetfairClient
from nautilus_trader.adapters.betfair.parsing import on_market_update
from nautilus_trader.adapters.betfair.providers import load_markets
from nautilus_trader.adapters.betfair.providers import load_markets_metadata
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.model.enums import InstrumentStatus
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming


@pytest.fixture(autouse=True)
def mock_data(mocker):
    mocker.patch.object(
        BetfairClient,
        "list_navigation",
        return_value=BetfairResponses.navigation_list_navigation(),
    )
    mocker.patch.object(
        BetfairClient,
        "list_market_catalogue",
        return_value=BetfairDataProvider.market_catalogue_short(),
    )
    mocker.patch.object(
        BetfairClient,
        "get_account_details",
        return_value=BetfairResponses.account_details()["result"],
    )


@pytest.fixture()
@pytest.mark.asyncio
async def market_metadata(betfair_client):
    markets = await load_markets(betfair_client, market_filter={"event_type_name": "Basketball"})
    return await load_markets_metadata(client=betfair_client, markets=markets)


@pytest.mark.asyncio
async def test_load_markets(provider, betfair_client):
    markets = await load_markets(betfair_client, market_filter={})
    assert len(markets) == 13227

    markets = await load_markets(betfair_client, market_filter={"event_type_name": "Basketball"})
    assert len(markets) == 302

    markets = await load_markets(betfair_client, market_filter={"market_id": "1.177125728"})
    assert len(markets) == 1


@pytest.mark.asyncio
async def test_load_markets_metadata(betfair_client):
    markets = await load_markets(betfair_client, market_filter={"event_type_name": "Basketball"})
    market_metadata = await load_markets_metadata(client=betfair_client, markets=markets)
    assert isinstance(market_metadata, dict)
    assert len(market_metadata) == 685


def test_load_instruments(market_metadata):
    instruments = [
        instrument
        for metadata in market_metadata.values()
        for instrument in make_instruments(metadata, currency="GBP")
    ]
    assert len(instruments) == 10250


@pytest.mark.asyncio
async def test_load_all(provider):
    await provider.load_all_async()
    assert len(provider.list_instruments()) == 10250


# def test_search_instruments(provider):
#     markets = provider.search_markets(market_filter={"market_marketType": "MATCH_ODDS"})
#     assert len(markets) == 1000


@pytest.mark.asyncio
async def test_get_betting_instrument(provider):
    await provider.load_all_async()
    kw = dict(
        market_id="1.180736294",
        selection_id="38849165",
        handicap="0.0",
    )
    instrument = provider.get_betting_instrument(**kw)
    assert instrument.market_id == "1.180736294"

    # Test throwing warning
    kw["handicap"] = "-1000"
    instrument = provider.get_betting_instrument(**kw)
    assert instrument is None

    # Test already in self._subscribed_instruments
    instrument = provider.get_betting_instrument(**kw)
    assert instrument is None


def test_market_update_runner_removed(provider):
    update = BetfairStreaming.market_definition_runner_removed()

    # Setup
    market_def = update["mc"][0]["marketDefinition"]
    market_def["marketId"] = update["mc"][0]["id"]
    instruments = make_instruments(
        market_definition=update["mc"][0]["marketDefinition"], currency="GBP"
    )
    provider.add_instruments(instruments)

    results = []
    for data in on_market_update(instrument_provider=provider, update=update):
        results.append(data)
    result = [r.status for r in results[:8]]
    expected = [InstrumentStatus.PRE_OPEN] * 7 + [InstrumentStatus.CLOSED]
    assert result == expected
