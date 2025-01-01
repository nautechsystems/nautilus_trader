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

import pytest
from pandas.core.generic import pickle

from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments.synthetic import SyntheticInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.test_kit.providers import TestInstrumentProvider


BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


def test_synthetic_instrument_initialization() -> None:
    # Arrange
    symbol = Symbol("BTC-ETH")
    price_precision = 8
    components = [BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id]
    formula = "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2"

    # Act
    synthetic = SyntheticInstrument(
        symbol=symbol,
        price_precision=price_precision,
        components=components,
        formula=formula,
        ts_event=0,
        ts_init=1,
    )

    # Assert
    assert synthetic.price_precision == price_precision
    assert synthetic.price_increment == Price.from_str("0.00000001")
    assert synthetic.id == InstrumentId.from_str("BTC-ETH.SYNTH")
    assert synthetic.formula == formula
    assert synthetic.components == components
    assert synthetic == synthetic
    assert isinstance(hash(synthetic), int)
    assert synthetic.ts_event == 0
    assert synthetic.ts_init == 1


def test_synthetic_instrument_with_invalid_formula() -> None:
    # Arrange, Act, Assert
    with pytest.raises(ValueError):
        SyntheticInstrument(
            symbol=Symbol("BTC-ETH"),
            price_precision=8,
            components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
            formula="z)(?,.",  # <-- Invalid
            ts_event=0,
            ts_init=0,
        )


@pytest.mark.parametrize(
    ("inputs"),
    [
        [],
        [100.0, float("nan")],
        [100.0],
        [100.0] * 3,
    ],
)
def test_synthetic_instrument_calculate_with_invalid_inputs(
    inputs: list[float],
) -> None:
    # Arrange
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    # Act, Assert
    with pytest.raises(ValueError):
        synthetic.calculate(inputs)


def test_synthetic_instrument_calculate() -> None:
    # Arrange
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    # Act
    inputs = [100.0, 200.0]
    price = synthetic.calculate(inputs)

    # Assert
    assert isinstance(price, Price)
    assert price.precision == synthetic.price_precision
    assert price == 150.0


def test_synthetic_instrument_change_formula_with_invalid_formula() -> None:
    # Arrange
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    inputs = [100.0, 200.0]
    synthetic.calculate(inputs)

    # Act, Assert
    new_formula = "z)(?,."  # <-- Invalid fromula
    with pytest.raises(ValueError):
        synthetic.change_formula(new_formula)


def test_synthetic_instrument_change_formula() -> None:
    # Arrange
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    inputs = [100.0, 200.0]
    price1 = synthetic.calculate(inputs)

    # Act
    new_formula = "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 4"
    synthetic.change_formula(new_formula)
    price2 = synthetic.calculate(inputs)

    # Assert
    assert price1.precision == synthetic.price_precision
    assert price2.precision == synthetic.price_precision
    assert price1 == 150.0
    assert price2 == 75.0
    assert synthetic.formula == new_formula


def test_synthetic_instrument_to_dict():
    # Arrange
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    # Act
    result = SyntheticInstrument.to_dict(synthetic)

    # Assert
    assert result == {
        "type": "SyntheticInstrument",
        "symbol": "BTC-ETH",
        "price_precision": 8,
        "components": ["BTCUSDT.BINANCE", "ETHUSDT.BINANCE"],
        "formula": "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        "ts_event": 0,
        "ts_init": 0,
    }


def test_synthetic_instrument_from_dict():
    # Arrange
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    # Act
    result = SyntheticInstrument.from_dict(SyntheticInstrument.to_dict(synthetic))

    # Assert
    assert result == synthetic


@pytest.mark.skip(reason="Currently not possible to pickle synthetic instruments")
def test_pickling_round_trip_results_in_expected_tick():
    # Arrange
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    # Act
    pickled = pickle.dumps(synthetic)
    unpickled = pickle.loads(pickled)

    # Assert
    assert synthetic == unpickled
