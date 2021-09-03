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

import datetime
import sys

import fsspec
import pyarrow.dataset as ds
import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import dicts_to_dataframes
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.core import split_and_serialize
from nautilus_trader.persistence.external.core import write_tables
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks import data_catalog_setup
from tests.test_kit.providers import TestInstrumentProvider


TEST_DATA_DIR = PACKAGE_ROOT + "/data"


@pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")
class TestPersistenceCatalog:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs
        self._loaded_data_into_catalog()

    def _loaded_data_into_catalog(self):
        self.instrument_provider = BetfairInstrumentProvider.from_instruments([])
        process_files(
            glob_path=PACKAGE_ROOT + "/data/1.166564490.bz2",
            reader=BetfairTestStubs.betfair_reader(instrument_provider=self.instrument_provider),
            instrument_provider=self.instrument_provider,
            catalog=self.catalog,
        )

    def test_data_catalog_instruments_df(self):
        instruments = self.catalog.instruments()
        assert len(instruments) == 2

    def test_data_catalog_instruments_filtered_df(self):
        instrument_id = (
            "Basketball,,29628709,20191221-001000,ODDS,MATCH_ODDS,1.166564490,237491,0.0.BETFAIR"
        )
        instruments = self.catalog.instruments(instrument_ids=[instrument_id])
        assert len(instruments) == 1
        assert instruments["id"].iloc[0] == instrument_id

    def test_data_catalog_instruments_as_nautilus(self):
        instruments = self.catalog.instruments(as_nautilus=True)
        assert all(isinstance(ins, BettingInstrument) for ins in instruments)

    def test_partition_key_correctly_remapped(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        tick = QuoteTick(
            instrument_id=instrument.id,
            bid=Price(10, 1),
            ask=Price(11, 1),
            bid_size=Quantity(10, 1),
            ask_size=Quantity(10, 1),
            ts_init=0,
            ts_event=0,
        )
        tables = dicts_to_dataframes(split_and_serialize([tick]))
        write_tables(catalog=self.catalog, tables=tables)

        # Act
        df = self.catalog.quote_ticks()

        # Assert
        assert len(df) == 1
        assert self.fs.isdir("/root/data/quote_tick.parquet/instrument_id=AUD-USD.SIM/")
        # Ensure we "unmap" the keys that we write the partition filenames as;
        # this instrument_id should be AUD/USD not AUD-USD
        assert df.iloc[0]["instrument_id"] == instrument.id.value

    def test_data_catalog_filter(self):
        # Arrange, Act
        deltas = self.catalog.order_book_deltas()
        filtered_deltas = self.catalog.order_book_deltas(
            filter_expr=ds.field("delta_type") == "DELETE"
        )

        # Assert
        assert len(deltas) == 2384
        assert len(filtered_deltas) == 351

    def test_data_catalog_query_filtered(self):
        ticks = self.catalog.trade_ticks()
        assert len(ticks) == 312

        ticks = self.catalog.trade_ticks(start="2019-12-20 20:56:18")
        assert len(ticks) == 123

        ticks = self.catalog.trade_ticks(start=1576875378384999936)
        assert len(ticks) == 123

        ticks = self.catalog.trade_ticks(start=datetime.datetime(2019, 12, 20, 20, 56, 18))
        assert len(ticks) == 123

        deltas = self.catalog.order_book_deltas()
        assert len(deltas) == 2384

        filtered_deltas = self.catalog.order_book_deltas(
            filter_expr=ds.field("delta_type") == "DELETE"
        )
        assert len(filtered_deltas) == 351
