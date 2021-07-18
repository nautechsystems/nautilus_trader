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

from pandas import Timestamp

from nautilus_trader.common.clock import TestClock
from nautilus_trader.data.wrangling import BarDataWrangler
from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.data.wrangling import TradeTickDataWrangler
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestStubs.audusd_id()


class TestQuoteTickDataWrangler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.usdjpy_ticks()

        # Assert
        assert len(ticks) == 1000

    def test_pre_process_with_tick_data(self):
        # Arrange
        tick_data = TestDataProvider.usdjpy_ticks()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy("USD/JPY"),
            data_quotes=tick_data,
            data_bars_bid=None,
            data_bars_ask=None,
        )

        # Act
        self.tick_builder.pre_process(0, 42)
        ticks = self.tick_builder.processed_data

        # Assert
        assert self.tick_builder.resolution == BarAggregation.TICK
        assert len(ticks) == 1000
        assert ticks.iloc[1].name == Timestamp("2013-01-01 22:02:35.907000", tz="UTC")

    def test_pre_process_with_bar_data(self):
        # Arrange
        bid_data = TestDataProvider.usdjpy_1min_bid()
        ask_data = TestDataProvider.usdjpy_1min_ask()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy("USD/JPY"),
            data_quotes=None,
            data_bars_bid={BarAggregation.MINUTE: bid_data},
            data_bars_ask={BarAggregation.MINUTE: ask_data},
        )

        # Act
        self.tick_builder.pre_process(0, 42)
        tick_data = self.tick_builder.processed_data

        # Assert
        assert self.tick_builder.resolution == BarAggregation.MINUTE
        assert len(tick_data) == 115044
        assert tick_data.iloc[0].name == Timestamp("2013-01-31 23:59:59.700000+0000", tz="UTC")
        assert tick_data.iloc[1].name == Timestamp("2013-01-31 23:59:59.800000+0000", tz="UTC")
        assert tick_data.iloc[2].name == Timestamp("2013-01-31 23:59:59.900000+0000", tz="UTC")
        assert tick_data.iloc[3].name == Timestamp("2013-02-01 00:00:00+0000", tz="UTC")
        assert tick_data.iloc[0]["instrument_id"] == 0
        assert tick_data.iloc[0]["bid_size"] == "1000000"
        assert tick_data.iloc[0]["ask_size"] == "1000000"
        assert tick_data.iloc[1]["bid_size"] == "1000000"
        assert tick_data.iloc[1]["ask_size"] == "1000000"
        assert tick_data.iloc[2]["bid_size"] == "1000000"
        assert tick_data.iloc[2]["ask_size"] == "1000000"
        assert tick_data.iloc[3]["bid_size"] == "1000000"
        assert tick_data.iloc[3]["ask_size"] == "1000000"

    def test_build_ticks_with_tick_data(self):
        # Arrange
        tick_data = TestDataProvider.audusd_ticks()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy("AUD/USD"),
            data_quotes=tick_data,
            data_bars_bid=None,
            data_bars_ask=None,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.build_ticks()

        # Assert
        assert len(ticks) == 100000
        assert ticks[0].bid == Price.from_str("0.67067")
        assert ticks[0].ask == Price.from_str("0.67070")
        assert ticks[0].bid_size == Quantity.from_str("1000000")
        assert ticks[0].ask_size == Quantity.from_str("1000000")
        assert ticks[0].ts_recv_ns == 1580398089820000000
        assert ticks[99999].ts_recv_ns == 1580504394500999936

    def test_build_ticks_with_bar_data(self):
        # Arrange
        bid_data = TestDataProvider.usdjpy_1min_bid()
        ask_data = TestDataProvider.usdjpy_1min_ask()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy("USD/JPY"),
            data_quotes=None,
            data_bars_bid={BarAggregation.MINUTE: bid_data},
            data_bars_ask={BarAggregation.MINUTE: ask_data},
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.build_ticks()

        # Assert
        assert len(ticks) == 115044
        assert ticks[0].bid == Price.from_str("91.715")
        assert ticks[0].ask == Price.from_str("91.717")
        assert ticks[0].bid_size == Quantity.from_str("1000000")
        assert ticks[0].ask_size == Quantity.from_str("1000000")
        assert ticks[0].ts_recv_ns == 1359676799700000000


class TestTradeTickDataWrangler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.ethusdt_trades()

        # Assert
        assert len(ticks) == 69806

    def test_process(self):
        # Arrange
        tick_data = TestDataProvider.ethusdt_trades()
        self.tick_builder = TradeTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy("USD/JPY"),
            data=tick_data,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.processed_data

        # Assert
        assert len(ticks) == 69806
        assert ticks.iloc[0].name == Timestamp("2020-08-14 10:00:00.223000+0000", tz="UTC")

    def test_build_ticks(self):
        # Arrange
        tick_data = TestDataProvider.ethusdt_trades()
        self.tick_builder = TradeTickDataWrangler(
            instrument=TestInstrumentProvider.ethusdt_binance(),
            data=tick_data,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.build_ticks()

        # Assert
        assert len(ticks) == 69806
        assert ticks[0].price == Price.from_str("423.760")
        assert ticks[0].size == Quantity.from_str("2.67900")
        assert ticks[0].aggressor_side == AggressorSide.SELL
        assert ticks[0].match_id == "148568980"
        assert ticks[0].ts_recv_ns == 1597399200223000064


class TestBarDataWrangler:
    def setup(self):
        # Fixture Setup
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        bar_type = TestStubs.bartype_gbpusd_1min_bid()
        self.bar_builder = BarDataWrangler(
            bar_type=bar_type,
            price_precision=5,
            size_precision=1,
            data=data,
        )

    def test_build_bars_all(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_all()

        # Assert
        assert len(bars) == 1000

    def test_build_bars_range_with_defaults(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_range()

        # Assert
        assert len(bars) == 999

    def test_build_bars_range_with_param(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_range(start=500)

        # Assert
        assert len(bars) == 499

    def test_build_bars_from_with_defaults(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_from()

        # Assert
        assert len(bars) == 1000

    def test_build_bars_from_with_param(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_from(index=500)

        # Assert
        assert len(bars) == 500


class TestTardisQuoteDataWrangler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.tardis_quotes()

        # Assert
        assert len(ticks) == 9999

    def test_pre_process_with_tick_data(self):
        # Arrange
        tick_data = TestDataProvider.tardis_quotes()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.btcusdt_binance(),
            data_quotes=tick_data,
            data_bars_bid=None,
            data_bars_ask=None,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.processed_data

        # Assert
        assert self.tick_builder.resolution == BarAggregation.TICK
        assert len(ticks) == 9999
        assert ticks.iloc[1].name == Timestamp("2020-02-22 00:00:03.522418+0000", tz="UTC")
        assert ticks.bid_size[0] == "0.670000"
        assert ticks.ask_size[0] == "0.840000"
        assert ticks.bid[0] == "9681.92"
        assert ticks.ask[0] == "9682.00"
        assert sorted(ticks.columns) == sorted(
            ["ask", "ask_size", "bid", "bid_size", "instrument_id", "symbol"]
        )


class TestTardisTradeDataWrangler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.tardis_trades()

        # Assert
        assert len(ticks) == 9999

    def test_process(self):
        # Arrange
        tick_data = TestDataProvider.tardis_trades()
        self.tick_builder = TradeTickDataWrangler(
            instrument=TestInstrumentProvider.btcusdt_binance(),
            data=tick_data,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.processed_data

        # Assert
        assert len(ticks) == 9999
        assert ticks.iloc[0].name == Timestamp("2020-02-22 00:00:02.418379+0000", tz="UTC")

    def test_build_ticks(self):
        # Arrange
        tick_data = TestDataProvider.tardis_trades()
        self.tick_builder = TradeTickDataWrangler(
            instrument=TestInstrumentProvider.btcusdt_binance(),
            data=tick_data,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.build_ticks()

        # Assert
        assert len(ticks) == 9999
        assert ticks[0].price == Price.from_str("9682.00")
        assert ticks[0].size == Quantity.from_str("0.132000")
        assert ticks[0].aggressor_side == AggressorSide.BUY
        assert ticks[0].match_id == "42377944"
        assert ticks[0].ts_recv_ns == 1582329602418379008
