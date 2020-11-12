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

import decimal
import unittest

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


class BacktestLoadersTests(unittest.TestCase):

    def test_default_fx_with_5_dp_returns_expected_instrument(self):
        # Arrange
        loader = InstrumentLoader()

        # Act
        instrument = loader.default_fx_ccy(Symbol("AUD/USD", Venue('FXCM')))

        # Assert
        self.assertEqual(Symbol("AUD/USD", Venue('FXCM')), instrument.symbol)
        self.assertEqual(5, instrument.price_precision)
        self.assertEqual(decimal.Decimal("0.00001"), instrument.tick_size)
        self.assertEqual(Currency(code='USD', precision=2, currency_type=CurrencyType.FIAT), instrument.quote_currency)

    def test_default_fx_with_3_dp_returns_expected_instrument(self):
        # Arrange
        loader = InstrumentLoader()

        # Act
        instrument = loader.default_fx_ccy(Symbol("USD/JPY", Venue('FXCM')))

        # Assert
        self.assertEqual(Symbol("USD/JPY", Venue('FXCM')), instrument.symbol)
        self.assertEqual(3, instrument.price_precision)
        self.assertEqual(decimal.Decimal("0.001"), instrument.tick_size)
        self.assertEqual(Currency(code='JPY', precision=2, currency_type=CurrencyType.FIAT), instrument.quote_currency)
