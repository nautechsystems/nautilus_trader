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

from nautilus_trader.adapters.bitmex.config import BitmexExecClientConfig
from nautilus_trader.adapters.bitmex.constants import BITMEX_VENUE
from nautilus_trader.adapters.bitmex.execution import BitmexExecutionClient
from nautilus_trader.adapters.bitmex.providers import BitmexInstrumentProvider
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


@pytest.fixture()
def venue() -> Venue:
    return BITMEX_VENUE


@pytest.fixture()
def account_id(venue) -> AccountId:
    return AccountId(f"{venue.value}-1234567")


@pytest.fixture()
def instrument() -> CryptoPerpetual:
    """
    Create a test XBTUSD perpetual instrument.
    """
    return CryptoPerpetual(
        instrument_id=InstrumentId(Symbol("XBTUSD"), BITMEX_VENUE),
        raw_symbol=Symbol("XBTUSD"),
        base_currency=BTC,
        quote_currency=USD,
        settlement_currency=BTC,
        is_inverse=True,
        price_precision=1,
        price_increment=Price.from_str("0.5"),
        size_precision=0,
        size_increment=Quantity.from_int(1),
        margin_init=Decimal("0.01"),
        margin_maint=Decimal("0.005"),
        maker_fee=Decimal("-0.00025"),
        taker_fee=Decimal("0.00075"),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture()
def account_state(account_id) -> AccountState:
    """
    Create a test account state with BitMEX-style margins.
    """
    # BitMEX uses XBt (satoshis) as the base unit
    xbt_currency = Currency(
        code="XBt",
        precision=0,
        iso4217=0,
        name="Bitcoin (satoshis)",
        currency_type=CurrencyType.CRYPTO,
    )

    return AccountState(
        account_id=account_id,
        account_type=AccountType.MARGIN,
        base_currency=None,  # Multi-currency account
        reported=True,
        balances=[
            AccountBalance(
                total=Money(1000000, xbt_currency),  # 0.01 BTC
                locked=Money(0, xbt_currency),
                free=Money(1000000, xbt_currency),
            ),
        ],
        margins=[
            MarginBalance(
                initial=Money(100000, xbt_currency),  # 0.001 BTC initial margin
                maintenance=Money(50000, xbt_currency),  # 0.0005 BTC maintenance margin
                instrument_id=InstrumentId(Symbol("XBTUSD"), BITMEX_VENUE),
            ),
        ],
        info={},
        event_id=TestIdStubs.uuid(),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture()
def mock_http_client():
    """
    Create a mock BitMEX HTTP client.
    """
    mock = MagicMock(spec=nautilus_pyo3.BitmexHttpClient)
    mock.api_key = "test_api_key"
    mock.api_secret = "test_api_secret"

    # Mock account number retrieval
    mock.http_get_margin = AsyncMock(return_value="1234567")

    # Mock server time retrieval
    mock.http_get_server_time = AsyncMock(return_value=1234567890000)

    # Mock account state request
    mock_account_state = MagicMock()
    mock_account_state.to_dict = MagicMock(
        return_value={
            "account_id": "BITMEX-1234567",
            "account_type": "MARGIN",
            "base_currency": None,
            "reported": True,
            "balances": [
                {
                    "currency": "XBt",
                    "total": "1000000.0",
                    "locked": "0.0",
                    "free": "1000000.0",
                },
            ],
            "margins": [],
            "info": {},
            "event_id": str(TestIdStubs.uuid()),
            "ts_event": 0,
            "ts_init": 0,
        },
    )
    mock.request_account_state = AsyncMock(return_value=mock_account_state)

    # Mock order submission
    mock.submit_order = AsyncMock()

    # Mock order modification
    mock.modify_order = AsyncMock()

    # Mock order cancellation
    mock.cancel_order = AsyncMock()
    mock.cancel_all_orders = AsyncMock()
    mock.batch_cancel_orders = AsyncMock()

    # Mock report generation
    mock.request_order_status_reports = AsyncMock(return_value=[])
    mock.request_fill_reports = AsyncMock(return_value=[])
    mock.request_position_status_reports = AsyncMock(return_value=[])

    # Mock instrument caching
    mock.add_instrument = MagicMock()
    mock.request_instruments = AsyncMock(return_value=[])

    return mock


@pytest.fixture()
def mock_ws_client():
    """
    Create a mock BitMEX WebSocket client.
    """
    mock = MagicMock(spec=nautilus_pyo3.BitmexWebSocketClient)
    mock.url = "wss://testnet.bitmex.com/realtime"
    mock.is_closed = MagicMock(return_value=False)

    # Mock connection methods
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()

    # Mock subscription methods
    mock.subscribe_orders = AsyncMock()
    mock.subscribe_executions = AsyncMock()
    mock.subscribe_positions = AsyncMock()
    mock.subscribe_margin = AsyncMock()
    mock.subscribe_wallet = AsyncMock()
    mock.subscribe_book = AsyncMock()
    mock.subscribe_book_25 = AsyncMock()
    mock.subscribe_book_depth10 = AsyncMock()
    mock.subscribe_quotes = AsyncMock()
    mock.subscribe_trades = AsyncMock()
    mock.subscribe_instruments = AsyncMock()
    mock.subscribe_instrument = AsyncMock()
    mock.subscribe_mark_prices = AsyncMock()
    mock.subscribe_index_prices = AsyncMock()
    mock.subscribe_funding_rates = AsyncMock()
    mock.subscribe_bars = AsyncMock()

    # Mock unsubscription methods
    mock.unsubscribe_orders = AsyncMock()
    mock.unsubscribe_executions = AsyncMock()
    mock.unsubscribe_positions = AsyncMock()
    mock.unsubscribe_margin = AsyncMock()
    mock.unsubscribe_wallet = AsyncMock()
    mock.unsubscribe_book = AsyncMock()
    mock.unsubscribe_book_25 = AsyncMock()
    mock.unsubscribe_book_depth10 = AsyncMock()
    mock.unsubscribe_quotes = AsyncMock()
    mock.unsubscribe_trades = AsyncMock()
    mock.unsubscribe_instruments = AsyncMock()
    mock.unsubscribe_instrument = AsyncMock()
    mock.unsubscribe_mark_prices = AsyncMock()
    mock.unsubscribe_index_prices = AsyncMock()
    mock.unsubscribe_funding_rates = AsyncMock()
    mock.unsubscribe_bars = AsyncMock()

    # Mock account ID setter
    mock.set_account_id = MagicMock()

    return mock


@pytest.fixture()
def mock_canceller():
    """
    Create a mock BitMEX cancel broadcaster.
    """
    mock = MagicMock(spec=nautilus_pyo3.CancelBroadcaster)

    # Mock lifecycle methods
    mock.start = AsyncMock()
    mock.stop = AsyncMock()

    # Mock instrument caching
    mock.add_instrument = MagicMock()

    # Mock cancel operations
    mock.broadcast_cancel = AsyncMock()
    mock.broadcast_cancel_all = AsyncMock()
    mock.broadcast_batch_cancel = AsyncMock()

    return mock


@pytest.fixture()
def mock_instrument_provider(instrument):
    """
    Create a mock BitMEX instrument provider.
    """
    mock = MagicMock(spec=BitmexInstrumentProvider)
    mock.initialize = AsyncMock()

    # Return pyo3 instruments
    mock_pyo3_instrument = MagicMock()
    mock.instruments_pyo3 = MagicMock(return_value=[mock_pyo3_instrument])
    mock.get_all = MagicMock(return_value={instrument.id: instrument})
    mock.currencies = MagicMock(return_value={})

    return mock


@pytest.fixture()
def exec_client(
    event_loop,
    mock_http_client,
    mock_ws_client,
    mock_canceller,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
    monkeypatch,
):
    """
    Create a BitMEX execution client with mocked dependencies.
    """
    # Patch the WebSocket client creation
    monkeypatch.setattr(
        "nautilus_trader.adapters.bitmex.execution.nautilus_pyo3.BitmexWebSocketClient",
        lambda *args, **kwargs: mock_ws_client,
    )

    # Patch the CancelBroadcaster creation
    monkeypatch.setattr(
        "nautilus_trader.adapters.bitmex.execution.nautilus_pyo3.CancelBroadcaster",
        lambda *args, **kwargs: mock_canceller,
    )

    config = BitmexExecClientConfig(
        api_key="test_api_key",
        api_secret="test_api_secret",
        testnet=True,
    )

    client = BitmexExecutionClient(
        loop=event_loop,
        client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        clock=live_clock,
        instrument_provider=mock_instrument_provider,
        config=config,
        name=None,
    )

    # Store the mocked clients for test access
    client._mock_http_client = mock_http_client
    client._mock_ws_client = mock_ws_client
    client._mock_canceller = mock_canceller

    return client


@pytest.fixture()
def instrument_provider():
    """
    Return None as we're using mock_instrument_provider in exec_client.
    """
    return None


@pytest.fixture()
def data_client():
    """
    Return None as we're focusing on execution tests.
    """
    return None
