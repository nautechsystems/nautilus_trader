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

from nautilus_trader.adapters.kraken.constants import KRAKEN_VENUE
from nautilus_trader.adapters.kraken.providers import KrakenInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CurrencyPair
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
    return KRAKEN_VENUE


@pytest.fixture
def account_id(venue) -> AccountId:
    return AccountId(f"{venue.value}-123")


@pytest.fixture
def instrument() -> CurrencyPair:
    return CurrencyPair(
        instrument_id=InstrumentId(Symbol("XBT/USDT"), KRAKEN_VENUE),
        raw_symbol=Symbol("XBTUSDT"),
        base_currency=BTC,
        quote_currency=USDT,
        price_precision=1,
        size_precision=8,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("0.00000001"),
        ts_event=0,
        ts_init=0,
        maker_fee=Decimal("0.0016"),
        taker_fee=Decimal("0.0026"),
    )


@pytest.fixture
def account_state(account_id) -> AccountState:
    return AccountState(
        account_id=account_id,
        account_type=AccountType.CASH,
        base_currency=USDT,
        reported=True,
        balances=[
            AccountBalance(
                total=Money(100_000, USDT),
                locked=Money(0, USDT),
                free=Money(100_000, USDT),
            ),
        ],
        margins=[],
        info={},
        event_id=TestIdStubs.uuid(),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture
def mock_http_client():
    mock = MagicMock(spec=nautilus_pyo3.KrakenSpotHttpClient)
    mock.api_key = "test_api_key"
    mock.api_secret = "test_api_secret"

    mock.request_instruments = AsyncMock(return_value=[])
    mock.cache_instrument = MagicMock()
    mock.cancel_all_requests = MagicMock()
    mock.is_initialized = MagicMock(return_value=True)

    return mock


def _create_ws_mock() -> MagicMock:
    mock = MagicMock(spec=nautilus_pyo3.KrakenSpotWebSocketClient)
    mock.url = "wss://ws.kraken.com/v2"
    mock.is_closed = MagicMock(return_value=False)
    mock.is_active = AsyncMock(return_value=True)
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()
    mock.subscribe_quotes = AsyncMock()
    mock.subscribe_trades = AsyncMock()
    mock.subscribe_book = AsyncMock()
    mock.subscribe_bars = AsyncMock()
    mock.unsubscribe_quotes = AsyncMock()
    mock.unsubscribe_trades = AsyncMock()
    mock.unsubscribe_book = AsyncMock()
    mock.unsubscribe_bars = AsyncMock()
    mock.cache_instrument = MagicMock()
    return mock


@pytest.fixture
def mock_ws_clients():
    return _create_ws_mock(), _create_ws_mock()


@pytest.fixture
def mock_instrument_provider(instrument):
    provider = MagicMock(spec=KrakenInstrumentProvider)
    provider.initialize = AsyncMock()
    provider.instruments_pyo3 = MagicMock(return_value=[MagicMock(name="py_instrument")])
    provider.get_all = MagicMock(return_value={instrument.id: instrument})
    provider.currencies = MagicMock(return_value={})
    provider.find = MagicMock(return_value=instrument)
    return provider


@pytest.fixture
def data_client():
    pass


@pytest.fixture
def exec_client():
    pass


def _create_mock_account_state():
    """
    Create a mock pyo3 account state for testing.
    """
    return nautilus_pyo3.AccountState(
        account_id=nautilus_pyo3.AccountId("KRAKEN-UNIFIED"),
        account_type=nautilus_pyo3.AccountType.CASH,
        base_currency=None,
        balances=[
            nautilus_pyo3.AccountBalance(
                total=nautilus_pyo3.Money.from_str("100000 USDT"),
                locked=nautilus_pyo3.Money.from_str("0 USDT"),
                free=nautilus_pyo3.Money.from_str("100000 USDT"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=nautilus_pyo3.UUID4(),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture
def mock_http_client_spot():
    """
    Mock for KrakenSpotHttpClient with execution methods.
    """
    mock = MagicMock(spec=nautilus_pyo3.KrakenSpotHttpClient)
    mock.api_key = "test_api_key"
    mock.api_secret = "test_api_secret"
    mock.api_key_masked = "test_api_****"

    # Instrument provider methods
    mock.request_instruments = AsyncMock(return_value=[])
    mock.cache_instrument = MagicMock()
    mock.cancel_all_requests = MagicMock()
    mock.is_initialized = MagicMock(return_value=True)

    # Execution methods
    mock.submit_order = AsyncMock()
    mock.cancel_order = AsyncMock()
    mock.cancel_all_orders = AsyncMock(return_value=0)

    # Reconciliation methods
    mock.request_account_state = AsyncMock(return_value=_create_mock_account_state())
    mock.request_order_status_reports = AsyncMock(return_value=[])
    mock.request_fill_reports = AsyncMock(return_value=[])
    mock.request_position_status_reports = AsyncMock(return_value=[])

    # Spot position reports config
    mock.set_use_spot_position_reports = MagicMock()
    mock.set_spot_positions_quote_currency = MagicMock()

    return mock


def _create_mock_account_state_futures():
    """
    Create a mock pyo3 account state for futures testing.
    """
    return nautilus_pyo3.AccountState(
        account_id=nautilus_pyo3.AccountId("KRAKEN-UNIFIED"),
        account_type=nautilus_pyo3.AccountType.MARGIN,
        base_currency=None,
        balances=[
            nautilus_pyo3.AccountBalance(
                total=nautilus_pyo3.Money.from_str("100000 USD"),
                locked=nautilus_pyo3.Money.from_str("0 USD"),
                free=nautilus_pyo3.Money.from_str("100000 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=nautilus_pyo3.UUID4(),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture
def mock_http_client_futures():
    """
    Mock for KrakenFuturesHttpClient with execution methods.
    """
    mock = MagicMock(spec=nautilus_pyo3.KrakenFuturesHttpClient)
    mock.api_key = "test_api_key"
    mock.api_secret = "test_api_secret"
    mock.api_key_masked = "test_api_****"

    # Instrument provider methods
    mock.request_instruments = AsyncMock(return_value=[])
    mock.cache_instrument = MagicMock()
    mock.cancel_all_requests = MagicMock()
    mock.is_initialized = MagicMock(return_value=True)

    # Execution methods
    mock.submit_order = AsyncMock()
    mock.cancel_order = AsyncMock()
    mock.cancel_all_orders = AsyncMock(return_value=0)

    # Reconciliation methods
    mock.request_account_state = AsyncMock(return_value=_create_mock_account_state_futures())
    mock.request_order_status_reports = AsyncMock(return_value=[])
    mock.request_fill_reports = AsyncMock(return_value=[])
    mock.request_position_status_reports = AsyncMock(return_value=[])

    return mock


def _create_exec_ws_mock_spot() -> MagicMock:
    """
    Create a mock for KrakenSpotWebSocketClient with execution methods.
    """
    mock = MagicMock(spec=nautilus_pyo3.KrakenSpotWebSocketClient)
    mock.url = "wss://ws-auth.kraken.com/v2"
    mock.is_closed = MagicMock(return_value=False)
    mock.is_active = AsyncMock(return_value=True)
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()

    # Execution-specific methods
    mock.authenticate = AsyncMock()
    mock.subscribe_executions = AsyncMock()
    mock.set_account_id = MagicMock()
    mock.cache_client_order = MagicMock()
    mock.cache_instrument = MagicMock()

    return mock


def _create_exec_ws_mock_futures() -> MagicMock:
    """
    Create a mock for KrakenFuturesWebSocketClient with execution methods.
    """
    mock = MagicMock(spec=nautilus_pyo3.KrakenFuturesWebSocketClient)
    mock.url = "wss://futures.kraken.com/ws/v1"
    mock.is_closed = MagicMock(return_value=False)
    mock.is_active = AsyncMock(return_value=True)
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()

    # Execution-specific methods
    mock.authenticate = AsyncMock()
    mock.subscribe_executions = AsyncMock()
    mock.set_account_id = MagicMock()
    mock.cache_instrument = MagicMock()

    return mock


def create_kraken_spot_instrument(base_currency, quote_currency):
    """
    Create a spot instrument for testing.
    """
    return CurrencyPair(
        instrument_id=InstrumentId.from_str(
            f"{base_currency.code}/{quote_currency.code}.KRAKEN",
        ),
        raw_symbol=Symbol(f"{base_currency.code}{quote_currency.code}"),
        base_currency=base_currency,
        quote_currency=quote_currency,
        price_precision=1,
        size_precision=8,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("0.00000001"),
        lot_size=None,
        max_quantity=Quantity.from_str("100000"),
        min_quantity=Quantity.from_str("0.00000001"),
        max_notional=None,
        min_notional=Money(1, quote_currency),
        max_price=Price.from_str("1000000"),
        min_price=Price.from_str("0.1"),
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal("0.0016"),
        taker_fee=Decimal("0.0026"),
        ts_event=0,
        ts_init=0,
    )
