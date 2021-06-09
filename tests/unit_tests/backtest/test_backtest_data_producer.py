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

import pandas as pd

from nautilus_trader.backtest.data_producer import BacktestDataProducer
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import MyData


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestBacktestDataProducer:
    def setup(self):
        self.logger = Logger(clock=TestClock())

    def test_producer_when_data_not_setup(self):
        # Arrange
        producer = BacktestDataProducer(logger=self.logger)

        # Act
        # Assert
        assert producer.min_timestamp_ns == 9223372036854774784  # int64 max
        assert producer.max_timestamp_ns == -9223372036854774784  # int64 min
        assert producer.min_timestamp == pd.Timestamp(
            "2262-04-11 23:47:16.854774+0000", tz="UTC"
        )
        assert producer.max_timestamp == pd.Timestamp(
            "1677-09-21 00:12:43.145226+0000", tz="UTC"
        )
        assert not producer.has_data
        assert producer.next() is None

    def test_with_mix_of_stream_data_produces_correct_stream_of_data(self):
        # Assert
        snapshot1 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            bids=[[1550.15, 0.51], [1580.00, 1.20]],
            asks=[[1552.15, 1.51], [1582.00, 2.20]],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        data_type = DataType(MyData, metadata={"news_wire": "hacks"})
        generic_data1 = [
            GenericData(
                data_type,
                data=MyData("AAPL hacked"),
            ),
            GenericData(
                data_type,
                data=MyData("AMZN hacked", 500_000, 500_000),
            ),
            GenericData(
                data_type,
                data=MyData("NFLX hacked", 1_000_000, 1_000_000),
            ),
            GenericData(
                data_type,
                data=MyData("MSFT hacked", 2_000_000, 2_000_000),
            ),
        ]

        snapshot2 = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            bids=[[1551.15, 0.51], [1581.00, 1.20]],
            asks=[[1553.15, 1.51], [1583.00, 2.20]],
            ts_event_ns=1_000_000,
            ts_recv_ns=1_000_000,
        )

        producer = BacktestDataProducer(
            logger=self.logger,
            instruments=[ETHUSDT_BINANCE],
            generic_data=generic_data1,
            order_book_data=[snapshot1, snapshot2],
        )
        producer.setup(producer.min_timestamp_ns, producer.max_timestamp_ns)

        # Act
        streamed_data = []

        while producer.has_data:
            streamed_data.append(producer.next())

        # Assert
        timestamps = [x.ts_recv_ns for x in streamed_data]
        assert timestamps == [0, 0, 500000, 1000000, 1000000, 2000000]
        assert producer.min_timestamp_ns == 0
        assert producer.max_timestamp_ns == 2_000_000
        assert producer.min_timestamp == pd.Timestamp(
            "1970-01-01 00:00:00.000000+0000", tz="UTC"
        )
        assert producer.max_timestamp == pd.Timestamp(
            "1970-01-01 00:00:00.002000+0000", tz="UTC"
        )

    def test_with_bars_produces_correct_stream_of_data(self):
        # Arrange
        producer = BacktestDataProducer(
            logger=self.logger,
            instruments=[USDJPY_SIM],
            bars_bid={
                USDJPY_SIM.id: {
                    BarAggregation.MINUTE: TestDataProvider.usdjpy_1min_bid()[:2000]
                }
            },
            bars_ask={
                USDJPY_SIM.id: {
                    BarAggregation.MINUTE: TestDataProvider.usdjpy_1min_ask()[:2000]
                }
            },
        )
        producer.setup(producer.min_timestamp_ns, producer.max_timestamp_ns)

        # Act
        next_data = producer.next()

        # Assert
        assert next_data.ts_recv_ns == 1359676799800000000
        assert next_data.instrument_id == USDJPY_SIM.id
        assert str(next_data.bid) == "91.715"
        assert str(next_data.ask) == "91.717"
        assert str(next_data.bid_size) == "1000000"
        assert str(next_data.ask_size) == "1000000"
