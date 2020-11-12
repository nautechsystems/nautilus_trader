# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
import unittest

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from tests.test_kit.stubs import UNIX_EPOCH


class InstrumentTests(unittest.TestCase):

    def test_calculate_order_margin_with_no_leverage_returns_zero(self):
        # Arrange
        instrument = InstrumentLoader.xbtusd_bitmex()

        # Act
        margin = instrument.calculate_order_margin(
            Quantity(100000),
            Price("11493.60"),
        )

        # Assert
        self.assertEqual(Money(0.00000000, BTC), margin)

    def test_calculate_order_margin_with_100x_leverage_returns_expected(self):
        # Arrange
        instrument = InstrumentLoader.xbtusd_bitmex(leverage=Decimal(100))

        # Act
        margin = instrument.calculate_order_margin(
            Quantity(100000),
            Price("11493.60"),
        )

        # Assert
        self.assertEqual(Money(0.01392079, BTC), margin)

    def test_calculate_position_margin_with_no_leverage_returns_zero(self):
        # Arrange
        instrument = InstrumentLoader.xbtusd_bitmex()

        last = QuoteTick(
            instrument.symbol,
            Price("11493.60"),
            Price("11493.65"),
            Quantity("19.3"),
            Quantity("1.43"),
            UNIX_EPOCH,
        )

        # Act
        margin = instrument.calculate_position_margin(
            PositionSide.LONG,
            Quantity(100000),
            last,
        )

        # Assert
        self.assertEqual(Money(0.00000000, BTC), margin)

    def test_calculate_position_margin_with_100x_leverage_returns_expected(self):
        # Arrange
        instrument = InstrumentLoader.xbtusd_bitmex(leverage=Decimal(100))

        last = QuoteTick(
            instrument.symbol,
            Price("11493.60"),
            Price("11493.65"),
            Quantity("19.3"),
            Quantity("1.43"),
            UNIX_EPOCH,
        )

        # Act
        margin = instrument.calculate_position_margin(
            PositionSide.LONG,
            Quantity(100000),
            last,
        )

        # Assert
        self.assertEqual(Money(0.00682989, BTC), margin)

    def test_calculate_open_value(self):
        # Arrange
        instrument = InstrumentLoader.btcusdt_binance()

        last = QuoteTick(
            instrument.symbol,
            Price("11493.60"),
            Price("11493.65"),
            Quantity("19.3"),
            Quantity("1.43"),
            UNIX_EPOCH,
        )

        # Act
        value = instrument.calculate_open_value(
            PositionSide.LONG,
            Quantity(10),
            last
        )

        # Assert
        self.assertEqual(Money(114936.00000000, USDT), value)

    def test_calculate_open_value_for_inverse(self):
        # Arrange
        instrument = InstrumentLoader.xbtusd_bitmex()

        last = QuoteTick(
            instrument.symbol,
            Price("11493.60"),
            Price("11493.65"),
            Quantity(55000),
            Quantity(12500),
            UNIX_EPOCH,
        )

        # Act
        value = instrument.calculate_open_value(
            PositionSide.LONG,
            Quantity(100000),
            last
        )

        # Assert
        self.assertEqual(Money(8.70049419, BTC), value)

    def test_calculate_commission_for_maker_crypto(self):
        # Arrange
        instrument = InstrumentLoader.xbtusd_bitmex()

        # Act
        commission = instrument.calculate_commission(
            Quantity(100000),
            Price("11450.50"),
            LiquiditySide.MAKER,
        )

        # Assert
        self.assertEqual(Money(-0.00218331, BTC), commission)

    def test_calculate_commission_for_taker_fx(self):
        # Arrange
        instrument = InstrumentLoader.default_fx_ccy(Symbol("AUD/USD", Venue("IDEALPRO")))

        # Act
        commission = instrument.calculate_commission(
            Quantity(1500000),
            Price("0.80050"),
            LiquiditySide.TAKER,
        )

        # Assert
        self.assertEqual(Money(24.02, USD), commission)

    def test_calculate_commission_crypto_taker(self):
        # Arrange
        instrument = InstrumentLoader.xbtusd_bitmex()

        # Act
        commission = instrument.calculate_commission(
            Quantity(100000),
            Price("11450.50"),
            LiquiditySide.TAKER,
        )

        # Assert
        self.assertEqual(Money(0.00654993, BTC), commission)

    def test_calculate_commission_fx_taker(self):
        # Arrange
        instrument = InstrumentLoader.default_fx_ccy(Symbol("USD/JPY", Venue("IDEALPRO")))

        # Act
        commission = instrument.calculate_commission(
            Quantity(2200000),
            Price("120.310"),
            LiquiditySide.TAKER,
        )

        # Assert
        self.assertEqual(Money(5293.64, JPY), commission)
