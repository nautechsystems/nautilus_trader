# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import tempfile
from decimal import Decimal
from typing import Optional

import pandas as pd
import pytest

from nautilus_trader.backtest.data.providers import TestDataProvider
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import BarDataWrangler
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler
from nautilus_trader.backtest.data.wranglers import TradeTickDataWrangler
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.config import StreamingConfig
from nautilus_trader.config.error import InvalidConfiguration
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.examples.strategies.signal_strategy import SignalStrategy
from nautilus_trader.examples.strategies.signal_strategy import SignalStrategyConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import Order
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.trading.strategy import Strategy
from tests.test_kit.stubs import MyData
from tests.test_kit.stubs.component import TestComponentStubs
from tests.test_kit.stubs.config import TestConfigStubs
from tests.test_kit.stubs.data import TestDataStubs


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestBacktestEngine:
    def setup(self):
        # Fixture Setup
        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
        self.engine = self.create_engine()

    def create_engine(self, config: Optional[BacktestEngineConfig] = None):
        engine = BacktestEngine(config)
        engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=FillModel(),
        )

        # Setup data
        wrangler = QuoteTickDataWrangler(self.usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv")[:2000],
            ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv")[:2000],
        )
        engine.add_instrument(USDJPY_SIM)
        engine.add_data(ticks)
        return engine

    def teardown(self):
        self.engine.reset()
        self.engine.dispose()

    def test_initialization(self):
        engine = BacktestEngine()

        # Arrange, Act, Assert
        assert engine.run_id is None
        assert engine.run_started is None
        assert engine.run_finished is None
        assert engine.backtest_start is None
        assert engine.backtest_end is None
        assert engine.iteration == 0

    def test_reset_engine(self):
        # Arrange
        self.engine.run()

        # Act
        self.engine.reset()

        # Assert
        assert self.engine.run_id is None
        assert self.engine.run_started is None
        assert self.engine.run_finished is None
        assert self.engine.backtest_start is None
        assert self.engine.backtest_end is None
        assert self.engine.iteration == 0  # No exceptions raised

    def test_run_with_no_strategies(self):
        # Arrange, Act
        self.engine.run()

        # Assert
        assert self.engine.iteration == 8000

    def test_run(self):
        # Arrange, Act
        self.engine.add_strategy(Strategy())
        self.engine.run()

        # Assert
        assert len(self.engine.trader.strategy_states()) == 1

    def test_change_fill_model(self):
        # Arrange, Act
        self.engine.change_fill_model(Venue("SIM"), FillModel())

        # Assert
        assert True  # No exceptions raised

    def test_account_state_timestamp(self):
        # Arrange
        start = pd.Timestamp("2013-01-31 23:59:59.700000+00:00")
        self.engine.run(start=start)

        # Act
        report = self.engine.trader.generate_account_report(Venue("SIM"))

        # Assert
        assert len(report) == 1
        assert report.index[0] == start

    def test_persistence_files_cleaned_up(self):
        # Arrange
        temp_dir = tempfile.mkdtemp()
        catalog = ParquetDataCatalog(
            path=str(temp_dir),
            fs_protocol="file",
        )
        config = TestConfigStubs.backtest_engine_config(persist=True, catalog=catalog)
        engine = TestComponentStubs.backtest_engine(
            config=config,
            instrument=self.usdjpy,
            ticks=TestDataStubs.quote_ticks_usdjpy(),
        )
        engine.run()
        engine.dispose()

        assert all([f.closed for f in engine.kernel.writer._files.values()])

    def test_backtest_engine_multiple_runs(self):
        for _ in range(2):
            config = SignalStrategyConfig(instrument_id=USDJPY_SIM.id.value)
            strategy = SignalStrategy(config)
            engine = self.create_engine(
                config=BacktestEngineConfig(
                    streaming=StreamingConfig(catalog_path="/", fs_protocol="memory")
                )
            )
            engine.add_strategy(strategy)
            engine.run()
            engine.dispose()

    def test_backtest_engine_strategy_timestamps(self):
        # Arrange
        config = SignalStrategyConfig(instrument_id=USDJPY_SIM.id.value)
        strategy = SignalStrategy(config)
        engine = self.create_engine(
            config=BacktestEngineConfig(
                streaming=StreamingConfig(catalog_path="/", fs_protocol="memory")
            )
        )
        engine.add_strategy(strategy)
        messages = []
        strategy.msgbus.subscribe("*", handler=messages.append)

        # Act
        engine.run()

        # Assert
        msg = messages[1]
        assert msg.__class__.__name__ == "SignalCounter"
        assert msg.ts_init == 1359676799700000000
        assert msg.ts_event == 1359676799700000000


class TestBacktestEngineData:
    def setup(self):
        # Fixture Setup
        self.engine = BacktestEngine()
        self.engine.add_venue(
            venue=Venue("BINANCE"),
            oms_type=OMSType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USDT)],
        )
        self.engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=FillModel(),
        )

    def test_add_generic_data_adds_to_engine(self, capsys):
        # Arrange
        data_type = DataType(MyData, metadata={"news_wire": "hacks"})

        generic_data1 = [
            GenericData(data_type, MyData("AAPL hacked")),
            GenericData(
                data_type,
                MyData("AMZN hacked", 1000, 1000),
            ),
            GenericData(
                data_type,
                MyData("NFLX hacked", 3000, 3000),
            ),
            GenericData(
                data_type,
                MyData("MSFT hacked", 2000, 2000),
            ),
        ]

        generic_data2 = [
            GenericData(
                data_type,
                MyData("FB hacked", 1500, 1500),
            ),
        ]

        # Act
        self.engine.add_data(generic_data1, ClientId("NEWS_CLIENT"))
        self.engine.add_data(generic_data2, ClientId("NEWS_CLIENT"))

        # Assert
        assert len(self.engine.data) == 5

    def test_add_instrument_when_no_venue_raises_exception(self):
        # Arrange
        engine = BacktestEngine()

        # Act, Assert
        with pytest.raises(InvalidConfiguration):
            engine.add_instrument(ETHUSDT_BINANCE)

    def test_add_order_book_snapshots_adds_to_engine(self, capsys):
        # Arrange
        self.engine.add_instrument(ETHUSDT_BINANCE)

        snapshot1 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            bids=[[1550.15, 0.51], [1580.00, 1.20]],
            asks=[[1552.15, 1.51], [1582.00, 2.20]],
            ts_event=0,
            ts_init=0,
        )

        snapshot2 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            bids=[[1551.15, 0.51], [1581.00, 1.20]],
            asks=[[1553.15, 1.51], [1583.00, 2.20]],
            ts_event=1_000_000_000,
            ts_init=1_000_000_000,
        )

        # Act
        self.engine.add_data([snapshot2, snapshot1])  # <-- reverse order

        # Assert
        assert len(self.engine.data) == 2
        assert self.engine.data[0] == snapshot1
        assert self.engine.data[1] == snapshot2

    def test_add_order_book_deltas_adds_to_engine(self, capsys):
        # Arrange
        self.engine.add_instrument(AUDUSD_SIM)
        self.engine.add_instrument(ETHUSDT_BINANCE)

        deltas = [
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                book_type=BookType.L2_MBP,
                action=BookAction.ADD,
                order=Order(
                    price=Price.from_str("13.0"),
                    size=Quantity.from_str("40"),
                    side=OrderSide.SELL,
                ),
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                book_type=BookType.L2_MBP,
                action=BookAction.ADD,
                order=Order(
                    price=Price.from_str("12.0"),
                    size=Quantity.from_str("30"),
                    side=OrderSide.SELL,
                ),
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                book_type=BookType.L2_MBP,
                action=BookAction.ADD,
                order=Order(
                    price=Price.from_str("11.0"),
                    size=Quantity.from_str("20"),
                    side=OrderSide.SELL,
                ),
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                book_type=BookType.L2_MBP,
                action=BookAction.ADD,
                order=Order(
                    price=Price.from_str("10.0"),
                    size=Quantity.from_str("20"),
                    side=OrderSide.BUY,
                ),
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                book_type=BookType.L2_MBP,
                action=BookAction.ADD,
                order=Order(
                    price=Price.from_str("9.0"),
                    size=Quantity.from_str("30"),
                    side=OrderSide.BUY,
                ),
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                book_type=BookType.L2_MBP,
                action=BookAction.ADD,
                order=Order(
                    price=Price.from_str("0.0"),
                    size=Quantity.from_str("40"),
                    side=OrderSide.BUY,
                ),
                ts_event=0,
                ts_init=0,
            ),
        ]

        operations1 = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            deltas=deltas,
            ts_event=0,
            ts_init=0,
        )

        operations2 = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            deltas=deltas,
            ts_event=1000,
            ts_init=1000,
        )

        # Act
        self.engine.add_data([operations2, operations1])  # <-- not sorted

        # Assert
        assert len(self.engine.data) == 2
        assert self.engine.data[0] == operations1
        assert self.engine.data[1] == operations2

    def test_add_quote_ticks_adds_to_engine(self, capsys):
        # Arrange, Setup data
        self.engine.add_instrument(AUDUSD_SIM)
        wrangler = QuoteTickDataWrangler(AUDUSD_SIM)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("truefx-audusd-ticks.csv"))

        # Act
        self.engine.add_data(ticks)

        # Assert
        assert len(self.engine.data) == 100000

    def test_add_trade_ticks_adds_to_engine(self, capsys):
        # Arrange
        self.engine.add_instrument(ETHUSDT_BINANCE)

        wrangler = TradeTickDataWrangler(ETHUSDT_BINANCE)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("binance-ethusdt-trades.csv"))

        # Act
        self.engine.add_data(ticks)

        # Assert
        assert len(self.engine.data) == 69806

    def test_add_bars_adds_to_engine(self, capsys):
        # Arrange
        bar_spec = BarSpecification(
            step=1,
            aggregation=BarAggregation.MINUTE,
            price_type=PriceType.BID,
        )

        bar_type = BarType(
            instrument_id=USDJPY_SIM.id,
            bar_spec=bar_spec,
            aggregation_source=AggregationSource.EXTERNAL,  # <-- important
        )

        wrangler = BarDataWrangler(
            bar_type=bar_type,
            instrument=USDJPY_SIM,
        )
        provider = TestDataProvider()
        bars = wrangler.process(provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv")[:2000])

        # Act
        self.engine.add_instrument(USDJPY_SIM)
        self.engine.add_data(data=bars)

        # Assert
        assert len(self.engine.data) == 2000

    def test_add_instrument_status_to_engine(self, capsys):
        # Arrange
        data = [
            InstrumentStatusUpdate(
                instrument_id=USDJPY_SIM.id,
                status=InstrumentStatus.CLOSED,
                ts_init=0,
                ts_event=0,
            ),
            InstrumentStatusUpdate(
                instrument_id=USDJPY_SIM.id,
                status=InstrumentStatus.OPEN,
                ts_init=0,
                ts_event=0,
            ),
        ]

        # Act
        self.engine.add_instrument(USDJPY_SIM)
        self.engine.add_data(data=data)

        # Assert
        assert len(self.engine.data) == 2
        assert self.engine.data == data


class TestBacktestWithAddedBars:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            bypass_logging=False,
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)
        self.venue = Venue("SIM")

        # Setup venue
        self.engine.add_venue(
            venue=self.venue,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
        )

        # Setup data
        bid_bar_type = BarType(
            instrument_id=GBPUSD_SIM.id,
            bar_spec=TestDataStubs.bar_spec_1min_bid(),
            aggregation_source=AggregationSource.EXTERNAL,  # <-- important
        )

        ask_bar_type = BarType(
            instrument_id=GBPUSD_SIM.id,
            bar_spec=TestDataStubs.bar_spec_1min_ask(),
            aggregation_source=AggregationSource.EXTERNAL,  # <-- important
        )

        bid_wrangler = BarDataWrangler(
            bar_type=bid_bar_type,
            instrument=GBPUSD_SIM,
        )

        ask_wrangler = BarDataWrangler(
            bar_type=ask_bar_type,
            instrument=GBPUSD_SIM,
        )

        provider = TestDataProvider()
        bid_bars = bid_wrangler.process(provider.read_csv_bars("fxcm-gbpusd-m1-bid-2012.csv"))
        ask_bars = ask_wrangler.process(provider.read_csv_bars("fxcm-gbpusd-m1-ask-2012.csv"))

        # Add data
        self.engine.add_instrument(GBPUSD_SIM)
        self.engine.add_data(bid_bars)
        self.engine.add_data(ask_bars)

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_added_bars(self):
        # Arrange
        bar_type = BarType(
            instrument_id=GBPUSD_SIM.id,
            bar_spec=TestDataStubs.bar_spec_1min_bid(),
            aggregation_source=AggregationSource.EXTERNAL,  # <-- important
        )
        config = EMACrossConfig(
            instrument_id=str(GBPUSD_SIM.id),
            bar_type=str(bar_type),
            trade_size=Decimal(100_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 30117
        assert self.engine.iteration == 60234
        assert self.engine.portfolio.account(self.venue).balance_total(USD) == Money(
            1011166.89, USD
        )

    def test_dump_pickled_data(self):
        # Arrange, # Act, # Assert
        assert len(self.engine.dump_pickled_data()) == 5181010

    def test_load_pickled_data(self):
        # Arrange
        bar_type = BarType(
            instrument_id=GBPUSD_SIM.id,
            bar_spec=TestDataStubs.bar_spec_1min_bid(),
            aggregation_source=AggregationSource.EXTERNAL,  # <-- important
        )
        config = EMACrossConfig(
            instrument_id=str(GBPUSD_SIM.id),
            bar_type=str(bar_type),
            trade_size=Decimal(100_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        data = self.engine.dump_pickled_data()

        # Act
        self.engine.load_pickled_data(data)
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 30117
        assert self.engine.iteration == 60234
        assert self.engine.portfolio.account(self.venue).balance_total(USD) == Money(
            1011166.89, USD
        )
