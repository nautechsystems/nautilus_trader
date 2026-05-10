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

from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Symbol
from nautilus_trader.model import SyntheticInstrument
from tests.providers import TestInstrumentProvider


BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


def test_synthetic_instrument_init():
    symbol = Symbol("BTC-ETH")
    components = [BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id]
    formula = "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2"

    synthetic = SyntheticInstrument(
        symbol=symbol,
        price_precision=8,
        components=components,
        formula=formula,
        ts_event=0,
        ts_init=1,
    )

    assert synthetic.price_precision == 8
    assert synthetic.price_increment == Price.from_str("0.00000001")
    assert synthetic.id == InstrumentId.from_str("BTC-ETH.SYNTH")
    assert synthetic.formula == formula
    assert synthetic.components == components
    assert synthetic.ts_event == 0
    assert synthetic.ts_init == 1


def test_synthetic_instrument_equality():
    components = [BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id]
    formula = "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2"

    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=components,
        formula=formula,
        ts_event=0,
        ts_init=0,
    )

    assert synthetic == synthetic


def test_synthetic_instrument_invalid_formula():
    with pytest.raises(ValueError, match="Unexpected character"):
        SyntheticInstrument(
            symbol=Symbol("BTC-ETH"),
            price_precision=8,
            components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
            formula="z)(?,.",
            ts_event=0,
            ts_init=0,
        )


def test_synthetic_instrument_calculate():
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    price = synthetic.calculate([100.0, 200.0])

    assert isinstance(price, Price)
    assert price.precision == 8
    assert price == 150.0


@pytest.mark.parametrize(
    "inputs",
    [
        [],
        [100.0, float("nan")],
        [100.0],
        [100.0] * 3,
    ],
)
def test_synthetic_instrument_calculate_invalid_inputs(inputs):
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    with pytest.raises(ValueError, match="input"):
        synthetic.calculate(inputs)


def test_synthetic_instrument_calculate_from_map():
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    price = synthetic.calculate_from_map(
        {
            str(BTCUSDT_BINANCE.id): 100.0,
            str(ETHUSDT_BINANCE.id): 200.0,
        },
    )

    assert isinstance(price, Price)
    assert price == 150.0


def test_synthetic_instrument_is_valid_formula():
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    assert synthetic.is_valid_formula("(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 4") is True
    assert synthetic.is_valid_formula("z)(?,") is False


def test_synthetic_instrument_change_formula():
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )
    price1 = synthetic.calculate([100.0, 200.0])

    new_formula = "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 4"
    synthetic.change_formula(new_formula)
    price2 = synthetic.calculate([100.0, 200.0])

    assert price1 == 150.0
    assert price2 == 75.0
    assert synthetic.formula == new_formula


def test_synthetic_instrument_change_formula_invalid():
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    with pytest.raises(ValueError, match="Unexpected character"):
        synthetic.change_formula("z)(?,.")
