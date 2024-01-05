# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.tardis.loaders import TardisQuoteDataLoader
from nautilus_trader.adapters.tardis.loaders import TardisTradeDataLoader
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from tests import TEST_DATA_DIR


def test_tardis_quote_data_loader():
    # Arrange, Act
    path = TEST_DATA_DIR / "tardis/quotes.csv"
    ticks = TardisQuoteDataLoader.load(path)

    # Assert
    assert len(ticks) == 9999


def test_pre_process_with_quote_tick_data():
    # Arrange
    instrument = TestInstrumentProvider.btcusdt_binance()
    wrangler = QuoteTickDataWrangler(instrument=instrument)
    path = TEST_DATA_DIR / "tardis/quotes.csv"
    data = TardisQuoteDataLoader.load(path)

    # Act
    ticks = wrangler.process(
        data,
        ts_init_delta=1_000_501,
    )

    # Assert
    assert len(ticks) == 9999
    assert ticks[0].bid_price == Price.from_str("9681.92")
    assert ticks[0].ask_price == Price.from_str("9682.00")
    assert ticks[0].bid_size == Quantity.from_str("0.670000")
    assert ticks[0].ask_size == Quantity.from_str("0.840000")
    assert ticks[0].ts_event == 1582329603502092000
    assert ticks[0].ts_init == 1582329603503092501


def test_tardis_trade_tick_loader():
    # Arrange, Act
    path = TEST_DATA_DIR / "tardis/trades.csv"
    ticks = TardisTradeDataLoader.load(path)

    # Assert
    assert len(ticks) == 9999


def test_pre_process_with_trade_tick_data():
    # Arrange
    instrument = TestInstrumentProvider.btcusdt_binance()
    wrangler = TradeTickDataWrangler(instrument=instrument)
    path = TEST_DATA_DIR / "tardis/trades.csv"
    data = TardisTradeDataLoader.load(path)

    # Act
    ticks = wrangler.process(data)

    # Assert
    assert len(ticks) == 9999
    assert ticks[0].price == Price.from_str("9682.00")
    assert ticks[0].size == Quantity.from_str("0.132000")
    assert ticks[0].aggressor_side == AggressorSide.BUYER
    assert ticks[0].trade_id == TradeId("42377944")
    assert ticks[0].ts_event == 1582329602418379000
    assert ticks[0].ts_init == 1582329602418379000
