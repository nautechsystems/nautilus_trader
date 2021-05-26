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

from nautilus_trader.indicators.spread_analyzer import SpreadAnalyzer
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class SpreadAnalyzerTests(unittest.TestCase):
    def test_instantiate(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_SIM.id, 1000)

        # Act
        # Assert
        self.assertEqual(0, analyzer.current)
        self.assertEqual(0, analyzer.current)
        self.assertEqual(0, analyzer.average)
        self.assertEqual(False, analyzer.initialized)

    def test_handle_ticks_initializes_indicator(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_SIM.id, 1)  # Only one tick
        tick = TestStubs.quote_tick_5decimal()

        # Act
        analyzer.handle_quote_tick(tick)
        analyzer.handle_quote_tick(tick)

        # Assert
        self.assertTrue(analyzer.initialized)

    def test_update_with_incorrect_tick_raises_exception(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_SIM.id, 1000)
        tick = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("117.80000"),
            Price.from_str("117.80010"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )
        # Act
        # Assert
        self.assertRaises(ValueError, analyzer.handle_quote_tick, tick)

    def test_update_correctly_updates_analyzer(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_SIM.id, 1000)
        tick1 = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("0.80000"),
            Price.from_str("0.80010"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        tick2 = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("0.80002"),
            Price.from_str("0.80008"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        analyzer.handle_quote_tick(tick1)
        analyzer.handle_quote_tick(tick2)

        # Assert
        self.assertAlmostEqual(6e-05, analyzer.current)
        self.assertAlmostEqual(8e-05, analyzer.average)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        instance = SpreadAnalyzer(AUDUSD_SIM.id, 1000)

        # Act
        instance.reset()

        # Assert
        self.assertFalse(instance.initialized)
        self.assertEqual(0, instance.current)
