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

from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.lighter.constants import LIGHTER_VENUE
from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


@pytest.fixture(scope="session")
def live_clock():
    return LiveClock()


@pytest.fixture(scope="session")
def live_logger():
    return Logger("TEST_LOGGER")


@pytest.fixture
def venue() -> Venue:
    return LIGHTER_VENUE


@pytest.fixture
def account_id(venue) -> AccountId:
    return AccountId(f"{venue.value}-001")


@pytest.fixture
def btc_instrument() -> CryptoPerpetual:
    """
    BTC-USD-PERP instrument matching the fixture data.
    """
    return CryptoPerpetual(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), LIGHTER_VENUE),
        raw_symbol=Symbol("BTC"),
        base_currency=BTC,
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=1,
        size_precision=4,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("0.0001"),
        max_quantity=None,
        min_quantity=Quantity.from_str("0.0010"),
        max_notional=None,
        min_notional=None,
        max_price=None,
        min_price=None,
        margin_init=Decimal("0.05"),
        margin_maint=Decimal("0.025"),
        maker_fee=Decimal("0.0002"),
        taker_fee=Decimal("0.0005"),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture
def instrument(btc_instrument) -> CryptoPerpetual:
    """
    Return default instrument fixture required by parent conftest.
    """
    return btc_instrument


@pytest.fixture
def account_state(account_id) -> AccountState:
    return AccountState(
        account_id=account_id,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        reported=True,
        balances=[
            AccountBalance(
                total=Money(100_000, USD),
                locked=Money(0, USD),
                free=Money(100_000, USD),
            ),
        ],
        margins=[],
        info={},
        event_id=TestIdStubs.uuid(),
        ts_event=0,
        ts_init=0,
    )


def _create_ws_mock() -> MagicMock:
    """
    Create a mocked LighterWebSocketClient.
    """
    mock = MagicMock()
    mock.url = "wss://test.lighter.xyz/ws"
    mock.is_closed = MagicMock(return_value=False)
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()
    mock.subscribe_order_book = AsyncMock()
    mock.subscribe_trades = AsyncMock()
    mock.subscribe_market_stats = AsyncMock()
    mock.unsubscribe_order_book = AsyncMock()
    mock.unsubscribe_trades = AsyncMock()
    mock.unsubscribe_market_stats = AsyncMock()
    mock.cache_instrument = MagicMock()
    return mock


def _create_http_mock() -> MagicMock:
    """
    Create a mocked LighterHttpClient.
    """
    mock = MagicMock()
    mock.load_instrument_definitions = AsyncMock(return_value=[])
    mock.get_market_index = MagicMock(return_value=1)
    mock.get_order_book_snapshot = AsyncMock(return_value=None)
    mock.cache_instrument = MagicMock()
    return mock


@pytest.fixture
def mock_ws_client():
    return _create_ws_mock()


@pytest.fixture
def mock_http_client():
    return _create_http_mock()


@pytest.fixture
def mock_instrument_provider(btc_instrument):
    """
    Create a mocked LighterInstrumentProvider.
    """
    provider = MagicMock(spec=LighterInstrumentProvider)
    provider.initialize = AsyncMock()
    provider.instruments_pyo3 = MagicMock(return_value=[])
    provider.get_all = MagicMock(return_value={btc_instrument.id: btc_instrument})
    provider.currencies = MagicMock(return_value={})
    provider.find = MagicMock(return_value=btc_instrument)
    provider.market_index_for = MagicMock(return_value=1)
    return provider


@pytest.fixture
def data_client():
    """
    Return None as we build via data_client_builder (required by parent conftest).
    """
    return None


@pytest.fixture
def exec_client():
    """
    Return None as execution is not yet implemented (required by parent conftest).
    """
    return None


@pytest.fixture
def instrument_provider(mock_instrument_provider):
    """
    Return mock instrument provider (required by parent conftest).
    """
    return mock_instrument_provider
