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

import pytest


def pytest_ignore_collect(collection_path, config):
    if sys.version_info >= (3, 14):
        return True
    return False


pytestmark = pytest.mark.skipif(
    sys.version_info >= (3, 14),
    reason="dYdX adapter requires Python < 3.14 (coincurve incompatibility)",
)

if sys.version_info < (3, 14):
    from nautilus_trader.adapters.dydx.common.constants import DYDX_VENUE
    from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
    from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.model.identifiers import InstrumentId
    from nautilus_trader.model.identifiers import Venue
else:
    DYDX_VENUE = None
    DYDXSymbol = None
    DYDXHttpClient = None
    LiveClock = None
    InstrumentId = None
    Venue = None


@pytest.fixture
def symbol() -> DYDXSymbol:
    return DYDXSymbol("BTC-USD")


@pytest.fixture
def instrument_id() -> InstrumentId:
    return InstrumentId.from_str("BTC-USD-PERP.DYDX")


@pytest.fixture(scope="session")
def live_clock() -> LiveClock:
    return LiveClock()


@pytest.fixture(scope="session")
def http_client(live_clock: LiveClock) -> DYDXHttpClient:
    return DYDXHttpClient(
        clock=live_clock,
        base_url="https://indexer.v4testnet.dydx.exchange/v4",
    )


@pytest.fixture
def venue() -> Venue:
    return DYDX_VENUE


@pytest.fixture
def instrument():
    return None


@pytest.fixture
def data_client():
    return None


@pytest.fixture
def exec_client():
    return None


@pytest.fixture
def account_state():
    return None
