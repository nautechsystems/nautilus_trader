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

import unittest

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Decimal
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from tests.test_kit.stubs import UNIX_EPOCH


class InstrumentTests(unittest.TestCase):

    def test_calculate_pnl_given_position_side_undefined_or_flat_raises_value_error(self):
        # Arrange
        instrument = InstrumentLoader.btcusdt_binance()

        # Act
        # Assert
        self.assertRaises(
            ValueError,
            instrument.calculate_pnl,
            PositionSide.UNDEFINED,
            Quantity(10),
            Price("10500.00"),
            Price("10500.00"),
        )

        self.assertRaises(
            ValueError,
            instrument.calculate_pnl,
            PositionSide.FLAT,
            Quantity(10),
            Price("10500.00"),
            Price("10500.00"),
        )

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
        pnl = instrument.calculate_order_margin(
            Quantity(100000),
            Price("11493.60"),
        )

        # Assert
        self.assertEqual(Money(0.01392079, BTC), pnl)

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
        pnl = instrument.calculate_open_value(
            PositionSide.LONG,
            Quantity(10),
            last
        )

        # Assert
        self.assertEqual(Money(10.00000000, BTC), pnl)

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
        pnl = instrument.calculate_open_value(
            PositionSide.LONG,
            Quantity(100000),
            last
        )

        # Assert
        self.assertEqual(Money(8.70049419, BTC), pnl)

    def test_calculate_pnl_for_long_position_win(self):
        # Arrange
        instrument = InstrumentLoader.btcusdt_binance()

        # Act
        pnl = instrument.calculate_pnl(
            PositionSide.LONG,
            Quantity(10),
            Price("10500.00"),
            Price("10510.00"),
        )

        # Assert
        self.assertEqual(Money(0.00952381, BTC), pnl)

    def test_calculate_pnl_for_long_position_loss(self):
        # Arrange
        instrument = InstrumentLoader.btcusdt_binance()

        # Act
        pnl = instrument.calculate_pnl(
            PositionSide.LONG,
            Quantity(10),
            Price("10500.00"),
            Price("10480.50"),
        )

        # Assert
        self.assertEqual(Money(-0.01857143, BTC), pnl)

    def test_calculate_pnl_for_short_position_win(self):
        # Arrange
        instrument = InstrumentLoader.btcusdt_binance()

        # Act
        pnl = instrument.calculate_pnl(
            PositionSide.SHORT,
            Quantity(10),
            Price("10500.00"),
            Price("10390.00"),
        )

        # Assert
        self.assertEqual(Money(0.10476190, BTC), pnl)

    def test_calculate_pnl_for_short_position_loss(self):
        # Arrange
        instrument = InstrumentLoader.btcusdt_binance()

        # Act
        pnl = instrument.calculate_pnl(
            PositionSide.SHORT,
            Quantity(10),
            Price("10500.00"),
            Price("10670.50"),
        )

        # Assert
        self.assertEqual(Money(-0.16238095, BTC), pnl)

    def test_calculate_pnl_for_inverse1(self):
        # Arrange
        instrument = InstrumentLoader.xbtusd_bitmex()

        # Act
        pnl = instrument.calculate_pnl(
            PositionSide.SHORT,
            Quantity(100000),
            Price("10500.00"),
            Price("10670.50"),
        )

        # Assert
        self.assertEqual(Money(-0.15217745, BTC), pnl)

    def test_calculate_pnl_for_inverse2(self):
        # Arrange
        instrument = InstrumentLoader.ethusd_bitmex()

        # Act
        pnl = instrument.calculate_pnl(
            PositionSide.SHORT,
            Quantity(100000),
            Price("375.95"),
            Price("365.50"),
        )

        # Assert
        self.assertEqual(Money(7.60499302, ETH), pnl)

    def test_calculate_unrealized_pnl_for_long(self):
        # Arrange
        instrument = InstrumentLoader.btcusdt_binance()

        last = QuoteTick(
            instrument.symbol,
            Price("10505.60"),
            Price("10506.65"),
            Quantity(20),
            Quantity(20),
            UNIX_EPOCH,
        )
        # Act
        pnl = instrument.calculate_unrealized_pnl(
            PositionSide.LONG,
            Quantity(10),
            Price("10500.00"),
            last,
        )

        # Assert
        self.assertEqual(Money(0.00533333, BTC), pnl)

    def test_calculate_unrealized_pnl_for_short(self):
        # Arrange
        instrument = InstrumentLoader.btcusdt_binance()

        last = QuoteTick(
            instrument.symbol,
            Price("10505.60"),
            Price("10506.65"),
            Quantity(20),
            Quantity(20),
            UNIX_EPOCH,
        )
        # Act
        pnl = instrument.calculate_unrealized_pnl(
            PositionSide.SHORT,
            Quantity(5),
            Price("10500.00"),
            last,
        )

        # Assert
        self.assertEqual(Money(-0.00316667, BTC), pnl)

    def test_calculate_unrealized_pnl_for_long_inverse(self):
        # Arrange
        instrument = InstrumentLoader.xbtusd_bitmex()

        last = QuoteTick(
            instrument.symbol,
            Price("10505.60"),
            Price("10506.65"),
            Quantity(25000),
            Quantity(25000),
            UNIX_EPOCH,
        )
        # Act
        pnl = instrument.calculate_unrealized_pnl(
            PositionSide.LONG,
            Quantity(200000),
            Price("10500.00"),
            last,
        )

        # Assert
        self.assertEqual(Money(0.01015332, BTC), pnl)

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
        self.assertEqual(Money(30.00, AUD), commission)

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
        self.assertEqual(Money(44.00, USD), commission)
