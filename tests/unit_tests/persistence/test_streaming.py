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

import pathlib
import sys

import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.config import PersistenceConfig
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.streaming import read_feather
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT


@pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")
class TestPersistenceStreaming:
    def setup(self):
        self.catalog = DataCatalog.from_env()
        self.fs = self.catalog.fs

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
            + self.catalog.ticker(as_nautilus=True)
        )
        return data

    def test_feather_writer(self):
        # Arrange
        # path = "/root/backtest001"
        # instruments = self.catalog.instruments(as_nautilus=True)
        base_data_config = BacktestDataConfig()
        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(),
            venues=BacktestVenueConfig(
                name="BETFAIR", type="EXCHANGE", oms_type="NETTING", account_type="BETTING"
            ),
            data=[base_data_config, base_data_config],
            persistence=PersistenceConfig(),
            strategies=[],
        )
        node = BacktestNode()

        # Act
        node.run_sync(run_configs=[run_config])

        # Assert
        result = {}
        for path in self.fs.ls("/root/backtest001/"):
            name = pathlib.Path(path).name
            persisted = read_feather(fs=self.fs, path=path)
            if persisted is not None:
                result[name] = persisted.shape
        expected = {
            "InstrumentStatusUpdate.feather": (2, 4),
            "OrderBookData.feather": (2384, 11),
            "TradeTick.feather": (624, 7),
        }
        assert result == expected
