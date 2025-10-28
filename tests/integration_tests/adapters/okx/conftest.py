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

from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.execution import OKXExecutionClient
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def _create_currency(
    code: str,
    precision: int,
    iso4217: int,
    name: str,
    currency_type: CurrencyType,
) -> Currency:
    return Currency(
        code=code,
        precision=precision,
        iso4217=iso4217,
        name=name,
        currency_type=currency_type,
    )


@pytest.fixture()
def venue():
    return OKX_VENUE


@pytest.fixture()
def account_id(venue) -> AccountId:
    return AccountId(f"{venue.value}-123")


@pytest.fixture()
def instrument() -> CurrencyPair:
    btc = _create_currency(
        "BTC",
        precision=8,
        iso4217=0,
        name="Bitcoin",
        currency_type=CurrencyType.CRYPTO,
    )
    usd = USD

    return CurrencyPair(
        instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
        raw_symbol=Symbol("BTC-USD"),
        base_currency=btc,
        quote_currency=usd,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        ts_event=0,
        ts_init=0,
        maker_fee=Decimal("-0.0002"),
        taker_fee=Decimal("0.0005"),
    )


@pytest.fixture()
def eth_usdt_instrument() -> CurrencyPair:
    eth = _create_currency(
        "ETH",
        precision=8,
        iso4217=0,
        name="Ethereum",
        currency_type=CurrencyType.CRYPTO,
    )
    usdt = _create_currency(
        "USDT",
        precision=8,
        iso4217=0,
        name="Tether",
        currency_type=CurrencyType.CRYPTO,
    )

    return CurrencyPair(
        instrument_id=InstrumentId(Symbol("ETH-USDT"), OKX_VENUE),
        raw_symbol=Symbol("ETH-USDT"),
        base_currency=eth,
        quote_currency=usdt,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        ts_event=0,
        ts_init=0,
        maker_fee=Decimal("-0.0002"),
        taker_fee=Decimal("0.0005"),
    )


@pytest.fixture()
def account_state(account_id) -> AccountState:
    usd_currency = USD

    return AccountState(
        account_id=account_id,
        account_type=AccountType.CASH,
        base_currency=None,  # Multi-currency account
        reported=True,
        balances=[
            AccountBalance(
                total=Money(100_000, usd_currency),
                locked=Money(0, usd_currency),
                free=Money(100_000, usd_currency),
            ),
        ],
        margins=[],
        info={},
        event_id=TestIdStubs.uuid(),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture()
def mock_http_client():
    mock = MagicMock(spec=nautilus_pyo3.OKXHttpClient)
    mock.api_key = "test_api_key"
    mock.api_secret = "test_api_secret"
    mock.api_passphrase = "test_passphrase"

    mock.request_instruments = AsyncMock(return_value=[])
    mock.add_instrument = MagicMock()
    mock.cancel_all_requests = MagicMock()
    mock.is_initialized = MagicMock(return_value=True)
    mock.http_get_server_time = AsyncMock(return_value=1234567890000)

    mock_account_state = MagicMock()
    mock_account_state.to_dict = MagicMock(
        return_value={
            "account_id": "OKX-123",
            "account_type": "CASH",
            "base_currency": None,
            "reported": True,
            "balances": [
                {
                    "currency": "USD",
                    "total": "100000.0",
                    "locked": "0.0",
                    "free": "100000.0",
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
    mock.request_trades = AsyncMock(return_value=[])
    mock.request_bars = AsyncMock(return_value=[])

    return mock


def _create_ws_mock() -> MagicMock:
    mock = MagicMock(spec=nautilus_pyo3.OKXWebSocketClient)
    mock.url = "wss://test.okx.com/realtime"
    mock.is_closed = MagicMock(return_value=False)
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()
    mock.subscribe_instruments = AsyncMock()
    mock.subscribe_book = AsyncMock()
    mock.subscribe_book50_l2_tbt = AsyncMock()
    mock.subscribe_book_l2_tbt = AsyncMock()
    mock.subscribe_book_depth5 = AsyncMock()
    mock.subscribe_book_with_depth = AsyncMock()
    mock.subscribe_quotes = AsyncMock()
    mock.subscribe_trades = AsyncMock()
    mock.subscribe_mark_prices = AsyncMock()
    mock.subscribe_index_prices = AsyncMock()
    mock.subscribe_funding_rates = AsyncMock()
    mock.subscribe_bars = AsyncMock()
    mock.unsubscribe_book = AsyncMock()
    mock.unsubscribe_book50_l2_tbt = AsyncMock()
    mock.unsubscribe_book_l2_tbt = AsyncMock()
    mock.unsubscribe_book_depth5 = AsyncMock()
    mock.unsubscribe_quotes = AsyncMock()
    mock.unsubscribe_trades = AsyncMock()
    mock.unsubscribe_mark_prices = AsyncMock()
    mock.unsubscribe_index_prices = AsyncMock()
    mock.unsubscribe_funding_rates = AsyncMock()
    mock.unsubscribe_bars = AsyncMock()
    mock.get_subscriptions = MagicMock(return_value=[])
    mock.subscribe_orders = AsyncMock()
    mock.subscribe_orders_algo = AsyncMock()
    mock.subscribe_fills = AsyncMock()
    mock.subscribe_account = AsyncMock()
    mock.batch_cancel_orders = AsyncMock()
    mock.mass_cancel_orders = AsyncMock()
    return mock


@pytest.fixture()
def mock_ws_clients():
    return _create_ws_mock(), _create_ws_mock()


@pytest.fixture()
def mock_instrument_provider(instrument):
    provider = MagicMock(spec=OKXInstrumentProvider)
    provider.initialize = AsyncMock()
    provider.instruments_pyo3 = MagicMock(return_value=[MagicMock(name="py_instrument")])
    provider.get_all = MagicMock(return_value={instrument.id: instrument})
    provider.currencies = MagicMock(return_value={})
    provider.find = MagicMock(return_value=instrument)
    provider.instrument_types = (nautilus_pyo3.OKXInstrumentType.SPOT,)
    provider._instrument_types = (nautilus_pyo3.OKXInstrumentType.SPOT,)
    return provider


@pytest.fixture()
def exec_client(
    event_loop,
    mock_http_client,
    mock_ws_clients,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
    monkeypatch,
):
    private_ws, business_ws = mock_ws_clients

    ws_iter = iter([private_ws, business_ws])

    def ws_with_credentials(*args, **kwargs):
        return next(ws_iter)

    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.nautilus_pyo3.OKXWebSocketClient.with_credentials",
        ws_with_credentials,
    )

    config = OKXExecClientConfig(
        api_key="test_api_key",
        api_secret="test_api_secret",
        api_passphrase="test_passphrase",
        instrument_types=(OKXInstrumentType.SPOT,),
    )

    client = OKXExecutionClient(
        loop=event_loop,
        client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        clock=live_clock,
        instrument_provider=mock_instrument_provider,
        config=config,
        name=None,
    )

    client._mock_ws_private = private_ws
    client._mock_ws_business = business_ws
    return client


@pytest.fixture()
def data_client():
    return None
