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

from parameterized import parameterized

from tests.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class InstrumentTests(unittest.TestCase):
    @parameterized.expand(
        [
            [AUDUSD_SIM, AUDUSD_SIM, True, False],
            [AUDUSD_SIM, USDJPY_SIM, False, True],
        ]
    )
    def test_equality(self, instrument1, instrument2, expected1, expected2):
        # Arrange
        # Act
        result1 = instrument1 == instrument2
        result2 = instrument1 != instrument2

        # Assert
        self.assertEqual(expected1, result1)
        self.assertEqual(expected2, result2)

    def test_str_repr_returns_expected(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("Instrument('BTC/USDT.BINANCE')", str(BTCUSDT_BINANCE))
        self.assertEqual("Instrument('BTC/USDT.BINANCE')", repr(BTCUSDT_BINANCE))

    def test_hash(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(int, type(hash(BTCUSDT_BINANCE)))
        self.assertEqual(hash(BTCUSDT_BINANCE), hash(BTCUSDT_BINANCE))
