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

import pytest

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import AssetClass
from nautilus_trader.core.nautilus_pyo3 import Currency as Pyo3Currency
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Price as Pyo3Price
from nautilus_trader.core.nautilus_pyo3 import Quantity as Pyo3Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.model.instruments import PerpetualContract
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_EURUSD_PERP = TestInstrumentProviderPyo3.perpetual_contract_eurusd()

# Inverse perpetual: quantity denominated in quote currency, settled in base
_XBTUSD_PERP_INVERSE = nautilus_pyo3.PerpetualContract(
    instrument_id=InstrumentId.from_str("XBTUSD-PERP.AX"),
    raw_symbol=Symbol("XBTUSD-PERP"),
    underlying="BTCUSD",
    asset_class=AssetClass.CRYPTOCURRENCY,
    quote_currency=Pyo3Currency.from_str("USD"),
    settlement_currency=Pyo3Currency.from_str("BTC"),
    is_inverse=True,
    price_precision=1,
    size_precision=0,
    price_increment=Pyo3Price.from_str("0.5"),
    size_increment=Pyo3Quantity.from_int(1),
    maker_fee=Decimal("-0.00025"),
    taker_fee=Decimal("0.00075"),
    margin_init=Decimal("0.01"),
    margin_maint=Decimal("0.005"),
    ts_event=0,
    ts_init=0,
    base_currency=Pyo3Currency.from_str("BTC"),
    max_quantity=Pyo3Quantity.from_int(10_000_000),
    min_quantity=Pyo3Quantity.from_int(1),
    max_price=Pyo3Price.from_str("1000000.0"),
    min_price=Pyo3Price.from_str("0.5"),
)

# Quanto perpetual: settled in a third currency (neither base nor quote)
_ETHUSD_PERP_QUANTO = nautilus_pyo3.PerpetualContract(
    instrument_id=InstrumentId.from_str("ETHUSD-PERP.AX"),
    raw_symbol=Symbol("ETHUSD-PERP"),
    underlying="ETHUSD",
    asset_class=AssetClass.CRYPTOCURRENCY,
    quote_currency=Pyo3Currency.from_str("USD"),
    settlement_currency=Pyo3Currency.from_str("BTC"),
    is_inverse=False,
    price_precision=2,
    size_precision=0,
    price_increment=Pyo3Price.from_str("0.05"),
    size_increment=Pyo3Quantity.from_int(1),
    maker_fee=Decimal(0),
    taker_fee=Decimal(0),
    margin_init=Decimal("0.01"),
    margin_maint=Decimal("0.005"),
    ts_event=0,
    ts_init=0,
    base_currency=Pyo3Currency.from_str("ETH"),
    max_quantity=Pyo3Quantity.from_int(10_000_000),
    min_quantity=Pyo3Quantity.from_int(1),
    max_price=Pyo3Price.from_str("1000000.00"),
    min_price=Pyo3Price.from_str("0.05"),
)


def test_equality():
    item_1 = TestInstrumentProviderPyo3.perpetual_contract_eurusd()
    item_2 = TestInstrumentProviderPyo3.perpetual_contract_eurusd()
    assert item_1 == item_2


def test_hash():
    assert hash(_EURUSD_PERP) == hash(_EURUSD_PERP)


def test_to_dict():
    result = _EURUSD_PERP.to_dict()
    assert nautilus_pyo3.PerpetualContract.from_dict(result) == _EURUSD_PERP
    assert result == {
        "type": "PerpetualContract",
        "id": "EURUSD-PERP.AX",
        "raw_symbol": "EURUSD-PERP",
        "underlying": "EURUSD",
        "asset_class": "FX",
        "base_currency": "EUR",
        "quote_currency": "USD",
        "settlement_currency": "USD",
        "is_inverse": False,
        "price_precision": 5,
        "size_precision": 0,
        "price_increment": "0.00001",
        "size_increment": "1",
        "multiplier": "1",
        "lot_size": "1",
        "max_quantity": None,
        "min_quantity": None,
        "max_notional": None,
        "min_notional": None,
        "max_price": None,
        "min_price": None,
        "margin_init": "0.03",
        "margin_maint": "0.03",
        "maker_fee": "0.00002",
        "taker_fee": "0.00002",
        "info": {},
        "ts_event": 0,
        "ts_init": 0,
    }


def test_pyo3_cython_conversion():
    pyo3_inst = TestInstrumentProviderPyo3.perpetual_contract_eurusd()
    pyo3_dict = pyo3_inst.to_dict()
    cython_inst = PerpetualContract.from_pyo3(pyo3_inst)
    cython_dict = PerpetualContract.to_dict(cython_inst)
    del cython_dict["tick_scheme_name"]  # TODO: Under development
    pyo3_back = nautilus_pyo3.PerpetualContract.from_dict(cython_dict)
    assert pyo3_inst == pyo3_back
    assert pyo3_dict == cython_dict


def test_get_base_currency_linear():
    linear = PerpetualContract.from_pyo3(_EURUSD_PERP)
    assert linear.get_base_currency() == Currency.from_str("EUR")


def test_get_base_currency_inverse():
    inverse = PerpetualContract.from_pyo3(_XBTUSD_PERP_INVERSE)
    assert inverse.get_base_currency() == Currency.from_str("BTC")


def test_get_settlement_currency():
    linear = PerpetualContract.from_pyo3(_EURUSD_PERP)
    assert linear.get_settlement_currency() == Currency.from_str("USD")


def test_get_cost_currency_linear():
    linear = PerpetualContract.from_pyo3(_EURUSD_PERP)
    assert linear.get_cost_currency() == Currency.from_str("USD")


def test_get_cost_currency_inverse():
    inverse = PerpetualContract.from_pyo3(_XBTUSD_PERP_INVERSE)
    assert inverse.get_cost_currency() == Currency.from_str("BTC")


def test_get_cost_currency_quanto():
    quanto = PerpetualContract.from_pyo3(_ETHUSD_PERP_QUANTO)
    assert quanto.get_cost_currency() == Currency.from_str("BTC")


def test_notional_value_linear():
    linear = PerpetualContract.from_pyo3(_EURUSD_PERP)
    quantity = Quantity.from_int(10)
    price = Price.from_str("1.10000")

    notional = linear.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("USD")
    assert notional.as_decimal() == Decimal("11.00000")


def test_notional_value_inverse():
    inverse = PerpetualContract.from_pyo3(_XBTUSD_PERP_INVERSE)
    quantity = Quantity.from_int(10_000)
    price = Price.from_str("50000.0")

    notional = inverse.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("BTC")
    assert notional.as_decimal() == Decimal("0.2")


def test_notional_value_inverse_with_quote_override():
    inverse = PerpetualContract.from_pyo3(_XBTUSD_PERP_INVERSE)
    quantity = Quantity.from_int(10_000)
    price = Price.from_str("50000.0")

    notional = inverse.notional_value(quantity, price, use_quote_for_inverse=True)

    assert notional.currency == Currency.from_str("USD")
    assert notional.as_decimal() == Decimal(10_000)


def test_notional_value_quanto():
    quanto = PerpetualContract.from_pyo3(_ETHUSD_PERP_QUANTO)
    quantity = Quantity.from_int(1000)
    price = Price.from_str("2500.00")

    notional = quanto.notional_value(quantity, price)

    assert notional.currency == Currency.from_str("BTC")
    expected = quantity.as_decimal() * quanto.multiplier.as_decimal() * price.as_decimal()
    assert notional.as_decimal() == expected


def test_inverse_without_base_currency_raises():
    with pytest.raises(ValueError, match="base_currency"):
        nautilus_pyo3.PerpetualContract(
            instrument_id=InstrumentId.from_str("TEST-PERP.AX"),
            raw_symbol=Symbol("TEST-PERP"),
            underlying="TEST",
            asset_class=AssetClass.ALTERNATIVE,
            quote_currency=Pyo3Currency.from_str("USD"),
            settlement_currency=Pyo3Currency.from_str("USD"),
            is_inverse=True,
            price_precision=2,
            size_precision=0,
            price_increment=Pyo3Price.from_str("0.01"),
            size_increment=Pyo3Quantity.from_int(1),
            maker_fee=Decimal(0),
            taker_fee=Decimal(0),
            margin_init=Decimal("0.01"),
            margin_maint=Decimal("0.005"),
            ts_event=0,
            ts_init=0,
        )
