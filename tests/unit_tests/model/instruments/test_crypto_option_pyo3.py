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
from nautilus_trader.core.nautilus_pyo3 import OptionKind
from nautilus_trader.core.nautilus_pyo3 import Price as Pyo3Price
from nautilus_trader.core.nautilus_pyo3 import Quantity as Pyo3Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.model.instruments import CryptoOption
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_BTCUSD_OPTION = TestInstrumentProviderPyo3.btcusd_option_deribit()

# Create an inverse option fixture (hypothetical inverse crypto option)
_XBTUSD_OPTION_INVERSE = nautilus_pyo3.CryptoOption(
    instrument_id=InstrumentId.from_str("XBTUSD-240329-50000-C.DERIBIT"),
    raw_symbol=Symbol("XBTUSD-240329-50000-C"),
    underlying=Pyo3Currency.from_str("BTC"),
    quote_currency=Pyo3Currency.from_str("USD"),
    settlement_currency=Pyo3Currency.from_str("BTC"),
    is_inverse=True,
    option_kind=OptionKind.CALL,
    strike_price=Pyo3Price.from_str("50000.00"),
    activation_ns=1640390400000000000,
    expiration_ns=1711670400000000000,
    price_precision=4,
    size_precision=0,
    price_increment=Pyo3Price.from_str("0.0001"),
    size_increment=Pyo3Quantity.from_int(1),
    maker_fee=Decimal("0.0003"),
    taker_fee=Decimal("0.0003"),
    margin_init=Decimal("0.15"),
    margin_maint=Decimal("0.075"),
    multiplier=None,
    lot_size=None,
    max_quantity=Pyo3Quantity.from_int(10_000),
    min_quantity=Pyo3Quantity.from_int(1),
    max_notional=None,
    min_notional=None,
    max_price=None,
    min_price=None,
    ts_event=0,
    ts_init=0,
)


def test_equality():
    item_1 = TestInstrumentProviderPyo3.btcusd_option_deribit()
    item_2 = TestInstrumentProviderPyo3.btcusd_option_deribit()
    assert item_1 == item_2


def test_hash():
    assert hash(_BTCUSD_OPTION) == hash(_BTCUSD_OPTION)


def test_to_dict():
    result = _BTCUSD_OPTION.to_dict()
    assert nautilus_pyo3.CryptoOption.from_dict(result) == _BTCUSD_OPTION
    assert result == {
        "type": "CryptoOption",
        "id": "BTC-13JAN23-16000-P.DERIBIT",
        "raw_symbol": "BTC-13JAN23-16000-P",
        "underlying": "BTC",
        "quote_currency": "USD",
        "settlement_currency": "BTC",
        "is_inverse": False,
        "option_kind": "PUT",
        "strike_price": "16000.00",
        "activation_ns": 1671696002000000000,
        "expiration_ns": 1673596800000000000,
        "price_precision": 2,
        "size_precision": 1,
        "price_increment": "0.01",
        "size_increment": "0.1",
        "multiplier": "1",
        "lot_size": "1",
        "margin_init": "0",
        "margin_maint": "0",
        "maker_fee": "0.0003",
        "taker_fee": "0.0003",
        "ts_event": 0,
        "ts_init": 0,
        "info": {},
        "max_quantity": "9000",
        "min_quantity": "0.1",
        "max_notional": None,
        "min_notional": "10.00 USD",
        "max_price": None,
        "min_price": None,
    }


def test_pyo3_cython_conversion():
    crypto_option_pyo3 = TestInstrumentProviderPyo3.btcusd_option_deribit()
    crypto_option_pyo3_dict = crypto_option_pyo3.to_dict()
    crypto_option_cython = CryptoOption.from_pyo3(crypto_option_pyo3)
    crypto_option_cython_dict = CryptoOption.to_dict(crypto_option_cython)
    del crypto_option_cython_dict["tick_scheme_name"]  # TODO: Under development
    crypto_option_pyo3_back = nautilus_pyo3.CryptoOption.from_dict(crypto_option_cython_dict)
    assert crypto_option_pyo3 == crypto_option_pyo3_back
    assert crypto_option_pyo3_dict == crypto_option_cython_dict


def test_get_base_currency_linear():
    # Linear option: base currency is the underlying
    linear = CryptoOption.from_pyo3(_BTCUSD_OPTION)
    assert linear.get_base_currency() == Currency.from_str("BTC")


def test_get_base_currency_inverse():
    # Inverse option: base currency is the underlying
    inverse = CryptoOption.from_pyo3(_XBTUSD_OPTION_INVERSE)
    assert inverse.get_base_currency() == Currency.from_str("BTC")


def test_get_cost_currency_linear():
    # Linear option: cost currency is quote currency
    linear = CryptoOption.from_pyo3(_BTCUSD_OPTION)
    assert linear.get_cost_currency() == Currency.from_str("USD")


def test_get_cost_currency_inverse():
    # Inverse option: cost currency is underlying (base)
    inverse = CryptoOption.from_pyo3(_XBTUSD_OPTION_INVERSE)
    assert inverse.get_cost_currency() == Currency.from_str("BTC")


def test_notional_value_linear():
    # Linear option: notional = quantity * multiplier * price (in quote currency)
    # Example: 10 contracts * 1 multiplier * $2,000 premium = $20,000 USD
    linear = CryptoOption.from_pyo3(_BTCUSD_OPTION)
    quantity = Quantity.from_int(10)
    price = Price.from_str("2000.00")

    notional = linear.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("USD")
    assert notional.as_decimal() == Decimal("20000.00")


def test_notional_value_inverse():
    # Inverse option: notional = quantity * multiplier * (1 / price) (in base currency)
    # Example: 100 contracts * 1 multiplier * (1 / $0.05) = 2000 BTC
    inverse = CryptoOption.from_pyo3(_XBTUSD_OPTION_INVERSE)
    quantity = Quantity.from_int(100)
    price = Price.from_str("0.05")

    notional = inverse.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("BTC")
    assert notional.as_decimal() == Decimal("2000")


def test_notional_value_inverse_with_quote_override():
    # Test the use_quote_for_inverse escape hatch
    # When True, returns notional in quote currency instead of base
    inverse = CryptoOption.from_pyo3(_XBTUSD_OPTION_INVERSE)
    quantity = Quantity.from_int(100)
    price = Price.from_str("0.05")

    notional = inverse.notional_value(quantity, price, use_quote_for_inverse=True)

    # Should return quantity directly in quote currency (not calculated)
    assert notional.currency == Currency.from_str("USD")
    assert notional.as_decimal() == Decimal("100")
