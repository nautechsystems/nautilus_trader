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

import sys
from decimal import Decimal
from pathlib import Path

import pandas as pd
import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ImportableControllerConfig
from nautilus_trader.config import InvalidConfiguration
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StreamingConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.examples.strategies.signal_strategy import SignalStrategy
from nautilus_trader.examples.strategies.signal_strategy import SignalStrategyConfig
from nautilus_trader.execution.algorithm import ExecAlgorithm
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence import wranglers_v2
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.config import TestConfigStubs
from nautilus_trader.test_kit.stubs.data import MyData
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.trading.messages import RemoveStrategy
from nautilus_trader.trading.strategy import Strategy


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestBacktestEngine:
    def setup(self):
        # Fixture Setup
        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
        self.engine = self.create_engine(
            BacktestEngineConfig(logging=LoggingConfig(bypass_logging=True)),
        )

    def create_engine(self, config: BacktestEngineConfig | None = None) -> BacktestEngine:
        engine = BacktestEngine(config)
        engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=FillModel(),
        )

        # Set up data
        wrangler = QuoteTickDataWrangler(self.usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv")[:2000],
            ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv")[:2000],
        )
        engine.add_instrument(USDJPY_SIM)
        engine.add_data(ticks)
        return engine

    def teardown(self):
        self.engine.reset()
        self.engine.dispose()

    def test_initialization(self):
        engine = BacktestEngine(BacktestEngineConfig(logging=LoggingConfig(bypass_logging=True)))

        # Arrange, Act, Assert
        assert engine.run_id is None
        assert engine.run_started is None
        assert engine.run_finished is None
        assert engine.backtest_start is None
        assert engine.backtest_end is None
        assert engine.iteration == 0
        assert engine.get_log_guard() is None  # Logging bypassed

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

    def test_clear_actors_with_no_actors(self):
        # Arrange, Act, Assert
        self.engine.clear_actors()

    def test_clear_actors(self):
        # Arrange
        self.engine.add_actor(Actor())

        # Act
        self.engine.clear_actors()

        # Assert
        assert self.engine.trader.actors() == []

    def test_clear_strategies_with_no_strategies(self):
        # Arrange, Act, Assert
        self.engine.clear_strategies()

    def test_clear_strategies(self):
        # Arrange
        self.engine.add_strategy(Strategy())

        # Act
        self.engine.clear_strategies()

        # Assert
        assert self.engine.trader.strategies() == []

    def test_clear_exec_algorithms_no_exec_algorithms(self):
        # Arrange, Act, Assert
        self.engine.clear_exec_algorithms()

    def test_clear_exec_algorithms(self):
        # Arrange
        self.engine.add_exec_algorithm(ExecAlgorithm())

        # Act
        self.engine.clear_exec_algorithms()

        # Assert
        assert self.engine.trader.exec_algorithms() == []

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

    @pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
    def test_persistence_files_cleaned_up(self, tmp_path: Path) -> None:
        # Arrange
        catalog = ParquetDataCatalog(
            path=tmp_path,
            fs_protocol="file",
        )
        config = TestConfigStubs.backtest_engine_config(catalog=catalog, persist=True)
        engine = TestComponentStubs.backtest_engine(
            config=config,
            instrument=self.usdjpy,
            ticks=TestDataStubs.quote_ticks_usdjpy(),
        )

        # Act
        engine.run()
        engine.dispose()

        # Assert
        assert all(f.closed for f in engine.kernel.writer._files.values())

    def test_run_with_venue_config_raises_invalid_config(
        self,
        config: BacktestEngineConfig | None = None,
    ) -> BacktestEngine:
        engine = BacktestEngine(config)
        engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            book_type=BookType.L2_MBP,  # <-- Invalid for data
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=FillModel(),
        )

        # Set up data
        wrangler = QuoteTickDataWrangler(self.usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv")[:2000],
            ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv")[:2000],
        )
        engine.add_instrument(USDJPY_SIM)
        engine.add_data(ticks)
        with pytest.raises(InvalidConfiguration):
            engine.run()

    def test_multiple_runs(self):
        for _ in range(2):
            config = SignalStrategyConfig(instrument_id=USDJPY_SIM.id)
            strategy = SignalStrategy(config)
            engine = self.create_engine(
                config=BacktestEngineConfig(
                    streaming=StreamingConfig(catalog_path="/", fs_protocol="memory"),
                    logging=LoggingConfig(bypass_logging=True),
                ),
            )
            engine.add_strategy(strategy)
            engine.run()
            engine.dispose()

    def test_strategy_timestamps(self):
        # Arrange
        config = SignalStrategyConfig(instrument_id=USDJPY_SIM.id)
        strategy = SignalStrategy(config)
        engine = self.create_engine(
            config=BacktestEngineConfig(
                streaming=StreamingConfig(catalog_path="/", fs_protocol="memory"),
                logging=LoggingConfig(bypass_logging=True),
            ),
        )
        engine.add_strategy(strategy)
        messages = []
        strategy.msgbus.subscribe("*", handler=messages.append)

        # Act
        engine.run()

        # Assert
        msg = messages[11]
        assert msg.__class__.__name__ == "SignalCounter"
        assert msg.ts_init == 1359676800000000000
        assert msg.ts_event == 1359676800000000000

    def test_set_instance_id(self):
        # Arrange
        instance_id = UUID4()

        # Act
        engine1 = self.create_engine(
            config=BacktestEngineConfig(
                instance_id=instance_id,
                logging=LoggingConfig(bypass_logging=True),
            ),
        )
        engine2 = self.create_engine(
            config=BacktestEngineConfig(
                logging=LoggingConfig(bypass_logging=True),
            ),
        )  # Engine sets instance id

        # Assert
        assert engine1.kernel.instance_id == instance_id
        assert engine2.kernel.instance_id != instance_id

    def test_controller(self):
        # Arrange - Controller class
        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            controller=ImportableControllerConfig(
                controller_path="nautilus_trader.test_kit.mocks.controller:MyController",
                config_path="nautilus_trader.test_kit.mocks.controller:ControllerConfig",
                config={},
            ),
        )
        engine = self.create_engine(config=config)

        # Act
        engine.run()

        # Assert
        assert len(engine.kernel.trader.strategies()) == 1

        # Act
        msg = RemoveStrategy(StrategyId("SignalStrategy-000"))
        engine.kernel.msgbus.send("Controller.execute", msg)

        # Assert
        assert len(engine.kernel.trader.strategies()) == 0


class TestBacktestEngineCashAccount:
    def setup(self) -> None:
        # Fixture Setup
        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
        self.engine = self.create_engine(
            BacktestEngineConfig(logging=LoggingConfig(bypass_logging=True)),
        )

    def create_engine(self, config: BacktestEngineConfig | None = None) -> BacktestEngine:
        engine = BacktestEngine(config)
        engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.CASH,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=FillModel(),
        )
        return engine

    def teardown(self):
        self.engine.reset()
        self.engine.dispose()

    def test_adding_currency_pair_for_single_currency_cash_account_raises_exception(self):
        # Arrange, Act, Assert
        with pytest.raises(InvalidConfiguration):
            self.engine.add_instrument(self.usdjpy)


class TestBacktestEngineData:
    def setup(self):
        # Fixture Setup
        self.engine = BacktestEngine(
            BacktestEngineConfig(logging=LoggingConfig(bypass_logging=True)),
        )
        self.engine.add_venue(
            venue=Venue("BINANCE"),
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USDT)],
        )
        self.engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=FillModel(),
        )

    def test_add_pyo3_data_raises_type_error(self) -> None:
        # Arrange
        path = TEST_DATA_DIR / "truefx" / "audusd-ticks.csv"
        df = pd.read_csv(path)

        wrangler = wranglers_v2.QuoteTickDataWranglerV2.from_instrument(AUDUSD_SIM)
        ticks = wrangler.from_pandas(df)

        # Act, Assert
        with pytest.raises(TypeError):
            self.engine.add_data(ticks)

    def test_add_custom_data_adds_to_engine(self):
        # Arrange
        data_type = DataType(MyData, metadata={"news_wire": "hacks"})

        custom_data1 = [
            CustomData(data_type, MyData("AAPL hacked")),
            CustomData(
                data_type,
                MyData("AMZN hacked", 1000, 1000),
            ),
            CustomData(
                data_type,
                MyData("NFLX hacked", 3000, 3000),
            ),
            CustomData(
                data_type,
                MyData("MSFT hacked", 2000, 2000),
            ),
        ]

        custom_data2 = [
            CustomData(
                data_type,
                MyData("FB hacked", 1500, 1500),
            ),
        ]

        # Act
        self.engine.add_data(custom_data1, ClientId("NEWS_CLIENT"))
        self.engine.add_data(custom_data2, ClientId("NEWS_CLIENT"))

        # Assert
        assert len(self.engine.data) == 5

    def test_add_instrument_when_no_venue_raises_exception(self):
        # Arrange
        engine = BacktestEngine(BacktestEngineConfig(logging=LoggingConfig(bypass_logging=True)))

        # Act, Assert
        with pytest.raises(InvalidConfiguration):
            engine.add_instrument(ETHUSDT_BINANCE)

    def test_add_order_book_deltas_adds_to_engine(self):
        # Arrange
        self.engine.add_instrument(AUDUSD_SIM)
        self.engine.add_instrument(ETHUSDT_BINANCE)

        deltas1 = [
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("13.0"),
                    size=Quantity.from_str("40"),
                    order_id=0,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("12.0"),
                    size=Quantity.from_str("30"),
                    order_id=1,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("11.0"),
                    size=Quantity.from_str("20"),
                    order_id=2,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("10.0"),
                    size=Quantity.from_str("20"),
                    order_id=3,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("9.0"),
                    size=Quantity.from_str("30"),
                    order_id=4,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("0.0"),
                    size=Quantity.from_str("40"),
                    order_id=4,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            ),
        ]

        deltas2 = [
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                action=BookAction.UPDATE,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("13.0"),
                    size=Quantity.from_str("45"),
                    order_id=0,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("12.5"),
                    size=Quantity.from_str("35"),
                    order_id=1,
                ),
                flags=0,
                sequence=0,
                ts_event=1000,
                ts_init=1000,
            ),
        ]

        operations1 = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            deltas=deltas1,
        )

        operations2 = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            deltas=deltas2,
        )

        # Act
        self.engine.add_data([operations2, operations1])  # <-- not sorted

        # Assert
        assert len(self.engine.data) == 2
        assert self.engine.data[0] == operations1
        assert self.engine.data[1] == operations2

    def test_add_quote_ticks_adds_to_engine(self):
        # Arrange - set up data
        self.engine.add_instrument(AUDUSD_SIM)
        wrangler = QuoteTickDataWrangler(AUDUSD_SIM)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("truefx/audusd-ticks.csv"))

        # Act
        self.engine.add_data(ticks)

        # Assert
        assert len(self.engine.data) == 100000

    def test_add_trade_ticks_adds_to_engine(self):
        # Arrange
        self.engine.add_instrument(ETHUSDT_BINANCE)

        wrangler = TradeTickDataWrangler(ETHUSDT_BINANCE)
        provider = TestDataProvider()
        ticks = wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv"))

        # Act
        self.engine.add_data(ticks)

        # Assert
        assert len(self.engine.data) == 69806

    def test_add_bars_adds_to_engine(self):
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
        bars = wrangler.process(provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv")[:2000])

        # Act
        self.engine.add_instrument(USDJPY_SIM)
        self.engine.add_data(data=bars)

        # Assert
        assert len(self.engine.data) == 2000

    def test_add_instrument_status_to_engine(self):
        # Arrange
        data = [
            InstrumentStatus(
                instrument_id=USDJPY_SIM.id,
                action=MarketStatusAction.CLOSE,
                ts_init=0,
                ts_event=0,
            ),
            InstrumentStatus(
                instrument_id=USDJPY_SIM.id,
                action=MarketStatusAction.TRADING,
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
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)
        self.venue = Venue("SIM")

        # Set up venue
        self.engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
        )

        # Set up data
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
        bid_bars = bid_wrangler.process(provider.read_csv_bars("fxcm/gbpusd-m1-bid-2012.csv"))
        ask_bars = ask_wrangler.process(provider.read_csv_bars("fxcm/gbpusd-m1-ask-2012.csv"))

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
            instrument_id=GBPUSD_SIM.id,
            bar_type=bar_type,
            trade_size=Decimal(100_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Act
        self.engine.run()

        # Assert
        assert strategy.fast_ema.count == 30117
        assert self.engine.iteration == 60234
        assert self.engine.portfolio.account(self.venue).balance_total(USD) == Money(
            990_569.89,
            USD,
        )

    def test_dump_pickled_data(self):
        # Arrange, Act, Assert
        pickled = self.engine.dump_pickled_data()
        assert 5_060_606 <= len(pickled) <= 6_205_205

    def test_load_pickled_data(self):
        # Arrange
        bar_type = BarType(
            instrument_id=GBPUSD_SIM.id,
            bar_spec=TestDataStubs.bar_spec_1min_bid(),
            aggregation_source=AggregationSource.EXTERNAL,  # <-- important
        )
        config = EMACrossConfig(
            instrument_id=GBPUSD_SIM.id,
            bar_type=bar_type,
            trade_size=Decimal(100_000),
            fast_ema_period=10,
            slow_ema_period=20,
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
            990_569.89,
            USD,
        )
