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

import sys
from collections import Counter

import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.core.data import Data
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.persistence.streaming import generate_signal_class
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.mark.skipif(sys.platform == "win32", reason="failing on Windows")
class TestPersistenceStreaming:
    def setup(self):
        self.catalog = data_catalog_setup(protocol="memory", path="/.nautilus/catalog")  # ,
        self.fs = self.catalog.fs
        self._load_data_into_catalog()
        self._logger = Logger(clock=LiveClock())
        self.logger = LoggerAdapter("test", logger=self._logger)

    def _load_data_into_catalog(self):
        self.instrument_provider = BetfairInstrumentProvider.from_instruments([])
        result = process_files(
            glob_path=TEST_DATA_DIR + "/1.166564490*.bz2",
            reader=BetfairTestStubs.betfair_reader(instrument_provider=self.instrument_provider),
            instrument_provider=self.instrument_provider,
            catalog=self.catalog,
        )
        assert result
        data = (
            self.catalog.instruments(as_nautilus=True)
            + self.catalog.instrument_status_updates(as_nautilus=True)
            + self.catalog.trade_ticks(as_nautilus=True)
            + self.catalog.order_book_deltas(as_nautilus=True)
            + self.catalog.tickers(as_nautilus=True)
        )
        assert len(data) == 2535

    @pytest.mark.skipif(sys.platform == "win32", reason="Currently flaky on Windows")
    def test_feather_writer(self):
        # Arrange
        instrument = self.catalog.instruments(as_nautilus=True)[0]
        run_config = BetfairTestStubs.betfair_backtest_run_config(
            catalog_path="/.nautilus/catalog",
            catalog_fs_protocol="memory",
            instrument_id=instrument.id.value,
        )
        run_config.engine.streaming.flush_interval_ms = 5000
        node = BacktestNode(configs=[run_config])

        # Act
        backtest_result = node.run()

        # Assert
        result = self.catalog.read_backtest(
            backtest_run_id=backtest_result[0].instance_id,
            raise_on_failed_deserialize=True,
        )
        result = dict(Counter([r.__class__.__name__ for r in result]))

        expected = {
            "ComponentStateChanged": 21,
            "OrderBookSnapshot": 1,
            "TradeTick": 198,
            "OrderBookDeltas": 1077,
            "AccountState": 648,
            "OrderAccepted": 324,
            "OrderFilled": 324,
            "OrderInitialized": 325,
            "OrderSubmitted": 324,
            "PositionOpened": 1,
            "PositionChanged": 323,
            "OrderDenied": 1,
            "BettingInstrument": 1,
        }

        assert result == expected

    def test_feather_writer_generic_data(self):

        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()

        process_files(
            glob_path=f"{TEST_DATA_DIR}/news_events.csv",
            reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
            catalog=self.catalog,
        )

        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="memory",
            data_cls=NewsEventData.fully_qualified_name(),
            client_id="NewsClient",
        )
        # Add some arbitrary instrument data to appease BacktestEngine
        instrument_data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="memory",
            data_cls=InstrumentStatusUpdate.fully_qualified_name(),
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
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
            backtest_run_id=r[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        result = Counter([r.__class__.__name__ for r in result])
        assert result["NewsEventData"] == 86985

    @pytest.mark.skip(reason="fix after merge")
    def test_feather_writer_signal_data(self):

        # Arrange
        instrument_id = self.catalog.instruments(as_nautilus=True)[0].id.value
        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="memory",
            data_cls=TradeTick,
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
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
            backtest_run_id=r[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        result = Counter([r.__class__.__name__ for r in result])
        assert result["SignalCounter"] == 114

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
