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

from nautilus_trader.adapters.deribit.config import DeribitDataClientConfig
from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.adapters.deribit.data import DeribitDataClient
from nautilus_trader.adapters.deribit.providers import DeribitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import DeribitInstrumentKind
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


@pytest.fixture(scope="session")
def live_clock() -> LiveClock:
    return LiveClock()


@pytest.fixture(scope="session")
def live_logger():
    return Logger("TEST_LOGGER")


@pytest.fixture
def mock_http_client():
    """
    Create a mock DeribitHttpClient.
    """
    client = MagicMock(spec=nautilus_pyo3.DeribitHttpClient)
    client.is_testnet = True
    client.is_initialized.return_value = True
    client.cache_instruments = MagicMock()
    client.cache_instrument = MagicMock()
    client.request_instruments = AsyncMock(return_value=[])
    client.request_instrument = AsyncMock()
    return client


def _create_ws_mock():
    """
    Create a mock DeribitWebSocketClient.
    """
    ws_client = MagicMock(spec=nautilus_pyo3.DeribitWebSocketClient)
    ws_client.url = "wss://test.deribit.com/ws/api/v2"
    ws_client.connect = AsyncMock()
    ws_client.close = AsyncMock()
    ws_client.is_closed.return_value = False
    ws_client.wait_until_active = AsyncMock()
    ws_client.cache_instruments = MagicMock()
    ws_client.cache_instrument = MagicMock()
    ws_client.subscribe_book = AsyncMock()
    ws_client.subscribe_quotes = AsyncMock()
    ws_client.subscribe_trades = AsyncMock()
    ws_client.unsubscribe_book = AsyncMock()
    ws_client.unsubscribe_quotes = AsyncMock()
    ws_client.unsubscribe_trades = AsyncMock()
    return ws_client


@pytest.fixture
def mock_instrument_provider(mock_http_client):
    """
    Create a mock DeribitInstrumentProvider.
    """
    provider = MagicMock(spec=DeribitInstrumentProvider)
    provider._client = mock_http_client
    provider.instrument_kinds = (DeribitInstrumentKind.FUTURE,)
    provider.initialize = AsyncMock()
    provider.instruments_pyo3.return_value = []
    provider.get_all.return_value = {}
    provider.currencies.return_value = {}
    return provider


@pytest.fixture
def venue():
    return DERIBIT_VENUE


@pytest.fixture
def instrument(venue: str) -> CryptoPerpetual:
    """
    Create a BTC-PERPETUAL test instrument.
    """
    return CryptoPerpetual(
        instrument_id=InstrumentId(
            symbol=Symbol("BTC-PERPETUAL"),
            venue=venue,
        ),
        raw_symbol=Symbol("BTC-PERPETUAL"),
        base_currency=BTC,
        quote_currency=USD,
        settlement_currency=BTC,
        is_inverse=True,
        price_precision=1,
        size_precision=0,
        price_increment=Price.from_str("0.5"),
        size_increment=Quantity.from_int(10),
        max_quantity=Quantity.from_int(1_000_000),
        min_quantity=Quantity.from_int(10),
        max_notional=None,
        min_notional=None,
        max_price=Price.from_str("1000000.0"),
        min_price=Price.from_str("0.5"),
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal("0.0000"),
        taker_fee=Decimal("0.0005"),
        ts_event=0,
        ts_init=0,
        info={"something": "here"},
    )


@pytest.fixture
def instrument_provider():
    pass


@pytest.fixture
def data_client():
    pass


@pytest.fixture
def exec_client():
    pass


@pytest.fixture
def account_state():
    pass


@pytest.fixture
def data_client_builder(
    event_loop,
    live_clock: LiveClock,
    live_logger,
    mock_http_client,
    mock_instrument_provider,
):
    """
    Provide a factory for creating DeribitDataClient instances.
    """

    def _builder(instrument_kinds: tuple[DeribitInstrumentKind, ...] | None = None):
        msgbus = MessageBus(
            trader_id=TestIdStubs.trader_id(),
            clock=live_clock,
        )
        cache_db = MockCacheDatabase()
        cache = Cache(database=cache_db)

        config = DeribitDataClientConfig(
            instrument_kinds=instrument_kinds or (DeribitInstrumentKind.FUTURE,),
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=True,
            http_timeout_secs=30,
        )

        client = DeribitDataClient(
            loop=event_loop,
            client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        # Replace WebSocket client with mock
        client._ws_client = _create_ws_mock()

        return client

    return _builder
