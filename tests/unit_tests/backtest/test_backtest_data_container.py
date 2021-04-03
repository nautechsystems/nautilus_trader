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

from nautilus_trader.backtest.data_container import BacktestDataContainer
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.enums import OrderBookOperationType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.orderbook.book import OrderBookOperation
from nautilus_trader.model.orderbook.book import OrderBookOperations
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.order import Order
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestBacktestDataContainer:
    def test_total_data_size_with_no_data_returns_0(self):
        # Arrange
        data = BacktestDataContainer()

        # Act
        # Assert: memory size of internal dicts currently 424
        assert data.total_data_size() > 0

    def test_add_generic_data_adds_to_container(self):
        # Arrange
        data = BacktestDataContainer()
        data_type = DataType(str, metadata={"news_wire": "hacks"})

        generic_data1 = [
            GenericData(data_type, data="AAPL hacked", timestamp_ns=0),
            GenericData(data_type, data="AMZN hacked", timestamp_ns=1000),
            GenericData(data_type, data="NFLX hacked", timestamp_ns=3000),
            GenericData(data_type, data="MSFT hacked", timestamp_ns=2000),
        ]

        generic_data2 = [
            GenericData(data_type, data="FB hacked", timestamp_ns=1500),
        ]

        # Act
        data.add_generic_data("NEWS_CLIENT", generic_data1)
        data.add_generic_data("NEWS_CLIENT", generic_data2)

        # Assert
        assert "NEWS_CLIENT" in data.clients
        assert len(data.generic_data) == 5
        assert data.generic_data[-1].timestamp_ns == 3000  # sorted

    def test_add_instrument_adds_to_container(self):
        # Arrange
        data = BacktestDataContainer()

        # Act
        data.add_instrument(ETHUSDT_BINANCE)

        # Assert
        assert ETHUSDT_BINANCE.id in data.instruments
        assert data.instruments[ETHUSDT_BINANCE.id] == ETHUSDT_BINANCE

    def test_add_order_book_snapshots_adds_to_container(self):
        # Arrange
        data = BacktestDataContainer()

        snapshot1 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            bids=[[1550.15, 0.51], [1580.00, 1.20]],
            asks=[[1552.15, 1.51], [1582.00, 2.20]],
            timestamp_ns=0,
        )

        snapshot2 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            bids=[[1551.15, 0.51], [1581.00, 1.20]],
            asks=[[1553.15, 1.51], [1583.00, 2.20]],
            timestamp_ns=1_000_000_000,
        )

        # Act
        data.add_order_book_snapshots([snapshot2, snapshot1])  # <-- reverse order

        # Assert
        assert "BINANCE" in data.clients
        assert ETHUSDT_BINANCE.id in data.books
        assert data.order_book_snapshots == [snapshot1, snapshot2]  # <-- sorted

    def test_add_order_book_operations_adds_to_container(self):
        # Arrange
        data = BacktestDataContainer()

        ops = [
            OrderBookOperation(
                OrderBookOperationType.ADD,
                Order(13.0, 40, OrderSide.SELL),
                timestamp_ns=0,
            ),
            OrderBookOperation(
                OrderBookOperationType.ADD,
                Order(12.0, 30, OrderSide.SELL),
                timestamp_ns=0,
            ),
            OrderBookOperation(
                OrderBookOperationType.ADD,
                Order(11.0, 20, OrderSide.SELL),
                timestamp_ns=0,
            ),
            OrderBookOperation(
                OrderBookOperationType.ADD,
                Order(10.0, 20, OrderSide.BUY),
                timestamp_ns=0,
            ),
            OrderBookOperation(
                OrderBookOperationType.ADD,
                Order(9.0, 30, OrderSide.BUY),
                timestamp_ns=0,
            ),
            OrderBookOperation(
                OrderBookOperationType.ADD,
                Order(0.0, 40, OrderSide.BUY),
                timestamp_ns=0,
            ),
        ]

        operations1 = OrderBookOperations(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            ops=ops,
            timestamp_ns=0,
        )

        operations2 = OrderBookOperations(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            ops=ops,
            timestamp_ns=1000,
        )

        # Act
        data.add_order_book_operations([operations2, operations1])  # <-- not sorted

        # Assert
        assert "BINANCE" in data.clients
        assert ETHUSDT_BINANCE.id in data.books
        assert data.order_book_operations == [operations1, operations2]  # <-- sorted

    def test_add_quote_ticks_adds_to_container(self):
        # Arrange
        data = BacktestDataContainer()

        # Act
        data.add_quote_ticks(
            instrument_id=AUDUSD_SIM.id,
            data=TestDataProvider.audusd_ticks(),
        )

        # Assert
        assert "SIM" in data.clients
        assert data.has_quote_data(AUDUSD_SIM.id)
        assert AUDUSD_SIM.id in data.quote_ticks
        assert len(data.quote_ticks[AUDUSD_SIM.id]) == 100000

    def test_add_trade_ticks_adds_to_container(self):
        # Arrange
        data = BacktestDataContainer()

        # Act
        data.add_trade_ticks(
            instrument_id=ETHUSDT_BINANCE.id,
            data=TestDataProvider.ethusdt_trades(),
        )

        # Assert
        assert "BINANCE" in data.clients
        assert data.has_trade_data(ETHUSDT_BINANCE.id)
        assert ETHUSDT_BINANCE.id in data.trade_ticks
        assert len(data.trade_ticks[ETHUSDT_BINANCE.id]) == 69806

    def test_add_bars_adds_to_container(self):
        # Arrange
        data = BacktestDataContainer()
        data.add_instrument(USDJPY_SIM)

        # Act
        data.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid()[:2000],
        )

        data.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask()[:2000],
        )

        # Assert
        assert "SIM" in data.clients
        assert USDJPY_SIM.id in data.bars_ask
        assert USDJPY_SIM.id in data.bars_bid
        assert len(data.bars_bid[USDJPY_SIM.id]) == 1  # MINUTE key
        assert len(data.bars_ask[USDJPY_SIM.id]) == 1  # MINUTE key

    def test_check_integrity_when_no_date_ok(self):
        # Arrange
        data = BacktestDataContainer()

        # Act
        data.check_integrity()

        # Assert
        assert True  # No exceptions raised

    def test_check_integrity_no_execution_data_for_instrument_raises_runtime_error(
        self,
    ):
        # Arrange
        data = BacktestDataContainer()
        data.add_instrument(USDJPY_SIM)

        # Act
        # Assert
        with pytest.raises(RuntimeError):
            data.check_integrity()

    def test_check_integrity_when_instrument_not_added_raises_runtime_error(self):
        # Arrange
        data = BacktestDataContainer()

        # Act
        data.add_trade_ticks(
            instrument_id=ETHUSDT_BINANCE.id,
            data=TestDataProvider.ethusdt_trades(),
        )

        # Assert
        with pytest.raises(RuntimeError):
            data.check_integrity()

    def test_check_integrity_when_bid_ask_bars_not_symmetrical_raises_runtime_error(
        self,
    ):
        # Arrange
        data = BacktestDataContainer()
        data.add_instrument(USDJPY_SIM)

        # Act
        data.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid()[:2000],
        )

        # Assert
        with pytest.raises(RuntimeError):
            data.check_integrity()
