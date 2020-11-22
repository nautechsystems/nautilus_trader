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

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.data.base import DataCacheFacade
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.stubs import TestStubs


FXCM = Venue("FXCM")
USDJPY_FXCM = InstrumentLoader.default_fx_ccy(Symbol('USD/JPY', FXCM))
AUDUSD_FXCM = InstrumentLoader.default_fx_ccy(Symbol('AUD/USD', FXCM))


class DataCacheFacadeTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup

        self.facade = DataCacheFacade()

    def test_symbols_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.symbols)

    def test_instruments_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.instruments)

    def test_quote_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.quote_ticks, AUDUSD_FXCM.symbol)

    def test_trade_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.trade_ticks, AUDUSD_FXCM.symbol)

    def test_bars_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.bars, TestStubs.bartype_gbpusd_1sec_mid())

    def test_instrument_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.instrument, AUDUSD_FXCM.symbol)

    def test_quote_tick_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.quote_tick, AUDUSD_FXCM.symbol)

    def test_trade_tick_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.trade_tick, AUDUSD_FXCM.symbol)

    def test_bar_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.bar, TestStubs.bartype_gbpusd_1sec_mid())

    def test_quote_tick_count_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.quote_tick_count, AUDUSD_FXCM.symbol)

    def test_trade_tick_count_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.trade_tick_count, AUDUSD_FXCM.symbol)

    def test_bar_count_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.bar_count, TestStubs.bartype_gbpusd_1sec_mid())

    def test_has_quote_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.has_quote_ticks, AUDUSD_FXCM.symbol)

    def test_has_trade_ticks_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.has_trade_ticks, AUDUSD_FXCM.symbol)

    def test_has_bars_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.facade.has_bars, TestStubs.bartype_gbpusd_1sec_mid())

    def test_get_xrate_when_not_implemented_raises_exception(self):
        self.assertRaises(
            NotImplementedError,
            self.facade.get_xrate,
            FXCM,
            AUDUSD_FXCM.base_currency,
            AUDUSD_FXCM.quote_currency,
        )
