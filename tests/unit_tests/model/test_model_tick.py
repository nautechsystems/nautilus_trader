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

from nautilus_trader.model.enums import Maker
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import MatchId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()


class TickTests(unittest.TestCase):

    def test_extract_price_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00000, 5),
            Price(1.00001, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        result1 = tick.extract_price(PriceType.ASK)
        result2 = tick.extract_price(PriceType.MID)
        result3 = tick.extract_price(PriceType.BID)

        # Assert
        self.assertEqual(Price(1.00001, 5), result1)
        self.assertEqual(Price(1.000005, 6), result2)
        self.assertEqual(Price(1.00000, 5), result3)

    def test_extract_volume_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00000, 5),
            Price(1.00001, 5),
            Quantity(500000),
            Quantity(800000),
            UNIX_EPOCH)

        # Act
        result1 = tick.extract_volume(PriceType.ASK)
        result2 = tick.extract_volume(PriceType.MID)
        result3 = tick.extract_volume(PriceType.BID)

        # Assert
        self.assertEqual(Quantity(800000), result1)
        self.assertEqual(Quantity(1300000), result2)
        self.assertEqual(Quantity(500000), result3)

    def test_can_parse_quote_tick_from_string(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00000, 5),
            Price(1.00001, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        result = QuoteTick.py_from_serializable_string(AUDUSD_FXCM, tick.to_serializable_string())

        # Assert
        self.assertEqual(tick, result)

    def test_can_parse_trade_tick_from_string(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_FXCM,
            Price(1.00000, 5),
            Quantity(10000),
            Maker.BUYER,
            MatchId("123456789"),
            UNIX_EPOCH)

        # Act
        result = TradeTick.py_from_serializable_string(AUDUSD_FXCM, tick.to_serializable_string())

        # Assert
        self.assertEqual(tick, result)

    def test_quote_tick_str_and_repr(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00000, 5),
            Price(1.00001, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        result0 = str(tick)
        result1 = repr(tick)

        # Assert
        self.assertEqual("AUD/USD.FXCM,1.00000,1.00001,1,1,1970-01-01T00:00:00.000Z", result0)
        self.assertTrue(result1.startswith("<QuoteTick(AUD/USD.FXCM,1.00000,1.00001,1,1,1970-01-01T00:00:00.000Z) object at"))
        self.assertTrue(result1.endswith(">"))

    def test_trade_tick_str_and_repr(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_FXCM,
            Price(1.00000, 5),
            Quantity(50000),
            Maker.BUYER,
            MatchId("123456789"),
            UNIX_EPOCH)

        # Act
        result0 = str(tick)
        result1 = repr(tick)

        # Assert
        self.assertEqual("AUD/USD.FXCM,1.00000,50000,BUYER,123456789,1970-01-01T00:00:00.000Z", result0)
        self.assertTrue(result1.startswith("<TradeTick(AUD/USD.FXCM,1.00000,50000,BUYER,123456789,1970-01-01T00:00:00.000Z) object at"))
        self.assertTrue(result1.endswith(">"))
