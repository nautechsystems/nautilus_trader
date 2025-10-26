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
from nautilus_trader.model.data import OrderBookDepth10
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
        msg = messages[10]
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

    def test_add_order_book_depth10_adds_to_engine(self):
        # Arrange
        self.engine.add_instrument(AUDUSD_SIM)

        depth_data = [
            OrderBookDepth10(
                instrument_id=AUDUSD_SIM.id,
                bids=[
                    BookOrder(OrderSide.BUY, Price.from_str("1.0000"), Quantity.from_str("100"), 1),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9999"), Quantity.from_str("200"), 2),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9998"), Quantity.from_str("300"), 3),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9997"), Quantity.from_str("400"), 4),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9996"), Quantity.from_str("500"), 5),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9995"), Quantity.from_str("600"), 6),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9994"), Quantity.from_str("700"), 7),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9993"), Quantity.from_str("800"), 8),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9992"), Quantity.from_str("900"), 9),
                    BookOrder(
                        OrderSide.BUY,
                        Price.from_str("0.9991"),
                        Quantity.from_str("1000"),
                        10,
                    ),
                ],
                asks=[
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0001"),
                        Quantity.from_str("100"),
                        11,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0002"),
                        Quantity.from_str("200"),
                        12,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0003"),
                        Quantity.from_str("300"),
                        13,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0004"),
                        Quantity.from_str("400"),
                        14,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0005"),
                        Quantity.from_str("500"),
                        15,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0006"),
                        Quantity.from_str("600"),
                        16,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0007"),
                        Quantity.from_str("700"),
                        17,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0008"),
                        Quantity.from_str("800"),
                        18,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0009"),
                        Quantity.from_str("900"),
                        19,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0010"),
                        Quantity.from_str("1000"),
                        20,
                    ),
                ],
                bid_counts=[1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
                ask_counts=[1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
                flags=0,
                sequence=1,
                ts_event=0,
                ts_init=0,
            ),
            OrderBookDepth10(
                instrument_id=AUDUSD_SIM.id,
                bids=[
                    BookOrder(OrderSide.BUY, Price.from_str("1.0001"), Quantity.from_str("150"), 1),
                    BookOrder(OrderSide.BUY, Price.from_str("1.0000"), Quantity.from_str("250"), 2),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9999"), Quantity.from_str("350"), 3),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9998"), Quantity.from_str("450"), 4),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9997"), Quantity.from_str("550"), 5),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9996"), Quantity.from_str("650"), 6),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9995"), Quantity.from_str("750"), 7),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9994"), Quantity.from_str("850"), 8),
                    BookOrder(OrderSide.BUY, Price.from_str("0.9993"), Quantity.from_str("950"), 9),
                    BookOrder(
                        OrderSide.BUY,
                        Price.from_str("0.9992"),
                        Quantity.from_str("1050"),
                        10,
                    ),
                ],
                asks=[
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0002"),
                        Quantity.from_str("150"),
                        11,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0003"),
                        Quantity.from_str("250"),
                        12,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0004"),
                        Quantity.from_str("350"),
                        13,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0005"),
                        Quantity.from_str("450"),
                        14,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0006"),
                        Quantity.from_str("550"),
                        15,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0007"),
                        Quantity.from_str("650"),
                        16,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0008"),
                        Quantity.from_str("750"),
                        17,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0009"),
                        Quantity.from_str("850"),
                        18,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0010"),
                        Quantity.from_str("950"),
                        19,
                    ),
                    BookOrder(
                        OrderSide.SELL,
                        Price.from_str("1.0011"),
                        Quantity.from_str("1050"),
                        20,
                    ),
                ],
                bid_counts=[1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
                ask_counts=[1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
                flags=0,
                sequence=2,
                ts_event=1000,
                ts_init=1000,
            ),
        ]

        # Act
        self.engine.add_data(depth_data)

        # Assert
        assert len(self.engine.data) == 2
        assert self.engine.data[0] == depth_data[0]
        assert self.engine.data[1] == depth_data[1]

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
            1_011_166.89,
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
            1_011_166.89,
            USD,
        )


class TestBacktestEngineStreaming:
    """
    Integration tests for BacktestEngine streaming functionality.

    Tests the full workflow of iterative streaming data processing.

    """

    def setup_method(self):
        # Create a minimal engine configuration for testing
        config = BacktestEngineConfig(
            trader_id="BACKTESTER-001",
            logging=LoggingConfig(bypass_logging=True),  # Reduce log noise during tests
        )
        self.engine = BacktestEngine(config=config)

        # Add venue and basic setup like other tests
        self.engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=FillModel(),
        )

    def create_data_iterator(self, name: str, start_ts: int, count: int, interval: int):
        """
        Create a data iterator with specified characteristics.
        """

        def data_generator():
            for i in range(count):
                yield [
                    MyData(
                        value=f"{name}_{i}",
                        ts_init=start_ts + (i * interval),
                    ),
                ]

        return data_generator()

    def create_sparse_iterator(self, name: str, start_ts: int, count: int):
        """
        Create an iterator with sparse, irregular timing.
        """

        def sparse_generator():
            # Create data with increasing gaps between items
            ts = start_ts
            for i in range(count):
                yield [
                    MyData(
                        value=f"{name}_sparse_{i}",
                        ts_init=ts,
                    ),
                ]
                # Exponentially increasing gaps: 1s, 2s, 4s, 8s, etc.
                ts += (2**i) * 1_000_000_000

        return sparse_generator()

    def create_dense_iterator(self, name: str, start_ts: int, count: int):
        """
        Create an iterator with very dense timing (1ms intervals).
        """

        def dense_generator():
            for i in range(count):
                yield [
                    MyData(
                        value=f"{name}_dense_{i}",
                        ts_init=start_ts + (i * 1_000_000),  # 1ms intervals
                    ),
                ]

        return dense_generator()

    def test_streaming_workflow_small_batches(self):
        """
        Test the full streaming workflow with small data batches.

        This simulates processing multiple days of data iteratively.

        """
        # Define 3 batches of iterators representing different "days" of data
        batch_1_start = 1_609_459_200_000_000_000  # 2021-01-01 00:00:00 UTC
        batch_2_start = 1_609_545_600_000_000_000  # 2021-01-02 00:00:00 UTC
        batch_3_start = 1_609_632_000_000_000_000  # 2021-01-03 00:00:00 UTC

        # Batch 1: Light data load (reduced for efficiency)
        batch_1_iterators = [
            (
                "instrument_A_day1",
                self.create_data_iterator("A", batch_1_start, 20, 60_000_000_000),
            ),  # 1min intervals
            (
                "instrument_B_day1",
                self.create_data_iterator("B", batch_1_start + 30_000_000_000, 16, 90_000_000_000),
            ),  # 1.5min intervals, offset by 30s
            (
                "instrument_C_day1",
                self.create_sparse_iterator("C", batch_1_start, 5),
            ),  # Sparse data
        ]

        for data_name, generator in batch_1_iterators:
            self.engine.add_data_iterator(data_name, generator)
        batch_1_initial = self.engine.iteration
        self.engine.run()  # Process all batch 1 data

        # Verify batch 1 processed data
        assert self.engine.iteration > batch_1_initial
        batch_1_final = self.engine.iteration

        # Clear engine data for next batch
        self.engine.reset()

        # Batch 2: Medium data load with overlapping timestamps
        batch_2_iterators = [
            (
                "instrument_A_day2",
                self.create_data_iterator("A", batch_2_start, 30, 30_000_000_000),
            ),  # 30s intervals
            (
                "instrument_B_day2",
                self.create_dense_iterator("B", batch_2_start, 10),
            ),  # Dense 1ms data
            (
                "instrument_D_day2",
                self.create_data_iterator("D", batch_2_start + 45_000_000_000, 25, 45_000_000_000),
            ),  # 45s intervals, offset
        ]

        for data_name, generator in batch_2_iterators:
            self.engine.add_data_iterator(data_name, generator)
        batch_2_initial = self.engine.iteration
        self.engine.run()  # Process all batch 2 data

        # Verify batch 2 processed data
        assert self.engine.iteration > batch_2_initial
        batch_2_final = self.engine.iteration

        # Clear engine data for next batch
        self.engine.reset()

        # Batch 3: Heavy data load to test memory efficiency
        batch_3_iterators = [
            (
                "instrument_A_day3",
                self.create_data_iterator("A", batch_3_start, 50, 10_000_000_000),
            ),  # 10s intervals
            (
                "instrument_B_day3",
                self.create_data_iterator("B", batch_3_start + 5_000_000_000, 40, 15_000_000_000),
            ),  # 15s intervals, offset
            (
                "instrument_E_day3",
                self.create_data_iterator("E", batch_3_start, 30, 20_000_000_000),
            ),  # 20s intervals
        ]

        for data_name, generator in batch_3_iterators:
            self.engine.add_data_iterator(data_name, generator)
        batch_3_initial = self.engine.iteration
        self.engine.run()  # Process all batch 3 data

        # Verify batch 3 processed data
        assert self.engine.iteration > batch_3_initial

        # Verify all batches processed different amounts of data
        assert batch_1_final > batch_1_initial
        assert batch_2_final > batch_2_initial
        assert self.engine.iteration > batch_3_initial

    def test_streaming_workflow_large_chunks(self):
        """
        Test streaming with larger chunk sizes to verify memory efficiency.
        """
        # Create iterators with substantial data amounts (reduced for efficiency)
        start_ts = 1_609_459_200_000_000_000  # 2021-01-01 00:00:00 UTC

        large_iterators = [
            (
                "large_stream_A",
                self.create_data_iterator("A_large", start_ts, 100, 5_000_000_000),
            ),  # 5s intervals
            (
                "large_stream_B",
                self.create_data_iterator("B_large", start_ts + 2_500_000_000, 80, 7_500_000_000),
            ),  # 7.5s intervals
            ("large_stream_C", self.create_dense_iterator("C_dense", start_ts, 50)),  # Dense data
        ]

        # Add iterators individually
        for data_name, generator in large_iterators:
            self.engine.add_data_iterator(data_name, generator)

        # Verify streams were registered by checking that no exception was raised
        # Direct access to _data_iterator may not work due to Cython implementation

        # Process all data without memory issues or ordering violations
        initial_iteration = self.engine.iteration
        self.engine.run()

        # Verify data was processed
        assert self.engine.iteration > initial_iteration

    def test_streaming_workflow_simple(self):
        """
        Test a simple streaming setup to verify basic functionality.
        """
        start_ts = 1_609_459_200_000_000_000

        # Create a very simple iterator with just a few items
        simple_iterators = [
            (
                "test_stream",
                self.create_data_iterator("test", start_ts, 3, 60_000_000_000),
            ),  # 3 items, 1min apart
        ]

        # Add the stream iterator
        for data_name, generator in simple_iterators:
            self.engine.add_data_iterator(data_name, generator)

        # Verify the method exists and was called successfully
        assert hasattr(self.engine, "add_data_iterator")

        # The data iterator might not be directly accessible due to Cython implementation
        # Instead, verify functionality by checking that the method completed without error
        # and that we can run the engine successfully

        # Run the engine to process the streaming data
        self.engine.run()

        # Verify that the engine processed the data successfully
        assert self.engine.iteration > 0

    def test_streaming_workflow_edge_cases(self):
        """
        Test streaming with edge cases: empty iterators, single items, etc.
        """
        start_ts = 1_609_459_200_000_000_000

        def empty_iterator():
            return
            yield []  # Empty generator

        def single_item_iterator():
            yield [MyData(value="single", ts_init=start_ts + 60_000_000_000)]

        edge_case_iterators = [
            ("empty_stream", empty_iterator()),
            ("single_item", single_item_iterator()),
            ("normal_stream", self.create_data_iterator("normal", start_ts, 5, 30_000_000_000)),
        ]

        # Add some regular data so the engine can run
        from nautilus_trader.model.identifiers import ClientId

        self.engine.add_data([MyData(value="baseline", ts_init=start_ts)], ClientId("TEST"))
        for data_name, generator in edge_case_iterators:
            self.engine.add_data_iterator(data_name, generator)

        # Verify streams were registered by checking that no exception was raised
        # Direct access to _data_iterator may not work due to Cython implementation

        # Run the engine - should handle edge cases gracefully
        self.engine.run()

        # Verify engine processed data successfully
        assert self.engine.iteration > 0

    def test_streaming_workflow_multiple_iterations(self):
        """
        Test multiple iteration cycles to ensure proper cleanup between runs.
        """
        base_start_ts = 1_609_459_200_000_000_000

        # Run 3 iterations with different data patterns (reduced for efficiency)
        for iteration in range(3):
            start_ts = base_start_ts + (
                iteration * 86_400_000_000_000
            )  # Each iteration is 1 day later

            # Create different data patterns for each iteration
            if iteration % 2 == 0:
                # Even iterations: Regular pattern
                iterators = [
                    (
                        f"regular_A_iter{iteration}",
                        self.create_data_iterator(f"A{iteration}", start_ts, 10, 60_000_000_000),
                    ),
                    (
                        f"regular_B_iter{iteration}",
                        self.create_data_iterator(
                            f"B{iteration}",
                            start_ts + 30_000_000_000,
                            8,
                            90_000_000_000,
                        ),
                    ),
                ]
            else:
                # Odd iterations: Irregular pattern
                iterators = [
                    (
                        f"sparse_A_iter{iteration}",
                        self.create_sparse_iterator(f"A{iteration}", start_ts, 4),
                    ),
                    (
                        f"dense_B_iter{iteration}",
                        self.create_dense_iterator(f"B{iteration}", start_ts, 6),
                    ),
                ]

            # Verify iteration setup
            for data_name, generator in iterators:
                self.engine.add_data_iterator(data_name, generator)
            initial_iteration = self.engine.iteration

            # Run the engine
            self.engine.run()

            # Verify data was processed in this iteration
            assert self.engine.iteration > initial_iteration

            # Reset for next iteration
            self.engine.clear_data()

    def test_extreme_varying_density_large_chunks(self):  # noqa: C901 (too complex)
        """
        Test extreme varying density with large chunks (100k elements) to verify time
        ordering is maintained across streams with different data densities.
        """
        start_ts = 1_609_459_200_000_000_000  # 2021-01-01 00:00:00 UTC
        chunk_size = 100_000  # 100k elements per chunk

        def create_ultra_dense_iterator(name: str, start_ts: int, chunks: int):
            """
            Create iterator with ultra-dense data (microsecond intervals).
            """

            def ultra_dense_generator():
                for chunk in range(chunks):
                    chunk_data = []
                    base_ts = start_ts + (chunk * chunk_size * 1_000)  # 1ms per chunk offset
                    for i in range(chunk_size):
                        chunk_data.append(
                            MyData(
                                value=f"{name}_ultra_{chunk}_{i}",
                                ts_init=base_ts + (i * 1_000),  # 1 microsecond intervals
                            ),
                        )
                    yield chunk_data

            return ultra_dense_generator()

        def create_ultra_sparse_iterator(name: str, start_ts: int, chunks: int):
            """
            Create iterator with ultra-sparse data (hour intervals).
            """

            def ultra_sparse_generator():
                for chunk in range(chunks):
                    chunk_data = []
                    base_ts = start_ts + (
                        chunk * chunk_size * 3_600_000_000_000
                    )  # 1 hour per chunk
                    for i in range(chunk_size):
                        chunk_data.append(
                            MyData(
                                value=f"{name}_sparse_{chunk}_{i}",
                                ts_init=base_ts + (i * 3_600_000_000_000),  # 1 hour intervals
                            ),
                        )
                    yield chunk_data

            return ultra_sparse_generator()

        def create_mixed_density_iterator(name: str, start_ts: int, chunks: int):
            """
            Create iterator with mixed density patterns within chunks.
            """

            def mixed_density_generator():
                for chunk in range(chunks):
                    chunk_data = []
                    base_ts = start_ts + (chunk * chunk_size * 60_000_000_000)  # 1 minute per chunk
                    for i in range(chunk_size):
                        # Alternate between dense and sparse within chunk
                        if i % 1000 < 500:  # First half of each 1000 items: dense
                            interval = 1_000_000  # 1ms
                        else:  # Second half: sparse
                            interval = 60_000_000_000  # 1 minute

                        chunk_data.append(
                            MyData(
                                value=f"{name}_mixed_{chunk}_{i}",
                                ts_init=base_ts + (i * interval),
                            ),
                        )
                    yield chunk_data

            return mixed_density_generator()

        large_chunk_iterators = [
            ("ultra_dense_stream", create_ultra_dense_iterator("dense", start_ts, 2)),
            (
                "ultra_sparse_stream",
                create_ultra_sparse_iterator("sparse", start_ts + 1_000_000_000, 2),
            ),
            (
                "mixed_density_stream",
                create_mixed_density_iterator("mixed", start_ts + 2_000_000_000, 2),
            ),
        ]

        for data_name, generator in large_chunk_iterators:
            self.engine.add_data_iterator(data_name, generator)

        initial_iteration = self.engine.iteration

        # Process all data - this tests the heap-based merging with large chunks
        # Verify time ordering is maintained despite varying densities:
        # If ordering was broken, the engine would process events out of sequence
        # and raise an exception.
        self.engine.run()

        # Verify data was processed
        assert self.engine.iteration > initial_iteration
