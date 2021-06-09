# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from tests.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class QuoteTickTests(unittest.TestCase):
    def test_equality_and_comparisons(self):
        # Arrange
        # These are based on timestamp for tick sorting
        tick1 = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            ts_event_ns=1,
            ts_recv_ns=1,
        )

        tick2 = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            ts_event_ns=2,
            ts_recv_ns=2,
        )

        tick3 = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            ts_event_ns=3,
            ts_recv_ns=3,
        )

        self.assertTrue(tick1 == tick1)
        self.assertTrue(tick1 != tick2)
        self.assertEqual(
            [tick1, tick2, tick3],
            sorted([tick2, tick3, tick1], key=lambda x: x.ts_recv_ns),
        )

    def test_tick_str_and_repr(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        result0 = str(tick)
        result1 = repr(tick)

        # Assert
        self.assertEqual("AUD/USD.SIM,1.00000,1.00001,1,1,0", result0)
        self.assertEqual(
            "QuoteTick(AUD/USD.SIM,1.00000,1.00001,1,1,0)",
            result1,
        )

    def test_extract_price_with_invalid_price_raises_value_error(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        # Assert
        self.assertRaises(ValueError, tick.extract_price, 0)

    def test_extract_price_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        result1 = tick.extract_price(PriceType.ASK)
        result2 = tick.extract_price(PriceType.MID)
        result3 = tick.extract_price(PriceType.BID)

        # Assert
        self.assertEqual(Price.from_str("1.00001"), result1)
        self.assertEqual(Price.from_str("1.000005"), result2)
        self.assertEqual(Price.from_str("1.00000"), result3)

    def test_extract_volume_with_invalid_price_raises_value_error(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        # Assert
        self.assertRaises(ValueError, tick.extract_volume, 0)

    def test_extract_volume_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(500000),
            Quantity.from_int(800000),
            0,
            0,
        )

        # Act
        result1 = tick.extract_volume(PriceType.ASK)
        result2 = tick.extract_volume(PriceType.MID)
        result3 = tick.extract_volume(PriceType.BID)

        # Assert
        self.assertEqual(Quantity.from_int(800000), result1)
        self.assertEqual(Quantity.from_int(650000), result2)  # Average size
        self.assertEqual(Quantity.from_int(500000), result3)

    def test_from_serializable_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            ValueError,
            QuoteTick.from_serializable_str,
            AUDUSD_SIM.id,
            "NOT_A_TICK",
        )

    def test_from_serializable_string_given_valid_string_returns_expected_tick(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        result = QuoteTick.from_serializable_str(
            AUDUSD_SIM.id, tick.to_serializable_str()
        )

        # Assert
        self.assertEqual(tick, result)

    def test_to_serializable_returns_expected_string(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        result = tick.to_serializable_str()

        # Assert
        self.assertEqual("1.00000,1.00001,1,1,0,0", result)


class TradeTickTests(unittest.TestCase):
    def test_equality_and_comparisons(self):
        # Arrange
        # These are based on timestamp for tick sorting
        tick1 = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(50000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            1000,
        )

        tick2 = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(50000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            1000,
            2000,
        )

        tick3 = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(50000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            2000,
            3000,
        )

        self.assertTrue(tick1 == tick1)
        self.assertEqual(
            [tick1, tick2, tick3],
            sorted([tick2, tick3, tick1], key=lambda x: x.ts_recv_ns),
        )

    def test_str_and_repr(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(50000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        # Act
        result0 = str(tick)
        result1 = repr(tick)

        # Assert
        self.assertEqual("AUD/USD.SIM,1.00000,50000,BUY,123456789,0", result0)
        self.assertEqual(
            "TradeTick(AUD/USD.SIM,1.00000,50000,BUY,123456789,0)",
            result1,
        )

    def test_from_serializable_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(
            ValueError,
            TradeTick.from_serializable_str,
            AUDUSD_SIM.id,
            "NOT_A_TICK",
        )

    def test_from_serializable_string_given_valid_string_returns_expected_tick(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        # Act
        result = TradeTick.from_serializable_str(
            AUDUSD_SIM.id, tick.to_serializable_str()
        )

        # Assert
        self.assertEqual(tick, result)

    def test_to_serializable_returns_expected_string(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        # Act
        result = tick.to_serializable_str()

        # Assert
        self.assertEqual("1.00000,10000,BUY,123456789,0,0", result)
