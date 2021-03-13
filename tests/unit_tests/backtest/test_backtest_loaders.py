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

from decimal import Decimal
import unittest

from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider


class BacktestLoadersTests(unittest.TestCase):

    def test_default_fx_with_5_dp_returns_expected_instrument(self):
        # Arrange
        loader = TestInstrumentProvider()

        # Act
        instrument = loader.default_fx_ccy("AUD/USD")

        # Assert
        self.assertEqual(InstrumentId(Symbol("AUD/USD"), Venue("SIM")), instrument.id)
        self.assertEqual(5, instrument.price_precision)
        self.assertEqual(Decimal("0.00001"), instrument.tick_size)
        self.assertEqual(Currency(code="USD", precision=2, currency_type=CurrencyType.FIAT), instrument.quote_currency)

    def test_default_fx_with_3_dp_returns_expected_instrument(self):
        # Arrange
        loader = TestInstrumentProvider()

        # Act
        instrument = loader.default_fx_ccy("USD/JPY", Venue("SIM"))

        # Assert
        self.assertEqual(InstrumentId(Symbol("USD/JPY"), Venue("SIM")), instrument.id)
        self.assertEqual(3, instrument.price_precision)
        self.assertEqual(Decimal("0.001"), instrument.tick_size)
        self.assertEqual(Currency(code='JPY', precision=2, currency_type=CurrencyType.FIAT), instrument.quote_currency)


class ParquetTickDataLoadersTests(unittest.TestCase):

    def test_btcusdt_trade_ticks_from_parquet_loader_return_expected_row(self):
        # Arrange
        # Act
        trade_ticks = TestDataProvider.parquet_btcusdt_trades()

        # Assert
        self.assertEqual(len(trade_ticks), 2001)
        self.assertIn('trade_id', trade_ticks.columns)
        self.assertIn('price', trade_ticks.columns)
        self.assertIn('quantity', trade_ticks.columns)
        self.assertIn('buyer_maker', trade_ticks.columns)
        self.assertEqual(trade_ticks.iloc[0]['trade_id'], 553287559)

    def test_btcusdt_quote_ticks_from_parquet_loader_return_expected_row(self):
        # Arrange
        # Act
        quote_ticks = TestDataProvider.parquet_btcusdt_quotes()

        # Assert
        self.assertEqual(len(quote_ticks), 451)
        self.assertIn('symbol', quote_ticks.columns)
        self.assertIn('ask_size', quote_ticks.columns)
        self.assertIn('ask', quote_ticks.columns)
        self.assertIn('bid_size', quote_ticks.columns)
        self.assertIn('bid', quote_ticks.columns)
        self.assertEqual(quote_ticks.iloc[0]['ask'], 39433.62)
        self.assertEqual(quote_ticks.iloc[0]['bid'], 39432.99)
