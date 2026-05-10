# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from pathlib import Path

import pytest

from nautilus_trader.common import LogLevel
from nautilus_trader.common import init_logging
from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue


# Add tests/ to sys.path so test strategies are importable by the engine
_TESTS_DIR = Path(__file__).resolve().parent
if str(_TESTS_DIR) not in sys.path:
    sys.path.insert(0, str(_TESTS_DIR))


@pytest.fixture(scope="session", autouse=True)
def bypass_logging():
    """
    Fixture to bypass logging for all tests.

    `autouse=True` will mean this function is run prior to every test. To disable this
    to debug specific tests, simply comment this out.

    """
    guard = init_logging(
        trader_id=TraderId("TESTER-000"),
        instance_id=UUID4(),
        level_stdout=LogLevel.DEBUG,
        is_bypassed=True,
        print_config=False,
    )
    return guard


@pytest.fixture
def trader_id():
    return TraderId("TRADER-001")


@pytest.fixture
def strategy_id():
    return StrategyId("S-001")


@pytest.fixture
def account_id():
    return AccountId("SIM-000")


@pytest.fixture
def venue():
    return Venue("SIM")


@pytest.fixture
def usd():
    return Currency.from_str("USD")


@pytest.fixture
def btc():
    return Currency.from_str("BTC")


@pytest.fixture
def usdt():
    return Currency.from_str("USDT")


@pytest.fixture
def audusd_id():
    return InstrumentId.from_str("AUD/USD.SIM")


@pytest.fixture
def usdjpy_id():
    return InstrumentId.from_str("USD/JPY.SIM")
