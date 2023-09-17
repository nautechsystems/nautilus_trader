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
import copy
import sys
from collections import Counter
from typing import Optional

import msgspec.json
import pytest

from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import NautilusKernelConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.rust.model import BookType
from nautilus_trader.model.data import InstrumentStatusUpdate
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orderbook import OrderBook
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.streaming.writer import generate_signal_class
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.mark.skipif(sys.platform == "win32", reason="failing on Windows")
class TestPersistenceStreaming:
    def setup(self):
        self.catalog: Optional[ParquetDataCatalog] = None

    def _run_default_backtest(self, betfair_catalog):
        self.catalog = betfair_catalog
        instrument = self.catalog.instruments()[0]
        run_config = BetfairTestStubs.betfair_backtest_run_config(
            catalog_path=betfair_catalog.path,
            catalog_fs_protocol="file",
            instrument_id=instrument.id.value,
            flush_interval_ms=5000,
            bypass_logging=False,
        )

        node = BacktestNode(configs=[run_config])

        # Act
        backtest_result = node.run()

        return backtest_result

    def test_feather_writer(self, betfair_catalog):
        # Arrange
        backtest_result = self._run_default_backtest(betfair_catalog)
        instance_id = backtest_result[0].instance_id

        # Assert
        result = self.catalog.read_backtest(
            instance_id=instance_id,
            raise_on_failed_deserialize=True,
        )
        result = dict(Counter([r.__class__.__name__ for r in result]))

        expected = {
            "AccountState": 772,
            "BettingInstrument": 1,
            "ComponentStateChanged": 21,
            "OrderAccepted": 375,
            "OrderBookDelta": 1307,
            "OrderFilled": 397,
            "OrderInitialized": 376,
            "OrderSubmitted": 376,
            "PositionChanged": 394,
            "PositionClosed": 2,
            "PositionOpened": 3,
            "TradeTick": 179,
        }

        assert result == expected

    def test_feather_writer_generic_data(self, betfair_catalog):
        # Arrange
        self.catalog = betfair_catalog
        TestPersistenceStubs.setup_news_event_persistence()

        # Load news events into catalog
        news_events = TestPersistenceStubs.news_events()
        self.catalog.write_data(news_events)

        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=NewsEventData.fully_qualified_name(),
            client_id="NewsClient",
        )
        # Add some arbitrary instrument data to appease BacktestEngine
        instrument_data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=InstrumentStatusUpdate.fully_qualified_name(),
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
        )

        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(streaming=streaming),
            data=[data_config, instrument_data_config],
            venues=[BetfairTestStubs.betfair_venue_config()],
        )

        # Act
        node = BacktestNode(configs=[run_config])
        r = node.run()

        # Assert
        result = self.catalog.read_backtest(
            instance_id=r[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        result = Counter([r.__class__.__name__ for r in result])
        assert result["NewsEventData"] == 86985

    def test_feather_writer_signal_data(self, betfair_catalog):
        # Arrange
        self.catalog = betfair_catalog
        instrument_id = self.catalog.instruments(as_nautilus=True)[0].id.value
        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=TradeTick,
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
        )
        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                streaming=streaming,
                strategies=[
                    ImportableStrategyConfig(
                        strategy_path="nautilus_trader.examples.strategies.signal_strategy:SignalStrategy",
                        config_path="nautilus_trader.examples.strategies.signal_strategy:SignalStrategyConfig",
                        config={"instrument_id": instrument_id},
                    ),
                ],
            ),
            data=[data_config],
            venues=[BetfairTestStubs.betfair_venue_config()],
        )

        # Act
        node = BacktestNode(configs=[run_config])
        r = node.run()

        # Assert
        result = self.catalog.read_backtest(
            instance_id=r[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        result = Counter([r.__class__.__name__ for r in result])
        assert result["SignalCounter"] == 179

    def test_generate_signal_class(self):
        # Arrange
        cls = generate_signal_class(name="test", value_type=float)

        # Act
        instance = cls(value=5.0, ts_event=0, ts_init=0)

        # Assert
        assert isinstance(instance, Data)
        assert instance.ts_event == 0
        assert instance.value == 5.0
        assert instance.ts_init == 0

    def test_config_write(self, betfair_catalog):
        # Arrange
        self.catalog = betfair_catalog
        instrument_id = self.catalog.instruments(as_nautilus=True)[0].id.value
        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
        )
        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=TradeTick,
        )

        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                streaming=streaming,
                strategies=[
                    ImportableStrategyConfig(
                        strategy_path="nautilus_trader.examples.strategies.signal_strategy:SignalStrategy",
                        config_path="nautilus_trader.examples.strategies.signal_strategy:SignalStrategyConfig",
                        config={"instrument_id": instrument_id},
                    ),
                ],
            ),
            data=[data_config],
            venues=[BetfairTestStubs.betfair_venue_config()],
        )

        # Act
        node = BacktestNode(configs=[run_config])
        r = node.run()

        # Assert
        config_file = f"{self.catalog.path}/backtest/{r[0].instance_id}/config.json"
        assert self.catalog.fs.exists(config_file)
        raw = self.catalog.fs.open(config_file, "rb").read()
        assert msgspec.json.decode(raw, type=NautilusKernelConfig)

    def test_feather_reader_returns_cython_objects(self, betfair_catalog):
        # Arrange
        backtest_result = self._run_default_backtest(betfair_catalog)
        instance_id = backtest_result[0].instance_id

        # Act
        result = self.catalog.read_backtest(
            instance_id=instance_id,
            raise_on_failed_deserialize=True,
        )

        # Assert
        assert len([d for d in result if isinstance(d, TradeTick)]) == 179
        assert len([d for d in result if isinstance(d, OrderBookDelta)]) == 1307

    def test_feather_reader_order_book_deltas(self, betfair_catalog):
        # Arrange
        backtest_result = self._run_default_backtest(betfair_catalog)
        book = OrderBook(
            instrument_id=InstrumentId.from_str("1.166564490-237491-0.0.BETFAIR"),
            book_type=BookType.L2_MBP,
        )

        # Act
        result = self.catalog.read_backtest(
            instance_id=backtest_result[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        updates = [d for d in result if isinstance(d, OrderBookDelta)]

        # Assert
        for update in updates[:10]:
            book.apply_delta(update)
            copy.deepcopy(book)

    def test_read_backtest(self, betfair_catalog: ParquetDataCatalog):
        # Arrange
        [backtest_result] = self._run_default_backtest(betfair_catalog)

        # Act
        data = betfair_catalog.read_backtest(backtest_result.instance_id)
        counts = dict(Counter([d.__class__.__name__ for d in data]))

        # Assert
        expected = {
            "OrderBookDelta": 1307,
            "AccountState": 772,
            "OrderFilled": 397,
            "PositionChanged": 394,
            "OrderInitialized": 376,
            "OrderSubmitted": 376,
            "OrderAccepted": 375,
            "TradeTick": 179,
            "ComponentStateChanged": 21,
            "PositionOpened": 3,
            "PositionClosed": 2,
            "BettingInstrument": 1,
        }
        assert counts == expected
