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

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


SIM = Venue("SIM")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class DataTypeTests(unittest.TestCase):
    def test_data_type_instantiation(self):
        # Arrange
        # Act
        data_type = DataType(str, {"type": "NEWS_WIRE"})

        # Assert
        self.assertEqual(str, data_type.type)
        self.assertEqual({"type": "NEWS_WIRE"}, data_type.metadata)
        self.assertEqual("<str> {'type': 'NEWS_WIRE'}", str(data_type))
        self.assertEqual(
            "DataType(type=str, metadata={'type': 'NEWS_WIRE'})", repr(data_type)
        )

    def test_data_equality_and_hash(self):
        # Arrange
        # Act
        data_type1 = DataType(str, {"type": "NEWS_WIRE", "topic": "Earthquake"})
        data_type2 = DataType(str, {"type": "NEWS_WIRE", "topic": "Flood"})
        data_type3 = DataType(int, {"type": "FED_DATA", "topic": "NonFarmPayroll"})

        # Assert
        self.assertTrue(data_type1 == data_type1)
        self.assertTrue(data_type1 != data_type2)
        self.assertTrue(data_type1 != data_type2)
        self.assertTrue(data_type1 != data_type3)
        self.assertEqual(int, type(hash(data_type1)))

    def test_data_type_as_key_in_dict(self):
        # Arrange
        # Act
        data_type = DataType(str, {"type": "NEWS_WIRE", "topic": "Earthquake"})

        hash_map = {data_type: []}

        # Assert
        self.assertIn(data_type, hash_map)

    def test_data_instantiation(self):
        # Arrange
        # Act
        data_type = DataType(NewsEvent, {"publisher": "NEWS_WIRE"})
        data = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=USD,
            ts_event_ns=0,
            ts_recv_ns=0,
        )
        generic_data = GenericData(data_type, data)

        # Assert
        self.assertEqual(data_type, generic_data.data_type)
        self.assertEqual(data, generic_data.data)


class CacheFacadeTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.facade = CacheFacade()

    def test_instrument_ids_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.instrument_ids)

    def test_instruments_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.instruments)

    def test_quote_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.quote_ticks, AUDUSD_SIM.id)

    def test_trade_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.trade_ticks, AUDUSD_SIM.id)

    def test_bars_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError, self.facade.bars, TestStubs.bartype_gbpusd_1sec_mid()
        )

    def test_instrument_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.instrument, AUDUSD_SIM.id)

    def test_price_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError, self.facade.price, AUDUSD_SIM.id, PriceType.MID
        )

    def test_order_book_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.order_book, AUDUSD_SIM.id)

    def test_quote_tick_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.quote_tick, AUDUSD_SIM.id)

    def test_trade_tick_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.trade_tick, AUDUSD_SIM.id)

    def test_bar_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError, self.facade.bar, TestStubs.bartype_gbpusd_1sec_mid()
        )

    def test_quote_tick_count_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError, self.facade.quote_tick_count, AUDUSD_SIM.id
        )

    def test_trade_tick_count_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError, self.facade.trade_tick_count, AUDUSD_SIM.id
        )

    def test_bar_count_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError,
            self.facade.bar_count,
            TestStubs.bartype_gbpusd_1sec_mid(),
        )

    def test_has_order_book_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError, self.facade.has_order_book, AUDUSD_SIM.id
        )

    def test_has_quote_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError, self.facade.has_quote_ticks, AUDUSD_SIM.id
        )

    def test_has_trade_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError, self.facade.has_trade_ticks, AUDUSD_SIM.id
        )

    def test_has_bars_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError,
            self.facade.has_bars,
            TestStubs.bartype_gbpusd_1sec_mid(),
        )

    def test_get_xrate_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError,
            self.facade.get_xrate,
            SIM,
            AUDUSD_SIM.base_currency,
            AUDUSD_SIM.quote_currency,
        )
