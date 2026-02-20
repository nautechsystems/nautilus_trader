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

from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.architect_ax.constants import AX_VENUE
from nautilus_trader.adapters.architect_ax.providers import AxInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core import nautilus_pyo3
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
    return AX_VENUE


@pytest.fixture
def account_id(venue) -> AccountId:
    return AccountId(f"{venue.value}-001")


@pytest.fixture
def instrument() -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        raw_symbol=Symbol("GBPUSD-PERP"),
        base_currency=USD,
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=5,
        size_precision=0,
        price_increment=Price.from_str("0.00001"),
        size_increment=Quantity.from_str("1"),
        max_quantity=None,
        min_quantity=Quantity.from_str("1"),
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
def account_state(account_id) -> AccountState:
    return AccountState(
        account_id=account_id,
        account_type=AccountType.MARGIN,
        base_currency=None,
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


@pytest.fixture
def mock_http_client():
    mock = MagicMock(spec=nautilus_pyo3.AxHttpClient)
    mock.api_key = "test_api_key"
    mock.api_key_masked = "test_api_****"

    mock.authenticate_auto = AsyncMock(return_value="test_bearer_token")
    mock.request_instruments = AsyncMock(return_value=[])
    mock.request_funding_rates = AsyncMock(return_value=[])
    mock.request_trade_ticks = AsyncMock(return_value=[])
    mock.request_bars = AsyncMock(return_value=[])
    mock.cache_instrument = MagicMock()
    mock.cancel_all_requests = MagicMock()
    mock.is_initialized = MagicMock(return_value=True)

    mock_account_state = MagicMock()
    mock_account_state.to_dict = MagicMock(
        return_value={
            "account_id": "AX-001",
            "account_type": "MARGIN",
            "base_currency": None,
            "reported": True,
            "balances": [
                {
                    "currency": "USD",
                    "total": "100000.00",
                    "locked": "0.00",
                    "free": "100000.00",
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
    mock.request_order_status = AsyncMock(return_value=MagicMock())
    mock.request_order_status_reports = AsyncMock(return_value=[])
    mock.request_fill_reports = AsyncMock(return_value=[])
    mock.request_position_reports = AsyncMock(return_value=[])
    mock.preview_aggressive_limit_order = AsyncMock(return_value=None)

    return mock


def _create_ws_mock() -> MagicMock:
    """
    Create a mock AxMdWebSocketClient.
    """
    mock = MagicMock(spec=nautilus_pyo3.AxMdWebSocketClient)
    mock.url = "wss://test.architect.exchange/md/ws"
    mock.is_closed = MagicMock(return_value=False)
    mock.is_active = AsyncMock(return_value=True)
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()
    mock.subscribe_book_deltas = AsyncMock()
    mock.subscribe_quotes = AsyncMock()
    mock.subscribe_trades = AsyncMock()
    mock.subscribe_bars = AsyncMock()
    mock.unsubscribe_book_deltas = AsyncMock()
    mock.unsubscribe_quotes = AsyncMock()
    mock.unsubscribe_trades = AsyncMock()
    mock.unsubscribe_bars = AsyncMock()
    mock.cache_instrument = MagicMock()
    mock.set_auth_token = MagicMock()
    return mock


def _create_orders_ws_mock() -> MagicMock:
    """
    Create a mock AxOrdersWebSocketClient.
    """
    mock = MagicMock(spec=nautilus_pyo3.AxOrdersWebSocketClient)
    mock.url = "wss://test.architect.exchange/orders/ws"
    mock.is_closed = MagicMock(return_value=False)
    mock.is_active = AsyncMock(return_value=True)
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()
    mock.submit_order = AsyncMock()
    mock.cancel_order = AsyncMock()
    mock.cache_instrument = MagicMock()
    return mock


@pytest.fixture
def mock_instrument_provider(instrument):
    provider = MagicMock(spec=AxInstrumentProvider)
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
