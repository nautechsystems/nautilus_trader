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

import pytest

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoOption
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


@pytest.fixture(scope="session")
def live_clock():
    return LiveClock()


@pytest.fixture(scope="session")
def live_logger():
    return Logger("TEST_LOGGER")


@pytest.fixture(scope="session")
def bybit_http_client(session_event_loop, live_clock):
    client = BybitHttpClient(
        clock=live_clock,
        api_key="BYBIT_API_KEY",
        api_secret="BYBIT_API_SECRET",
        base_url="https://api-testnet.bybit.com",
    )
    return client


@pytest.fixture()
def venue() -> Venue:
    raise BYBIT_VENUE


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


@pytest.fixture()
def linear_btcusdt_symbol():
    return BybitSymbol("BTCUSDT.LINEAR")


def create_bybit_spot_instrument(base_currency, quote_currency):
    """
    Create a spot instrument for testing.
    """
    return CurrencyPair(
        instrument_id=InstrumentId.from_str(
            f"{base_currency.code}{quote_currency.code}-SPOT.BYBIT",
        ),
        raw_symbol=Symbol(f"{base_currency.code}{quote_currency.code}"),
        base_currency=base_currency,
        quote_currency=quote_currency,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        lot_size=None,
        max_quantity=Quantity.from_str("10000"),
        min_quantity=Quantity.from_str("0.00001"),
        max_notional=None,
        min_notional=Money(1, quote_currency),
        max_price=Price.from_str("1000000"),
        min_price=Price.from_str("0.01"),
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal("0.001"),
        taker_fee=Decimal("0.001"),
        ts_event=0,
        ts_init=0,
    )


def create_bybit_linear_perpetual():
    """
    Create a linear perpetual (USDT settled) for testing.
    """
    return CryptoPerpetual(
        instrument_id=InstrumentId.from_str("BTCUSDT-LINEAR.BYBIT"),
        raw_symbol=Symbol("BTCUSDT"),
        base_currency=BTC,
        quote_currency=USDT,
        settlement_currency=USDT,
        is_inverse=False,
        price_precision=2,
        size_precision=3,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.001"),
        max_quantity=Quantity.from_str("1000"),
        min_quantity=Quantity.from_str("0.001"),
        max_notional=None,
        min_notional=Money(5, USDT),
        max_price=Price.from_str("1000000"),
        min_price=Price.from_str("0.01"),
        margin_init=Decimal("0.01"),
        margin_maint=Decimal("0.005"),
        maker_fee=Decimal("0.0001"),
        taker_fee=Decimal("0.0006"),
        ts_event=0,
        ts_init=0,
    )


def create_bybit_inverse_perpetual():
    """
    Create an inverse perpetual (BTC settled) for testing.
    """
    return CryptoPerpetual(
        instrument_id=InstrumentId.from_str("BTCUSD-INVERSE.BYBIT"),
        raw_symbol=Symbol("BTCUSD"),
        base_currency=BTC,
        quote_currency=BTC,  # Inverse uses BTC as quote
        settlement_currency=BTC,
        is_inverse=True,
        price_precision=1,
        size_precision=0,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("1"),
        max_quantity=Quantity.from_str("1000000"),
        min_quantity=Quantity.from_str("1"),
        max_notional=None,
        min_notional=None,
        max_price=Price.from_str("1000000"),
        min_price=Price.from_str("0.1"),
        margin_init=Decimal("0.01"),
        margin_maint=Decimal("0.005"),
        maker_fee=Decimal("-0.00025"),  # Rebate
        taker_fee=Decimal("0.00075"),
        ts_event=0,
        ts_init=0,
    )


def create_bybit_option_instrument():
    """
    Create an option instrument for testing.
    """
    return CryptoOption(
        instrument_id=InstrumentId.from_str("BTC-280325-100000-C.BYBIT"),
        raw_symbol=Symbol("BTC-280325-100000-C"),
        underlying=BTC,
        quote_currency=USDC,
        settlement_currency=USDC,
        is_inverse=False,
        activation_ns=0,
        expiration_ns=0,
        strike_price=Price.from_str("100000"),
        option_kind=OptionKind.CALL,
        price_precision=2,
        size_precision=3,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.001"),
        max_quantity=Quantity.from_str("1000"),
        min_quantity=Quantity.from_str("0.001"),
        max_notional=None,
        min_notional=Money(5, USDC),
        max_price=Price.from_str("1000000"),
        min_price=Price.from_str("0.01"),
        margin_init=Decimal("0.01"),
        margin_maint=Decimal("0.005"),
        maker_fee=Decimal("0.0003"),
        taker_fee=Decimal("0.0003"),
        ts_event=0,
        ts_init=0,
    )
