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
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_ETHUSDT_PERP = TestInstrumentProviderPyo3.ethusdt_perp_binance()

_ETHUSD_PERP_QUANTO = nautilus_pyo3.CryptoPerpetual(
    instrument_id=nautilus_pyo3.InstrumentId.from_str("ETHUSDQ-PERP.BITMEX"),
    raw_symbol=nautilus_pyo3.Symbol("ETHUSDQ-PERP"),
    base_currency=nautilus_pyo3.Currency.from_str("ETH"),
    quote_currency=nautilus_pyo3.Currency.from_str("USD"),
    settlement_currency=nautilus_pyo3.Currency.from_str("BTC"),
    is_inverse=False,
    price_precision=1,
    size_precision=0,
    price_increment=nautilus_pyo3.Price.from_str("0.5"),
    size_increment=nautilus_pyo3.Quantity.from_int(1),
    maker_fee=Decimal("0"),
    taker_fee=Decimal("0"),
    margin_init=Decimal("0.01"),
    margin_maint=Decimal("0.005"),
    multiplier=None,
    lot_size=None,
    max_quantity=nautilus_pyo3.Quantity.from_int(1_000_000),
    min_quantity=nautilus_pyo3.Quantity.from_int(1),
    max_notional=None,
    min_notional=None,
    max_price=nautilus_pyo3.Price.from_str("1000000.0"),
    min_price=nautilus_pyo3.Price.from_str("0.5"),
    ts_event=0,
    ts_init=0,
)


def test_equality():
    item_1 = TestInstrumentProviderPyo3.ethusdt_perp_binance()
    item_2 = TestInstrumentProviderPyo3.ethusdt_perp_binance()
    assert item_1 == item_2


def test_hash():
    assert hash(_ETHUSDT_PERP) == hash(_ETHUSDT_PERP)


def test_to_dict():
    result = _ETHUSDT_PERP.to_dict()
    assert nautilus_pyo3.CryptoPerpetual.from_dict(result) == _ETHUSDT_PERP
    assert result == {
        "type": "CryptoPerpetual",
        "id": "ETHUSDT-PERP.BINANCE",
        "raw_symbol": "ETHUSDT-PERP",
        "base_currency": "ETH",
        "quote_currency": "USDT",
        "settlement_currency": "USDT",
        "is_inverse": False,
        "price_precision": 2,
        "size_precision": 3,
        "price_increment": "0.01",
        "size_increment": "0.001",
        "multiplier": "1",
        "lot_size": "1",
        "max_quantity": "10000",
        "min_quantity": "0.001",
        "max_notional": None,
        "min_notional": "10.00000000 USDT",
        "max_price": "15000.0",
        "min_price": "1.0",
        "maker_fee": "0.0002",
        "margin_init": "1.00",
        "margin_maint": "0.35",
        "taker_fee": "0.0004",
        "info": {},
        "ts_event": 0,
        "ts_init": 0,
    }


def test_pyo3_cython_conversion():
    crypto_perpetual_pyo3 = TestInstrumentProviderPyo3.ethusdt_perp_binance()
    crypto_perpetual_pyo3_dict = crypto_perpetual_pyo3.to_dict()
    crypto_perpetual_cython = CryptoPerpetual.from_pyo3(crypto_perpetual_pyo3)
    crypto_perpetual_cython_dict = CryptoPerpetual.to_dict(crypto_perpetual_cython)
    del crypto_perpetual_cython_dict["tick_scheme_name"]  # TODO: Under development
    crypto_perpetual_pyo3_back = nautilus_pyo3.CryptoPerpetual.from_dict(
        crypto_perpetual_cython_dict,
    )
    assert crypto_perpetual_pyo3 == crypto_perpetual_pyo3_back
    assert crypto_perpetual_pyo3_dict == crypto_perpetual_cython_dict


def test_get_cost_currency_linear():
    linear = CryptoPerpetual.from_pyo3(_ETHUSDT_PERP)
    assert linear.get_cost_currency() == Currency.from_str("USDT")


def test_get_cost_currency_inverse():
    inverse = CryptoPerpetual.from_pyo3(TestInstrumentProviderPyo3.xbtusd_bitmex())
    assert inverse.get_cost_currency() == Currency.from_str("BTC")


def test_get_cost_currency_quanto():
    quanto = CryptoPerpetual.from_pyo3(_ETHUSD_PERP_QUANTO)
    assert quanto.get_cost_currency() == Currency.from_str("BTC")


def test_notional_value_linear():
    linear = CryptoPerpetual.from_pyo3(_ETHUSDT_PERP)
    quantity = Quantity.from_str("5")
    price = Price.from_str("2000.0")

    notional = linear.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("USDT")
    assert notional.as_decimal() == Decimal("10000.0")


def test_notional_value_inverse():
    inverse = CryptoPerpetual.from_pyo3(TestInstrumentProviderPyo3.xbtusd_bitmex())
    quantity = Quantity.from_int(100_000)
    price = Price.from_str("370.00")

    notional = inverse.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("BTC")
    assert notional.as_decimal() == Decimal("270.27027027")


def test_notional_value_inverse_with_quote_override():
    inverse = CryptoPerpetual.from_pyo3(TestInstrumentProviderPyo3.xbtusd_bitmex())
    quantity = Quantity.from_int(100_000)
    price = Price.from_str("370.00")

    notional = inverse.notional_value(quantity, price, use_quote_for_inverse=True)

    assert notional.currency == Currency.from_str("USD")
    assert notional.as_decimal() == Decimal("100000")


def test_notional_value_quanto():
    quanto = CryptoPerpetual.from_pyo3(_ETHUSD_PERP_QUANTO)
    quantity = Quantity.from_int(1000)
    price = Price.from_str("2500.0")

    notional = quanto.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("BTC")
    expected = quantity.as_decimal() * quanto.multiplier.as_decimal() * price.as_decimal()
    assert notional.as_decimal() == expected
