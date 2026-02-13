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

from nautilus_trader.backtest.models import FixedFeeModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoOption
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


@pytest.fixture
def instrument() -> Instrument:
    return TestInstrumentProvider.default_fx_ccy("EUR/USD")


@pytest.mark.parametrize("order_side", [OrderSide.BUY, OrderSide.SELL])
def test_fixed_commission_single_fill(instrument, order_side):
    # Arrange
    expected = Money(1, USD)
    fee_model = FixedFeeModel(expected)
    order = TestExecStubs.make_accepted_order(
        instrument=instrument,
        order_side=order_side,
    )

    # Act
    commission = fee_model.get_commission(
        order,
        instrument.make_qty(10),
        Price.from_str("1.1234"),
        instrument,
    )

    # Assert
    assert commission == expected


@pytest.mark.parametrize(
    ("order_side", "charge_commission_once", "expected_first_fill", "expected_next_fill"),
    [
        [OrderSide.BUY, True, Money(1, USD), Money(0, USD)],
        [OrderSide.SELL, True, Money(1, USD), Money(0, USD)],
        [OrderSide.BUY, False, Money(1, USD), Money(1, USD)],
        [OrderSide.SELL, False, Money(1, USD), Money(1, USD)],
    ],
)
def test_fixed_commission_multiple_fills(
    instrument,
    order_side,
    charge_commission_once,
    expected_first_fill,
    expected_next_fill,
):
    # Arrange
    fee_model = FixedFeeModel(
        commission=expected_first_fill,
        charge_commission_once=charge_commission_once,
    )
    order = TestExecStubs.make_accepted_order(
        instrument=instrument,
        order_side=order_side,
    )

    # Act
    commission_first_fill = fee_model.get_commission(
        order,
        instrument.make_qty(10),
        Price.from_str("1.1234"),
        instrument,
    )
    fill = TestEventStubs.order_filled(order=order, instrument=instrument)
    order.apply(fill)
    commission_next_fill = fee_model.get_commission(
        order,
        instrument.make_qty(10),
        Price.from_str("1.1234"),
        instrument,
    )

    # Assert
    assert commission_first_fill == expected_first_fill
    assert commission_next_fill == expected_next_fill


def test_instrument_percent_commission_maker(instrument):
    # Arrange
    fee_model = MakerTakerFeeModel()
    order = TestExecStubs.make_filled_order(
        instrument=instrument,
        order_side=OrderSide.SELL,
    )
    expected = order.quantity * order.price * instrument.maker_fee

    # Act
    commission = fee_model.get_commission(
        order,
        order.quantity,
        order.price,
        instrument,
    )

    # Assert
    assert isinstance(commission, Money)
    assert commission.as_decimal() == expected


def test_instrument_percent_commission_taker(instrument):
    # Arrange
    fee_model = MakerTakerFeeModel()
    order = TestExecStubs.make_filled_order(
        instrument=instrument,
        order_side=OrderSide.SELL,
    )
    expected = order.quantity * order.price * instrument.taker_fee

    # Act
    commission = fee_model.get_commission(
        order,
        order.quantity,
        order.price,
        instrument,
    )

    # Assert
    assert isinstance(commission, Money)
    assert commission.as_decimal() == expected


def test_maker_taker_fee_model_inverse_perpetual():
    # Arrange
    instrument = TestInstrumentProvider.xbtusd_bitmex()
    assert instrument.is_inverse

    fee_model = MakerTakerFeeModel()
    order = TestExecStubs.make_filled_order(
        instrument=instrument,
        order_side=OrderSide.SELL,
    )

    # Act
    commission = fee_model.get_commission(
        order,
        order.quantity,
        order.price,
        instrument,
    )

    # Assert
    assert isinstance(commission, Money)
    assert commission.currency == instrument.get_base_currency()


def test_maker_taker_fee_model_inverse_crypto_option():
    # Arrange
    instrument = CryptoOption(
        instrument_id=InstrumentId(
            symbol=Symbol("BTC-20FEB26-78000-P"),
            venue=Venue("DERIBIT"),
        ),
        raw_symbol=Symbol("BTC-20FEB26-78000-P"),
        underlying=BTC,
        quote_currency=USD,
        settlement_currency=BTC,
        is_inverse=True,
        option_kind=OptionKind.PUT,
        strike_price=Price.from_str("78000.00"),
        activation_ns=1671696002000000000,
        expiration_ns=1673596800000000000,
        price_precision=4,
        size_precision=1,
        price_increment=Price.from_str("0.0001"),
        size_increment=Quantity.from_str("0.1"),
        maker_fee=Decimal("0.0003"),
        taker_fee=Decimal("0.0003"),
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        max_quantity=Quantity.from_str("9000"),
        min_quantity=Quantity.from_str("0.1"),
        min_notional=None,
        ts_event=0,
        ts_init=0,
    )
    assert instrument.is_inverse

    fee_model = MakerTakerFeeModel()
    order = TestExecStubs.make_filled_order(
        instrument=instrument,
        order_side=OrderSide.SELL,
    )

    # Act
    commission = fee_model.get_commission(
        order,
        order.quantity,
        order.price,
        instrument,
    )

    # Assert
    assert isinstance(commission, Money)
    assert commission.currency == instrument.get_base_currency()
    assert commission.currency == BTC
