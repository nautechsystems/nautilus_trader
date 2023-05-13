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
from datetime import timedelta
from decimal import Decimal

import pandas as pd
import pytest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.data.aggregation import BarBuilder
from nautilus_trader.data.aggregation import TickBarAggregator
from nautilus_trader.data.aggregation import TimeBarAggregator
from nautilus_trader.data.aggregation import ValueBarAggregator
from nautilus_trader.data.aggregation import VolumeBarAggregator
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.loaders import ParquetTickDataLoader
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests import TEST_DATA_DIR


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BITMEX = TestInstrumentProvider.ethusd_bitmex()


class TestBarBuilder:
    def test_instantiate(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        # Act, Assert
        assert not builder.initialized
        assert builder.ts_last == 0
        assert builder.count == 0

    def test_str_repr(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        # Act, Assert
        assert (
            str(builder)
            == "BarBuilder(BTCUSDT.BINANCE-100-TICK-LAST-EXTERNAL,None,None,None,None,0.000000)"
        )
        assert (
            repr(builder)
            == "BarBuilder(BTCUSDT.BINANCE-100-TICK-LAST-EXTERNAL,None,None,None,None,0.000000)"
        )

    def test_set_partial_updates_bar_to_expected_properties(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        partial_bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00002"),
            volume=Quantity.from_str("1"),
            ts_event=1_000_000_000,
            ts_init=1_000_000_000,
        )

        # Act
        builder.set_partial(partial_bar)

        bar = builder.build_now()

        # Assert
        assert bar.open == Price.from_str("1.00001")
        assert bar.high == Price.from_str("1.00010")
        assert bar.low == Price.from_str("1.00000")
        assert bar.close == Price.from_str("1.00002")
        assert bar.volume == Quantity.from_str("1")
        assert bar.ts_init == 1_000_000_000
        assert builder.ts_last == 1_000_000_000

    def test_set_partial_when_already_set_does_not_update(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        partial_bar1 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00002"),
            volume=Quantity.from_str("1"),
            ts_event=1_000_000_000,
            ts_init=1_000_000_000,
        )

        partial_bar2 = Bar(
            bar_type=bar_type,
            open=Price.from_str("2.00001"),
            high=Price.from_str("2.00010"),
            low=Price.from_str("2.00000"),
            close=Price.from_str("2.00002"),
            volume=Quantity.from_str("2"),
            ts_event=1_000_000_000,
            ts_init=3_000_000_000,
        )

        # Act
        builder.set_partial(partial_bar1)
        builder.set_partial(partial_bar2)

        bar = builder.build(4_000_000_000, 4_000_000_000)

        # Assert
        assert bar.open == Price.from_str("1.00001")
        assert bar.high == Price.from_str("1.00010")
        assert bar.low == Price.from_str("1.00000")
        assert bar.close == Price.from_str("1.00002")
        assert bar.volume == Quantity.from_str("1")
        assert bar.ts_init == 4_000_000_000
        assert builder.ts_last == 1_000_000_000

    def test_single_update_results_in_expected_properties(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        # Act
        builder.update(Price.from_str("1.00000"), Quantity.from_str("1"), 0)

        # Assert
        assert builder.initialized
        assert builder.ts_last == 0
        assert builder.count == 1

    def test_single_update_when_timestamp_less_than_last_update_ignores(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)
        builder.update(Price.from_str("1.00000"), Quantity.from_str("1"), 1_000)

        # Act
        builder.update(Price.from_str("1.00001"), Quantity.from_str("1"), 500)

        # Assert
        assert builder.initialized
        assert builder.ts_last == 1_000
        assert builder.count == 1

    def test_multiple_updates_correctly_increments_count(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        # Act
        builder.update(Price.from_str("1.00000"), Quantity.from_int(1), 1_000)
        builder.update(Price.from_str("1.00000"), Quantity.from_int(1), 1_000)
        builder.update(Price.from_str("1.00000"), Quantity.from_int(1), 1_000)
        builder.update(Price.from_str("1.00000"), Quantity.from_int(1), 1_000)
        builder.update(Price.from_str("1.00000"), Quantity.from_int(1), 1_000)

        # Assert
        assert builder.count == 5

    def test_build_when_no_updates_raises_exception(self):
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        builder = BarBuilder(AUDUSD_SIM, bar_type)

        # Act, Assert
        with pytest.raises(TypeError):
            builder.build()

    def test_build_when_received_updates_returns_expected_bar(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        builder.update(Price.from_str("1.00001"), Quantity.from_str("1.0"), 0)
        builder.update(Price.from_str("1.00002"), Quantity.from_str("1.5"), 0)
        builder.update(
            Price.from_str("1.00000"),
            Quantity.from_str("1.5"),
            1_000_000_000,
        )

        # Act
        bar = builder.build_now()  # Also resets builder

        # Assert
        assert bar.open == Price.from_str("1.00001")
        assert bar.high == Price.from_str("1.00002")
        assert bar.low == Price.from_str("1.00000")
        assert bar.close == Price.from_str("1.00000")
        assert bar.volume == Quantity.from_str("4.0")
        assert bar.ts_init == 1_000_000_000
        assert builder.ts_last == 1_000_000_000
        assert builder.count == 0

    def test_build_with_previous_close(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        builder.update(Price.from_str("1.00001"), Quantity.from_str("1.0"), 0)
        builder.build_now()  # This close should become the next open

        builder.update(Price.from_str("1.00000"), Quantity.from_str("1.0"), 0)
        builder.update(Price.from_str("1.00003"), Quantity.from_str("1.0"), 0)
        builder.update(Price.from_str("1.00002"), Quantity.from_str("1.0"), 0)

        bar2 = builder.build_now()

        # Assert
        assert bar2.open == Price.from_str("1.00000")
        assert bar2.high == Price.from_str("1.00003")
        assert bar2.low == Price.from_str("1.00000")
        assert bar2.close == Price.from_str("1.00002")
        assert bar2.volume == Quantity.from_str("3.0")


class TestTickBarAggregator:
    def test_handle_quote_tick_when_count_below_threshold_updates(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = TickBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00001"),
            ask=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_quote_tick(tick1)

        # Assert
        assert len(handler) == 0

    def test_handle_trade_tick_when_count_below_threshold_updates(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = TickBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("1.00001"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_trade_tick(tick1)

        # Assert
        assert len(handler) == 0

    def test_handle_quote_tick_when_count_at_threshold_sends_bar_to_handler(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = TickBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00001"),
            ask=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00002"),
            ask=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00000"),
            ask=Price.from_str("1.00003"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)

        # Assert
        assert len(handler) == 1
        assert handler[0].open == Price.from_str("1.000025")
        assert handler[0].high == Price.from_str("1.000035")
        assert handler[0].low == Price.from_str("1.000015")
        assert handler[0].close == Price.from_str("1.000015")
        assert handler[0].volume == Quantity.from_int(3)

    def test_handle_trade_tick_when_count_at_threshold_sends_bar_to_handler(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = TickBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00001"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

        tick2 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00002"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123457"),
            ts_event=0,
            ts_init=0,
        )

        tick3 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123458"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_trade_tick(tick1)
        aggregator.handle_trade_tick(tick2)
        aggregator.handle_trade_tick(tick3)

        # Assert
        assert len(handler) == 1
        assert handler[0].open == Price.from_str("1.00001")
        assert handler[0].high == Price.from_str("1.00002")
        assert handler[0].low == Price.from_str("1.00000")
        assert handler[0].close == Price.from_str("1.00000")
        assert handler[0].volume == Quantity.from_int(3)

    def test_run_quote_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(100, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = TickBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        # Setup data
        wrangler = QuoteTickDataWrangler(instrument)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("truefx-audusd-ticks.csv")[:1000])

        # Act
        for tick in ticks:
            aggregator.handle_quote_tick(tick)

        # Assert
        last_bar = handler[-1]
        assert len(handler) == 10
        assert last_bar.open == Price.from_str("0.670340")
        assert last_bar.high == Price.from_str("0.670345")
        assert last_bar.low == Price.from_str("0.670225")
        assert last_bar.close == Price.from_str("0.670230")
        assert last_bar.volume == Quantity.from_int(100_000_000)

    def test_run_trade_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        handler = []
        instrument = ETHUSDT_BITMEX
        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = TickBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BITMEX)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("binance-ethusdt-trades.csv")[:10000])

        # Act
        for tick in ticks:
            aggregator.handle_trade_tick(tick)

        # Assert
        last_bar = handler[-1]
        assert len(handler) == 10
        assert last_bar.open == Price.from_str("424.69")
        assert last_bar.high == Price.from_str("425.25")
        assert last_bar.low == Price.from_str("424.51")
        assert last_bar.close == Price.from_str("425.15")
        assert last_bar.volume == Quantity.from_int(3142)


class TestVolumeBarAggregator:
    def test_handle_quote_tick_when_volume_below_threshold_updates(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.BID)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = VolumeBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00001"),
            ask=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(3_000),
            ask_size=Quantity.from_int(2_000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_quote_tick(tick1)

        # Assert
        assert len(handler) == 0

    def test_handle_trade_tick_when_volume_below_threshold_updates(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = VolumeBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00001"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_trade_tick(tick1)

        # Assert
        assert len(handler) == 0

    def test_handle_quote_tick_when_volume_at_threshold_sends_bar_to_handler(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.BID)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = VolumeBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00001"),
            ask=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(3_000),
            ask_size=Quantity.from_int(2_000),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00002"),
            ask=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(4_000),
            ask_size=Quantity.from_int(2_000),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00000"),
            ask=Price.from_str("1.00003"),
            bid_size=Quantity.from_int(3_000),
            ask_size=Quantity.from_int(2_000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)

        # Assert
        assert len(handler) == 1
        assert handler[0].open == Price.from_str("1.00001")
        assert handler[0].high == Price.from_str("1.00002")
        assert handler[0].low == Price.from_str("1.00000")
        assert handler[0].close == Price.from_str("1.00000")
        assert handler[0].volume == Quantity.from_int(10_000)

    def test_handle_trade_tick_when_volume_at_threshold_sends_bar_to_handler(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = VolumeBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00001"),
            size=Quantity.from_int(3_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

        tick2 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00002"),
            size=Quantity.from_int(4_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123457"),
            ts_event=0,
            ts_init=0,
        )

        tick3 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(3_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123458"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_trade_tick(tick1)
        aggregator.handle_trade_tick(tick2)
        aggregator.handle_trade_tick(tick3)

        # Assert
        assert len(handler) == 1
        assert handler[0].open == Price.from_str("1.00001")
        assert handler[0].high == Price.from_str("1.00002")
        assert handler[0].low == Price.from_str("1.00000")
        assert handler[0].close == Price.from_str("1.00000")
        assert handler[0].volume == Quantity.from_int(10_000)

    def test_handle_quote_tick_when_volume_beyond_threshold_sends_bars_to_handler(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.BID)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = VolumeBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00001"),
            ask=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(2_000),
            ask_size=Quantity.from_int(2_000),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00002"),
            ask=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(3_000),
            ask_size=Quantity.from_int(3_000),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=instrument.id,
            bid=Price.from_str("1.00000"),
            ask=Price.from_str("1.00003"),
            bid_size=Quantity.from_int(25_000),
            ask_size=Quantity.from_int(25_000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)

        # Assert
        assert len(handler) == 3
        assert handler[0].open == Price.from_str("1.00001")
        assert handler[0].high == Price.from_str("1.00002")
        assert handler[0].low == Price.from_str("1.00000")
        assert handler[0].close == Price.from_str("1.00000")
        assert handler[0].volume == Quantity.from_int(10_000)
        assert handler[1].open == Price.from_str("1.00000")
        assert handler[1].high == Price.from_str("1.00000")
        assert handler[1].low == Price.from_str("1.00000")
        assert handler[1].close == Price.from_str("1.00000")
        assert handler[1].volume == Quantity.from_int(10_000)
        assert handler[2].open == Price.from_str("1.00000")
        assert handler[2].high == Price.from_str("1.00000")
        assert handler[2].low == Price.from_str("1.00000")
        assert handler[2].close == Price.from_str("1.00000")
        assert handler[2].volume == Quantity.from_int(10_000)

    def test_handle_trade_tick_when_volume_beyond_threshold_sends_bars_to_handler(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = VolumeBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00001"),
            size=Quantity.from_int(2_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

        tick2 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00002"),
            size=Quantity.from_int(3_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123457"),
            ts_event=0,
            ts_init=0,
        )

        tick3 = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(25_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123458"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_trade_tick(tick1)
        aggregator.handle_trade_tick(tick2)
        aggregator.handle_trade_tick(tick3)

        # Assert
        assert len(handler) == 3
        assert handler[0].open == Price.from_str("1.00001")
        assert handler[0].high == Price.from_str("1.00002")
        assert handler[0].low == Price.from_str("1.00000")
        assert handler[0].close == Price.from_str("1.00000")
        assert handler[0].volume == Quantity.from_int(10_000)
        assert handler[1].open == Price.from_str("1.00000")
        assert handler[1].high == Price.from_str("1.00000")
        assert handler[1].low == Price.from_str("1.00000")
        assert handler[1].close == Price.from_str("1.00000")
        assert handler[1].volume == Quantity.from_int(10_000)
        assert handler[2].open == Price.from_str("1.00000")
        assert handler[2].high == Price.from_str("1.00000")
        assert handler[2].low == Price.from_str("1.00000")
        assert handler[2].close == Price.from_str("1.00000")
        assert handler[2].volume == Quantity.from_int(10_000)

    def test_run_quote_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(1000, BarAggregation.VOLUME, PriceType.MID)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = VolumeBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        # Setup data
        wrangler = QuoteTickDataWrangler(instrument)
        provider = TestDataProvider()
        ticks = wrangler.process(
            data=provider.read_csv_ticks("truefx-audusd-ticks.csv")[:10000],
            default_volume=1,
        )

        # Act
        for tick in ticks:
            aggregator.handle_quote_tick(tick)

        # Assert
        last_bar = handler[-1]
        assert len(handler) == 10
        assert last_bar.open == Price.from_str("0.670650")
        assert last_bar.high == Price.from_str("0.670705")
        assert last_bar.low == Price.from_str("0.670370")
        assert last_bar.close == Price.from_str("0.670655")
        assert last_bar.volume == Quantity.from_int(1_000)

    def test_run_trade_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        handler = []
        instrument = ETHUSDT_BITMEX
        bar_spec = BarSpecification(1000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = VolumeBarAggregator(
            instrument,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BITMEX)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("binance-ethusdt-trades.csv")[:10000])

        # Act
        for tick in ticks:
            aggregator.handle_trade_tick(tick)

        # Assert
        last_bar = handler[-1]
        assert len(handler) == 27
        assert last_bar.open == Price.from_str("425.07")
        assert last_bar.high == Price.from_str("425.20")
        assert last_bar.low == Price.from_str("424.69")
        assert last_bar.close == Price.from_str("425.06")
        assert last_bar.volume == Quantity.from_int(1_000)


class TestTestValueBarAggregator:
    def test_handle_quote_tick_when_value_below_threshold_updates(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.BID)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = ValueBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid=Price.from_str("1.00001"),
            ask=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(3_000),
            ask_size=Quantity.from_int(2_000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_quote_tick(tick1)

        # Assert
        assert len(handler) == 0
        assert aggregator.get_cumulative_value() == Decimal("3000.03000")

    def test_handle_trade_tick_when_value_below_threshold_updates(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.LAST)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = ValueBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("15000.00"),
            size=Quantity.from_str("3.5"),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_trade_tick(tick1)

        # Assert
        assert len(handler) == 0
        assert aggregator.get_cumulative_value() == Decimal("52500.000")

    def test_handle_quote_tick_when_value_beyond_threshold_sends_bar_to_handler(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.BID)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = ValueBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid=Price.from_str("1.00001"),
            ask=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(20_000),
            ask_size=Quantity.from_int(20_000),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid=Price.from_str("1.00002"),
            ask=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(60_000),
            ask_size=Quantity.from_int(20_000),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid=Price.from_str("1.00000"),
            ask=Price.from_str("1.00003"),
            bid_size=Quantity.from_int(30_500),
            ask_size=Quantity.from_int(20_000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)

        # Assert
        assert len(handler) == 1
        assert handler[0].open == Price.from_str("1.00001")
        assert handler[0].high == Price.from_str("1.00002")
        assert handler[0].low == Price.from_str("1.00000")
        assert handler[0].close == Price.from_str("1.00000")
        assert handler[0].volume == Quantity.from_str("99999")
        assert aggregator.get_cumulative_value() == Decimal("10501.400")

    def test_handle_trade_tick_when_volume_beyond_threshold_sends_bars_to_handler(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.LAST)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = ValueBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        tick1 = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("20.00001"),
            size=Quantity.from_str("3000.00"),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

        tick2 = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("20.00002"),
            size=Quantity.from_str("4000.00"),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123457"),
            ts_event=0,
            ts_init=0,
        )

        tick3 = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("20.00000"),
            size=Quantity.from_str("5000.00"),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123458"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_trade_tick(tick1)
        aggregator.handle_trade_tick(tick2)
        aggregator.handle_trade_tick(tick3)

        # Assert
        assert len(handler) == 2
        assert handler[0].open == Price.from_str("20.00001")
        assert handler[0].high == Price.from_str("20.00002")
        assert handler[0].low == Price.from_str("20.00001")
        assert handler[0].close == Price.from_str("20.00002")
        assert handler[0].volume == Quantity.from_str("5000.00")
        assert handler[1].open == Price.from_str("20.00002")
        assert handler[1].high == Price.from_str("20.00002")
        assert handler[1].low == Price.from_str("20.00000")
        assert handler[1].close == Price.from_str("20.00000")
        assert handler[1].volume == Quantity.from_str("5000.00")
        assert aggregator.get_cumulative_value() == Decimal("40000.11000")

    def test_run_quote_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(1000, BarAggregation.VALUE, PriceType.MID)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = ValueBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        # Setup data
        wrangler = QuoteTickDataWrangler(AUDUSD_SIM)
        provider = TestDataProvider()
        ticks = wrangler.process(
            data=provider.read_csv_ticks("truefx-audusd-ticks.csv")[:10000],
            default_volume=1,
        )

        # Act
        for tick in ticks:
            aggregator.handle_quote_tick(tick)

        # Assert
        last_bar = handler[-1]
        assert len(handler) == 6
        assert last_bar.open == Price.from_str("0.671230")
        assert last_bar.high == Price.from_str("0.671330")
        assert last_bar.low == Price.from_str("0.670370")
        assert last_bar.close == Price.from_str("0.670630")
        assert last_bar.volume == Quantity.from_int(1_491)

    def test_run_trade_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        handler = []
        bar_spec = BarSpecification(10000, BarAggregation.VALUE, PriceType.LAST)
        bar_type = BarType(ETHUSDT_BITMEX.id, bar_spec)
        aggregator = ValueBarAggregator(
            ETHUSDT_BITMEX,
            bar_type,
            handler.append,
            Logger(TestClock()),
        )

        wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BITMEX)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("binance-ethusdt-trades.csv")[:1000])

        # Act
        for tick in ticks:
            aggregator.handle_trade_tick(tick)

        # Assert
        last_bar = handler[-1]
        assert len(handler) == 109
        assert last_bar.open == Price.from_str("423.19")
        assert last_bar.high == Price.from_str("423.25")
        assert last_bar.low == Price.from_str("423.19")
        assert last_bar.close == Price.from_str("423.25")
        assert last_bar.volume == Quantity.from_int(24)


class TestTimeBarAggregator:
    def test_instantiate_given_invalid_bar_spec_raises_value_error(self):
        # Arrange
        clock = TestClock()
        handler = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(100, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(instrument.id, bar_spec)

        # Act, Assert
        with pytest.raises(ValueError):
            TimeBarAggregator(
                instrument,
                bar_type,
                handler.append,
                clock,
                Logger(clock),
            )

    @pytest.mark.parametrize(
        ("bar_spec", "expected"),
        [
            [
                BarSpecification(10, BarAggregation.SECOND, PriceType.MID),
                pd.Timestamp(1970, 1, 1, 0, 0, 10).value,
            ],
            [
                BarSpecification(60, BarAggregation.SECOND, PriceType.MID),
                pd.Timestamp(1970, 1, 1, 0, 1, 0).value,
            ],
            [
                BarSpecification(300, BarAggregation.SECOND, PriceType.MID),
                pd.Timestamp(1970, 1, 1, 0, 5, 0).value,
            ],
            [
                BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
                pd.Timestamp(1970, 1, 1, 0, 1).value,
            ],
            [
                BarSpecification(60, BarAggregation.MINUTE, PriceType.MID),
                pd.Timestamp(1970, 1, 1, 1, 0).value,
            ],
            [
                BarSpecification(1, BarAggregation.HOUR, PriceType.MID),
                pd.Timestamp(1970, 1, 1, 1, 0).value,
            ],
            [
                BarSpecification(1, BarAggregation.DAY, PriceType.MID),
                pd.Timestamp(1970, 1, 2, 0, 0).value,
            ],
        ],
    )
    def test_instantiate_with_various_bar_specs(self, bar_spec, expected):
        # Arrange
        clock = TestClock()
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_type = BarType(instrument_id, bar_spec)

        # Act
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            Logger(clock),
        )

        # Assert
        assert aggregator.next_close_ns == expected

    def test_update_timer_with_test_clock_sends_single_bar_to_handler(self):
        # Arrange
        clock = TestClock()
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            Logger(clock),
        )

        tick1 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid=Price.from_str("1.00001"),
            ask=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid=Price.from_str("1.00002"),
            ask=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid=Price.from_str("1.00000"),
            ask=Price.from_str("1.00003"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=1 * 60 * 1_000_000_000,  # 1 minute in nanoseconds
            ts_init=1 * 60 * 1_000_000_000,  # 1 minute in nanoseconds
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)
        events = clock.advance_time(tick3.ts_event)
        events[0].handle()

        # Assert
        bar = handler[0]
        assert len(handler) == 1
        assert Price.from_str("1.000025") == bar.open
        assert Price.from_str("1.000035") == bar.high
        assert Price.from_str("1.000015") == bar.low
        assert Price.from_str("1.000015") == bar.close
        assert Quantity.from_int(3) == bar.volume
        assert bar.ts_init == 60_000_000_000

    @pytest.mark.parametrize(
        ("step", "aggregation"),
        [
            [
                1,
                BarAggregation.SECOND,
            ],
            [
                1000,
                BarAggregation.MILLISECOND,
            ],
        ],
    )
    def test_aggregation_for_same_sec_and_minute_intervals(self, step, aggregation):
        # Arrange - prepare data
        path = os.path.join(TEST_DATA_DIR, "binance-btcusdt-quotes.parquet")
        df_ticks = ParquetTickDataLoader.load(path)

        wrangler = QuoteTickDataWrangler(BTCUSDT_BINANCE)
        ticks = wrangler.process(df_ticks)
        clock = TestClock()
        clock.set_time(ticks[0].ts_init)
        handler = []

        bar_spec = BarSpecification(step, aggregation, PriceType.BID)
        bar_type = BarType(BTCUSDT_BINANCE.id, bar_spec, AggregationSource.INTERNAL)
        aggregator = TimeBarAggregator(
            BTCUSDT_BINANCE,
            bar_type,
            handler.append,
            clock,
            Logger(clock),
        )

        # Act - mini backtest loop
        for tick in ticks:
            aggregator.handle_quote_tick(tick)
            events = clock.advance_time(tick.ts_init)
            for event in events:
                event.handle()

        # Assert
        assert clock.timestamp_ns() == 1610064046674000128
        assert aggregator.interval_ns == 1_000_000_000
        assert aggregator.next_close_ns == 1610064047000000000
        assert handler[0].open == Price.from_str("39432.99")
        assert handler[0].high == Price.from_str("39435.66")
        assert handler[0].low == Price.from_str("39430.29")
        assert handler[0].close == Price.from_str("39435.66")
        assert handler[0].volume == Quantity.from_str("6.169286")
        assert handler[0].ts_event == 1610064002000000000
        assert handler[0].ts_init == 1610064002000000000

    def test_do_not_build_with_no_updates(self):
        # Arrange
        path = os.path.join(TEST_DATA_DIR, "binance-btcusdt-quotes.parquet")
        df_ticks = ParquetTickDataLoader.load(path)

        wrangler = QuoteTickDataWrangler(BTCUSDT_BINANCE)
        ticks = wrangler.process(df_ticks)

        clock = TestClock()
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(instrument_id, bar_spec)

        # Act
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            Logger(clock),
            build_with_no_updates=False,  # <-- set this True and test will fail
        )
        aggregator.handle_quote_tick(ticks[0])

        events = clock.advance_time(dt_to_unix_nanos(UNIX_EPOCH + timedelta(minutes=5)))
        for event in events:
            event.handle()

        # Assert
        assert len(handler) == 1  # <-- only 1 bar even after 5 minutes

    def test_timestamp_on_close_false_timestamps_ts_event_as_open(self):
        # Arrange
        path = os.path.join(TEST_DATA_DIR, "binance-btcusdt-quotes.parquet")
        df_ticks = ParquetTickDataLoader.load(path)

        wrangler = QuoteTickDataWrangler(BTCUSDT_BINANCE)
        ticks = wrangler.process(df_ticks)

        clock = TestClock()
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(instrument_id, bar_spec)

        # Act
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            Logger(clock),
            timestamp_on_close=False,  # <-- set this True and test will fail
        )
        aggregator.handle_quote_tick(ticks[0])

        events = clock.advance_time(dt_to_unix_nanos(UNIX_EPOCH + timedelta(minutes=2)))
        for event in events:
            event.handle()

        # Assert
        assert len(handler) == 2
        assert handler[0].ts_event == 0  # <-- bar open
        assert handler[0].ts_init == 60_000_000_000  # <-- bar close
        assert handler[1].ts_event == 60_000_000_000  # <-- bar open
        assert handler[1].ts_init == 120_000_000_000  # <-- bar close
