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

from datetime import timedelta
from decimal import ROUND_HALF_EVEN
from decimal import Decimal
from typing import Any

import pandas as pd
import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.common.component import TestClock
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.data.aggregation import BarBuilder
from nautilus_trader.data.aggregation import TickBarAggregator
from nautilus_trader.data.aggregation import TimeBarAggregator
from nautilus_trader.data.aggregation import ValueBarAggregator
from nautilus_trader.data.aggregation import VolumeBarAggregator
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BarIntervalType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.loaders import ParquetTickDataLoader
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


NANOSECONDS_IN_SECOND = 1_000_000_000
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


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
            ts_event=NANOSECONDS_IN_SECOND,
            ts_init=NANOSECONDS_IN_SECOND,
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
        assert bar.ts_init == NANOSECONDS_IN_SECOND
        assert builder.ts_last == NANOSECONDS_IN_SECOND

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
            ts_event=NANOSECONDS_IN_SECOND,
            ts_init=NANOSECONDS_IN_SECOND,
        )

        partial_bar2 = Bar(
            bar_type=bar_type,
            open=Price.from_str("2.00001"),
            high=Price.from_str("2.00010"),
            low=Price.from_str("2.00000"),
            close=Price.from_str("2.00002"),
            volume=Quantity.from_str("2"),
            ts_event=NANOSECONDS_IN_SECOND,
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
        assert builder.ts_last == NANOSECONDS_IN_SECOND

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

    def test_single_bar_update_results_in_expected_properties(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        input_bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00005"),
            volume=Quantity.from_str("1.5"),
            ts_event=NANOSECONDS_IN_SECOND,
            ts_init=NANOSECONDS_IN_SECOND,
        )

        # Act
        builder.update_bar(input_bar, input_bar.volume, input_bar.ts_event)

        # Assert
        assert builder.initialized
        assert builder.ts_last == NANOSECONDS_IN_SECOND
        assert builder.count == 1

        built_bar = builder.build_now()
        assert built_bar.open == Price.from_str("1.00001")
        assert built_bar.high == Price.from_str("1.00010")
        assert built_bar.low == Price.from_str("1.00000")
        assert built_bar.close == Price.from_str("1.00005")
        assert built_bar.volume == Quantity.from_str("1.5")

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

    def test_single_bar_update_when_timestamp_less_than_last_update_ignores(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        bar1 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00000"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00005"),
            volume=Quantity.from_str("1.0"),
            ts_event=1_000,
            ts_init=1_000,
        )
        builder.update_bar(bar1, bar1.volume, bar1.ts_event)

        bar2 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00011"),
            low=Price.from_str("1.00001"),
            close=Price.from_str("1.00006"),
            volume=Quantity.from_str("1.0"),
            ts_event=500,
            ts_init=500,
        )

        # Act
        builder.update_bar(bar2, bar2.volume, bar2.ts_event)

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

    def test_multiple_bar_updates_correctly_increments_count(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        # Act
        for i in range(5):
            bar = Bar(
                bar_type=bar_type,
                open=Price.from_str(f"1.0000{i}"),
                high=Price.from_str(f"1.0001{i}"),
                low=Price.from_str(f"1.0000{i}"),
                close=Price.from_str(f"1.0000{i+1}"),
                volume=Quantity.from_int(1),
                ts_event=1_000 * (i + 1),
                ts_init=1_000 * (i + 1),
            )
            builder.update_bar(bar, bar.volume, bar.ts_init)

        # Assert
        assert builder.count == 5
        assert builder.ts_last == 5_000

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
            NANOSECONDS_IN_SECOND,
        )

        # Act
        bar = builder.build_now()  # Also resets builder

        # Assert
        assert bar.open == Price.from_str("1.00001")
        assert bar.high == Price.from_str("1.00002")
        assert bar.low == Price.from_str("1.00000")
        assert bar.close == Price.from_str("1.00000")
        assert bar.volume == Quantity.from_str("4.0")
        assert bar.ts_init == NANOSECONDS_IN_SECOND
        assert builder.ts_last == NANOSECONDS_IN_SECOND
        assert builder.count == 0

    def test_build_when_received_bar_updates_returns_expected_bar(self):
        # Arrange
        bar_type = TestDataStubs.bartype_btcusdt_binance_100tick_last()
        builder = BarBuilder(BTCUSDT_BINANCE, bar_type)

        bar1 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00002"),
            low=Price.from_str("1.00001"),
            close=Price.from_str("1.00002"),
            volume=Quantity.from_str("1.0"),
            ts_event=0,
            ts_init=0,
        )
        builder.update_bar(bar1, bar1.volume, bar1.ts_init)

        bar2 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00002"),
            high=Price.from_str("1.00003"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00001"),
            volume=Quantity.from_str("1.5"),
            ts_event=500_000_000,
            ts_init=500_000_000,
        )
        builder.update_bar(bar2, bar2.volume, bar2.ts_init)

        bar3 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00002"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00000"),
            volume=Quantity.from_str("1.5"),
            ts_event=NANOSECONDS_IN_SECOND,
            ts_init=NANOSECONDS_IN_SECOND,
        )
        builder.update_bar(bar3, bar3.volume, bar3.ts_init)

        # Act
        bar = builder.build_now()  # Also resets builder

        # Assert
        assert bar.open == Price.from_str("1.00001")
        assert bar.high == Price.from_str("1.00003")
        assert bar.low == Price.from_str("1.00000")
        assert bar.close == Price.from_str("1.00000")
        assert bar.volume == Quantity.from_str("4.0")
        assert bar.ts_init == NANOSECONDS_IN_SECOND
        assert builder.ts_last == NANOSECONDS_IN_SECOND
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
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00001"),
            ask_price=Price.from_str("1.00004"),
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

    def test_handle_bar_when_count_below_threshold_updates(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = TickBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
        )

        bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00002"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00001"),
            volume=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_bar(bar)

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
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00001"),
            ask_price=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00002"),
            ask_price=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00003"),
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

    def test_handle_bar_when_count_at_threshold_sends_bar_to_handler(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = TickBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
        )

        bar1 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00002"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00001"),
            volume=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        bar2 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00002"),
            high=Price.from_str("1.00003"),
            low=Price.from_str("1.00001"),
            close=Price.from_str("1.00002"),
            volume=Quantity.from_int(1),
            ts_event=1000,
            ts_init=1000,
        )

        bar3 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00003"),
            high=Price.from_str("1.00004"),
            low=Price.from_str("1.00002"),
            close=Price.from_str("1.00003"),
            volume=Quantity.from_int(1),
            ts_event=2000,
            ts_init=2000,
        )

        # Act
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)
        aggregator.handle_bar(bar3)

        # Assert
        assert len(handler) == 1
        assert handler[0].open == Price.from_str("1.00001")
        assert handler[0].high == Price.from_str("1.00004")
        assert handler[0].low == Price.from_str("1.00000")
        assert handler[0].close == Price.from_str("1.00003")
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
        )

        # Set up data
        wrangler = QuoteTickDataWrangler(instrument)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("truefx/audusd-ticks.csv")[:1000])

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
        instrument = ETHUSDT_BINANCE
        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = TickBarAggregator(
            instrument,
            bar_type,
            handler.append,
        )

        wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BINANCE)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv")[:10000])

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
        assert last_bar.volume == Quantity.from_str("3141.91117")

    def test_run_bars_through_aggregator_results_in_expected_bars(self):
        handler = []
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)
        aggregator = TickBarAggregator(
            ETHUSDT_BINANCE,
            bar_type,
            handler.append,
        )

        bars = [
            Bar(
                bar_type,
                Price.from_str("100.00"),
                Price.from_str("101.00"),
                Price.from_str("99.00"),
                Price.from_str("100.50"),
                Quantity.from_str("10"),
                1000,
                1000,
            ),
            Bar(
                bar_type,
                Price.from_str("100.50"),
                Price.from_str("102.00"),
                Price.from_str("100.00"),
                Price.from_str("101.50"),
                Quantity.from_str("15"),
                2000,
                2000,
            ),
            Bar(
                bar_type,
                Price.from_str("101.50"),
                Price.from_str("103.00"),
                Price.from_str("101.00"),
                Price.from_str("102.50"),
                Quantity.from_str("20"),
                3000,
                3000,
            ),
        ]

        for bar in bars:
            aggregator.handle_bar(bar)

        last_bar = handler[-1]
        assert len(handler) == 1
        assert last_bar.open == Price.from_str("100.00")
        assert last_bar.high == Price.from_str("103.00")
        assert last_bar.low == Price.from_str("99.00")
        assert last_bar.close == Price.from_str("102.50")
        assert last_bar.volume == Quantity.from_str("45")


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
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00001"),
            ask_price=Price.from_str("1.00004"),
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

    def test_handle_bar_when_volume_below_threshold_updates(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(100000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = VolumeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
        )

        bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00002"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00001"),
            volume=Quantity.from_int(50000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_bar(bar)

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
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00001"),
            ask_price=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(3_000),
            ask_size=Quantity.from_int(2_000),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00002"),
            ask_price=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(4_000),
            ask_size=Quantity.from_int(2_000),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00003"),
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

    def test_handle_bar_when_volume_at_threshold_sends_bar_to_handler(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(100000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = VolumeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
        )

        bar1 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00002"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00001"),
            volume=Quantity.from_int(60000),
            ts_event=0,
            ts_init=0,
        )

        bar2 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00002"),
            high=Price.from_str("1.00003"),
            low=Price.from_str("1.00001"),
            close=Price.from_str("1.00002"),
            volume=Quantity.from_int(40000),
            ts_event=1000,
            ts_init=1000,
        )

        # Act
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)

        # Assert
        assert len(handler) == 1
        assert handler[0].open == Price.from_str("1.00001")
        assert handler[0].high == Price.from_str("1.00003")
        assert handler[0].low == Price.from_str("1.00000")
        assert handler[0].close == Price.from_str("1.00002")
        assert handler[0].volume == Quantity.from_int(100000)

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
        )

        tick1 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00001"),
            ask_price=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(2_000),
            ask_size=Quantity.from_int(2_000),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00002"),
            ask_price=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(3_000),
            ask_size=Quantity.from_int(3_000),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00003"),
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

    def test_handle_bar_when_volume_beyond_threshold_sends_bars_to_handler(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(100000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = VolumeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
        )

        bar1 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00001"),
            high=Price.from_str("1.00002"),
            low=Price.from_str("1.00000"),
            close=Price.from_str("1.00001"),
            volume=Quantity.from_int(80000),
            ts_event=0,
            ts_init=0,
        )

        bar2 = Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00002"),
            high=Price.from_str("1.00003"),
            low=Price.from_str("1.00001"),
            close=Price.from_str("1.00002"),
            volume=Quantity.from_int(140000),
            ts_event=1000,
            ts_init=1000,
        )

        # Act
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)

        # Assert
        assert len(handler) == 2
        assert handler[0].open == Price.from_str("1.00001")
        assert handler[0].high == Price.from_str("1.00003")
        assert handler[0].low == Price.from_str("1.00000")
        assert handler[0].close == Price.from_str("1.00002")
        assert handler[0].volume == Quantity.from_int(100000)
        assert handler[1].open == Price.from_str("1.00002")
        assert handler[1].high == Price.from_str("1.00003")
        assert handler[1].low == Price.from_str("1.00001")
        assert handler[1].close == Price.from_str("1.00002")
        assert handler[1].volume == Quantity.from_int(100000)

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
        )

        # Set up data
        wrangler = QuoteTickDataWrangler(instrument)
        provider = TestDataProvider()
        ticks = wrangler.process(
            data=provider.read_csv_ticks("truefx/audusd-ticks.csv")[:10000],
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
        instrument = ETHUSDT_BINANCE
        bar_spec = BarSpecification(1000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        aggregator = VolumeBarAggregator(
            instrument,
            bar_type,
            handler.append,
        )

        wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BINANCE)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv")[:10000])

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

    def test_run_bars_through_aggregator_results_in_expected_bars(self):
        handler = []
        bar_spec = BarSpecification(30, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)
        aggregator = VolumeBarAggregator(
            ETHUSDT_BINANCE,
            bar_type,
            handler.append,
        )

        bars = [
            Bar(
                bar_type,
                Price.from_str("100.00"),
                Price.from_str("101.00"),
                Price.from_str("99.00"),
                Price.from_str("100.50"),
                Quantity.from_str("10"),
                1000,
                1000,
            ),
            Bar(
                bar_type,
                Price.from_str("100.50"),
                Price.from_str("102.00"),
                Price.from_str("100.00"),
                Price.from_str("101.50"),
                Quantity.from_str("15"),
                2000,
                2000,
            ),
            Bar(
                bar_type,
                Price.from_str("101.50"),
                Price.from_str("103.00"),
                Price.from_str("101.00"),
                Price.from_str("102.50"),
                Quantity.from_str("20"),
                3000,
                3000,
            ),
        ]

        for bar in bars:
            aggregator.handle_bar(bar)

        last_bar = handler[-1]
        assert len(handler) == 1
        assert last_bar.open == Price.from_str("100.00")
        assert last_bar.high == Price.from_str("103.00")
        assert last_bar.low == Price.from_str("99.00")
        assert last_bar.close == Price.from_str("102.50")
        assert last_bar.volume == Quantity.from_str("30")


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
        )

        tick1 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00001"),
            ask_price=Price.from_str("1.00004"),
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

    def test_handle_bar_when_value_below_threshold_updates(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.LAST)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = ValueBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
        )

        bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("15000.00"),
            high=Price.from_str("15001.00"),
            low=Price.from_str("14999.00"),
            close=Price.from_str("15000.00"),
            volume=Quantity.from_str("3.5"),
            ts_event=0,
            ts_init=0,
        )

        # Act
        aggregator.handle_bar(bar)

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
        )

        tick1 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00001"),
            ask_price=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(20_000),
            ask_size=Quantity.from_int(20_000),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00002"),
            ask_price=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(60_000),
            ask_size=Quantity.from_int(20_000),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00003"),
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
        expected = Decimal("40000.11")
        assert aggregator.get_cumulative_value().quantize(expected, ROUND_HALF_EVEN) == expected

    def test_handle_bar_when_value_beyond_threshold_sends_bars_to_handler(self):
        # Arrange
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.LAST)
        bar_type = BarType(instrument_id, bar_spec)
        aggregator = ValueBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
        )

        bar1 = Bar(
            bar_type=bar_type,
            open=Price.from_str("20.00001"),
            high=Price.from_str("20.00010"),
            low=Price.from_str("20.00000"),
            close=Price.from_str("20.00005"),
            volume=Quantity.from_str("3000.00"),
            ts_event=0,
            ts_init=0,
        )

        bar2 = Bar(
            bar_type=bar_type,
            open=Price.from_str("20.00006"),
            high=Price.from_str("20.00015"),
            low=Price.from_str("20.00002"),
            close=Price.from_str("20.00010"),
            volume=Quantity.from_str("4000.00"),
            ts_event=30_000_000_000,
            ts_init=30_000_000_000,
        )

        bar3 = Bar(
            bar_type=bar_type,
            open=Price.from_str("20.00011"),
            high=Price.from_str("20.00020"),
            low=Price.from_str("20.00000"),
            close=Price.from_str("20.00015"),
            volume=Quantity.from_str("5000.00"),
            ts_event=60_000_000_000,
            ts_init=60_000_000_000,
        )

        # Act
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)
        aggregator.handle_bar(bar3)

        # Assert
        assert len(handler) == 2
        assert handler[0].open == Price.from_str("20.00001")
        assert handler[0].high == Price.from_str("20.00015")
        assert handler[0].low == Price.from_str("20.00000")
        assert handler[0].close == Price.from_str("20.00010")
        assert handler[0].volume == Quantity.from_str("5000.00")
        assert handler[1].open == Price.from_str("20.00006")
        assert handler[1].high == Price.from_str("20.00020")
        assert handler[1].low == Price.from_str("20.00000")
        assert handler[1].close == Price.from_str("20.00015")
        assert handler[1].volume == Quantity.from_str("5000.00")
        expected = Decimal("40001.11")
        assert (
            aggregator.get_cumulative_value().quantize(expected, rounding=ROUND_HALF_EVEN)
            == expected
        )

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
        )

        # Set up data
        wrangler = QuoteTickDataWrangler(AUDUSD_SIM)
        provider = TestDataProvider()
        ticks = wrangler.process(
            data=provider.read_csv_ticks("truefx/audusd-ticks.csv")[:10000],
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
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)
        aggregator = ValueBarAggregator(
            ETHUSDT_BINANCE,
            bar_type,
            handler.append,
        )

        wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BINANCE)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv")[:1000])

        # Act
        for tick in ticks:
            aggregator.handle_trade_tick(tick)

        # Assert
        last_bar = handler[-1]
        assert len(handler) == 112
        assert last_bar.open == Price.from_str("423.19")
        assert last_bar.high == Price.from_str("423.25")
        assert last_bar.low == Price.from_str("423.19")
        assert last_bar.close == Price.from_str("423.25")
        assert last_bar.volume == Quantity.from_str("23.62824")

    def test_run_bars_through_aggregator_results_in_expected_bars(self):
        handler = []
        bar_spec = BarSpecification(3000, BarAggregation.VALUE, PriceType.LAST)
        bar_type = BarType(ETHUSDT_BINANCE.id, bar_spec)
        aggregator = ValueBarAggregator(
            ETHUSDT_BINANCE,
            bar_type,
            handler.append,
        )

        bars = [
            Bar(
                bar_type,
                Price.from_str("100.00"),
                Price.from_str("101.00"),
                Price.from_str("99.00"),
                Price.from_str("100.50"),
                Quantity.from_str("10"),
                1000,
                1000,
            ),
            Bar(
                bar_type,
                Price.from_str("100.50"),
                Price.from_str("102.00"),
                Price.from_str("100.00"),
                Price.from_str("101.50"),
                Quantity.from_str("15"),
                2000,
                2000,
            ),
            Bar(
                bar_type,
                Price.from_str("101.50"),
                Price.from_str("103.00"),
                Price.from_str("101.00"),
                Price.from_str("102.50"),
                Quantity.from_str("20"),
                3000,
                3000,
            ),
        ]

        for bar in bars:
            aggregator.handle_bar(bar)

        last_bar = handler[-1]
        assert len(handler) == 1
        assert last_bar.open == Price.from_str("100.00")
        assert last_bar.high == Price.from_str("103.00")
        assert last_bar.low == Price.from_str("99.00")
        assert last_bar.close == Price.from_str("102.50")
        assert last_bar.volume == Quantity.from_str("30")


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
            )

    @pytest.mark.parametrize(
        ("bar_aggregation", "step", "time_bars_origin_offset"),
        [
            [BarAggregation.MILLISECOND, 20, pd.DateOffset(microseconds=1)],
            [BarAggregation.MILLISECOND, 20, pd.Timedelta(seconds=1)],
            [BarAggregation.MILLISECOND, 20, pd.Timedelta(seconds=-1)],
            [BarAggregation.SECOND, 3, pd.DateOffset(microseconds=1)],
            [BarAggregation.SECOND, 12, pd.Timedelta(seconds=12)],
            [BarAggregation.SECOND, 6, pd.Timedelta(seconds=-6)],
            [BarAggregation.MINUTE, 5, pd.DateOffset(microseconds=1)],
            [BarAggregation.MINUTE, 30, pd.Timedelta(minutes=30)],
            [BarAggregation.MINUTE, 30, pd.Timedelta(minutes=-30)],
            [BarAggregation.MINUTE, 30, pd.Timedelta(minutes=30, seconds=1)],
            [BarAggregation.MINUTE, 1, pd.Timedelta(minutes=1)],
            [BarAggregation.HOUR, 4, pd.DateOffset(microseconds=1)],
            [BarAggregation.HOUR, 2, pd.Timedelta(hours=2)],
            [BarAggregation.HOUR, 2, pd.Timedelta(hours=2, microseconds=1)],
            [BarAggregation.HOUR, 12, pd.Timedelta(hours=-12)],
            [BarAggregation.DAY, 1, pd.DateOffset(microseconds=1)],
            [BarAggregation.DAY, 1, pd.Timedelta(days=1)],
            [BarAggregation.DAY, 1, pd.Timedelta(days=-1)],
            [BarAggregation.WEEK, 1, pd.DateOffset(microseconds=1)],
            [BarAggregation.WEEK, 1, pd.Timedelta(weeks=1)],
            [BarAggregation.WEEK, 1, pd.Timedelta(weeks=-1)],
            [BarAggregation.WEEK, 1, pd.Timedelta(weeks=1, microseconds=1)],
            [BarAggregation.MONTH, 2, pd.Timedelta(milliseconds=-1)],
            [BarAggregation.MONTH, 3, pd.Timedelta(days=28, microseconds=1)],
            [BarAggregation.MONTH, 3, pd.Timedelta(days=29)],
        ],
    )
    def test_instantiate_given_invalid_time_bars_origin_raises_value_error(
        self,
        bar_aggregation: BarAggregation,
        step: int,
        time_bars_origin_offset: pd.Timedelta,
    ):
        # Arrange
        start_time_ns = pd.Timestamp(1990, 1, 1).value

        clock = TestClock()
        clock.set_time(start_time_ns)

        handler: list[Bar] = []
        instrument = AUDUSD_SIM
        bar_spec = BarSpecification(step, bar_aggregation, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec, aggregation_source=AggregationSource.INTERNAL)

        # Act, Assert
        with pytest.raises(ValueError):
            TimeBarAggregator(
                instrument,
                bar_type,
                handler.append,
                clock,
                time_bars_origin_offset=time_bars_origin_offset,
            )

    # The method is so large, because it can't be split.
    # ruff: noqa: C901
    @staticmethod
    def get_data_test_instantiate_and_update_timer_with_various_bar_specs_data(
        month_test: bool,
        batch_test: bool,
    ) -> list[list[Any]]:
        if month_test is False:
            data = [
                [
                    BarSpecification(1, BarAggregation.MILLISECOND, PriceType.MID),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 0),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 1000),
                ],
                [
                    BarSpecification(10, BarAggregation.MILLISECOND, PriceType.LAST),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 0),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 10000),
                ],
                [
                    BarSpecification(5, BarAggregation.MILLISECOND, PriceType.BID),
                    pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 5010),
                ],
                [
                    BarSpecification(5, BarAggregation.MILLISECOND, PriceType.BID),
                    pd.Timedelta(microseconds=-4_990),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 5010),
                ],
                [
                    BarSpecification(5, BarAggregation.MILLISECOND, PriceType.ASK),
                    pd.Timedelta(microseconds=4_990),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 4_990),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 4_990 + 5_000),
                ],
                [
                    BarSpecification(5, BarAggregation.MILLISECOND, PriceType.LAST),
                    pd.Timedelta(microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 4_990),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0, 4_990 + 5_000),
                ],
                [
                    BarSpecification(15, BarAggregation.SECOND, PriceType.ASK),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0, 0),
                    pd.Timestamp(1990, 1, 1, 0, 0, 15),
                ],
                [
                    BarSpecification(10, BarAggregation.SECOND, PriceType.ASK),
                    pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 10) + pd.Timedelta(microseconds=10),
                ],
                [
                    BarSpecification(10, BarAggregation.SECOND, PriceType.LAST),
                    pd.Timedelta(microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0) + pd.Timedelta(seconds=10, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 10) + pd.Timedelta(seconds=10, microseconds=-10),
                ],
                [
                    BarSpecification(10, BarAggregation.SECOND, PriceType.MID),
                    pd.Timedelta(seconds=-10, microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 10) + pd.Timedelta(microseconds=10),
                ],
                [
                    BarSpecification(10, BarAggregation.SECOND, PriceType.LAST),
                    pd.Timedelta(seconds=10, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 0) + pd.Timedelta(seconds=10, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0, 10) + pd.Timedelta(seconds=10, microseconds=-10),
                ],
                [
                    BarSpecification(60, BarAggregation.SECOND, PriceType.BID),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0, 0),
                    pd.Timestamp(1990, 1, 1, 0, 1, 0),
                ],
                [
                    BarSpecification(12, BarAggregation.SECOND, PriceType.MID),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0, 0),
                    pd.Timestamp(1990, 1, 1, 0, 0, 12),
                ],
                [
                    BarSpecification(1, BarAggregation.MINUTE, PriceType.ASK),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1990, 1, 1, 0, 1),
                ],
                [
                    BarSpecification(2, BarAggregation.MINUTE, PriceType.LAST),
                    pd.Timedelta(minutes=2, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(minutes=2, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 2) + pd.Timedelta(minutes=2, microseconds=-10),
                ],
                [
                    BarSpecification(2, BarAggregation.MINUTE, PriceType.LAST),
                    pd.Timedelta(minutes=-2, microseconds=10),
                    (pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(microseconds=10)),
                    (pd.Timestamp(1990, 1, 1, 0, 2) + pd.Timedelta(microseconds=10)),
                ],
                [
                    BarSpecification(2, BarAggregation.MINUTE, PriceType.LAST),
                    pd.Timedelta(microseconds=10),
                    (pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(microseconds=10)),
                    (pd.Timestamp(1990, 1, 1, 0, 2) + pd.Timedelta(microseconds=10)),
                ],
                [
                    BarSpecification(2, BarAggregation.MINUTE, PriceType.ASK),
                    pd.Timedelta(microseconds=-10),
                    (pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(minutes=2, microseconds=-10)),
                    (pd.Timestamp(1990, 1, 1, 0, 2) + pd.Timedelta(minutes=2, microseconds=-10)),
                ],
                [
                    BarSpecification(10, BarAggregation.MINUTE, PriceType.BID),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1990, 1, 1, 0, 10),
                ],
                [
                    BarSpecification(60, BarAggregation.MINUTE, PriceType.MID),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1990, 1, 1, 1, 0),
                ],
                [
                    BarSpecification(1, BarAggregation.HOUR, PriceType.BID),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1990, 1, 1, 1, 0),
                ],
                [
                    BarSpecification(6, BarAggregation.HOUR, PriceType.ASK),
                    pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 6, 0) + pd.Timedelta(microseconds=10),
                ],
                [
                    BarSpecification(6, BarAggregation.HOUR, PriceType.ASK),
                    pd.Timedelta(hours=6, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(hours=6, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 6, 0) + pd.Timedelta(hours=6, microseconds=-10),
                ],
                [
                    BarSpecification(6, BarAggregation.HOUR, PriceType.ASK),
                    pd.Timedelta(microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(hours=6, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 6, 0) + pd.Timedelta(hours=6, microseconds=-10),
                ],
                [
                    BarSpecification(6, BarAggregation.HOUR, PriceType.ASK),
                    pd.Timedelta(hours=-6, microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 6, 0) + pd.Timedelta(microseconds=10),
                ],
                [
                    BarSpecification(12, BarAggregation.HOUR, PriceType.LAST),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1990, 1, 1, 12, 0),
                ],
                [
                    BarSpecification(1, BarAggregation.DAY, PriceType.ASK),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1990, 1, 2, 0, 0),
                ],
                [
                    BarSpecification(1, BarAggregation.DAY, PriceType.LAST),
                    pd.Timedelta(hours=24, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(hours=24, microseconds=-10),
                    pd.Timestamp(1990, 1, 2, 0, 0) + pd.Timedelta(hours=24, microseconds=-10),
                ],
                [
                    BarSpecification(1, BarAggregation.DAY, PriceType.LAST),
                    pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 2, 0, 0) + pd.Timedelta(microseconds=10),
                ],
                [
                    BarSpecification(1, BarAggregation.DAY, PriceType.LAST),
                    pd.Timedelta(microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(hours=24, microseconds=-10),
                    pd.Timestamp(1990, 1, 2, 0, 0) + pd.Timedelta(hours=24, microseconds=-10),
                ],
                [
                    BarSpecification(1, BarAggregation.DAY, PriceType.LAST),
                    pd.Timedelta(hours=-24, microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 2, 0, 0) + pd.Timedelta(microseconds=10),
                ],
                # Based on the calendar
                [
                    BarSpecification(1, BarAggregation.WEEK, PriceType.LAST),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1990, 1, 8, 0, 0),
                ],
                # Based on the calendar
                [
                    BarSpecification(1, BarAggregation.WEEK, PriceType.MID),
                    pd.Timedelta(days=7, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(days=7, microseconds=-10),
                    pd.Timestamp(1990, 1, 8, 0) + pd.Timedelta(days=7, microseconds=-10),
                ],
                # Based on the calendar
                [
                    BarSpecification(1, BarAggregation.WEEK, PriceType.MID),
                    pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 8, 0, 0) + pd.Timedelta(microseconds=10),
                ],
                # Based on the calendar
                [
                    BarSpecification(1, BarAggregation.WEEK, PriceType.MID),
                    pd.Timedelta(days=-7, microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 8, 0, 0) + pd.Timedelta(microseconds=10),
                ],
                # Based on the calendar
                [
                    BarSpecification(1, BarAggregation.WEEK, PriceType.MID),
                    pd.Timedelta(microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(days=7, microseconds=-10),
                    pd.Timestamp(1990, 1, 8, 0, 0) + pd.Timedelta(days=7, microseconds=-10),
                ],
            ]
        else:
            data = [
                # TODO: Test time_bars_origin with DateOffset for MONTH
                [
                    BarSpecification(12, BarAggregation.MONTH, PriceType.MID),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1991, 1, 1, 0, 0),
                    pd.Timestamp(1992, 1, 1, 0, 0),
                ],
                [
                    BarSpecification(1, BarAggregation.MONTH, PriceType.BID),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1990, 2, 1, 0, 0),
                    pd.Timestamp(1990, 3, 1, 0, 0),
                ],
                [
                    BarSpecification(6, BarAggregation.MONTH, PriceType.LAST),
                    None,
                    pd.Timestamp(1990, 1, 1, 0, 0),
                    pd.Timestamp(1990, 7, 1, 0, 0),
                    pd.Timestamp(1991, 1, 1, 0, 0),
                ],
                [
                    BarSpecification(2, BarAggregation.MONTH, PriceType.ASK),
                    pd.Timedelta(days=28, microseconds=-10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(days=28, microseconds=-10),
                    pd.Timestamp(1990, 3, 1, 0, 0) + pd.Timedelta(days=28, microseconds=-10),
                    pd.Timestamp(1990, 5, 1, 0, 0) + pd.Timedelta(days=28, microseconds=-10),
                ],
                [
                    BarSpecification(2, BarAggregation.MONTH, PriceType.LAST),
                    pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 1, 1, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 3, 1, 0, 0) + pd.Timedelta(microseconds=10),
                    pd.Timestamp(1990, 5, 1, 0, 0) + pd.Timedelta(microseconds=10),
                ],
            ]

        ret: list[list[Any]] = []

        for test_case in data:
            bar_spec: BarSpecification = test_case[0]
            time_bars_origin_offset = test_case[1]
            expected1 = test_case[2]
            expected2 = test_case[3]

            if (
                bar_spec.price_type == PriceType.BID
                or bar_spec.price_type == PriceType.ASK
                or bar_spec.price_type == PriceType.MID
            ):
                _tick1 = QuoteTick(
                    instrument_id=AUDUSD_SIM.id,
                    bid_price=Price.from_str("1.00005"),
                    ask_price=Price.from_str("1.00005"),
                    bid_size=Quantity.from_int(2),
                    ask_size=Quantity.from_int(2),
                    ts_event=expected1.value,
                    ts_init=expected1.value,
                )

                _shifted_tick1 = QuoteTick(
                    instrument_id=AUDUSD_SIM.id,
                    bid_price=Price.from_str("1.00005"),
                    ask_price=Price.from_str("1.00005"),
                    bid_size=Quantity.from_int(2),
                    ask_size=Quantity.from_int(2),
                    ts_event=expected1.value - 1,
                    ts_init=expected1.value - 1,
                )

                _tick2 = QuoteTick(
                    instrument_id=AUDUSD_SIM.id,
                    bid_price=Price.from_str("0.99999"),
                    ask_price=Price.from_str("0.99999"),
                    bid_size=Quantity.from_int(1),
                    ask_size=Quantity.from_int(1),
                    ts_event=expected2.value,
                    ts_init=expected2.value,
                )

                _shifted_tick2 = QuoteTick(
                    instrument_id=AUDUSD_SIM.id,
                    bid_price=Price.from_str("0.99999"),
                    ask_price=Price.from_str("0.99999"),
                    bid_size=Quantity.from_int(1),
                    ask_size=Quantity.from_int(1),
                    ts_event=expected2.value - 1,
                    ts_init=expected2.value - 1,
                )

            elif bar_spec.price_type == PriceType.LAST:
                _tick1 = TradeTick(
                    instrument_id=AUDUSD_SIM.id,
                    price=Price.from_str("1.00005"),
                    size=Quantity.from_int(2),
                    ts_event=expected1.value,
                    ts_init=expected1.value,
                    aggressor_side=AggressorSide.BUYER,
                    trade_id=TradeId("123457"),
                )

                _shifted_tick1 = TradeTick(
                    instrument_id=AUDUSD_SIM.id,
                    price=Price.from_str("1.00005"),
                    size=Quantity.from_int(2),
                    ts_event=expected1.value - 1000,
                    ts_init=expected1.value - 1000,
                    aggressor_side=AggressorSide.BUYER,
                    trade_id=TradeId("123457"),
                )

                _tick2 = TradeTick(
                    instrument_id=AUDUSD_SIM.id,
                    price=Price.from_str("0.99999"),
                    size=Quantity.from_int(1),
                    ts_event=expected2.value,
                    ts_init=expected2.value,
                    aggressor_side=AggressorSide.SELLER,
                    trade_id=TradeId("123459"),
                )

                _shifted_tick2 = TradeTick(
                    instrument_id=AUDUSD_SIM.id,
                    price=Price.from_str("0.99999"),
                    size=Quantity.from_int(1),
                    ts_event=expected2.value - 1000,
                    ts_init=expected2.value - 1000,
                    aggressor_side=AggressorSide.SELLER,
                    trade_id=TradeId("123459"),
                )

            else:
                raise ValueError(f"{bar_spec.PriceType} is not supported in the test")

            for tick1 in [None, _tick1, _shifted_tick1]:
                for tick2 in [None, _tick2, _shifted_tick2]:
                    ret.append([*test_case, tick1, tick2, pd.Timedelta(0)])
                    ret.append([*test_case, tick1, tick2, pd.Timedelta(-1)])

                    if time_bars_origin_offset is not None:
                        diff = (
                            abs(time_bars_origin_offset.value) - pd.Timedelta(microseconds=10).value
                        )
                        if diff > 0:
                            ret.append([*test_case, tick1, tick2, pd.Timedelta(1)])

        if batch_test:
            ret_orig = ret
            ret = []

            for case in ret_orig:
                for time_shift2 in [pd.Timedelta(0), pd.Timedelta(1)]:
                    ret.append([*case, time_shift2])

        return ret

    # TODO (in Rust): Test precision of time_bars_origin to nanoseconds
    # TODO: Test MARK price when implemented
    @pytest.mark.parametrize(
        (
            "bar_spec",
            "time_bars_origin_offset",
            "expected1",
            "expected2",
            "tick1",
            "tick2",
            "time_shift",
        ),
        get_data_test_instantiate_and_update_timer_with_various_bar_specs_data(False, False),
    )
    def test_instantiate_and_update_timer_with_various_bar_specs(
        self,
        bar_spec: BarSpecification,
        time_bars_origin_offset: pd.Timedelta,
        expected1: pd.Timestamp,
        expected2: pd.Timestamp,
        tick1: QuoteTick | TradeTick | None,
        tick2: QuoteTick | TradeTick | None,
        time_shift: pd.Timedelta,
    ):
        """
        Test TimeBarAggregator timer functionality with comprehensive bar specification
        coverage.

        This parameterized test validates that the TimeBarAggregator correctly:

        1. **Timer Scheduling**: Calculates and schedules the next bar close times accurately for various
           time-based bar specifications (milliseconds, seconds, minutes, hours, days, weeks).

        2. **Time Origin Offset Handling**: Properly applies time_bars_origin_offset to shift bar
           boundaries while maintaining correct intervals for all supported time units.

        3. **Bar Generation Logic**: Generates bars at the exact expected timestamps when timer events
           fire, regardless of when ticks arrive (before, at, or after bar boundaries).

        4. **Price Type Support**: Handles different price types (BID, ASK, MID, LAST) with appropriate
           tick types (QuoteTick for BID/ASK/MID, TradeTick for LAST).

        5. **Edge Case Handling**: Correctly processes scenarios with:
           - No ticks received (empty bars with zero volume)
           - Ticks arriving before the first bar boundary
           - Ticks arriving exactly at bar boundaries
           - Various time shifts and origin offsets

        6. **Bar Content Accuracy**: Ensures generated bars have correct OHLCV data, timestamps
           (ts_event, ts_init), and metadata (bar_type, aggregation_source).

        7. **Timer Progression**: Verifies that after each bar is generated, the aggregator correctly
           calculates the next bar close time, maintaining consistent intervals.

        The test uses a comprehensive parameter matrix covering different time units, offset scenarios,
        and tick combinations to ensure the TimeBarAggregator behaves consistently across all supported
        time-based bar specifications in live trading mode.

        """
        # Arrange
        start_time_ns = (pd.Timestamp(1990, 1, 1) + time_shift).value

        clock = TestClock()
        clock.set_time(start_time_ns)

        handler: list[Bar] = []
        instrument_id = TestIdStubs.audusd_id()
        bar_type = BarType(instrument_id, bar_spec, AggregationSource.INTERNAL)

        # Act
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            time_bars_origin_offset=time_bars_origin_offset,
        )

        initial_next_close = aggregator.next_close_ns

        def handle_tick(tick: QuoteTick | TradeTick | None):
            if type(tick) is QuoteTick:
                aggregator.handle_quote_tick(tick)
            elif type(tick) is TradeTick:
                aggregator.handle_trade_tick(tick)

        handle_tick(tick1)
        events = clock.advance_time(initial_next_close)
        events[0].handle()

        handle_tick(tick2)
        second_next_close = aggregator.next_close_ns
        events = clock.advance_time(second_next_close)
        events[0].handle()

        # Assert
        interval = expected2 - expected1
        expected3 = expected2 + interval

        assert pd.Timestamp(initial_next_close) == expected1
        assert pd.Timestamp(second_next_close) == expected2
        assert pd.Timestamp(aggregator.next_close_ns) == expected3

        def assert_bar(bar: Bar, price: Price, volume: Quantity | int, expected_time: pd.Timestamp):
            assert bar.volume == volume

            assert pd.Timestamp(bar.ts_init) == expected_time
            assert pd.Timestamp(bar.ts_event) == expected_time

            assert bar.high == price
            assert bar.low == price
            assert bar.open == price
            assert bar.close == price

            assert bar.bar_type.instrument_id == AUDUSD_SIM.id
            assert bar.bar_type.spec == bar_spec
            assert bar.bar_type.aggregation_source == AggregationSource.INTERNAL

        price1 = Price.from_str("1.00005")
        volume1 = Quantity.from_int(2)
        price2 = Price.from_str("0.99999")
        volume2 = Quantity.from_int(1)

        if tick1 is not None and tick2 is not None:
            assert len(handler) == 2
            assert_bar(handler[0], price1, volume1, expected1)
            assert_bar(handler[1], price2, volume2, expected2)
        elif tick1 is not None:
            assert len(handler) == 2
            assert_bar(handler[0], price1, volume1, expected1)
            assert_bar(handler[1], price1, 0, expected2)
        elif tick2 is not None:
            assert len(handler) == 1
            assert_bar(handler[0], price2, volume2, expected2)
        else:
            assert len(handler) == 0

    # TODO (in Rust): Test precision of time_bars_origin to nanoseconds
    # TODO: Test MARK price when implemented
    @pytest.mark.parametrize(
        (
            "bar_spec",
            "time_bars_origin_offset",
            "expected1",
            "expected2",
            "expected3",
            "tick1",
            "tick2",
            "time_shift",
        ),
        get_data_test_instantiate_and_update_timer_with_various_bar_specs_data(True, False),
    )
    def test_instantiate_and_update_month_timer_with_various_bar_specs(
        self,
        bar_spec: BarSpecification,
        time_bars_origin_offset: pd.Timedelta,
        expected1: pd.Timestamp,
        expected2: pd.Timestamp,
        expected3: pd.Timestamp,
        tick1: QuoteTick | TradeTick | None,
        tick2: QuoteTick | TradeTick | None,
        time_shift: pd.Timedelta,
    ):
        """
        Same as test_instantiate_and_update_timer_with_various_bar_specs, only for
        MONTH.
        """
        # Arrange
        start_time_ns = (pd.Timestamp(1990, 1, 1) + time_shift).value

        clock = TestClock()
        clock.set_time(start_time_ns)

        handler: list[Bar] = []
        instrument_id = TestIdStubs.audusd_id()
        bar_type = BarType(instrument_id, bar_spec, AggregationSource.INTERNAL)

        # Act
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            time_bars_origin_offset=time_bars_origin_offset,
        )

        initial_next_close = aggregator.next_close_ns

        def handle_tick(tick: QuoteTick | TradeTick | None):
            if type(tick) is QuoteTick:
                aggregator.handle_quote_tick(tick)
            elif type(tick) is TradeTick:
                aggregator.handle_trade_tick(tick)

        handle_tick(tick1)
        events = clock.advance_time(initial_next_close)
        events[0].handle()

        handle_tick(tick2)
        second_next_close = aggregator.next_close_ns
        events = clock.advance_time(second_next_close)
        events[0].handle()

        allowed_diff_ns = 10

        # Assert
        assert abs(pd.Timestamp(initial_next_close).value - expected1.value) < allowed_diff_ns
        assert abs(pd.Timestamp(second_next_close).value - expected2.value) < allowed_diff_ns
        assert abs(pd.Timestamp(aggregator.next_close_ns).value - expected3.value) < allowed_diff_ns

        def assert_bar(bar: Bar, price: Price, volume: Quantity | int, expected_time: pd.Timestamp):
            assert bar.volume == volume

            assert abs(pd.Timestamp(bar.ts_init).value - expected_time.value) < allowed_diff_ns
            assert abs(pd.Timestamp(bar.ts_event).value - expected_time.value) < allowed_diff_ns

            assert bar.high == price
            assert bar.low == price
            assert bar.open == price
            assert bar.close == price

            assert bar.bar_type.instrument_id == AUDUSD_SIM.id
            assert bar.bar_type.spec == bar_spec
            assert bar.bar_type.aggregation_source == AggregationSource.INTERNAL

        price1 = Price.from_str("1.00005")
        volume1 = Quantity.from_int(2)
        price2 = Price.from_str("0.99999")
        volume2 = Quantity.from_int(1)

        if tick1 is not None and tick2 is not None:
            assert len(handler) == 2
            assert_bar(handler[0], price1, volume1, expected1)
            assert_bar(handler[1], price2, volume2, expected2)
        elif tick1 is not None:
            assert len(handler) == 2
            assert_bar(handler[0], price1, volume1, expected1)
            assert_bar(handler[1], price1, 0, expected2)
        elif tick2 is not None:
            assert len(handler) == 1
            assert_bar(handler[0], price2, volume2, expected2)
        else:
            assert len(handler) == 0

    # TODO (in Rust): Test precision of time_bars_origin to nanoseconds
    # TODO: Test MARK price when implemented
    @pytest.mark.parametrize(
        (
            "bar_spec",
            "time_bars_origin_offset",
            "expected1",
            "expected2",
            "tick1",
            "tick2",
            "time_shift",
            "time_shift2",
        ),
        get_data_test_instantiate_and_update_timer_with_various_bar_specs_data(False, True),
    )
    def test_instantiate_and_batch_process_with_various_bar_specs(
        self,
        bar_spec: BarSpecification,
        time_bars_origin_offset: pd.Timedelta,
        expected1: pd.Timestamp,
        expected2: pd.Timestamp,
        tick1: QuoteTick | TradeTick | None,
        tick2: QuoteTick | TradeTick | None,
        time_shift: pd.Timedelta,
        time_shift2: pd.Timedelta,
    ):
        # Arrange
        start_time_ns = (expected2 + time_shift2).value

        clock = TestClock()
        clock.set_time(start_time_ns)

        handler: list[Bar] = []
        handler_batch: list[Bar] = []
        instrument_id = TestIdStubs.audusd_id()
        bar_type = BarType(instrument_id, bar_spec, AggregationSource.INTERNAL)

        # Act
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            time_bars_origin_offset=time_bars_origin_offset,
        )

        initial_next_close = aggregator.next_close_ns

        def handle_tick(tick: QuoteTick | TradeTick | None):
            if type(tick) is QuoteTick:
                aggregator.handle_quote_tick(tick)
            elif type(tick) is TradeTick:
                aggregator.handle_trade_tick(tick)

        aggregator.start_batch_update(
            handler_batch.append,
            (pd.Timestamp(1990, 1, 1) + time_shift).value,
        )
        handle_tick(tick1)
        handle_tick(tick2)
        aggregator.stop_batch_update(start_time_ns)

        for event in clock.advance_time(start_time_ns):
            event.handle()

        # Assert
        interval = expected2 - expected1
        expected3 = expected2 + interval

        if time_shift2 == pd.Timedelta(0):
            assert pd.Timestamp(initial_next_close) == expected2
            assert pd.Timestamp(aggregator.next_close_ns) == expected3
        else:
            assert pd.Timestamp(initial_next_close) == expected3
            assert pd.Timestamp(aggregator.next_close_ns) == expected3

        assert len(handler) == 0

        def assert_bar(bar: Bar, price: Price, volume: Quantity | int, expected_time: pd.Timestamp):
            assert bar.volume == volume

            assert pd.Timestamp(bar.ts_init) == expected_time
            assert pd.Timestamp(bar.ts_event) == expected_time

            assert bar.high == price
            assert bar.low == price
            assert bar.open == price
            assert bar.close == price

            assert bar.bar_type.instrument_id == AUDUSD_SIM.id
            assert bar.bar_type.spec == bar_spec
            assert bar.bar_type.aggregation_source == AggregationSource.INTERNAL

        price1 = Price.from_str("1.00005")
        volume1 = Quantity.from_int(2)
        price2 = Price.from_str("0.99999")
        volume2 = Quantity.from_int(1)

        if tick1 is not None and tick2 is not None:
            assert len(handler_batch) == 2
            assert_bar(handler_batch[0], price1, volume1, expected1)
            assert_bar(handler_batch[1], price2, volume2, expected2)
        elif tick1 is not None:
            assert len(handler_batch) == 2
            assert_bar(handler_batch[0], price1, volume1, expected1)
            assert_bar(handler_batch[1], price1, 0, expected2)
        elif tick2 is not None:
            assert len(handler_batch) == 1
            assert_bar(handler_batch[0], price2, volume2, expected2)
        else:
            assert len(handler_batch) == 0

    def test_update_timer_with_test_clock_sends_single_bar_to_handler(self):
        # Arrange
        start_time_ns = pd.Timestamp(1990, 1, 1).value

        clock = TestClock()
        clock.set_time(start_time_ns + 1)

        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(instrument_id, bar_spec, aggregation_source=AggregationSource.INTERNAL)
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
        )

        tick1 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00001"),
            ask_price=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=start_time_ns + 1,
            ts_init=start_time_ns + 1,
        )

        tick2 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00002"),
            ask_price=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=start_time_ns + 30 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 30 * NANOSECONDS_IN_SECOND,
        )

        tick3 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00003"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
        )

        initial_next_close = aggregator.next_close_ns

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
        assert pd.Timestamp(bar.ts_init) == pd.Timestamp(
            start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
        )
        assert pd.Timestamp(initial_next_close) == pd.Timestamp(
            start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
        )
        assert pd.Timestamp(aggregator.next_close_ns) == pd.Timestamp(
            start_time_ns + 2 * 60 * NANOSECONDS_IN_SECOND,
        )

    def test_batch_update_sends_single_bar_to_handler(self):
        # Arrange
        start_time_ns = pd.Timestamp(1990, 1, 1).value

        clock = TestClock()
        clock.set_time(start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND)

        handler: list[Bar] = []
        handler_batch: list[Bar] = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(3, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(instrument_id, bar_spec, aggregation_source=AggregationSource.INTERNAL)
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
        )

        tick1 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00001"),
            ask_price=Price.from_str("1.00004"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
        )

        tick2 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00002"),
            ask_price=Price.from_str("1.00005"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=start_time_ns + 2 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 2 * 60 * NANOSECONDS_IN_SECOND,
        )

        tick3 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00003"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
        )

        # Act
        aggregator.start_batch_update(handler_batch.append, tick1.ts_event)
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.stop_batch_update(tick2.ts_init)

        aggregator.handle_quote_tick(tick3)

        for event in clock.advance_time(tick3.ts_init):
            event.handle()

        # Assert
        bar = handler[0]
        assert len(handler) == 1
        assert Price.from_str("1.000025") == bar.open
        assert Price.from_str("1.000035") == bar.high
        assert Price.from_str("1.000015") == bar.low
        assert Price.from_str("1.000015") == bar.close
        assert Quantity.from_int(3) == bar.volume
        assert pd.Timestamp(bar.ts_init) == pd.Timestamp(
            start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
        )

    def test_update_timer_with_test_clock_sends_single_bar_to_handler_with_bars(self):
        # Arrange
        start_time_ns = pd.Timestamp(1990, 1, 1).value

        clock = TestClock()
        clock.set_time(start_time_ns + 1)

        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec3 = BarSpecification(3, BarAggregation.MINUTE, PriceType.LAST)
        bar_spec1 = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
        bar_type = BarType.new_composite(
            instrument_id,
            bar_spec3,
            AggregationSource.INTERNAL,
            bar_spec1.step,
            bar_spec1.aggregation,
            AggregationSource.EXTERNAL,
        )
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
        )
        initial_next_close = aggregator.next_close_ns
        composite_bar_type = bar_type.composite()

        bar1 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00005"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00004"),
            close=Price.from_str("1.00007"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
        )

        bar2 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00007"),
            high=Price.from_str("1.00020"),
            low=Price.from_str("1.00003"),
            close=Price.from_str("1.00015"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 2 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 2 * 60 * NANOSECONDS_IN_SECOND,
        )

        bar3 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00015"),
            high=Price.from_str("1.00015"),
            low=Price.from_str("1.00007"),
            close=Price.from_str("1.00008"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
        )

        # Act
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)
        aggregator.handle_bar(bar3)
        events = clock.advance_time(bar3.ts_event)
        events[0].handle()

        # Assert
        bar = handler[0]
        assert len(handler) == 1
        assert bar.bar_type == bar_type.standard()
        assert bar.open == Price.from_str("1.00005")
        assert bar.high == Price.from_str("1.00020")
        assert bar.low == Price.from_str("1.00003")
        assert bar.close == Price.from_str("1.00008")
        assert bar.volume == Quantity.from_int(3)
        assert pd.Timestamp(bar.ts_init) == pd.Timestamp(
            start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
        )
        assert pd.Timestamp(initial_next_close) == pd.Timestamp(
            start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
        )
        assert pd.Timestamp(aggregator.next_close_ns) == pd.Timestamp(
            start_time_ns + 6 * 60 * NANOSECONDS_IN_SECOND,
        )

    # TODO: Test this behavior in batch loading
    # TODO: Test this behavior between batch loading (unfinished and finished first interval) and normal loading
    @pytest.mark.parametrize(
        ("time_shift", "handler_length_expected"),
        [
            [0, 1],
            [1, 0],
            [9900, 0],
            [1 * 60 * NANOSECONDS_IN_SECOND, 0],
            # TODO: BUG
            # [-2, 1],
            # [-10000, 1]
            # [-1 * 60 * NANOSECONDS_IN_SECOND, 1],
        ],
    )
    def test_update_timer_with_test_clock_sends_correct_number_bar_to_handler_with_skip_first_non_full_bar(
        self,
        time_shift: int,
        handler_length_expected: int,
    ):
        # Arrange
        start_time_ns = pd.Timestamp(1990, 1, 1).value

        clock = TestClock()
        clock.set_time(start_time_ns + time_shift)

        handler: list[Bar] = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec3 = BarSpecification(3, BarAggregation.MINUTE, PriceType.LAST)
        bar_spec1 = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
        bar_type = BarType.new_composite(
            instrument_id,
            bar_spec3,
            AggregationSource.INTERNAL,
            bar_spec1.step,
            bar_spec1.aggregation,
            AggregationSource.EXTERNAL,
        )

        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            skip_first_non_full_bar=True,
            bar_build_delay=0,
        )
        composite_bar_type = bar_type.composite()

        for event in clock.advance_time(start_time_ns + time_shift):
            event.handle()

        bar1 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00005"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00004"),
            close=Price.from_str("1.00007"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
        )

        bar2 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00007"),
            high=Price.from_str("1.00020"),
            low=Price.from_str("1.00003"),
            close=Price.from_str("1.00015"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 2 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 2 * 60 * NANOSECONDS_IN_SECOND,
        )

        bar3 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00015"),
            high=Price.from_str("1.00015"),
            low=Price.from_str("1.00007"),
            close=Price.from_str("1.00008"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
        )

        # Act
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)
        aggregator.handle_bar(bar3)
        events = clock.advance_time(bar3.ts_event)
        events[0].handle()

        # Assert
        assert len(handler) == handler_length_expected

    def test_update_timer_with_test_clock_sends_single_bar_to_handler_with_bars_and_time_origin(
        self,
    ):
        # Arrange
        start_time_ns = pd.Timestamp(1990, 1, 1).value

        clock = TestClock()
        clock.set_time(start_time_ns + (30 * 60 + 30) * NANOSECONDS_IN_SECOND + 10_000)

        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec3 = BarSpecification(3, BarAggregation.MINUTE, PriceType.LAST)
        bar_spec1 = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
        bar_type = BarType.new_composite(
            instrument_id,
            bar_spec3,
            AggregationSource.INTERNAL,
            bar_spec1.step,
            bar_spec1.aggregation,
            AggregationSource.EXTERNAL,
        )
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            time_bars_origin_offset=pd.Timedelta(seconds=30),
            bar_build_delay=10,
        )
        composite_bar_type = bar_type.composite()
        initial_next_close = aggregator.next_close_ns

        bar1 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00005"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00004"),
            close=Price.from_str("1.00007"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 31 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 31 * 60 * NANOSECONDS_IN_SECOND,
        )

        bar2 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00007"),
            high=Price.from_str("1.00020"),
            low=Price.from_str("1.00003"),
            close=Price.from_str("1.00015"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 32 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 32 * 60 * NANOSECONDS_IN_SECOND,
        )

        bar3 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00015"),
            high=Price.from_str("1.00015"),
            low=Price.from_str("1.00007"),
            close=Price.from_str("1.00008"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 33 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 33 * 60 * NANOSECONDS_IN_SECOND,
        )

        # Act
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)
        aggregator.handle_bar(bar3)
        events = clock.advance_time((33 * 60 + 30) * NANOSECONDS_IN_SECOND + 10_000)
        events[0].handle()

        # Assert
        bar = handler[0]
        assert len(handler) == 1
        assert bar.bar_type == bar_type.standard()
        assert bar.open == Price.from_str("1.00005")
        assert bar.high == Price.from_str("1.00020")
        assert bar.low == Price.from_str("1.00003")
        assert bar.close == Price.from_str("1.00008")
        assert bar.volume == Quantity.from_int(3)

        assert pd.Timestamp(bar.ts_init) == pd.Timestamp(
            start_time_ns + 33 * 60 * NANOSECONDS_IN_SECOND,
        )
        assert pd.Timestamp(initial_next_close) == pd.Timestamp(
            start_time_ns + 33 * 60 * NANOSECONDS_IN_SECOND,
        )
        assert pd.Timestamp(aggregator.next_close_ns) == pd.Timestamp(
            start_time_ns + 36 * 60 * NANOSECONDS_IN_SECOND,
        )

    def test_update_timer_with_test_clock_sends_single_monthly_bar_to_handler_with_bars(self):
        # Arrange
        start_time_ns = pd.Timestamp("2024-3-23").value

        clock = TestClock()
        clock.set_time(start_time_ns)

        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec3 = BarSpecification(1, BarAggregation.MONTH, PriceType.LAST)
        bar_spec1 = BarSpecification(1, BarAggregation.DAY, PriceType.LAST)
        bar_type = BarType.new_composite(
            instrument_id,
            bar_spec3,
            AggregationSource.INTERNAL,
            bar_spec1.step,
            bar_spec1.aggregation,
            AggregationSource.EXTERNAL,
        )
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
        )
        composite_bar_type = bar_type.composite()

        bar1 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00005"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00004"),
            close=Price.from_str("1.00007"),
            volume=Quantity.from_int(1),
            ts_event=pd.Timestamp("2024-3-24").value,  # time in nanoseconds
            ts_init=pd.Timestamp("2024-3-24").value,
        )

        bar2 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00007"),
            high=Price.from_str("1.00020"),
            low=Price.from_str("1.00003"),
            close=Price.from_str("1.00015"),
            volume=Quantity.from_int(1),
            ts_event=pd.Timestamp("2024-3-25").value,
            ts_init=pd.Timestamp("2024-3-25").value,
        )

        bar3 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00015"),
            high=Price.from_str("1.00015"),
            low=Price.from_str("1.00007"),
            close=Price.from_str("1.00008"),
            volume=Quantity.from_int(1),
            ts_event=pd.Timestamp("2024-3-26").value,
            ts_init=pd.Timestamp("2024-3-26").value,
        )

        # Act
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)
        aggregator.handle_bar(bar3)
        events = clock.advance_time(pd.Timestamp("2024-4-1").value)
        events[0].handle()

        # Assert
        bar = handler[0]
        assert len(handler) == 1
        assert bar.bar_type == bar_type.standard()
        assert bar.open == Price.from_str("1.00005")
        assert bar.high == Price.from_str("1.00020")
        assert bar.low == Price.from_str("1.00003")
        assert bar.close == Price.from_str("1.00008")
        assert bar.volume == Quantity.from_int(3)
        assert pd.Timestamp(bar.ts_init) == pd.Timestamp("2024-4-1")

    def test_update_timer_with_test_clock_sends_single_weekly_bar_to_handler_with_bars(self):
        # Arrange
        start_time_ns = pd.Timestamp("2024-3-23").value

        clock = TestClock()
        clock.set_time(start_time_ns)
        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec3 = BarSpecification(1, BarAggregation.WEEK, PriceType.LAST)
        bar_spec1 = BarSpecification(1, BarAggregation.DAY, PriceType.LAST)
        bar_type = BarType.new_composite(
            instrument_id,
            bar_spec3,
            AggregationSource.INTERNAL,
            bar_spec1.step,
            bar_spec1.aggregation,
            AggregationSource.EXTERNAL,
        )
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
        )
        composite_bar_type = bar_type.composite()
        initial_next_close = aggregator.next_close_ns

        bar1 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00005"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00004"),
            close=Price.from_str("1.00007"),
            volume=Quantity.from_int(1),
            ts_event=pd.Timestamp("2024-3-20").value,  # time in nanoseconds
            ts_init=pd.Timestamp("2024-3-20").value,
        )

        bar2 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00007"),
            high=Price.from_str("1.00020"),
            low=Price.from_str("1.00003"),
            close=Price.from_str("1.00015"),
            volume=Quantity.from_int(1),
            ts_event=pd.Timestamp("2024-3-21").value,
            ts_init=pd.Timestamp("2024-3-21").value,
        )

        bar3 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00015"),
            high=Price.from_str("1.00015"),
            low=Price.from_str("1.00007"),
            close=Price.from_str("1.00008"),
            volume=Quantity.from_int(1),
            ts_event=pd.Timestamp("2024-3-22").value,
            ts_init=pd.Timestamp("2024-3-22").value,
        )

        # Act
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)
        aggregator.handle_bar(bar3)
        events = clock.advance_time(pd.Timestamp("2024-3-25").value)
        events[0].handle()

        # Assert
        bar = handler[0]
        assert len(handler) == 1
        assert bar.bar_type == bar_type.standard()
        assert bar.open == Price.from_str("1.00005")
        assert bar.high == Price.from_str("1.00020")
        assert bar.low == Price.from_str("1.00003")
        assert bar.close == Price.from_str("1.00008")
        assert bar.volume == Quantity.from_int(3)
        assert pd.Timestamp(bar.ts_init) == pd.Timestamp("2024-3-25")
        assert pd.Timestamp(initial_next_close) == pd.Timestamp("2024-3-25")
        assert pd.Timestamp(aggregator.next_close_ns) == pd.Timestamp("2024-4-1")

    def test_batch_update_sends_single_bar_to_handler_with_bars(self):
        # Arrange
        start_time_ns = pd.Timestamp(1990, 1, 1).value

        clock = TestClock()
        clock.set_time(start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND)

        handler = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec3 = BarSpecification(3, BarAggregation.MINUTE, PriceType.LAST)
        bar_spec1 = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
        bar_type = BarType.new_composite(
            instrument_id,
            bar_spec3,
            AggregationSource.INTERNAL,
            bar_spec1.step,
            bar_spec1.aggregation,
            AggregationSource.EXTERNAL,
        )
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
        )
        composite_bar_type = bar_type.composite()

        bar1 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00005"),
            high=Price.from_str("1.00010"),
            low=Price.from_str("1.00004"),
            close=Price.from_str("1.00007"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 1 * 60 * NANOSECONDS_IN_SECOND,
        )

        bar2 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00007"),
            high=Price.from_str("1.00020"),
            low=Price.from_str("1.00003"),
            close=Price.from_str("1.00015"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 2 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 2 * 60 * NANOSECONDS_IN_SECOND,
        )

        bar3 = Bar(
            bar_type=composite_bar_type,
            open=Price.from_str("1.00015"),
            high=Price.from_str("1.00015"),
            low=Price.from_str("1.00007"),
            close=Price.from_str("1.00008"),
            volume=Quantity.from_int(1),
            ts_event=start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
            ts_init=start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND,
        )

        # Act
        aggregator.start_batch_update(handler.append, bar1.ts_init)
        aggregator.handle_bar(bar1)
        aggregator.handle_bar(bar2)
        aggregator.stop_batch_update(bar2.ts_init)
        aggregator.handle_bar(bar3)

        # Assert
        bar = handler[0]
        assert len(handler) == 1
        assert bar.bar_type == bar_type.standard()
        assert bar.open == Price.from_str("1.00005")
        assert bar.high == Price.from_str("1.00020")
        assert bar.low == Price.from_str("1.00003")
        assert bar.close == Price.from_str("1.00008")
        assert bar.volume == Quantity.from_int(3)
        assert bar.ts_init == start_time_ns + 3 * 60 * NANOSECONDS_IN_SECOND

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
        path = TEST_DATA_DIR / "binance/btcusdt-quotes.parquet"
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
        )

        # Act - mini backtest loop
        for tick in ticks:
            aggregator.handle_quote_tick(tick)
            events = clock.advance_time(tick.ts_init)
            for event in events:
                event.handle()

        # Assert
        assert clock.timestamp_ns() == 1610064046674000000
        assert aggregator.interval_ns == NANOSECONDS_IN_SECOND
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
        path = TEST_DATA_DIR / "binance/btcusdt-quotes.parquet"
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
        path = TEST_DATA_DIR / "binance/btcusdt-quotes.parquet"
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

    @pytest.mark.parametrize(
        "timestamp_on_close, interval_type, ts_event1, ts_event2",
        [
            (False, BarIntervalType.LEFT_OPEN, 0, 60_000_000_000),
            (False, BarIntervalType.RIGHT_OPEN, 0, 60_000_000_000),
            (True, BarIntervalType.LEFT_OPEN, 60_000_000_000, 120_000_000_000),
            (True, BarIntervalType.RIGHT_OPEN, 0, 60_000_000_000),
        ],
    )
    def test_timebar_aggregator_interval_types(
        self,
        timestamp_on_close: bool,
        interval_type: BarIntervalType,
        ts_event1: int,
        ts_event2: int,
    ) -> None:
        # Arrange
        path = TEST_DATA_DIR / "binance/btcusdt-quotes.parquet"
        df_ticks = ParquetTickDataLoader.load(path)

        wrangler = QuoteTickDataWrangler(BTCUSDT_BINANCE)
        ticks = wrangler.process(df_ticks)

        clock = TestClock()
        handler: list[Bar] = []
        instrument_id = TestIdStubs.audusd_id()
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(instrument_id, bar_spec)

        # Act
        aggregator = TimeBarAggregator(
            AUDUSD_SIM,
            bar_type,
            handler.append,
            clock,
            interval_type=interval_type,
            timestamp_on_close=timestamp_on_close,
        )
        aggregator.handle_quote_tick(ticks[0])

        events = clock.advance_time(dt_to_unix_nanos(UNIX_EPOCH + timedelta(minutes=2)))
        for event in events:
            event.handle()

        # Assert
        assert len(handler) == 2
        assert handler[0].ts_event == ts_event1
        assert handler[1].ts_event == ts_event2
