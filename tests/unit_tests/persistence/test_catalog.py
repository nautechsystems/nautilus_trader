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
import pandas as pd
import pyarrow.dataset as ds
import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.core.datetime import maybe_dt_to_unix_nanos
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import dicts_to_dataframes
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.core import split_and_serialize
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.persistence.external.core import write_tables
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.serialization.arrow.serializer import register_parquet
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact
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

    def test_list_data_types(self):
        data_types = self.catalog.list_data_types()
        expected = [
            "betfair_ticker",
            "betting_instrument",
            "instrument_status_update",
            "order_book_data",
            "trade_tick",
        ]
        assert data_types == expected

    def test_data_catalog_instruments_df(self):
        instruments = self.catalog.instruments()
        assert len(instruments) == 2

    def test_writing_instruments_doesnt_overwrite(self):
        instruments = self.catalog.instruments(as_nautilus=True)
        write_objects(catalog=self.catalog, chunk=[instruments[0]])
        write_objects(catalog=self.catalog, chunk=[instruments[1]])
        instruments = self.catalog.instruments(as_nautilus=True)
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

    def test_data_catalog_currency_with_null_max_price_loads(self):
        # Arrange
        catalog = DataCatalog.from_env()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
        write_objects(catalog=catalog, chunk=[instrument])

        # Act
        instrument = catalog.instruments(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]

        # Assert
        assert instrument.max_price is None

    def test_data_catalog_instrument_ids_correctly_unmapped(self):
        # Arrange
        catalog = DataCatalog.from_env()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
        trade_tick = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("2.0"),
            size=Quantity.from_int(10),
            aggressor_side=AggressorSide.UNKNOWN,
            match_id="1",
            ts_event=0,
            ts_init=0,
        )
        write_objects(catalog=catalog, chunk=[instrument, trade_tick])

        # Act
        instrument = catalog.instruments(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]
        trade_tick = catalog.trade_ticks(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]

        # Assert
        assert instrument.id.value == "AUD/USD.SIM"
        assert trade_tick.instrument_id.value == "AUD/USD.SIM"

    def test_data_catalog_trade_ticks_as_nautilus(self):
        trade_ticks = self.catalog.trade_ticks(as_nautilus=True)
        assert all(isinstance(tick, TradeTick) for tick in trade_ticks)
        assert len(trade_ticks) == 312

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
        filtered_deltas = self.catalog.order_book_deltas(filter_expr=ds.field("action") == "DELETE")

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

        filtered_deltas = self.catalog.order_book_deltas(filter_expr=ds.field("action") == "DELETE")
        assert len(filtered_deltas) == 351

    def test_data_loader_generic_data(self):
        import pyarrow as pa

        def _news_event_to_dict(self):
            return {
                "name": self.name,
                "impact": self.impact.name,
                "currency": self.currency.code,
                "ts_event": self.ts_event,
                "ts_init": self.ts_init,
            }

        def _news_event_from_dict(data):
            data.update(
                {
                    "impact": getattr(NewsImpact, data["impact"]),
                    "currency": Currency.from_str(data["currency"]),
                }
            )
            return NewsEvent(**data)

        register_parquet(
            cls=NewsEvent,
            serializer=_news_event_to_dict,
            deserializer=_news_event_from_dict,
            partition_keys=("currency",),
            schema=pa.schema(
                {
                    "name": pa.string(),
                    "impact": pa.string(),
                    "currency": pa.string(),
                    "ts_event": pa.int64(),
                    "ts_init": pa.int64(),
                }
            ),
            force=True,
        )

        def make_news_event(df, state=None):
            for _, row in df.iterrows():
                yield NewsEvent(
                    name=str(row["Name"]),
                    impact=getattr(NewsImpact, row["Impact"]),
                    currency=Currency.from_str(row["Currency"]),
                    ts_event=maybe_dt_to_unix_nanos(pd.Timestamp(row["Start"])),
                    ts_init=maybe_dt_to_unix_nanos(pd.Timestamp(row["Start"])),
                )

        process_files(
            glob_path=f"{TEST_DATA_DIR}/news_events.csv",
            reader=CSVReader(block_parser=make_news_event),
            catalog=self.catalog,
        )
        df = self.catalog.generic_data(cls=NewsEvent, filter_expr=ds.field("currency") == "USD")
        assert len(df) == 22925
        data = self.catalog.generic_data(
            cls=NewsEvent, filter_expr=ds.field("currency") == "CHF", as_nautilus=True
        )
        assert len(data) == 2745 and isinstance(data[0], GenericData)
