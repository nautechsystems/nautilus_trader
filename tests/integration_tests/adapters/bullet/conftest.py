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

from nautilus_trader.adapters.bullet.constants import BULLET_VENUE
from nautilus_trader.adapters.bullet.providers import BulletInstrumentProvider
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


_TEST_ADDRESS = "4XN8Apf9powArmYLH1DRb2QvmyuYuvZp4qtdU8AtCavU"


@pytest.fixture(scope="session")
def live_clock():
    return LiveClock()


@pytest.fixture(scope="session")
def live_logger():
    return Logger("TEST_LOGGER")


@pytest.fixture
def venue() -> Venue:
    return BULLET_VENUE


@pytest.fixture
def account_id(venue) -> AccountId:
    return AccountId(f"{venue.value}-master")


@pytest.fixture
def instrument() -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=InstrumentId(Symbol("SOL-USD-PERP"), BULLET_VENUE),
        raw_symbol=Symbol("SOL-USD"),
        base_currency=Currency.from_str("SOL"),
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=2,
        size_precision=2,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.01"),
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
def btc_instrument() -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), BULLET_VENUE),
        raw_symbol=Symbol("BTC-USD"),
        base_currency=Currency.from_str("BTC"),
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=1,
        size_precision=4,
        price_increment=Price.from_str("0.1"),
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
        base_currency=None,
        reported=True,
        balances=[
            AccountBalance(
                total=Money(20_000, USD),
                locked=Money(100, USD),
                free=Money(19_900, USD),
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
    mock = MagicMock(spec=nautilus_pyo3.BulletHttpClient)
    mock.base_url = "https://tradingapi.testnet.bullet.xyz"

    _account_json = (
        '{"totalWalletBalance":"20000.00","availableBalance":"19900.00","positions":[]}'
    )
    mock.account_json = AsyncMock(return_value=_account_json)
    mock.open_orders_json = AsyncMock(return_value="[]")
    mock.exchange_info_json = AsyncMock(return_value='{"symbols":[]}')
    mock.balances_json = AsyncMock(return_value="[]")
    mock.best_bid_ask = AsyncMock(return_value='{"bidPrice":"90.00","askPrice":"91.00"}')
    return mock


@pytest.fixture
def mock_order_client():
    mock = MagicMock(spec=nautilus_pyo3.BulletOrderClient)
    mock.account_address = _TEST_ADDRESS
    mock.connect = AsyncMock()
    mock.is_connected = MagicMock(return_value=True)
    mock.place_order = AsyncMock(return_value="0xabc123")
    mock.cancel_order = AsyncMock(return_value="0xdef456")
    mock.amend_order = AsyncMock(return_value="0xghi789")
    mock.cancel_all_orders = AsyncMock(return_value="0xcancall")
    mock.cancel_market_orders = AsyncMock(return_value="0xcancmkt")
    return mock


def _create_ws_mock() -> MagicMock:
    mock = MagicMock(spec=nautilus_pyo3.BulletWebSocketClient)
    mock.url = "wss://tradingapi.testnet.bullet.xyz/ws"
    mock.is_connected = MagicMock(return_value=True)
    mock.is_started = MagicMock(return_value=True)
    mock.connect = AsyncMock()
    mock.close = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.subscribe_order_updates = AsyncMock()
    mock.subscribe_quotes = AsyncMock()
    mock.subscribe_book = AsyncMock()
    mock.subscribe_trades = AsyncMock()
    mock.subscribe_mark_prices = AsyncMock()
    mock.unsubscribe_order_updates = AsyncMock()
    mock.unsubscribe_quotes = AsyncMock()
    mock.unsubscribe_book = AsyncMock()
    mock.unsubscribe_trades = AsyncMock()
    mock.unsubscribe_mark_prices = AsyncMock()
    return mock


@pytest.fixture
def mock_ws_client():
    return _create_ws_mock()


@pytest.fixture
def mock_instrument_provider(instrument, btc_instrument):
    provider = MagicMock(spec=BulletInstrumentProvider)
    provider.initialize = AsyncMock()
    provider.load_all_async = AsyncMock()
    provider.load_ids_async = AsyncMock()
    provider.load_async = AsyncMock()
    provider.instruments_pyo3 = MagicMock(return_value=[])
    provider.get_all = MagicMock(
        return_value={
            instrument.id: instrument,
            btc_instrument.id: btc_instrument,
        },
    )
    provider.currencies = MagicMock(return_value={})
    provider.find = MagicMock(return_value=instrument)
    provider.list_all = MagicMock(return_value=[instrument, btc_instrument])
    return provider


@pytest.fixture
def data_client():
    return None


@pytest.fixture
def exec_client():
    return None
