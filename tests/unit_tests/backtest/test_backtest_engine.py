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
from decimal import Decimal

import pandas as pd
import pytest

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.data.wrangling import BarDataWrangler
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import DeltaType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import Order
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.trading.strategy import TradingStrategy
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.strategies import EMACross
from tests.test_kit.strategies import EMACrossConfig
from tests.test_kit.stubs import MyData
from tests.test_kit.stubs import TestStubs


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestBacktestEngineData:
    def test_add_generic_data_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()

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
        engine.add_generic_data(ClientId("NEWS_CLIENT"), generic_data1)
        engine.add_generic_data(ClientId("NEWS_CLIENT"), generic_data2)

        # Assert
        log = "".join(capsys.readouterr())
        assert "Added 4 GenericData points." in log
        assert "Added 1 GenericData points." in log

    def test_add_instrument_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()

        # Act
        engine.add_instrument(ETHUSDT_BINANCE)

        # Assert
        log = "".join(capsys.readouterr())
        assert "Added ETH/USDT.BINANCE Instrument." in log

    def test_add_order_book_snapshots_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(ETHUSDT_BINANCE)

        snapshot1 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=BookLevel.L2,
            bids=[[1550.15, 0.51], [1580.00, 1.20]],
            asks=[[1552.15, 1.51], [1582.00, 2.20]],
            ts_event=0,
            ts_init=0,
        )

        snapshot2 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=BookLevel.L2,
            bids=[[1551.15, 0.51], [1581.00, 1.20]],
            asks=[[1553.15, 1.51], [1583.00, 2.20]],
            ts_event=1_000_000_000,
            ts_init=1_000_000_000,
        )

        # Act
        engine.add_order_book_data([snapshot2, snapshot1])  # <-- reverse order

        # Assert
        log = "".join(capsys.readouterr())
        assert "Added 2 ETH/USDT.BINANCE OrderBookData elements (total: 2)." in log

    def test_add_order_book_operations_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(AUDUSD_SIM)
        engine.add_instrument(ETHUSDT_BINANCE)

        deltas = [
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                level=BookLevel.L2,
                delta_type=DeltaType.ADD,
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
                level=BookLevel.L2,
                delta_type=DeltaType.ADD,
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
                level=BookLevel.L2,
                delta_type=DeltaType.ADD,
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
                level=BookLevel.L2,
                delta_type=DeltaType.ADD,
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
                level=BookLevel.L2,
                delta_type=DeltaType.ADD,
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
                level=BookLevel.L2,
                delta_type=DeltaType.ADD,
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
            level=BookLevel.L2,
            deltas=deltas,
            ts_event=0,
            ts_init=0,
        )

        operations2 = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            level=BookLevel.L2,
            deltas=deltas,
            ts_event=1000,
            ts_init=1000,
        )

        # Act
        engine.add_order_book_data([operations2, operations1])  # <-- not sorted

        # Assert
        log = "".join(capsys.readouterr())
        assert "Added 2 ETH/USDT.BINANCE OrderBookData elements (total: 2)." in log

    def test_add_quote_ticks_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(AUDUSD_SIM)

        # Act
        engine.add_quote_ticks(
            instrument_id=AUDUSD_SIM.id,
            data=TestDataProvider.audusd_ticks(),
        )

        # Assert
        log = "".join(capsys.readouterr())
        assert "Added 100000 AUD/USD.SIM QuoteTick data elements." in log

    def test_add_trade_ticks_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(ETHUSDT_BINANCE)

        # Act
        engine.add_trade_ticks(
            instrument_id=ETHUSDT_BINANCE.id,
            data=TestDataProvider.ethusdt_trades(),
        )

        # Assert
        log = "".join(capsys.readouterr())
        assert "Added 69806 ETH/USDT.BINANCE TradeTick data elements." in log

    def test_add_trade_tick_objects_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(ETHUSDT_BINANCE)

        # Act
        engine.add_trade_tick_objects(
            instrument_id=ETHUSDT_BINANCE.id,
            data=BetfairDataProvider.betfair_trade_ticks(),
        )
        log = "".join(capsys.readouterr())
        assert "Added 17798 ETH/USDT.BINANCE TradeTick data elements." in log

    def test_add_bars_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(USDJPY_SIM)

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

        # Act
        engine.add_bars(
            instrument=USDJPY_SIM,
            bar_type=bar_type,
            data=TestDataProvider.usdjpy_1min_bid()[:2000],
        )

        # Assert
        log = "".join(capsys.readouterr())
        assert "Added USD/JPY.SIM Instrument." in log
        assert "Added 2000 USD/JPY.SIM MINUTE-BID tick rows." in log
        assert "Added 2000 USD/JPY.SIM-1-MINUTE-BID-EXTERNAL bar elements." in log

    def test_add_bar_objects_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(USDJPY_SIM)

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

        bars = BarDataWrangler(
            bar_type=bar_type,
            price_precision=USDJPY_SIM.price_precision,
            size_precision=USDJPY_SIM.size_precision,
            data=TestDataProvider.usdjpy_1min_bid()[:2000],
        ).build_bars_all()

        # Act
        engine.add_bar_objects(
            bar_type=bar_type,
            bars=bars,
        )

        # Assert
        log = "".join(capsys.readouterr())
        assert "Added USD/JPY.SIM Instrument." in log
        assert "Added 2000 USD/JPY.SIM-1-MINUTE-BID-EXTERNAL bar elements." in log

    def test_add_bars_as_ticks_adds_to_engine(self, capsys):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(USDJPY_SIM)

        # Act
        engine.add_bars_as_ticks(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid()[:2000],
        )

        engine.add_bars_as_ticks(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask()[:2000],
        )

        # Assert
        log = "".join(capsys.readouterr())
        assert "Added 2000 USD/JPY.SIM MINUTE-BID tick rows." in log
        assert "Added 2000 USD/JPY.SIM MINUTE-ASK tick rows." in log

    def test_check_integrity_when_instrument_not_added_raises_runtime_error(self):
        # Arrange
        engine = BacktestEngine()

        # Act, Assert
        with pytest.raises(ValueError):
            engine.add_trade_ticks(
                instrument_id=ETHUSDT_BINANCE.id,
                data=TestDataProvider.ethusdt_trades(),
            )

    def test_check_integrity_when_bid_ask_bars_not_symmetrical_raises_runtime_error(
        self,
    ):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(USDJPY_SIM)

        # Act
        engine.add_bars_as_ticks(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid()[:2000],
        )

        # Assert
        with pytest.raises(RuntimeError):
            engine.add_bars_as_ticks(
                USDJPY_SIM.id,
                BarAggregation.MINUTE,
                PriceType.ASK,
                TestDataProvider.usdjpy_1min_ask()[:1999],
            )


class TestBacktestEngine:
    def setup(self):
        # Fixture Setup
        self.engine = BacktestEngine(use_data_cache=True)

        usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        self.engine.add_instrument(usdjpy)
        self.engine.add_bars_as_ticks(
            usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid()[:2000],
        )
        self.engine.add_bars_as_ticks(
            usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask()[:2000],
        )

        self.engine.add_venue(
            venue=Venue("SIM"),
            venue_type=VenueType.BROKERAGE,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=FillModel(),
        )

    def teardown(self):
        self.engine.reset()
        self.engine.dispose()

    def test_initialization(self):
        # Arrange, Act
        self.engine.run(strategies=[TradingStrategy()])
        # Assert
        assert len(self.engine.trader.strategy_states()) == 1

    def test_reset_engine(self):
        # Arrange
        self.engine.run()

        # Act
        self.engine.reset()

        # Assert
        assert self.engine.iteration == 0  # No exceptions raised

    def test_run_empty_strategy(self):
        # Arrange, Act
        self.engine.run()

        # Assert
        assert self.engine.iteration == 7999

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


class TestBacktestWithAddedBars:
    def setup(self):
        # Fixture Setup
        self.engine = BacktestEngine(
            bypass_logging=True,
            run_analysis=False,
        )

        self.venue = Venue("SIM")
        self.engine.add_instrument(GBPUSD_SIM)

        bid_bar_type = BarType(
            instrument_id=GBPUSD_SIM.id,
            bar_spec=TestStubs.bar_spec_1min_bid(),
            aggregation_source=AggregationSource.EXTERNAL,  # <-- important
        )

        self.engine.add_bars(
            instrument=GBPUSD_SIM,
            bar_type=bid_bar_type,
            data=TestDataProvider.gbpusd_1min_bid(),
        )

        ask_bar_type = BarType(
            instrument_id=GBPUSD_SIM.id,
            bar_spec=TestStubs.bar_spec_1min_ask(),
            aggregation_source=AggregationSource.EXTERNAL,  # <-- important
        )

        self.engine.add_bars(
            instrument=GBPUSD_SIM,
            bar_type=ask_bar_type,
            data=TestDataProvider.gbpusd_1min_ask(),
        )

        self.engine.add_venue(
            venue=self.venue,
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_added_bars(self):
        # Arrange
        bar_type = BarType(
            instrument_id=GBPUSD_SIM.id,
            bar_spec=TestStubs.bar_spec_1min_bid(),
            aggregation_source=AggregationSource.EXTERNAL,  # <-- important
        )
        config = EMACrossConfig(
            instrument_id=str(GBPUSD_SIM.id),
            bar_type=str(bar_type),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)

        # Act
        self.engine.run(strategies=[strategy])

        # Assert
        assert strategy.fast_ema.count == 30117
        assert self.engine.iteration == 180701
        assert self.engine.portfolio.account(self.venue).balance_total(USD) == Money(749122.06, USD)
