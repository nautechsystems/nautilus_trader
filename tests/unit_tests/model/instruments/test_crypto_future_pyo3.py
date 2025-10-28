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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import Currency as Pyo3Currency
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Price as Pyo3Price
from nautilus_trader.core.nautilus_pyo3 import Quantity as Pyo3Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_BTCUSDT_FUTURE = TestInstrumentProviderPyo3.btcusdt_future_binance()

# Create an inverse future fixture (BitMEX-style inverse contract)
_XBTUSD_FUTURE_INVERSE = nautilus_pyo3.CryptoFuture(
    instrument_id=InstrumentId.from_str("XBTUSD_240329.BITMEX"),
    raw_symbol=Symbol("XBTUSD_240329"),
    underlying=Pyo3Currency.from_str("BTC"),
    quote_currency=Pyo3Currency.from_str("USD"),
    settlement_currency=Pyo3Currency.from_str("BTC"),
    is_inverse=True,
    activation_ns=1640390400000000000,
    expiration_ns=1711670400000000000,
    price_precision=1,
    size_precision=0,
    price_increment=Pyo3Price.from_str("0.5"),
    size_increment=Pyo3Quantity.from_int(1),
    maker_fee=Decimal("-0.00025"),
    taker_fee=Decimal("0.00075"),
    margin_init=Decimal("0.01"),
    margin_maint=Decimal("0.005"),
    multiplier=None,
    lot_size=None,
    max_quantity=Pyo3Quantity.from_int(10_000_000),
    min_quantity=Pyo3Quantity.from_int(1),
    max_notional=None,
    min_price=Pyo3Price.from_str("0.5"),
    max_price=Pyo3Price.from_str("1000000.0"),
    ts_event=0,
    ts_init=0,
)

# Create a quanto future fixture (BitMEX-style quanto contract)
_ETHUSD_FUTURE_QUANTO = nautilus_pyo3.CryptoFuture(
    instrument_id=InstrumentId.from_str("ETHUSD_240329.BITMEX"),
    raw_symbol=Symbol("ETHUSD_240329"),
    underlying=Pyo3Currency.from_str("ETH"),
    quote_currency=Pyo3Currency.from_str("USD"),
    settlement_currency=Pyo3Currency.from_str("BTC"),
    is_inverse=False,
    activation_ns=1640390400000000000,
    expiration_ns=1711670400000000000,
    price_precision=1,
    size_precision=0,
    price_increment=Pyo3Price.from_str("0.5"),
    size_increment=Pyo3Quantity.from_int(1),
    maker_fee=Decimal("0"),
    taker_fee=Decimal("0"),
    margin_init=Decimal("0.01"),
    margin_maint=Decimal("0.005"),
    multiplier=None,
    lot_size=None,
    max_quantity=Pyo3Quantity.from_int(10_000_000),
    min_quantity=Pyo3Quantity.from_int(1),
    max_notional=None,
    min_price=Pyo3Price.from_str("0.5"),
    max_price=Pyo3Price.from_str("1000000.0"),
    ts_event=0,
    ts_init=0,
)


def test_equality():
    item_1 = TestInstrumentProviderPyo3.btcusdt_future_binance()
    item_2 = TestInstrumentProviderPyo3.btcusdt_future_binance()
    assert item_1 == item_2


def test_hash():
    assert hash(_BTCUSDT_FUTURE) == hash(_BTCUSDT_FUTURE)


def test_to_dict():
    result = _BTCUSDT_FUTURE.to_dict()
    assert nautilus_pyo3.CryptoFuture.from_dict(result) == _BTCUSDT_FUTURE
    assert result == {
        "type": "CryptoFuture",
        "id": "BTCUSDT_220325.BINANCE",
        "raw_symbol": "BTCUSDT_220325",
        "underlying": "BTC",
        "quote_currency": "USDT",
        "settlement_currency": "USDT",
        "is_inverse": False,
        "activation_ns": 1640390400000000000,
        "expiration_ns": 1648166400000000000,
        "price_precision": 2,
        "size_precision": 6,
        "price_increment": "0.01",
        "size_increment": "0.000001",
        "maker_fee": "0",
        "taker_fee": "0",
        "margin_maint": "0",
        "margin_init": "0",
        "multiplier": "1",
        "lot_size": "1",
        "max_notional": None,
        "max_price": "1000000.0",
        "max_quantity": "9000",
        "min_notional": "10.00000000 USDT",
        "min_price": "0.01",
        "min_quantity": "0.00001",
        "info": {},
        "ts_event": 0,
        "ts_init": 0,
    }


def test_pyo3_cython_conversion():
    crypto_future_pyo3 = TestInstrumentProviderPyo3.btcusdt_future_binance()
    crypto_future_pyo3_dict = crypto_future_pyo3.to_dict()
    crypto_future_cython = CryptoFuture.from_pyo3(crypto_future_pyo3)
    crypto_future_cython_dict = CryptoFuture.to_dict(crypto_future_cython)
    del crypto_future_cython_dict["tick_scheme_name"]  # TODO: Under development
    crypto_future_pyo3_back = nautilus_pyo3.CryptoFuture.from_dict(crypto_future_cython_dict)
    assert crypto_future_pyo3 == crypto_future_pyo3_back
    assert crypto_future_pyo3_dict == crypto_future_cython_dict


def test_get_base_currency_linear():
    linear = CryptoFuture.from_pyo3(_BTCUSDT_FUTURE)
    assert linear.get_base_currency() == Currency.from_str("BTC")


def test_get_base_currency_inverse():
    inverse = CryptoFuture.from_pyo3(_XBTUSD_FUTURE_INVERSE)
    assert inverse.get_base_currency() == Currency.from_str("BTC")


def test_get_cost_currency_linear():
    linear = CryptoFuture.from_pyo3(_BTCUSDT_FUTURE)
    assert linear.get_cost_currency() == Currency.from_str("USDT")


def test_get_cost_currency_inverse():
    inverse = CryptoFuture.from_pyo3(_XBTUSD_FUTURE_INVERSE)
    assert inverse.get_cost_currency() == Currency.from_str("BTC")


def test_get_cost_currency_quanto():
    quanto = CryptoFuture.from_pyo3(_ETHUSD_FUTURE_QUANTO)
    assert quanto.get_cost_currency() == Currency.from_str("BTC")


def test_notional_value_linear():
    linear = CryptoFuture.from_pyo3(_BTCUSDT_FUTURE)
    quantity = Quantity.from_int(10)
    price = Price.from_str("50000.00")

    notional = linear.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("USDT")
    assert notional.as_decimal() == Decimal("500000.00")


def test_notional_value_inverse():
    inverse = CryptoFuture.from_pyo3(_XBTUSD_FUTURE_INVERSE)
    quantity = Quantity.from_int(10_000)
    price = Price.from_str("50000.0")

    notional = inverse.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("BTC")
    assert notional.as_decimal() == Decimal("0.2")


def test_notional_value_inverse_with_quote_override():
    inverse = CryptoFuture.from_pyo3(_XBTUSD_FUTURE_INVERSE)
    quantity = Quantity.from_int(10_000)
    price = Price.from_str("50000.0")

    notional = inverse.notional_value(quantity, price, use_quote_for_inverse=True)

    assert notional.currency == Currency.from_str("USD")
    assert notional.as_decimal() == Decimal("10000")


def test_notional_value_quanto():
    quanto = CryptoFuture.from_pyo3(_ETHUSD_FUTURE_QUANTO)
    quantity = Quantity.from_int(1000)
    price = Price.from_str("2000.0")

    notional = quanto.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("BTC")
    expected = quantity.as_decimal() * quanto.multiplier.as_decimal() * price.as_decimal()
    assert notional.as_decimal() == expected
