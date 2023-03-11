# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import os

from nautilus_trader.common.clock import TestClock
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.loaders import TardisQuoteDataLoader
from nautilus_trader.persistence.loaders import TardisTradeDataLoader
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests import TEST_DATA_DIR


AUDUSD_SIM = TestIdStubs.audusd_id()


class TestQuoteTickDataWrangler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange, Act
        provider = TestDataProvider()
        ticks = provider.read_csv_ticks("truefx-usdjpy-ticks.csv")

        # Assert
        assert len(ticks) == 1000

    def test_process_tick_data(self):
        # Arrange
        usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        wrangler = QuoteTickDataWrangler(instrument=usdjpy)
        provider = TestDataProvider()

        # Act
        ticks = wrangler.process(
            data=provider.read_csv_ticks("truefx-usdjpy-ticks.csv"),
            default_volume=1000000,
        )

        # Assert
        assert len(ticks) == 1000
        assert ticks[0].instrument_id == usdjpy.id
        assert ticks[0].bid == Price.from_str("86.655")
        assert ticks[0].ask == Price.from_str("86.728")
        assert ticks[0].bid_size == Quantity.from_int(1_000_000)
        assert ticks[0].ask_size == Quantity.from_int(1_000_000)
        assert ticks[0].ts_event == 1357077600295000064
        assert ticks[0].ts_event == 1357077600295000064

    def test_process_tick_data_with_delta(self):
        # Arrange
        usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        wrangler = QuoteTickDataWrangler(instrument=usdjpy)
        provider = TestDataProvider()

        # Act
        ticks = wrangler.process(
            data=provider.read_csv_ticks("truefx-usdjpy-ticks.csv"),
            default_volume=1000000,
            ts_init_delta=1_000_500,
        )

        # Assert
        assert len(ticks) == 1000
        assert ticks[0].instrument_id == usdjpy.id
        assert ticks[0].bid == Price.from_str("86.655")
        assert ticks[0].ask == Price.from_str("86.728")
        assert ticks[0].bid_size == Quantity.from_int(1_000_000)
        assert ticks[0].ask_size == Quantity.from_int(1_000_000)
        assert ticks[0].ts_event == 1357077600295000064
        assert ticks[0].ts_init == 1357077600296000564  # <-- delta diff

    def test_pre_process_bar_data_with_delta(self):
        # Arrange
        usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
        provider = TestDataProvider()
        bid_data = provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv")[:100]
        ask_data = provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv")[:100]

        wrangler = QuoteTickDataWrangler(instrument=usdjpy)

        # Act
        ticks = wrangler.process_bar_data(
            bid_data=bid_data,
            ask_data=ask_data,
            default_volume=1000000,
            ts_init_delta=1_000_500,
        )

        # Assert
        assert len(ticks) == 400
        assert ticks[0].instrument_id == usdjpy.id
        assert ticks[0].bid == Price.from_str("91.715")
        assert ticks[0].ask == Price.from_str("91.717")
        assert ticks[0].bid_size == Quantity.from_int(1_000_000)
        assert ticks[0].ask_size == Quantity.from_int(1_000_000)
        assert ticks[0].ts_event == 1359676799700000000
        assert ticks[0].ts_init == 1359676799701000500  # <-- delta diff

    def test_pre_process_bar_data_with_random_seed(self):
        # Arrange
        usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
        provider = TestDataProvider()
        bid_data = provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv")[:100]
        ask_data = provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv")[:100]

        wrangler = QuoteTickDataWrangler(instrument=usdjpy)

        # Act
        ticks = wrangler.process_bar_data(
            bid_data=bid_data,
            ask_data=ask_data,
            default_volume=1000000,
            random_seed=42,  # <-- with random seed
        )

        # Assert
        assert ticks[0].bid == Price.from_str("91.715")
        assert ticks[0].ask == Price.from_str("91.717")
        assert ticks[1].bid == Price.from_str("91.653")
        assert ticks[1].ask == Price.from_str("91.655")
        assert ticks[2].bid == Price.from_str("91.715")
        assert ticks[2].ask == Price.from_str("91.717")
        assert ticks[3].bid == Price.from_str("91.653")
        assert ticks[3].ask == Price.from_str("91.655")


class TestTradeTickDataWrangler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange, Act
        ticks = TestDataProvider().read_csv_ticks("binance-ethusdt-trades.csv")[:100]

        # Assert
        assert len(ticks) == 100

    def test_process(self):
        # Arrange
        ethusdt = TestInstrumentProvider.ethusdt_binance()
        wrangler = TradeTickDataWrangler(instrument=ethusdt)
        provider = TestDataProvider()

        # Act
        ticks = wrangler.process(provider.read_csv_ticks("binance-ethusdt-trades.csv")[:100])

        # Assert
        assert len(ticks) == 100
        assert ticks[0].price == Price.from_str("423.760")
        assert ticks[0].size == Quantity.from_str("2.67900")
        assert ticks[0].aggressor_side == AggressorSide.SELLER
        assert ticks[0].trade_id == TradeId("148568980")
        assert ticks[0].ts_event == 1597399200223000064
        assert ticks[0].ts_init == 1597399200223000064

    def test_process_with_delta(self):
        # Arrange
        ethusdt = TestInstrumentProvider.ethusdt_binance()
        wrangler = TradeTickDataWrangler(instrument=ethusdt)
        provider = TestDataProvider()

        # Act
        ticks = wrangler.process(
            provider.read_csv_ticks("binance-ethusdt-trades.csv")[:100],
            ts_init_delta=1_000_500,
        )

        # Assert
        assert len(ticks) == 100
        assert ticks[0].price == Price.from_str("423.760")
        assert ticks[0].size == Quantity.from_str("2.67900")
        assert ticks[0].aggressor_side == AggressorSide.SELLER
        assert ticks[0].trade_id == TradeId("148568980")
        assert ticks[0].ts_event == 1597399200223000064
        assert ticks[0].ts_init == 1597399200224000564  # <-- delta diff


class TestBarDataWrangler:
    def setup(self):
        # Fixture Setup
        instrument = TestInstrumentProvider.default_fx_ccy("GBP/USD")
        bar_type = TestDataStubs.bartype_gbpusd_1min_bid()
        self.wrangler = BarDataWrangler(
            bar_type=bar_type,
            instrument=instrument,
        )

    def test_process(self):
        # Arrange, Act
        provider = TestDataProvider()
        bars = self.wrangler.process(provider.read_csv_bars("fxcm-gbpusd-m1-bid-2012.csv")[:1000])

        # Assert
        assert len(bars) == 1000
        assert bars[0].open == Price.from_str("1.57597")
        assert bars[0].high == Price.from_str("1.57606")
        assert bars[0].low == Price.from_str("1.57576")
        assert bars[0].close == Price.from_str("1.57576")
        assert bars[0].volume == Quantity.from_int(1_000_000)
        assert bars[0].ts_event == 1328054400000000000
        assert bars[0].ts_init == 1328054400000000000

    def test_process_with_default_volume_and_delta(self):
        # Arrange, Act
        provider = TestDataProvider()
        bars = self.wrangler.process(
            data=provider.read_csv_bars("fxcm-gbpusd-m1-bid-2012.csv")[:1000],
            default_volume=10,
            ts_init_delta=1_000_500,
        )

        # Assert
        assert len(bars) == 1000
        assert bars[0].open == Price.from_str("1.57597")
        assert bars[0].high == Price.from_str("1.57606")
        assert bars[0].low == Price.from_str("1.57576")
        assert bars[0].close == Price.from_str("1.57576")
        assert bars[0].volume == Quantity.from_int(10)  # <-- default volume
        assert bars[0].ts_event == 1328054400000000000
        assert bars[0].ts_init == 1328054400001000500  # <-- delta diff


class TestBarDataWranglerHeaderless:
    def setup(self):
        # Fixture Setup
        instrument = TestInstrumentProvider.adabtc_binance()
        bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
        self.wrangler = BarDataWrangler(
            bar_type=bar_type,
            instrument=instrument,
        )

    def test_process(self):
        # Arrange, Act
        provider = TestDataProvider()
        config = {
            "names": [
                "timestamp",
                "open",
                "high",
                "low",
                "close",
                "volume",
                "ts_close",
                "quote_volume",
                "n_trades",
                "taker_buy_base_volume",
                "taker_buy_quote_volume",
                "ignore",
            ],
        }
        data = provider.read_csv("ADABTC-1m-2021-11-27.csv", **config)
        data["timestamp"] = data["timestamp"].astype("datetime64[ms]")
        data = data.set_index("timestamp")
        bars = self.wrangler.process(data)

        # Assert
        assert len(bars) == 10
        assert bars[0].open == Price.from_str("0.00002853")
        assert bars[0].high == Price.from_str("0.00002854")
        assert bars[0].low == Price.from_str("0.00002851")
        assert bars[0].close == Price.from_str("0.00002854")
        assert bars[0].volume == Quantity.from_str("36304.2")
        assert bars[0].ts_event == 1637971200000000000
        assert bars[0].ts_init == 1637971200000000000


class TestTardisQuoteDataWrangler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange, Act
        path = os.path.join(TEST_DATA_DIR, "tardis_quotes.csv")
        ticks = TardisQuoteDataLoader.load(path)

        # Assert
        assert len(ticks) == 9999

    def test_pre_process_with_tick_data(self):
        # Arrange
        instrument = TestInstrumentProvider.btcusdt_binance()
        wrangler = QuoteTickDataWrangler(instrument=instrument)
        path = os.path.join(TEST_DATA_DIR, "tardis_quotes.csv")
        data = TardisQuoteDataLoader.load(path)

        # Act
        ticks = wrangler.process(
            data,
            ts_init_delta=1_000_501,
        )

        # Assert
        assert len(ticks) == 9999
        assert ticks[0].bid == Price.from_str("9681.92")
        assert ticks[0].ask == Price.from_str("9682.00")
        assert ticks[0].bid_size == Quantity.from_str("0.670000")
        assert ticks[0].ask_size == Quantity.from_str("0.840000")
        assert ticks[0].ts_event == 1582329603502091776
        assert ticks[0].ts_init == 1582329603503092277


class TestTardisTradeDataWrangler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange, Act
        path = os.path.join(TEST_DATA_DIR, "tardis_trades.csv")
        ticks = TardisTradeDataLoader.load(path)

        # Assert
        assert len(ticks) == 9999

    def test_process(self):
        # Arrange
        instrument = TestInstrumentProvider.btcusdt_binance()
        wrangler = TradeTickDataWrangler(instrument=instrument)
        path = os.path.join(TEST_DATA_DIR, "tardis_trades.csv")
        data = TardisTradeDataLoader.load(path)

        # Act
        ticks = wrangler.process(data)

        # Assert
        assert len(ticks) == 9999
        assert ticks[0].price == Price.from_str("9682.00")
        assert ticks[0].size == Quantity.from_str("0.132000")
        assert ticks[0].aggressor_side == AggressorSide.BUYER
        assert ticks[0].trade_id == TradeId("42377944")
        assert ticks[0].ts_event == 1582329602418379008
        assert ticks[0].ts_init == 1582329602418379008
