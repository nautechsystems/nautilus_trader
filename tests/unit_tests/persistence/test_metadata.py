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

import fsspec

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.persistence.external.metadata import load_mappings
from tests.test_kit.mocks.data import data_catalog_setup
from tests.test_kit.stubs.data import TestDataStubs


class TestPersistenceBatching:
    def setup(self):
        data_catalog_setup()
        self.catalog = ParquetDataCatalog.from_env()
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs

    def test_metadata_multiple_instruments(self):
        # Arrange
        audusd = TestInstrumentProvider.default_fx_ccy("AUD/USD", Venue("OANDA"))
        gbpusd = TestInstrumentProvider.default_fx_ccy("GBP/USD", Venue("OANDA"))
        audusd_trade = TestDataStubs.trade_tick_3decimal(instrument_id=audusd.id)
        gbpusd_trade = TestDataStubs.trade_tick_3decimal(instrument_id=gbpusd.id)

        # Act
        write_objects(self.catalog, [audusd_trade, gbpusd_trade])

        # Assert
        meta = load_mappings(fs=self.fs, path="/.nautilus/catalog/data/trade_tick.parquet")
        expected = {
            "instrument_id": {
                "GBP/USD.OANDA": "GBP-USD.OANDA",
                "AUD/USD.OANDA": "AUD-USD.OANDA",
            },
        }
        assert meta == expected
