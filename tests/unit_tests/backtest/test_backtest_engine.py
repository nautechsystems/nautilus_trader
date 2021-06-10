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

import pytest

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import DeltaType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.order import Order
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import MyData


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestBacktestEngineData:
    def test_add_generic_data_adds_to_container(self):
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
        # TODO: WIP - Implement asserts
        # assert ClientId("NEWS_CLIENT") in data.clients
        # assert len(data.generic_data) == 5
        # assert data.generic_data[-1].ts_recv_ns == 3000  # sorted

    def test_add_instrument_adds_to_container(self):
        # Arrange
        engine = BacktestEngine()

        # Act
        engine.add_instrument(ETHUSDT_BINANCE)

        # Assert
        # TODO: WIP - Implement asserts
        # assert ETHUSDT_BINANCE.id in data.instruments
        # assert data.instruments[ETHUSDT_BINANCE.id] == ETHUSDT_BINANCE

    def test_add_order_book_snapshots_adds_to_container(self):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(ETHUSDT_BINANCE)

        snapshot1 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            bids=[[1550.15, 0.51], [1580.00, 1.20]],
            asks=[[1552.15, 1.51], [1582.00, 2.20]],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        snapshot2 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            bids=[[1551.15, 0.51], [1581.00, 1.20]],
            asks=[[1553.15, 1.51], [1583.00, 2.20]],
            ts_event_ns=1_000_000_000,
            ts_recv_ns=1_000_000_000,
        )

        # Act
        engine.add_order_book_data([snapshot2, snapshot1])  # <-- reverse order

        # Assert
        # TODO: WIP - Implement asserts
        # assert ClientId("BINANCE") in data.clients
        # assert ETHUSDT_BINANCE.id in data.books
        # assert data.order_book_data == [snapshot1, snapshot2]  # <-- sorted

    def test_add_order_book_operations_adds_to_container(self):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(AUDUSD_SIM)
        engine.add_instrument(ETHUSDT_BINANCE)

        deltas = [
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                level=OrderBookLevel.L2,
                delta_type=DeltaType.ADD,
                order=Order(
                    price=Price.from_str("13.0"),
                    volume=Quantity.from_str("40"),
                    side=OrderSide.SELL,
                ),
                ts_event_ns=0,
                ts_recv_ns=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                level=OrderBookLevel.L2,
                delta_type=DeltaType.ADD,
                order=Order(
                    price=Price.from_str("12.0"),
                    volume=Quantity.from_str("30"),
                    side=OrderSide.SELL,
                ),
                ts_event_ns=0,
                ts_recv_ns=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                level=OrderBookLevel.L2,
                delta_type=DeltaType.ADD,
                order=Order(
                    price=Price.from_str("11.0"),
                    volume=Quantity.from_str("20"),
                    side=OrderSide.SELL,
                ),
                ts_event_ns=0,
                ts_recv_ns=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                level=OrderBookLevel.L2,
                delta_type=DeltaType.ADD,
                order=Order(
                    price=Price.from_str("10.0"),
                    volume=Quantity.from_str("20"),
                    side=OrderSide.BUY,
                ),
                ts_event_ns=0,
                ts_recv_ns=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                level=OrderBookLevel.L2,
                delta_type=DeltaType.ADD,
                order=Order(
                    price=Price.from_str("9.0"),
                    volume=Quantity.from_str("30"),
                    side=OrderSide.BUY,
                ),
                ts_event_ns=0,
                ts_recv_ns=0,
            ),
            OrderBookDelta(
                instrument_id=AUDUSD_SIM.id,
                level=OrderBookLevel.L2,
                delta_type=DeltaType.ADD,
                order=Order(
                    price=Price.from_str("0.0"),
                    volume=Quantity.from_str("40"),
                    side=OrderSide.BUY,
                ),
                ts_event_ns=0,
                ts_recv_ns=0,
            ),
        ]

        operations1 = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            deltas=deltas,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        operations2 = OrderBookDeltas(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            deltas=deltas,
            ts_event_ns=1000,
            ts_recv_ns=1000,
        )

        # Act
        engine.add_order_book_data([operations2, operations1])  # <-- not sorted

        # Assert
        # TODO: WIP - Implement asserts
        # assert ClientId("BINANCE") in data.clients
        # assert ETHUSDT_BINANCE.id in data.books
        # assert data.order_book_data == [operations1, operations2]  # <-- sorted

    def test_add_quote_ticks_adds_to_container(self):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(AUDUSD_SIM)

        # Act
        engine.add_quote_ticks(
            instrument_id=AUDUSD_SIM.id,
            data=TestDataProvider.audusd_ticks(),
        )

        # Assert
        # TODO: WIP - Implement asserts
        # assert ClientId("SIM") in data.clients
        # assert data.has_quote_data(AUDUSD_SIM.id)
        # assert AUDUSD_SIM.id in data.quote_ticks
        # assert len(data.quote_ticks[AUDUSD_SIM.id]) == 100000

    def test_add_trade_ticks_adds_to_container(self):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(ETHUSDT_BINANCE)

        # Act
        engine.add_trade_ticks(
            instrument_id=ETHUSDT_BINANCE.id,
            data=TestDataProvider.ethusdt_trades(),
        )

        # Assert
        # TODO: WIP - Implement asserts
        # assert ClientId("BINANCE") in data.clients
        # assert data.has_trade_data(ETHUSDT_BINANCE.id)
        # assert ETHUSDT_BINANCE.id in data.trade_ticks
        # assert len(data.trade_ticks[ETHUSDT_BINANCE.id]) == 69806

    def test_add_trade_tick_objects_adds_to_container(self):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(ETHUSDT_BINANCE)

        # Act
        engine.add_trade_tick_objects(
            instrument_id=ETHUSDT_BINANCE.id,
            data=TestDataProvider.betfair_trade_ticks(),
        )

    def test_add_bars_adds_to_container(self):
        # Arrange
        engine = BacktestEngine()
        engine.add_instrument(USDJPY_SIM)

        # Act
        engine.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid()[:2000],
        )

        engine.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask()[:2000],
        )

        # Assert
        # TODO: WIP - Implement asserts
        # assert ClientId("SIM") in data.clients
        # assert USDJPY_SIM.id in data.bars_ask
        # assert USDJPY_SIM.id in data.bars_bid
        # assert len(data.bars_bid[USDJPY_SIM.id]) == 1  # MINUTE key
        # assert len(data.bars_ask[USDJPY_SIM.id]) == 1  # MINUTE key

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
        engine.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid()[:2000],
        )

        # Assert
        with pytest.raises(RuntimeError):
            engine.add_bars(
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
        self.engine.add_bars(
            usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid()[:2000],
        )
        self.engine.add_bars(
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
        # Arrange
        # Act
        self.engine.run(strategies=[TradingStrategy("000")])
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
        # Arrange
        # Act
        self.engine.run()

        # Assert
        assert self.engine.iteration == 7999

    def test_change_fill_model(self):
        # Arrange
        # Act
        self.engine.change_fill_model(Venue("SIM"), FillModel())

        # Assert
        assert True  # No exceptions raised
