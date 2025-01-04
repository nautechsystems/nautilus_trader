# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


def test_pyo3_delta_to_legacy_cython(benchmark):
    pyo3_delta = TestDataProviderPyo3.order_book_delta()
    benchmark(OrderBookDelta.from_pyo3, pyo3_delta)


def test_pyo3_deltas_to_legacy_cython_list(benchmark):
    pyo3_deltas = [TestDataProviderPyo3.order_book_delta()] * 10_000
    benchmark(OrderBookDelta.from_pyo3_list, pyo3_deltas)


def test_pyo3_quote_to_legacy_cython(benchmark):
    pyo3_quote = TestDataProviderPyo3.quote_tick()
    benchmark(QuoteTick.from_pyo3, pyo3_quote)


def test_pyo3_quotes_to_legacy_cython_list(benchmark):
    pyo3_quotes = [TestDataProviderPyo3.quote_tick()] * 10_000
    benchmark(QuoteTick.from_pyo3_list, pyo3_quotes)


def test_pyo3_trade_to_legacy_cython(benchmark):
    pyo3_trade = TestDataProviderPyo3.trade_tick()
    benchmark(TradeTick.from_pyo3, pyo3_trade)


def test_pyo3_trades_to_legacy_cython_list(benchmark):
    pyo3_trades = [TestDataProviderPyo3.trade_tick()] * 10_000
    benchmark(TradeTick.from_pyo3_list, pyo3_trades)


def test_pyo3_bar_to_legacy_cython(benchmark):
    pyo3_bar = TestDataProviderPyo3.bar_5decimal()
    benchmark(Bar.from_pyo3, pyo3_bar)


def test_pyo3_bars_to_legacy_cython_list(benchmark):
    pyo3_bars = [TestDataProviderPyo3.bar_5decimal()] * 10_000
    benchmark(Bar.from_pyo3_list, pyo3_bars)
