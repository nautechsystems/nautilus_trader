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

import datetime
import os
from decimal import Decimal

import fsspec
import pyarrow.dataset as ds
import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.external.core import dicts_to_dataframes
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.core import split_and_serialize
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.persistence.external.core import write_tables
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


pytestmark = pytest.mark.skip(reason="WIP pending catalog refactor")


# TODO: Implement with new Rust datafusion backend
# class TestPersistenceCatalogRust:
#     def setup(self) -> None:
#         self.catalog = data_catalog_setup(protocol="file")
#         self.fs: fsspec.AbstractFileSystem = self.catalog.fs
#         self.instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD", Venue("SIM"))
#
#     def teardown(self) -> None:
#         # Cleanup
#         path = self.catalog.path
#         fs = self.catalog.fs
#         if fs.exists(path):
#             fs.rm(path, recursive=True)
#
#     def _load_quote_ticks_into_catalog_rust(self) -> list[QuoteTick]:
#         parquet_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.parquet")
#         assert os.path.exists(parquet_data_path)
#
#         reader = ParquetReader(
#             parquet_data_path,
#             1000,
#             ParquetType.QuoteTick,
#             ParquetReaderType.File,
#         )
#
#         mapped_chunk = map(QuoteTick.list_from_capsule, reader)
#         quotes = list(itertools.chain(*mapped_chunk))
#
#         min_timestamp = str(quotes[0].ts_init).rjust(19, "0")
#         max_timestamp = str(quotes[-1].ts_init).rjust(19, "0")
#
#         # Write EUR/USD and USD/JPY rust quotes
#         for instrument_id in ("EUR/USD.SIM", "USD/JPY.SIM"):
#             # Reset reader
#             reader = ParquetReader(
#                 parquet_data_path,
#                 1000,
#                 ParquetType.QuoteTick,
#                 ParquetReaderType.File,
#             )
#
#             metadata = {
#                 "instrument_id": instrument_id,
#                 "price_precision": "5",
#                 "size_precision": "0",
#             }
#             writer = ParquetWriter(
#                 ParquetType.QuoteTick,
#                 metadata,
#             )
#
#             file_path = os.path.join(
#                 self.catalog.path,
#                 "data",
#                 "quote_tick.parquet",
#                 f"instrument_id={instrument_id.replace('/', '-')}",  # EUR-USD.SIM, USD-JPY.SIM
#                 f"{min_timestamp}-{max_timestamp}-0.parquet",
#             )
#
#             os.makedirs(os.path.dirname(file_path), exist_ok=True)
#             with open(file_path, "wb") as f:
#                 for chunk in reader:
#                     writer.write(chunk)
#                 data: bytes = writer.flush_bytes()
#                 f.write(data)
#
#         return quotes
#
#     def _load_trade_ticks_into_catalog_rust(self) -> list[TradeTick]:
#         parquet_data_path = os.path.join(TEST_DATA_DIR, "trade_tick_data.parquet")
#         assert os.path.exists(parquet_data_path)
#         reader = ParquetReader(
#             parquet_data_path,
#             100,
#             ParquetType.TradeTick,
#             ParquetReaderType.File,
#         )
#
#         mapped_chunk = map(TradeTick.list_from_capsule, reader)
#         trades = list(itertools.chain(*mapped_chunk))
#
#         min_timestamp = str(trades[0].ts_init).rjust(19, "0")
#         max_timestamp = str(trades[-1].ts_init).rjust(19, "0")
#
#         # Reset reader
#         reader = ParquetReader(
#             parquet_data_path,
#             100,
#             ParquetType.TradeTick,
#             ParquetReaderType.File,
#         )
#
#         metadata = {
#             "instrument_id": "EUR/USD.SIM",
#             "price_precision": "5",
#             "size_precision": "0",
#         }
#         writer = ParquetWriter(
#             ParquetType.TradeTick,
#             metadata,
#         )
#
#         file_path = os.path.join(
#             self.catalog.path,
#             "data",
#             "trade_tick.parquet",
#             "instrument_id=EUR-USD.SIM",
#             f"{min_timestamp}-{max_timestamp}-0.parquet",
#         )
#
#         os.makedirs(os.path.dirname(file_path), exist_ok=True)
#         with open(file_path, "wb") as f:
#             for chunk in reader:
#                 writer.write(chunk)
#             data: bytes = writer.flush_bytes()
#             f.write(data)
#
#         return trades
#
#     def test_get_files_for_expected_instrument_id(self):
#         # Arrange
#         self._load_quote_ticks_into_catalog_rust()
#
#         # Act
#         files1 = self.catalog.get_files(cls=QuoteTick, instrument_id="USD/JPY.SIM")
#         files2 = self.catalog.get_files(cls=QuoteTick, instrument_id="EUR/USD.SIM")
#         files3 = self.catalog.get_files(cls=QuoteTick, instrument_id="USD/CHF.SIM")
#
#         # Assert
#         assert files1 == [
#             f"{self.catalog.path}/data/quote_tick.parquet/instrument_id=USD-JPY.SIM/1577898000000000065-1577919652000000125-0.parquet",
#         ]
#         assert files2 == [
#             f"{self.catalog.path}/data/quote_tick.parquet/instrument_id=EUR-USD.SIM/1577898000000000065-1577919652000000125-0.parquet",
#         ]
#         assert files3 == []
#
#     def test_get_files_for_no_instrument_id(self):
#         # Arrange
#         self._load_quote_ticks_into_catalog_rust()
#
#         # Act
#         files = self.catalog.get_files(cls=QuoteTick)
#
#         # Assert
#         assert files == [
#             f"{self.catalog.path}/data/quote_tick.parquet/instrument_id=EUR-USD.SIM/1577898000000000065-1577919652000000125-0.parquet",
#             f"{self.catalog.path}/data/quote_tick.parquet/instrument_id=USD-JPY.SIM/1577898000000000065-1577919652000000125-0.parquet",
#         ]
#
#     def test_get_files_for_timestamp_range(self):
#         # Arrange
#         self._load_quote_ticks_into_catalog_rust()
#         start = 1577898000000000065
#         end = 1577919652000000125
#
#         # Act
#         files1 = self.catalog.get_files(
#             cls=QuoteTick,
#             instrument_id="EUR/USD.SIM",
#             start_nanos=start,
#             end_nanos=start,
#         )
#
#         files2 = self.catalog.get_files(
#             cls=QuoteTick,
#             instrument_id="EUR/USD.SIM",
#             start_nanos=0,
#             end_nanos=start - 1,
#         )
#
#         files3 = self.catalog.get_files(
#             cls=QuoteTick,
#             instrument_id="EUR/USD.SIM",
#             start_nanos=end + 1,
#             end_nanos=sys.maxsize,
#         )
#
#         # Assert
#         assert files1 == [
#             f"{self.catalog.path}/data/quote_tick.parquet/instrument_id=EUR-USD.SIM/1577898000000000065-1577919652000000125-0.parquet",
#         ]
#         assert files2 == []
#         assert files3 == []
#
#     def test_data_catalog_quote_ticks_as_nautilus_use_rust(self):
#         # Arrange
#         self._load_quote_ticks_into_catalog_rust()
#
#         # Act
#         quote_ticks = self.catalog.quote_ticks(
#             as_nautilus=True,
#             use_rust=True,
#             instrument_ids=["EUR/USD.SIM"],
#         )
#
#         # Assert
#         assert all(isinstance(tick, QuoteTick) for tick in quote_ticks)
#         assert len(quote_ticks) == 9500
#
#     def test_data_catalog_quote_ticks_as_nautilus_use_rust_with_date_range(self):
#         # Arrange
#         self._load_quote_ticks_into_catalog_rust()
#
#         start_timestamp = 1577898181000000440  # index 44
#         end_timestamp = 1577898572000000953  # index 99
#
#         # Act
#         quote_ticks = self.catalog.quote_ticks(
#             as_nautilus=True,
#             use_rust=True,
#             instrument_ids=["EUR/USD.SIM"],
#             start=start_timestamp,
#             end=end_timestamp,
#         )
#
#         # Assert
#         assert all(isinstance(tick, QuoteTick) for tick in quote_ticks)
#         assert len(quote_ticks) == 54
#         assert quote_ticks[0].ts_init == start_timestamp
#         assert quote_ticks[-1].ts_init == end_timestamp
#
#     def test_data_catalog_quote_ticks_as_nautilus_use_rust_with_date_range_with_multiple_instrument_ids(
#         self,
#     ):
#         # Arrange
#         self._load_quote_ticks_into_catalog_rust()
#
#         start_timestamp = 1577898181000000440  # EUR/USD.SIM index 44
#         end_timestamp = 1577898572000000953  # EUR/USD.SIM index 99
#
#         # Act
#         quote_ticks = self.catalog.quote_ticks(
#             as_nautilus=True,
#             use_rust=True,
#             instrument_ids=["EUR/USD.SIM", "USD/JPY.SIM"],
#             start=start_timestamp,
#             end=end_timestamp,
#         )
#
#         # Assert
#         assert all(isinstance(tick, QuoteTick) for tick in quote_ticks)
#
#         instrument1_quote_ticks = [t for t in quote_ticks if str(t.instrument_id) == "EUR/USD.SIM"]
#         assert len(instrument1_quote_ticks) == 54
#
#         instrument2_quote_ticks = [t for t in quote_ticks if str(t.instrument_id) == "USD/JPY.SIM"]
#         assert len(instrument2_quote_ticks) == 54
#
#         assert quote_ticks[0].ts_init == start_timestamp
#         assert quote_ticks[-1].ts_init == end_timestamp

# def test_data_catalog_use_rust_quote_ticks_round_trip(self):
#     # Arrange
#     instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
#
#     parquet_data_glob_path = TEST_DATA_DIR + "/quote_tick_data.parquet"
#     assert os.path.exists(parquet_data_glob_path)
#
#     def block_parser(df):
#         df = df.set_index("ts_event")
#         df.index = df.ts_init.apply(unix_nanos_to_dt)
#         objs = QuoteTickDataWrangler(instrument=instrument).process(df)
#         yield from objs
#
#     # Act
#     process_files(
#         glob_path=parquet_data_glob_path,
#         reader=ParquetByteReader(parser=block_parser),
#         use_rust=True,
#         catalog=self.catalog,
#         instrument=instrument,
#     )
#
#     quote_ticks = self.catalog.quote_ticks(
#         as_nautilus=True,
#         use_rust=True,
#         instrument_ids=["EUR/USD.SIM"],
#     )
#
#     assert all(isinstance(tick, QuoteTick) for tick in quote_ticks)
#     assert len(quote_ticks) == 9500

# def test_data_catalog_quote_ticks_use_rust(self):
#     # Arrange
#     quotes = self._load_quote_ticks_into_catalog_rust()
#
#     # Act
#     qdf = self.catalog.quote_ticks(use_rust=True, instrument_ids=["EUR/USD.SIM"])
#
#     # Assert
#     assert isinstance(qdf, pd.DataFrame)
#     assert len(qdf) == 9500
#     assert qdf.bid.equals(pd.Series([float(q.bid) for q in quotes]))
#     assert qdf.ask.equals(pd.Series([float(q.ask) for q in quotes]))
#     assert qdf.bid_size.equals(pd.Series([float(q.bid_size) for q in quotes]))
#     assert qdf.ask_size.equals(pd.Series([float(q.ask_size) for q in quotes]))
#     assert (qdf.instrument_id == "EUR/USD.SIM").all
#
# def test_data_catalog_trade_ticks_as_nautilus_use_rust(self):
#     # Arrange
#     self._load_trade_ticks_into_catalog_rust()
#
#     # Act
#     trade_ticks = self.catalog.trade_ticks(
#         as_nautilus=True,
#         use_rust=True,
#         instrument_ids=["EUR/USD.SIM"],
#     )
#
#     # Assert
#     assert all(isinstance(tick, TradeTick) for tick in trade_ticks)
#     assert len(trade_ticks) == 100


class _TestPersistenceCatalog:
    def setup(self) -> None:
        self.catalog = data_catalog_setup(protocol=self.fs_protocol)  # type: ignore
        self._load_data_into_catalog()
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs

    def teardown(self):
        # Cleanup
        path = self.catalog.path
        fs = self.catalog.fs
        if fs.exists(path):
            fs.rm(path, recursive=True)

    def _load_data_into_catalog(self):
        self.instrument_provider = BetfairInstrumentProvider.from_instruments([])
        # Write some betfair trades and orderbook
        process_files(
            glob_path=TEST_DATA_DIR + "/betfair/1.166564490.bz2",
            reader=BetfairTestStubs.betfair_reader(instrument_provider=self.instrument_provider),
            instrument_provider=self.instrument_provider,
            catalog=self.catalog,
        )

    def test_partition_key_correctly_remapped(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        tick = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price(10, 1),
            ask_price=Price(11, 1),
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
        self.fs.isdir(
            os.path.join(self.catalog.path, "data", "quote_tick.parquet/instrument_id=AUD-USD.SIM"),
        )
        # Ensure we "unmap" the keys that we write the partition filenames as;
        # this instrument_id should be AUD/USD not AUD-USD
        assert df.iloc[0]["instrument_id"] == instrument.id.value

    def test_list_data_types(self):
        data_types = self.catalog.list_data_types()

        expected = [
            "betfair_ticker",
            "betting_instrument",
            "instrument_status_update",
            "order_book_delta",
            "trade_tick",
        ]
        assert data_types == expected

    def test_list_partitions(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        tick = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price(10, 1),
            ask_price=Price(11, 1),
            bid_size=Quantity(10, 1),
            ask_size=Quantity(10, 1),
            ts_init=0,
            ts_event=0,
        )
        tables = dicts_to_dataframes(split_and_serialize([tick]))
        write_tables(catalog=self.catalog, tables=tables)

        # Act
        self.catalog.list_partitions(QuoteTick)

        # Assert
        # TODO(cs): Assert new HivePartitioning object for catalog v2

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

    def test_data_catalog_trade_ticks_as_nautilus(self):
        trade_ticks = self.catalog.trade_ticks(as_nautilus=True)
        assert all(isinstance(tick, TradeTick) for tick in trade_ticks)
        assert len(trade_ticks) == 312

    def test_data_catalog_instruments_df(self):
        instruments = self.catalog.instruments()
        assert len(instruments) == 2

    def test_writing_instruments_doesnt_overwrite(self):
        instruments = self.catalog.instruments(as_nautilus=True)
        write_objects(catalog=self.catalog, chunk=[instruments[0]])
        write_objects(catalog=self.catalog, chunk=[instruments[1]])
        instruments = self.catalog.instruments(as_nautilus=True)
        assert len(instruments) == 2

    def test_writing_instruments_overwrite(self):
        instruments = self.catalog.instruments(as_nautilus=True)
        write_objects(catalog=self.catalog, chunk=[instruments[0]], merge_existing_data=False)
        write_objects(catalog=self.catalog, chunk=[instruments[1]], merge_existing_data=False)
        instruments = self.catalog.instruments(as_nautilus=True)
        assert len(instruments) == 1

    def test_data_catalog_instruments_filtered_df(self):
        instrument_id = self.catalog.instruments(as_nautilus=True)[0].id.value
        instruments = self.catalog.instruments(instrument_ids=[instrument_id])
        assert len(instruments) == 1
        assert instruments["id"].iloc[0] == instrument_id

    def test_data_catalog_instruments_as_nautilus(self):
        instruments = self.catalog.instruments(as_nautilus=True)
        assert all(isinstance(ins, BettingInstrument) for ins in instruments)

    def test_data_catalog_currency_with_null_max_price_loads(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
        write_objects(catalog=self.catalog, chunk=[instrument])

        # Act
        instrument = self.catalog.instruments(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]

        # Assert
        assert instrument.max_price is None

    def test_data_catalog_instrument_ids_correctly_unmapped(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
        trade_tick = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("2.0"),
            size=Quantity.from_int(10),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("1"),
            ts_event=0,
            ts_init=0,
        )
        write_objects(catalog=self.catalog, chunk=[instrument, trade_tick])

        # Act
        self.catalog.instruments()
        instrument = self.catalog.instruments(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]
        trade_tick = self.catalog.trade_ticks(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]

        # Assert
        assert instrument.id.value == "AUD/USD.SIM"
        assert trade_tick.instrument_id.value == "AUD/USD.SIM"

    def test_data_catalog_filter(self):
        # Arrange, Act
        deltas = self.catalog.order_book_deltas()
        filtered_deltas = self.catalog.order_book_deltas(filter_expr=ds.field("action") == "DELETE")

        # Assert
        assert len(deltas) == 2384
        assert len(filtered_deltas) == 351

    def test_data_catalog_generic_data(self):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        process_files(
            glob_path=f"{TEST_DATA_DIR}/news_events.csv",
            reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
            catalog=self.catalog,
        )

        # Act
        df = self.catalog.generic_data(cls=NewsEventData, filter_expr=ds.field("currency") == "USD")
        data = self.catalog.generic_data(
            cls=NewsEventData,
            filter_expr=ds.field("currency") == "CHF",
            as_nautilus=True,
        )

        # Assert
        assert df is not None
        assert data is not None
        assert len(df) == 22925
        assert len(data) == 2745
        assert isinstance(data[0], GenericData)

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
            raw_symbol=Symbol("AAPL"),
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
            bid_price=Price.from_str("2.1"),
            ask_price=Price.from_str("2.0"),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=0,
            ts_init=0,
        )

        # Act
        write_objects(catalog=self.catalog, chunk=[instrument, quote_tick])
        instrument_from_catalog = self.catalog.instruments(
            as_nautilus=True,
            instrument_ids=[instrument.id.value],
        )[0]

        # Assert
        assert instrument.taker_fee == instrument_from_catalog.taker_fee
        assert instrument.maker_fee == instrument_from_catalog.maker_fee
        assert instrument.margin_init == instrument_from_catalog.margin_init
        assert instrument.margin_maint == instrument_from_catalog.margin_maint


class TestPersistenceCatalogFile(_TestPersistenceCatalog):
    fs_protocol = "file"


class TestPersistenceCatalogMemory(_TestPersistenceCatalog):
    fs_protocol = "memory"
