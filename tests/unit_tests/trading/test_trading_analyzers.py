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

from nautilus_trader.model.identifiers import Symbol, Venue
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.objects import Quantity, Price
from nautilus_trader.trading.analyzers import SpreadAnalyzer
from tests.test_kit.stubs import UNIX_EPOCH

USDJPY_FXCM = Symbol('USD/JPY', Venue('FXCM'))
AUDUSD_FXCM = Symbol('AUD/USD', Venue('FXCM'))


class SpreadAnalyzerTests(unittest.TestCase):

    def test_can_instantiate(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_FXCM, 1000)

        # Act
        # Assert
        self.assertEqual(0, analyzer.current_spread)
        self.assertEqual(0, analyzer.current_spread)
        self.assertEqual(0, analyzer.average_spread)
        self.assertEqual(False, analyzer.initialized)

    def test_update_with_incorrect_tick_raises_exception(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_FXCM, 1000)
        tick = QuoteTick(
            USDJPY_FXCM,
            Price(117.80000, 5),
            Price(117.80010, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)
        # Act
        # Assert
        self.assertRaises(ValueError, analyzer.update, tick)

    def test_update_correctly_updates_analyzer(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_FXCM, 1000)
        tick1 = QuoteTick(
            AUDUSD_FXCM,
            Price(0.80000, 5),
            Price(0.80010, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        tick2 = QuoteTick(
            AUDUSD_FXCM,
            Price(0.80002, 5),
            Price(0.80008, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        analyzer.update(tick1)
        analyzer.update(tick2)

        # Assert
        self.assertAlmostEqual(6e-05, analyzer.current_spread)
        self.assertAlmostEqual(8e-05, analyzer.average_spread)
