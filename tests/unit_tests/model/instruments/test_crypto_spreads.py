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

import pytest

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoFuturesSpread
from nautilus_trader.model.instruments import CryptoOptionSpread
from nautilus_trader.model.instruments import instruments_from_pyo3
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_crypto_futures_spread_inverse_notional_returns_underlying():
    # Arrange: BTC inverse futures spread, contract size 10 USD/contract
    instrument = TestInstrumentProvider.crypto_futures_spread_inverse()
    quantity = Quantity.from_int(100)
    price = Price.from_str("10.0")

    # Act
    notional = instrument.notional_value(quantity, price)

    # Assert: inverse notional = quantity * multiplier / price, in BTC
    # 100 * 10 / 10.0 = 100 BTC
    assert notional == Money(100.0, BTC)


def test_crypto_futures_spread_inverse_notional_use_quote_for_inverse():
    instrument = TestInstrumentProvider.crypto_futures_spread_inverse()

    notional = instrument.notional_value(
        Quantity.from_int(100),
        Price.from_str("10.0"),
        use_quote_for_inverse=True,
    )

    # When use_quote_for_inverse=True, returns quantity as USD directly
    assert notional == Money(100.0, USD)


def test_crypto_futures_spread_linear_notional_returns_quote():
    # Linear spread: underlying ETH, quote USDT, settles in USDT
    instrument = CryptoFuturesSpread(
        instrument_id=InstrumentId(
            symbol=Symbol("ETH-FS-LINEAR"),
            venue=Venue("DERIBIT"),
        ),
        raw_symbol=Symbol("ETH-FS-LINEAR"),
        underlying=ETH,
        quote_currency=USDT,
        settlement_currency=USDT,
        is_inverse=False,
        strategy_type="FS",
        activation_ns=0,
        expiration_ns=0,
        price_precision=2,
        size_precision=0,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )

    notional = instrument.notional_value(
        Quantity.from_int(2),
        Price.from_str("1500.00"),
    )

    # Linear notional = quantity * multiplier * price, in quote_currency
    assert notional == Money(3000.0, USDT)


def test_crypto_futures_spread_quanto_notional_returns_settlement():
    # Quanto: settlement currency != quote AND != underlying
    instrument = CryptoFuturesSpread(
        instrument_id=InstrumentId(
            symbol=Symbol("ETH-FS-QUANTO"),
            venue=Venue("DERIBIT"),
        ),
        raw_symbol=Symbol("ETH-FS-QUANTO"),
        underlying=ETH,
        quote_currency=USD,
        settlement_currency=BTC,  # neither quote nor underlying -> quanto
        is_inverse=False,
        strategy_type="FS",
        activation_ns=0,
        expiration_ns=0,
        price_precision=2,
        size_precision=0,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )

    assert instrument.is_quanto is True

    notional = instrument.notional_value(
        Quantity.from_int(2),
        Price.from_str("1500.00"),
    )

    # Quanto notional = quantity * multiplier * price, in settlement_currency
    assert notional == Money(3000.0, BTC)


def test_crypto_option_spread_inverse_notional_returns_underlying():
    # Deribit BTC option combo: inverse, multiplier 1, settles in BTC
    instrument = TestInstrumentProvider.crypto_option_spread_inverse()

    notional = instrument.notional_value(
        Quantity.from_str("1.0"),
        Price.from_str("0.0500"),
    )

    # Inverse notional = quantity * multiplier / price, in underlying (BTC)
    # 1.0 * 1 / 0.05 = 20 BTC
    assert notional == Money(20.0, BTC)


def test_crypto_option_spread_linear_notional_returns_quote():
    instrument = CryptoOptionSpread(
        instrument_id=InstrumentId(
            symbol=Symbol("ETH-CS-LINEAR"),
            venue=Venue("DERIBIT"),
        ),
        raw_symbol=Symbol("ETH-CS-LINEAR"),
        underlying=ETH,
        quote_currency=USDT,
        settlement_currency=USDT,
        is_inverse=False,
        strategy_type="CS",
        activation_ns=0,
        expiration_ns=0,
        price_precision=2,
        size_precision=0,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )

    notional = instrument.notional_value(
        Quantity.from_int(3),
        Price.from_str("100.00"),
    )

    assert notional == Money(300.0, USDT)


@pytest.mark.parametrize(
    ("instrument", "expected"),
    [
        (TestInstrumentProvider.crypto_futures_spread_inverse(), BTC),  # inverse -> underlying
        (TestInstrumentProvider.crypto_option_spread_inverse(), BTC),  # inverse -> underlying
    ],
)
def test_crypto_spread_get_cost_currency_inverse(instrument, expected):
    assert instrument.get_cost_currency() == expected


def test_crypto_futures_spread_get_cost_currency_quanto():
    # Quanto: cost currency = settlement_currency
    instrument = CryptoFuturesSpread(
        instrument_id=InstrumentId(
            symbol=Symbol("ETH-FS-QUANTO"),
            venue=Venue("DERIBIT"),
        ),
        raw_symbol=Symbol("ETH-FS-QUANTO"),
        underlying=ETH,
        quote_currency=USD,
        settlement_currency=BTC,
        is_inverse=False,
        strategy_type="FS",
        activation_ns=0,
        expiration_ns=0,
        price_precision=2,
        size_precision=0,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    assert instrument.get_cost_currency() == BTC


def test_crypto_futures_spread_is_quanto_linear_is_false():
    instrument = TestInstrumentProvider.crypto_futures_spread_inverse()
    # Inverse Deribit futures spread: settlement=BTC=underlying, not quanto
    assert instrument.is_quanto is False


def test_crypto_futures_spread_from_pyo3_round_trip_preserves_fractional():
    p_inst = nautilus_pyo3.CryptoFuturesSpread(
        instrument_id=nautilus_pyo3.InstrumentId.from_str("BTC-FS-19MAY26_PERP.DERIBIT"),
        raw_symbol=nautilus_pyo3.Symbol("BTC-FS-19MAY26_PERP"),
        underlying=nautilus_pyo3.Currency.from_str("BTC"),
        quote_currency=nautilus_pyo3.Currency.from_str("USD"),
        settlement_currency=nautilus_pyo3.Currency.from_str("BTC"),
        is_inverse=True,
        strategy_type="FS",
        activation_ns=0,
        expiration_ns=1_000_000_000,
        price_precision=1,
        size_precision=0,
        price_increment=nautilus_pyo3.Price.from_str("0.5"),
        size_increment=nautilus_pyo3.Quantity.from_str("1"),
        ts_event=0,
        ts_init=0,
    )

    [cython_inst] = instruments_from_pyo3([p_inst])

    assert isinstance(cython_inst, CryptoFuturesSpread)
    assert cython_inst.id == InstrumentId.from_str("BTC-FS-19MAY26_PERP.DERIBIT")
    assert cython_inst.is_inverse is True
    assert cython_inst.strategy_type == "FS"
    assert cython_inst.underlying == BTC
    assert cython_inst.settlement_currency == BTC
    assert cython_inst.size_precision == 0
    assert cython_inst.size_increment == Quantity.from_int(1)


def test_crypto_option_spread_from_pyo3_round_trip_preserves_fractional():
    # Deribit option combos with min_trade_amount=0.1 must survive the pyo3
    # to Cython boundary without size_precision collapsing back to 0
    p_inst = nautilus_pyo3.CryptoOptionSpread(
        instrument_id=nautilus_pyo3.InstrumentId.from_str(
            "BTC-CS-19MAY26-70000_75000.DERIBIT",
        ),
        raw_symbol=nautilus_pyo3.Symbol("BTC-CS-19MAY26-70000_75000"),
        underlying=nautilus_pyo3.Currency.from_str("BTC"),
        quote_currency=nautilus_pyo3.Currency.from_str("BTC"),
        settlement_currency=nautilus_pyo3.Currency.from_str("BTC"),
        is_inverse=True,
        strategy_type="CS",
        activation_ns=0,
        expiration_ns=1_000_000_000,
        price_precision=4,
        size_precision=1,
        price_increment=nautilus_pyo3.Price.from_str("0.0001"),
        size_increment=nautilus_pyo3.Quantity.from_str("0.1"),
        ts_event=0,
        ts_init=0,
    )

    [cython_inst] = instruments_from_pyo3([p_inst])

    assert isinstance(cython_inst, CryptoOptionSpread)
    assert cython_inst.is_inverse is True
    assert cython_inst.strategy_type == "CS"
    assert cython_inst.size_precision == 1
    assert cython_inst.size_increment == Quantity.from_str("0.1")


def test_crypto_futures_spread_dict_round_trip():
    original = TestInstrumentProvider.crypto_futures_spread_inverse()
    restored = CryptoFuturesSpread.from_dict(CryptoFuturesSpread.to_dict(original))
    assert restored == original
    assert restored.is_inverse is True
    assert restored.strategy_type == "FS"
    assert restored.size_precision == 0


def test_crypto_option_spread_dict_round_trip_preserves_fractional_lot_size():
    original = TestInstrumentProvider.crypto_option_spread_inverse()
    restored = CryptoOptionSpread.from_dict(CryptoOptionSpread.to_dict(original))
    assert restored == original
    assert restored.is_inverse is True
    assert restored.strategy_type == "CS"
    assert restored.size_precision == 1
    assert restored.size_increment == Quantity.from_str("0.1")
    assert restored.lot_size == Quantity.from_str("0.1")
