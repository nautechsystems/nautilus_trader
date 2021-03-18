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
import json

import pytest

from adapters.betfair.parsing import load_markets
from adapters.betfair.parsing import load_markets_metadata
from adapters.betfair.parsing import make_instrument
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/betfair/responses/"


@pytest.fixture(autouse=True)
def mocks(mocker):
    mock_list_nav = mocker.patch(
        "betfairlightweight.endpoints.navigation.Navigation.list_navigation"
    )
    mock_list_nav.return_value = json.loads(open("./responses/navigation.json").read())

    mock_market_catalogue = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_market_catalogue"
    )
    mock_market_catalogue.return_value = json.loads(
        open("./responses/market_metadata.json").read()
    )


@pytest.fixture()
def provider(betfair_client) -> BetfairInstrumentProvider:
    # TODO Mock client login
    return BetfairInstrumentProvider(client=betfair_client)


@pytest.fixture()
def market_metadata(betfair_client):
    markets = load_markets(betfair_client, filter={"event_type_name": "Basketball"})
    return load_markets_metadata(client=betfair_client, markets=markets)


def test_load_markets(provider, betfair_client):
    markets = load_markets(betfair_client, filter={})
    assert len(markets) == 13227

    markets = load_markets(betfair_client, filter={"event_type_name": "Basketball"})
    assert len(markets) == 302


def test_load_markets_metadata(betfair_client):
    markets = load_markets(betfair_client, filter={"event_type_name": "Basketball"})
    market_metadata = load_markets_metadata(client=betfair_client, markets=markets)
    assert isinstance(market_metadata, dict)
    assert len(market_metadata) == 12035


def test_load_instruments(market_metadata):
    instruments = [
        instrument
        for metadata in market_metadata.values()
        for instrument in make_instrument(metadata)
    ]
    assert len(instruments) == 172535


# def test_load_all(provider):
#     provider.load_all()


def test_search_instruments(provider):
    # instruments = provider.search()
    pass
