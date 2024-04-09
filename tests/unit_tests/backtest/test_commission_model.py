# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

# fmt: off
from nautilus_trader.backtest.models import FixedCommissionModel
from nautilus_trader.backtest.models import InstrumentSpecificPercentCommissionModel
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders import Order
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


# fmt: on


@pytest.fixture()
def instrument() -> Instrument:
    return TestInstrumentProvider.default_fx_ccy("EUR/USD")


@pytest.fixture()
def buy_order(instrument: Instrument) -> Order:
    return TestExecStubs.make_filled_order(
        instrument=instrument,
        order_side=OrderSide.BUY,
    )


@pytest.fixture()
def sell_order(instrument: Instrument) -> Order:
    return TestExecStubs.make_filled_order(
        instrument=instrument,
        order_side=OrderSide.SELL,
    )


def test_fixed_commission(buy_order, instrument):
    # Arrange
    expected = Money(1, USD)
    commission_model = FixedCommissionModel(expected)

    # Act
    commission = commission_model.get_commission(
        buy_order,
        buy_order.quantity,
        Price.from_str("1.1234"),
        instrument,
    )

    # Assert
    assert commission == expected


def test_instrument_percent_commission_maker(instrument, buy_order):
    # Arrange
    commission_model = InstrumentSpecificPercentCommissionModel()
    expected = buy_order.quantity * buy_order.price * instrument.maker_fee

    # Act
    commission = commission_model.get_commission(
        buy_order,
        buy_order.quantity,
        buy_order.price,
        instrument,
    )

    # Assert
    assert isinstance(commission, Money)
    assert commission.as_decimal() == expected


def test_instrument_percent_commission_taker(instrument, sell_order):
    # Arrange
    commission_model = InstrumentSpecificPercentCommissionModel()
    expected = sell_order.quantity * sell_order.price * instrument.taker_fee

    # Act
    commission = commission_model.get_commission(
        sell_order,
        sell_order.quantity,
        sell_order.price,
        instrument,
    )

    # Assert
    assert isinstance(commission, Money)
    assert commission.as_decimal() == expected
