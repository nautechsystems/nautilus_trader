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

import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.identifiers import Venue


@pytest.fixture(scope="session")
def loop():
    return asyncio.get_event_loop()


@pytest.fixture(scope="session")
def live_clock():
    return LiveClock()


@pytest.fixture(scope="session")
def live_logger(live_clock):
    return Logger(clock=live_clock)


@pytest.fixture(scope="session")
def binance_http_client(loop, live_clock, live_logger):
    client = BinanceHttpClient(
        loop=asyncio.get_event_loop(),
        clock=live_clock,
        logger=live_logger,
        key="SOME_BINANCE_API_KEY",
        secret="SOME_BINANCE_API_SECRET",
    )
    return client


@pytest.fixture()
def venue() -> Venue:
    raise BINANCE_VENUE


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
