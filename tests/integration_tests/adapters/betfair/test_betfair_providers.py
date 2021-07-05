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

from nautilus_trader.adapters.betfair.providers import load_markets
from nautilus_trader.adapters.betfair.providers import load_markets_metadata
from nautilus_trader.adapters.betfair.providers import make_instruments
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.fixture(autouse=True)
def fix_mocks(mocker):
    """
    Override the `_short` version of `list_market_catalogue` used by the
    top-level conftest.
    """
    # Mock market catalogue endpoints
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_market_catalogue",
        return_value=BetfairTestStubs.market_catalogue(),
    )


@pytest.fixture()
def market_metadata(betfair_client):
    markets = load_markets(betfair_client, market_filter={"event_type_name": "Basketball"})
    return load_markets_metadata(client=betfair_client, markets=markets)


def test_load_markets(provider, betfair_client):
    markets = load_markets(betfair_client, market_filter={})
    assert len(markets) == 13227

    markets = load_markets(betfair_client, market_filter={"event_type_name": "Basketball"})
    assert len(markets) == 302

    markets = load_markets(betfair_client, market_filter={"market_id": "1.177125728"})
    assert len(markets) == 1


def test_load_markets_metadata(betfair_client):
    markets = load_markets(betfair_client, market_filter={"event_type_name": "Basketball"})
    market_metadata = load_markets_metadata(client=betfair_client, markets=markets)
    assert isinstance(market_metadata, dict)
    assert len(market_metadata) == 12035


def test_load_instruments(market_metadata):
    instruments = [
        instrument
        for metadata in market_metadata.values()
        for instrument in make_instruments(metadata, currency="GBP")
    ]
    assert len(instruments) == 172535


def test_load_all(provider):
    provider.load_all()


def test_search_instruments(provider):
    markets = provider.search_markets(market_filter={"market_marketType": "MATCH_ODDS"})
    assert len(markets) == 1000
