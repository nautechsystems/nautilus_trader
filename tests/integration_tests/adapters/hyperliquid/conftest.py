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

from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
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
from nautilus_trader.model.objects import Currency
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
    return HYPERLIQUID_VENUE


@pytest.fixture
def account_id(venue) -> AccountId:
    return AccountId(f"{venue.value}-0x1234567890abcdef1234567890abcdef12345678")


@pytest.fixture
def instrument() -> CryptoPerpetual:
    """
    Return a BTC-USD perpetual instrument for Hyperliquid.
    """
    return CryptoPerpetual(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        raw_symbol=Symbol("BTC"),
        base_currency=Currency.from_str("BTC"),
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=1,
        size_precision=5,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("0.00001"),
        max_quantity=None,
        min_quantity=None,
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
def eth_instrument() -> CryptoPerpetual:
    """
    Return an ETH-USD perpetual instrument for Hyperliquid.
    """
    return CryptoPerpetual(
        instrument_id=InstrumentId(Symbol("ETH-USD-PERP"), HYPERLIQUID_VENUE),
        raw_symbol=Symbol("ETH"),
        base_currency=Currency.from_str("ETH"),
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=2,
        size_precision=4,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.0001"),
        max_quantity=None,
        min_quantity=None,
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


@pytest.fixture
def mock_http_client():
    """
    Create a mock HyperliquidHttpClient.
    """
    mock = MagicMock(spec=nautilus_pyo3.HyperliquidHttpClient)

    mock.is_testnet = MagicMock(return_value=False)

    mock.load_instrument_definitions = AsyncMock(return_value=[])
    mock.request_instruments = AsyncMock(return_value=[])
    mock.cache_instrument = MagicMock()
    mock.cache_instruments = MagicMock()

    mock_account_state = MagicMock()
    mock_account_state.to_dict = MagicMock(
        return_value={
            "account_id": "HYPERLIQUID-0x1234",
            "account_type": "MARGIN",
            "base_currency": "USD",
            "reported": True,
            "balances": [
                {
                    "currency": "USD",
                    "total": "100000.00000000",
                    "locked": "0.00000000",
                    "free": "100000.00000000",
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

    mock.request_order_status_reports = AsyncMock(return_value=[])
    mock.request_fill_reports = AsyncMock(return_value=[])
    mock.request_position_status_reports = AsyncMock(return_value=[])

    mock.submit_order = AsyncMock()
    mock.submit_order_from_order_any = AsyncMock()
    mock.submit_orders = AsyncMock(return_value=[])
    mock.modify_order = AsyncMock()
    mock.cancel_order = AsyncMock()

    mock.info_meta = AsyncMock(return_value=MagicMock())
    mock.info_l2_book = AsyncMock(return_value=MagicMock())
    mock.info_clearinghouse_state = AsyncMock(return_value={})

    return mock


def _create_ws_mock() -> MagicMock:
    mock = MagicMock(spec=nautilus_pyo3.HyperliquidWebSocketClient)
    mock.url = "wss://api.hyperliquid.xyz/ws"
    mock.is_closed = MagicMock(return_value=False)
    mock.is_active = MagicMock(return_value=True)
    mock.connect = AsyncMock()
    mock.close = AsyncMock()
    mock.disconnect = AsyncMock()
    mock.get_cloid_mapping = MagicMock(return_value=None)
    mock.subscribe_book = AsyncMock()
    mock.subscribe_trades = AsyncMock()
    mock.subscribe_quotes = AsyncMock()
    mock.subscribe_bars = AsyncMock()
    mock.subscribe_mark_prices = AsyncMock()
    mock.subscribe_index_prices = AsyncMock()
    mock.subscribe_funding_rates = AsyncMock()
    mock.unsubscribe_book = AsyncMock()
    mock.unsubscribe_trades = AsyncMock()
    mock.unsubscribe_quotes = AsyncMock()
    mock.unsubscribe_bars = AsyncMock()
    mock.unsubscribe_mark_prices = AsyncMock()
    mock.unsubscribe_index_prices = AsyncMock()
    mock.unsubscribe_funding_rates = AsyncMock()
    mock.cache_instrument = MagicMock()
    mock.cache_instruments = MagicMock()
    mock.subscribe_order_updates = AsyncMock()
    mock.subscribe_user_events = AsyncMock()
    mock.subscribe_user_fills = AsyncMock()
    mock.next_event = AsyncMock(return_value=None)
    return mock


@pytest.fixture
def mock_ws_client():
    return _create_ws_mock()


@pytest.fixture
def mock_instrument_provider(instrument, eth_instrument):
    provider = MagicMock(spec=HyperliquidInstrumentProvider)
    provider.initialize = AsyncMock()
    provider.load_all_async = AsyncMock()
    provider.load_ids_async = AsyncMock()
    provider.load_async = AsyncMock()
    provider.instruments_pyo3 = MagicMock(return_value=[])
    provider.get_all = MagicMock(
        return_value={
            instrument.id: instrument,
            eth_instrument.id: eth_instrument,
        },
    )
    provider.currencies = MagicMock(return_value={})
    provider.find = MagicMock(return_value=instrument)
    provider.list_all = MagicMock(return_value=[instrument, eth_instrument])
    return provider


@pytest.fixture
def data_client():
    return None


@pytest.fixture
def exec_client():
    return None
