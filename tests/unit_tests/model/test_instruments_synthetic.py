# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    precision = 8
    components = [BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id]
    formula = "(BTCUSDT_BINANCE + ETHUSDT_BINANCE) / 2"

    # Act
    synthetic = SyntheticInstrument(
        symbol=symbol,
        precision=precision,
        components=components,
        formula=formula,
    )

    # Assert
    assert synthetic.precision == precision
    assert synthetic.id == InstrumentId.from_str("BTC-ETH.SYNTH")
    assert synthetic.formula == formula
    assert synthetic.components == components


def test_synthetic_instrument_calculate() -> None:
    # Arrange
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT_BINANCE + ETHUSDT_BINANCE) / 2",
    )

    # Act
    inputs = [100.0, 200.0]
    price = synthetic.calculate(inputs)

    # Assert
    assert isinstance(price, Price)
    assert price.precision == synthetic.precision
    assert price == 150.0


def test_synthetic_instrument_change_formula() -> None:
    # Arrange
    synthetic = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        precision=8,
        components=[BTCUSDT_BINANCE.id, ETHUSDT_BINANCE.id],
        formula="(BTCUSDT_BINANCE + ETHUSDT_BINANCE) / 2",
    )

    inputs = [100.0, 200.0]
    price1 = synthetic.calculate(inputs)

    # Act
    new_formula = "(BTCUSDT_BINANCE + ETHUSDT_BINANCE) / 4"
    synthetic.change_formula(new_formula)
    price2 = synthetic.calculate(inputs)

    # Assert
    assert price1.precision == synthetic.precision
    assert price2.precision == synthetic.precision
    assert price1 == 150.0
    assert price2 == 75.0
    assert synthetic.formula == new_formula
