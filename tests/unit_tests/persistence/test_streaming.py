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

from collections import Counter

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks.data import NewsEventData
from tests.test_kit.mocks.data import data_catalog_setup
from tests.test_kit.stubs.persistence import TestPersistenceStubs


class TestPersistenceStreaming:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()
        self.fs = self.catalog.fs
        self._loaded_data_into_catalog()

    def _loaded_data_into_catalog(self):
        self.instrument_provider = BetfairInstrumentProvider.from_instruments([])
        result = process_files(
            glob_path=PACKAGE_ROOT + "/data/1.166564490*.bz2",
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
        return data

    def test_feather_writer(self):
        # Arrange
        instrument = self.catalog.instruments(as_nautilus=True)[0]
        run_config = BetfairTestStubs.betfair_backtest_run_config(
            catalog_path=str(self.catalog.path),
            catalog_fs_protocol=self.catalog.fs.protocol,
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
            "AccountState": 666,
            "BettingInstrument": 2,
            "ComponentStateChanged": 11,
            "OrderAccepted": 322,
            "OrderBookDeltas": 1077,
            "OrderBookSnapshot": 1,
            "OrderFilled": 344,
            "OrderInitialized": 323,
            "OrderSubmitted": 323,
            "PositionChanged": 343,
            "PositionOpened": 1,
            "TradeTick": 198,
        }
        assert result == expected

    def test_feather_writer_generic_data(self):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        process_files(
            glob_path=f"{PACKAGE_ROOT}/data/news_events.csv",
            reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
            catalog=self.catalog,
        )
        data_config = BacktestDataConfig(
            catalog_path="/.nautilus/catalog",
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            client_id="NewsClient",
        )
        # Add some arbitrary instrument data to appease BacktestEngine
        instrument_data_config = BacktestDataConfig(
            catalog_path="/.nautilus/catalog",
            catalog_fs_protocol="memory",
            data_cls=InstrumentStatusUpdate,
        )
        streaming = BetfairTestStubs.streaming_config(catalog_path=self.catalog.path)
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
