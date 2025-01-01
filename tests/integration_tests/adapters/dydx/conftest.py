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
"""
Create fixtures for commonly used objects.
"""

import pytest

from nautilus_trader.adapters.dydx.common.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


@pytest.fixture
def symbol() -> DYDXSymbol:
    """
    Create a stub symbol.
    """
    return DYDXSymbol("BTC-USD")


@pytest.fixture
def instrument_id() -> InstrumentId:
    """
    Create a stub instrument id.
    """
    return InstrumentId.from_str("BTC-USD-PERP.DYDX")


@pytest.fixture(scope="session")
def live_clock() -> LiveClock:
    """
    Create a stub live clock.
    """
    return LiveClock()


@pytest.fixture(scope="session")
def http_client(live_clock: LiveClock) -> DYDXHttpClient:
    """
    Create a stub HTTP client.
    """
    return DYDXHttpClient(
        clock=live_clock,
        base_url="https://indexer.v4testnet.dydx.exchange/v4",
    )


@pytest.fixture()
def venue() -> Venue:
    """
    Create a stub dYdX venue.
    """
    return DYDX_VENUE


@pytest.fixture()
def data_client():
    pass


@pytest.fixture()
def exec_client():
    pass


@pytest.fixture()
def instrument():
    pass


@pytest.fixture()
def account_state():
    pass
