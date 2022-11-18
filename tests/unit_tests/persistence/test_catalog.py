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

import datetime
import pathlib
import sys
from decimal import Decimal

import fsspec
import pyarrow.dataset as ds
import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import BarDataWrangler
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.catalog.parquet import resolve_path
from nautilus_trader.persistence.external.core import dicts_to_dataframes
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.core import split_and_serialize
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.persistence.external.core import write_tables
from nautilus_trader.persistence.external.readers import CSVReader
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks.data import NewsEventData
from tests.test_kit.mocks.data import data_catalog_setup
from tests.test_kit.stubs.data import TestDataStubs
from tests.test_kit.stubs.identifiers import TestIdStubs
from tests.test_kit.stubs.persistence import TestPersistenceStubs


TEST_DATA_DIR = PACKAGE_ROOT + "/data"


class TestPersistenceCatalog:
    def setup(self):
        data_catalog_setup()
        self.catalog = ParquetDataCatalog.from_env()
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs
        self._load_data_into_catalog()

    def _load_data_into_catalog(self):
        self.instrument_provider = BetfairInstrumentProvider.from_instruments([])
        process_files(
            glob_path=PACKAGE_ROOT + "/data/1.166564490.bz2",
            reader=BetfairTestStubs.betfair_reader(instrument_provider=self.instrument_provider),
            instrument_provider=self.instrument_provider,
            catalog=self.catalog,
        )

    @pytest.mark.skipif(sys.platform != "win32", reason="windows only")
    def test_catalog_root_path_windows_local(self):
        from tempfile import tempdir

        catalog = ParquetDataCatalog(path=tempdir, fs_protocol="file")
        path = resolve_path(path=catalog.path / "test", fs=catalog.fs)
        assert path == str(pathlib.Path(tempdir) / "test")

    @pytest.mark.skipif(sys.platform != "win32", reason="windows only")
    def test_catalog_root_path_windows_non_local(self):
        catalog = ParquetDataCatalog(path="/some/path", fs_protocol="memory")
        path = resolve_path(path=catalog.path / "test", fs=catalog.fs)
        assert path == "/some/path/test"

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

    @pytest.mark.skip(reason="schema change")
    def test_writing_instruments_doesnt_overwrite(self):
        instruments = self.catalog.instruments(as_nautilus=True)
        write_objects(catalog=self.catalog, chunk=[instruments[0]])
        write_objects(catalog=self.catalog, chunk=[instruments[1]])
        instruments = self.catalog.instruments(as_nautilus=True)
        assert len(instruments) == 3

    def test_data_catalog_instruments_filtered_df(self):
        instrument_id = "296287091.1665644902374910.0.BETFAIR"
        instruments = self.catalog.instruments(instrument_ids=[instrument_id])
        assert len(instruments) == 1
        assert instruments["id"].iloc[0] == instrument_id

    def test_data_catalog_instruments_as_nautilus(self):
        instruments = self.catalog.instruments(as_nautilus=True)
        assert all(isinstance(ins, BettingInstrument) for ins in instruments)

    def test_data_catalog_currency_with_null_max_price_loads(self):
        # Arrange
        catalog = ParquetDataCatalog.from_env()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
        write_objects(catalog=catalog, chunk=[instrument])

        # Act
        instrument = catalog.instruments(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]

        # Assert
        assert instrument.max_price is None

    def test_data_catalog_instrument_ids_correctly_unmapped(self):
        # Arrange
        catalog = ParquetDataCatalog.from_env()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
        trade_tick = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("2.0"),
            size=Quantity.from_int(10),
            aggressor_side=AggressorSide.NONE,
            trade_id=TradeId("1"),
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
        assert self.fs.isdir(
            "/.nautilus/catalog/data/quote_tick.parquet/instrument_id=AUD-USD.SIM/"
        )
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

    def test_data_catalog_generic_data(self):
        TestPersistenceStubs.setup_news_event_persistence()
        process_files(
            glob_path=f"{TEST_DATA_DIR}/news_events.csv",
            reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
            catalog=self.catalog,
        )
        df = self.catalog.generic_data(cls=NewsEventData, filter_expr=ds.field("currency") == "USD")
        assert len(df) == 22925
        data = self.catalog.generic_data(
            cls=NewsEventData, filter_expr=ds.field("currency") == "CHF", as_nautilus=True
        )
        assert len(data) == 2745 and isinstance(data[0], GenericData)

    def test_data_catalog_bars(self):
        # Arrange
        bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
        instrument = TestInstrumentProvider.adabtc_binance()
        wrangler = BarDataWrangler(bar_type, instrument)

        def parser(data):
            data["timestamp"] = data["timestamp"].astype("datetime64[ms]")
            bars = wrangler.process(data.set_index("timestamp"))
            return bars

        binance_spot_header = [
            "timestamp",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "ts_close",
            "quote_volume",
            "n_trades",
            "taker_buy_base_volume",
            "taker_buy_quote_volume",
            "ignore",
        ]
        reader = CSVReader(block_parser=parser, header=binance_spot_header)

        # Act
        _ = process_files(
            glob_path=f"{TEST_DATA_DIR}/ADABTC-1m-2021-11-*.csv",
            reader=reader,
            catalog=self.catalog,
        )

        # Assert
        bars = self.catalog.bars()
        assert len(bars) == 21

    def test_catalog_bar_query_instrument_id(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()
        write_objects(catalog=self.catalog, chunk=[bar])

        # Act
        objs = self.catalog.bars(instrument_ids=[TestIdStubs.audusd_id().value], as_nautilus=True)
        data = self.catalog.bars(instrument_ids=[TestIdStubs.audusd_id().value])

        # Assert
        assert len(objs) == 1
        assert data.shape[0] == 1
        assert "instrument_id" in data.columns

    def test_catalog_projections(self):
        projections = {"tid": ds.field("trade_id")}
        trades = self.catalog.trade_ticks(projections=projections)
        assert "tid" in trades.columns
        assert trades["trade_id"].equals(trades["tid"])

    def test_catalog_persists_equity(self):
        # Arrange
        instrument = Equity(
            instrument_id=InstrumentId(symbol=Symbol("AAPL"), venue=Venue("NASDAQ")),
            native_symbol=Symbol("AAPL"),
            currency=USD,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            isin="US0378331005",
            ts_event=0,
            ts_init=0,
            margin_init=Decimal("0.01"),
            margin_maint=Decimal("0.005"),
            maker_fee=Decimal("0.005"),
            taker_fee=Decimal("0.01"),
        )

        quote_tick = QuoteTick(
            instrument_id=instrument.id,
            ask=Price.from_str("2.0"),
            bid=Price.from_str("2.1"),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=0,
            ts_init=0,
        )

        # Act
        catalog = ParquetDataCatalog.from_env()
        write_objects(catalog=catalog, chunk=[instrument, quote_tick])
        instrument_from_catalog = catalog.instruments(
            as_nautilus=True,
            instrument_ids=[instrument.id.value],
        )[0]

        # Assert
        assert instrument.taker_fee == instrument_from_catalog.taker_fee
        assert instrument.maker_fee == instrument_from_catalog.maker_fee
        assert instrument.margin_init == instrument_from_catalog.margin_init
        assert instrument.margin_maint == instrument_from_catalog.margin_maint

    def test_list_partitions(self):
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
        parts = self.catalog.list_partitions(QuoteTick)

        # Assert
        assert parts == {"instrument_id": ["AUD-USD.SIM"]}
