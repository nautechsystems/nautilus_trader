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

from datetime import timedelta
import unittest

from nautilus_trader.model.enums import Maker
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd_fxcm())


class QuoteTickTests(unittest.TestCase):

    def test_equality_and_comparisons(self):
        # Arrange
        # These are based on timestamp for tick sorting
        tick1 = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH + timedelta(seconds=1),
        )

        tick2 = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH + timedelta(seconds=2),
        )

        tick3 = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH + timedelta(seconds=3),
        )

        self.assertTrue(tick1 == tick1)
        self.assertTrue(tick1 != tick2)
        self.assertTrue(tick1 <= tick1)
        self.assertTrue(tick1 <= tick2)
        self.assertTrue(tick1 < tick2)
        self.assertTrue(tick3 > tick2)
        self.assertTrue(tick3 >= tick2)
        self.assertTrue(tick3 >= tick3)
        self.assertEqual([tick1, tick2, tick3], sorted([tick2, tick3, tick1]))

    def test_tick_str_and_repr(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        result0 = str(tick)
        result1 = repr(tick)

        # Assert
        self.assertEqual("AUD/USD.SIM,1.00000,1.00001,1,1,1970-01-01T00:00:00.000Z", result0)
        self.assertEqual("QuoteTick(AUD/USD.SIM,1.00000,1.00001,1,1,1970-01-01T00:00:00.000Z)", result1)

    def test_extract_price_with_invalid_price_raises_value_error(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertRaises(ValueError, tick.extract_price, PriceType.UNDEFINED)

    def test_extract_price_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        result1 = tick.extract_price(PriceType.ASK)
        result2 = tick.extract_price(PriceType.MID)
        result3 = tick.extract_price(PriceType.BID)

        # Assert
        self.assertEqual(Price("1.00001"), result1)
        self.assertEqual(Price("1.000005"), result2)
        self.assertEqual(Price("1.00000"), result3)

    def test_extract_volume_with_invalid_price_raises_value_error(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertRaises(ValueError, tick.extract_volume, PriceType.UNDEFINED)

    def test_extract_volume_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(500000),
            Quantity(800000),
            UNIX_EPOCH,
        )

        # Act
        result1 = tick.extract_volume(PriceType.ASK)
        result2 = tick.extract_volume(PriceType.MID)
        result3 = tick.extract_volume(PriceType.BID)

        # Assert
        self.assertEqual(Quantity(800000), result1)
        self.assertEqual(Quantity(1300000), result2)
        self.assertEqual(Quantity(500000), result3)

    def test_from_serializable_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            ValueError,
            QuoteTick.from_serializable_string,
            AUDUSD_SIM.symbol,
            "NOT_A_TICK",
        )

    def test_from_serializable_string_given_valid_string_returns_expected_tick(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        result = QuoteTick.from_serializable_string(AUDUSD_SIM.symbol, tick.to_serializable_string())

        # Assert
        self.assertEqual(tick, result)

    def test_to_serializable_returns_expected_string(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        result = tick.to_serializable_string()

        # Assert
        self.assertEqual("1.00000,1.00001,1,1,0", result)


class TradeTickTests(unittest.TestCase):

    def test_equality_and_comparisons(self):
        # Arrange
        # These are based on timestamp for tick sorting
        tick1 = TradeTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Quantity(50000),
            Maker.BUYER,
            TradeMatchId("123456789"),
            UNIX_EPOCH + timedelta(seconds=1),
        )

        tick2 = TradeTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Quantity(50000),
            Maker.BUYER,
            TradeMatchId("123456789"),
            UNIX_EPOCH + timedelta(seconds=2),
        )

        tick3 = TradeTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Quantity(50000),
            Maker.BUYER,
            TradeMatchId("123456789"),
            UNIX_EPOCH + timedelta(seconds=3),
        )

        self.assertTrue(tick1 == tick1)
        self.assertTrue(tick1 != tick2)
        self.assertTrue(tick1 <= tick1)
        self.assertTrue(tick1 <= tick2)
        self.assertTrue(tick1 < tick2)
        self.assertTrue(tick3 > tick2)
        self.assertTrue(tick3 >= tick2)
        self.assertTrue(tick3 >= tick3)
        self.assertEqual([tick1, tick2, tick3], sorted([tick2, tick3, tick1]))

    def test_str_and_repr(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Quantity(50000),
            Maker.BUYER,
            TradeMatchId("123456789"),
            UNIX_EPOCH,
        )

        # Act
        result0 = str(tick)
        result1 = repr(tick)

        # Assert
        self.assertEqual("AUD/USD.SIM,1.00000,50000,BUYER,123456789,1970-01-01T00:00:00.000Z", result0)
        self.assertEqual("TradeTick(AUD/USD.SIM,1.00000,50000,BUYER,123456789,1970-01-01T00:00:00.000Z)", result1)

    def test_from_serializable_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            ValueError,
            TradeTick.from_serializable_string,
            AUDUSD_SIM.symbol,
            "NOT_A_TICK",
        )

    def test_from_serializable_string_given_valid_string_returns_expected_tick(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Quantity(10000),
            Maker.BUYER,
            TradeMatchId("123456789"),
            UNIX_EPOCH,
        )

        # Act
        result = TradeTick.from_serializable_string(AUDUSD_SIM.symbol, tick.to_serializable_string())

        # Assert
        self.assertEqual(tick, result)

    def test_to_serializable_returns_expected_string(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.symbol,
            Price("1.00000"),
            Quantity(10000),
            Maker.BUYER,
            TradeMatchId("123456789"),
            UNIX_EPOCH,
        )

        # Act
        result = tick.to_serializable_string()

        # Assert
        self.assertEqual("1.00000,10000,BUYER,123456789,0", result)
