import pickle
from unittest.mock import patch

import fsspec
import numpy as np
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as ds
import pyarrow.parquet as pq
import pytest
from distlib.util import CSVReader

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.util import make_betfair_reader
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.data.wrangling import TradeTickDataWrangler
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import RawFile
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.core import read_and_clear_existing_data
from nautilus_trader.persistence.external.core import scan_files
from nautilus_trader.persistence.external.core import split_and_serialize
from nautilus_trader.persistence.external.core import write_parquet
from nautilus_trader.persistence.external.parsers import ParquetReader
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks import MockReader
from tests.test_kit.mocks import data_catalog_setup
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.unit_tests.backtest.test_backtest_config import TEST_DATA_DIR


TEST_DATA = PACKAGE_ROOT + "/data"


class TestPersistenceCore:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()
        self.loaded_catalog = self.catalog
        self.reader = MockReader()

    def test_raw_file_block_size_read(self):
        # Arrange
        raw_file = RawFile(
            fsspec.open(f"{TEST_DATA}/1.166564490.bz2"), catalog=self.catalog, reader=self.reader
        )
        data = b"".join(raw_file.iter())

        # Act
        raw_file = RawFile(
            fsspec.open(f"{TEST_DATA}/1.166564490.bz2"),
            catalog=self.catalog,
            reader=self.reader,
            block_size=1000,
        )
        blocks = list(raw_file.iter())

        # Assert
        assert len(blocks) == 18
        assert b"".join(blocks) == data

    def test_raw_file_process(self):
        # Arrange
        rf = RawFile(
            open_file=fsspec.open(f"{TEST_DATA}/1.166564490.bz2", compression="infer"),
            reader=make_betfair_reader(),
            catalog=self.catalog,
            block_size=None,
        )

        # Act
        rf.process()

        # Assert
        assert len(self.catalog.instruments()) == 2

    def test_raw_file_pickleable(self):
        # Arrange
        path = TEST_DATA_DIR + "/betfair/1.166811431.bz2"  # total size = 151707
        expected = RawFile(
            open_file=fsspec.open(path, compression="infer"),
            reader=make_betfair_reader(),
            catalog=self.catalog,
        )

        # Act
        data = pickle.dumps(expected)
        result = pickle.loads(data)  # noqa: S301

        # Assert
        assert result.fs == expected.fs
        assert result.path == expected.path
        assert result.chunk_size == expected.chunk_size
        assert result.compression == "bz2"

    def test_raw_file_distributed_serializable(self):
        from distributed.protocol import deserialize
        from distributed.protocol import serialize

        # Arrange
        fs = fsspec.implementations.local.LocalFileSystem()
        path = TEST_DATA_DIR + "/betfair/1.166811431.bz2"  # total size = 151707
        r = RawFile(fs=fs, path=path, chunk_size=-1, compression="bz2")

        # Act
        result1 = deserialize(*serialize(r))

        # Assert
        assert result1.fs == r.fs
        assert result1.path == r.path
        assert result1.chunk_size == r.chunk_size
        assert result1.compression == "bz2"

    @pytest.mark.parametrize(
        "glob, num_files",
        [
            ("**.json", 3),
            ("**.txt", 1),
            ("**.parquet", 2),
            ("**.csv", 11),
        ],
    )
    def test_scan_paths(self, glob, num_files):
        files = scan_files(glob_path=f"{TEST_DATA_DIR}/{glob}")
        assert len(files) == num_files

    def test_scan_file_filter(
        self,
    ):
        files = scan_files(glob_path=f"{TEST_DATA_DIR}/*.csv")
        assert len(files) == 11

        files = scan_files(glob_path=f"{TEST_DATA_DIR}/*jpy*.csv")
        assert len(files) == 3

    @patch("nautilus_trader.persistence.backtest.scanner.load_processed_raw_files")
    def test_scan_processed(self, mock_load_processed_raw_files):
        mock_load_processed_raw_files.return_value = [
            TEST_DATA_DIR + "/truefx-audusd-ticks.csv",
            TEST_DATA_DIR + "/news_events.csv",
            TEST_DATA_DIR + "/tardis_trades.csv",
        ]
        files = scan_files(glob_path=f"{TEST_DATA_DIR}/*.csv")
        assert len(files) == 8

    def test_nautilus_chunk_to_dataframes(self, betfair_nautilus_objects):
        dfs = split_and_serialize(betfair_nautilus_objects)
        result = {}
        for cls in dfs:
            for ins in dfs[cls]:
                result[(cls.__name__, ins)] = len(dfs[cls][ins])
        expected = {
            (
                "BettingInstrument",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,237478,0.0.BETFAIR",
            ): 1,
            (
                "BettingInstrument",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,60424,0.0.BETFAIR",
            ): 1,
            (
                "BettingInstrument",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,2696769,0.0.BETFAIR",
            ): 1,
            (
                "BettingInstrument",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,4297085,0.0.BETFAIR",
            ): 1,
            (
                "InstrumentStatusUpdate",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,2696769,0.0.BETFAIR",
            ): 6,
            (
                "InstrumentStatusUpdate",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,4297085,0.0.BETFAIR",
            ): 6,
            (
                "InstrumentStatusUpdate",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,60424,0.0.BETFAIR",
            ): 7,
            (
                "InstrumentStatusUpdate",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,237478,0.0.BETFAIR",
            ): 7,
            (
                "OrderBookData",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,4297085,0.0.BETFAIR",
            ): 1714,
            (
                "OrderBookData",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,2696769,0.0.BETFAIR",
            ): 7695,
            (
                "OrderBookData",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,60424,0.0.BETFAIR",
            ): 9374,
            (
                "OrderBookData",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,237478,0.0.BETFAIR",
            ): 9348,
            (
                "BetfairTicker",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,237478,0.0.BETFAIR",
            ): 852,
            (
                "BetfairTicker",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,60424,0.0.BETFAIR",
            ): 967,
            (
                "BetfairTicker",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,2696769,0.0.BETFAIR",
            ): 2305,
            (
                "BetfairTicker",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,4297085,0.0.BETFAIR",
            ): 981,
            (
                "TradeTick",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,2696769,0.0.BETFAIR",
            ): 2987,
            (
                "TradeTick",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,4297085,0.0.BETFAIR",
            ): 1387,
            (
                "TradeTick",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,60424,0.0.BETFAIR",
            ): 1120,
            (
                "TradeTick",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,237478,0.0.BETFAIR",
            ): 1013,
            (
                "InstrumentClosePrice",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,2696769,0.0.BETFAIR",
            ): 1,
            (
                "InstrumentClosePrice",
                "Cricket,,30339025,20210309-033000,ODDS,MATCH_ODDS,1.180305278,4297085,0.0.BETFAIR",
            ): 1,
            (
                "InstrumentClosePrice",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,60424,0.0.BETFAIR",
            ): 1,
            (
                "InstrumentClosePrice",
                "Basketball,,29635049,20191229-011000,ODDS,MATCH_ODDS,1.166811431,237478,0.0.BETFAIR",
            ): 1,
        }
        assert result == expected

    def test_write_parquet_no_partitions(
        self,
    ):
        df = pd.DataFrame(
            {"value": np.random.random(5), "instrument_id": ["a", "a", "a", "b", "b"]}
        )
        catalog = DataCatalog.from_env()
        fs = catalog.fs
        root = catalog.path

        write_parquet(
            fs=fs,
            root=root,
            path="sample.parquet",
            df=df,
            instrument_id=None,
            schema=pa.schema({"value": pa.float64(), "instrument_id": pa.string()}),
            partition_cols=None,
            append=False,
        )
        result = (
            ds.dataset(str(root.joinpath("sample.parquet")), filesystem=fs).to_table().to_pandas()
        )
        assert result.equals(df)

    def test_write_parquet_partitions(
        self,
    ):
        catalog = DataCatalog.from_env()
        fs = catalog.fs
        root = catalog.path
        path = "sample.parquet"

        df = pd.DataFrame(
            {"value": np.random.random(5), "instrument_id": ["a", "a", "a", "b", "b"]}
        )
        write_parquet(
            fs=fs,
            root=root,
            path=path,
            df=df,
            instrument_id=None,
            schema=pa.schema({"value": pa.float64(), "instrument_id": pa.string()}),
            partition_cols=["instrument_id"],
            append=False,
        )
        dataset = ds.dataset(str(root.joinpath("sample.parquet")), filesystem=fs)
        result = dataset.to_table().to_pandas()
        assert result.equals(df[["value"]])  # instrument_id is a partition now
        assert dataset.files[0].startswith("/root/sample.parquet/instrument_id=a/")
        assert dataset.files[1].startswith("/root/sample.parquet/instrument_id=b/")

    def test_write_parquet_determine_partitions_writes_instrument_id(
        self,
    ):
        # Arrange
        catalog = DataCatalog.from_env()
        fs = catalog.fs
        rf = RawFile(fs=fs, path="/")

        # Act
        quote = QuoteTick(
            instrument_id=TestStubs.audusd_id(),
            bid=Price.from_str("0.80"),
            ask=Price.from_str("0.81"),
            bid_size=Quantity.from_int(1000),
            ask_size=Quantity.from_int(1000),
            ts_event=0,
            ts_init=0,
        )
        chunk = [quote]
        assert chunk, rf
        # write_chunk(raw_file=rf, chunk=chunk)

        # Assert
        files = fs.ls("/root/data/quote_tick.parquet")
        expected = "/root/data/quote_tick.parquet/instrument_id=AUD-USD.SIM"
        assert expected in files

    def test_read_and_clear_existing_data_single_partition(
        self,
    ):
        # Arrange
        catalog = DataCatalog.from_env()
        fs = catalog.fs
        root = catalog.path

        path = "sample.parquet"
        df = pd.DataFrame(
            {"value": np.random.random(5), "instrument_id": ["a", "a", "a", "b", "b"]}
        )
        write_parquet(
            fs=fs,
            root=root,
            path=path,
            df=df,
            instrument_id=None,
            schema=pa.schema({"value": pa.float64(), "instrument_id": pa.string()}),
            partition_cols=["instrument_id"],
            append=False,
        )

        # Act
        result = read_and_clear_existing_data(
            fs=fs, root=root, path=path, instrument_id="a", partition_cols=["instrument_id"]
        )
        dataset = ds.dataset(str(root.joinpath("sample.parquet")), filesystem=fs)

        # Assert
        expected = df[df["instrument_id"] == "a"]
        assert result.equals(expected)
        assert len(dataset.files) == 1
        assert dataset.files[0].startswith("/root/sample.parquet/instrument_id=b/")

    def test_read_and_clear_existing_data_invalid_partition_column_raises(
        self,
    ):
        # Arrange
        catalog = DataCatalog.from_env()
        fs = catalog.fs
        root = catalog.path

        path = "sample.parquet"
        df = pd.DataFrame(
            {"value": np.random.random(5), "instrument_id": ["a", "a", "a", "b", "b"]}
        )
        write_parquet(
            fs=fs,
            root=root,
            path=path,
            df=df,
            instrument_id=None,
            schema=pa.schema({"value": pa.float64(), "instrument_id": pa.string()}),
            partition_cols=["instrument_id"],
            append=False,
        )

        # Assert
        with pytest.raises(AssertionError):
            read_and_clear_existing_data(
                fs=fs, root=root, path=path, instrument_id="a", partition_cols=["value"]
            )

    def test_process_files_csv(self, executor, get_parser):
        files = scan_files(
            glob_path=f"{TEST_DATA_DIR}/truefx*.csv",
            reader=None,
            catalog=None,
            block_size=1_000_000,
        )
        assert len(files) == 2
        reader = CSVReader(chunk_parser=get_parser("parse_csv_quotes"), as_dataframe=True)
        result = process_files(files, reader=reader, executor=executor)
        expected = [
            (TEST_DATA_DIR + "/truefx-audusd-ticks.csv", 20410),
            (TEST_DATA_DIR + "/truefx-audusd-ticks.csv", 20411),
            (TEST_DATA_DIR + "/truefx-audusd-ticks.csv", 20412),
            (TEST_DATA_DIR + "/truefx-audusd-ticks.csv", 20411),
            (TEST_DATA_DIR + "/truefx-audusd-ticks.csv", 18356),
            (TEST_DATA_DIR + "/truefx-usdjpy-ticks.csv", 1000),
        ]
        assert result == expected

    def test_data_loader_parquet(
        self,
    ):
        def filename_to_instrument(fn):
            if "btcusd" in fn:
                return TestInstrumentProvider.btcusdt_binance()
            else:
                raise KeyError()

        def parser(data_type, df, filename):
            instrument = filename_to_instrument(fn=filename)
            if data_type == "quote_ticks":
                df = df.set_index("timestamp")[["bid", "ask", "bid_size", "ask_size"]]
                wrangler = QuoteTickDataWrangler(data_quotes=df, instrument=instrument)
            elif data_type == "trade_ticks":
                wrangler = TradeTickDataWrangler(df=df, instrument=instrument)
            else:
                raise TypeError()
            wrangler.pre_process(0)
            yield from wrangler.build_ticks()

        results = process_files(
            glob_path=f"{TEST_DATA_DIR}/*quote*.parquet",
            reader=ParquetReader(
                data_type="quote_ticks",
                parser=parser,
            ),
            catalog=self.catalog,
            instrument_provider=InstrumentProvider(),
        )
        expected = {}
        assert results == expected

    def test_load_text_betfair(self, betfair_reader):
        instrument_provider = BetfairInstrumentProvider.from_instruments([])

        files = process_files(
            glob_path=f"{TEST_DATA_DIR}/**.bz2",
            reader=betfair_reader(instrument_provider=instrument_provider),
            catalog=self.catalog,
            instrument_provider=instrument_provider,
        )
        expected = {
            TEST_DATA_DIR + "/1.166564490.bz2": 2908,
            TEST_DATA_DIR + "/betfair/1.180305278.bz2": 17085,
            TEST_DATA_DIR + "/betfair/1.166811431.bz2": 22692,
        }
        assert files == expected

    def test_data_catalog_instruments_no_partition(self):
        path = str(self.loaded_catalog.path / "data" / "betting_instrument.parquet/")
        dataset = pq.ParquetDataset(
            path_or_paths=path,
            filesystem=self.loaded_catalog.fs,
        )
        partitions = dataset.partitions
        assert not partitions.levels

    def test_data_catalog_metadata(self):
        assert ds.parquet_dataset(
            f"{self.loaded_catalog.path}/data/trade_tick.parquet/_metadata",
            filesystem=self.loaded_catalog.fs,
        )
        assert ds.parquet_dataset(
            f"{self.loaded_catalog.path}/data/trade_tick.parquet/_common_metadata",
            filesystem=self.loaded_catalog.fs,
        )

    def test_data_catalog_dataset_types(self):
        dataset = ds.dataset(
            str(self.loaded_catalog.path / "data" / "trade_tick.parquet"),
            filesystem=self.loaded_catalog.fs,
        )
        schema = {
            n: t.__class__.__name__ for n, t in zip(dataset.schema.names, dataset.schema.types)
        }
        expected = {
            "price": "DataType",
            "size": "DataType",
            "aggressor_side": "DictionaryType",
            "match_id": "DataType",
            "ts_event": "DataType",
            "ts_init": "DataType",
        }
        assert schema == expected

    def test_load_dask_distributed_client(self):
        from distributed import Client

        instrument_provider = BetfairInstrumentProvider.from_instruments([])

        # Arrange
        with Client(processes=False, threads_per_worker=1) as c:
            tasks = process_files(
                path=f"{TEST_DATA_DIR}/1.166564490*",
                reader=make_betfair_reader(instrument_provider),
                catalog=self.catalog,
                instrument_provider=instrument_provider,
            )

            # Act
            results = c.gather(c.compute(tasks))

        # Assert
        expected = {}
        assert results == expected
