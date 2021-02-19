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

from nautilus_trader.data.base import Data
from nautilus_trader.data.base import DataCacheFacade
from nautilus_trader.data.base import DataType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs

SIM = Venue("SIM")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy(Symbol("USD/JPY", SIM))
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(Symbol("AUD/USD", SIM))


class DataTypeTests(unittest.TestCase):

    def test_data_type_instantiation(self):
        # Arrange
        # Act
        data_type = DataType(str, {"Type": "NEWS_FLASH"})

        # Assert
        self.assertEqual(str, data_type.type)
        self.assertEqual({"Type": "NEWS_FLASH"}, data_type.metadata)
        self.assertEqual("<str> {'Type': 'NEWS_FLASH'}", str(data_type))
        self.assertEqual("DataType(type=str, metadata={'Type': 'NEWS_FLASH'})", repr(data_type))

    def test_data_instantiation(self):
        # Arrange
        # Act
        data_type = DataType(str, {"Type": "NEWS_FLASH"})
        data = Data(data_type, "SOME_NEWS_HEADLINE")

        # Assert
        self.assertEqual(data_type, data.data_type)
        self.assertEqual("SOME_NEWS_HEADLINE", data.data)


class DataCacheFacadeTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.facade = DataCacheFacade()

    def test_symbols_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.symbols)

    def test_instruments_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.instruments)

    def test_quote_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.quote_ticks, AUDUSD_SIM.symbol)

    def test_trade_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.trade_ticks, AUDUSD_SIM.symbol)

    def test_bars_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.bars, TestStubs.bartype_gbpusd_1sec_mid())

    def test_instrument_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.instrument, AUDUSD_SIM.symbol)

    def test_price_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.price, AUDUSD_SIM.symbol, PriceType.MID)

    def test_order_book_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.order_book, AUDUSD_SIM.symbol)

    def test_quote_tick_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.quote_tick, AUDUSD_SIM.symbol)

    def test_trade_tick_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.trade_tick, AUDUSD_SIM.symbol)

    def test_bar_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.bar, TestStubs.bartype_gbpusd_1sec_mid())

    def test_quote_tick_count_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.quote_tick_count, AUDUSD_SIM.symbol)

    def test_trade_tick_count_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.trade_tick_count, AUDUSD_SIM.symbol)

    def test_bar_count_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.bar_count, TestStubs.bartype_gbpusd_1sec_mid())

    def test_has_order_book_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.has_order_book, AUDUSD_SIM.symbol)

    def test_has_quote_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.has_quote_ticks, AUDUSD_SIM.symbol)

    def test_has_trade_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.has_trade_ticks, AUDUSD_SIM.symbol)

    def test_has_bars_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.has_bars, TestStubs.bartype_gbpusd_1sec_mid())

    def test_get_xrate_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError,
            self.facade.get_xrate,
            SIM,
            AUDUSD_SIM.base_currency,
            AUDUSD_SIM.quote_currency,
        )
