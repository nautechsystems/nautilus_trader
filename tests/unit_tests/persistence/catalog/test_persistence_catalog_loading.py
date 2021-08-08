# import os
import pathlib
import sys

# import pyarrow.dataset as ds
# import pyarrow.parquet as pq
import orjson
import pytest

from nautilus_trader.adapters.betfair.parsing import on_market_update

# from nautilus_trader.data.wrangling import TradeTickDataWrangler
# from nautilus_trader.model import currencies
# from nautilus_trader.model.data.base import Data
# from nautilus_trader.model.data.base import GenericData
# from nautilus_trader.model.data.tick import QuoteTick
# from nautilus_trader.model.identifiers import InstrumentId
# from nautilus_trader.model.identifiers import Symbol
# from nautilus_trader.model.identifiers import Venue
# from nautilus_trader.model.instruments.currency import CurrencySpot
# from nautilus_trader.model.objects import Price
# from nautilus_trader.model.objects import Quantity
# from nautilus_trader.serialization.arrow.serializer import register_parquet
# from nautilus_trader.persistence.catalog.core import DataCatalog
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
from nautilus_trader.persistence.catalog.loading import load
from nautilus_trader.persistence.catalog.loading import process_files
from nautilus_trader.persistence.catalog.parsers import CSVReader
from nautilus_trader.persistence.catalog.parsers import TextReader

# from nautilus_trader.persistence.catalog.parsers import ParquetParser
# from nautilus_trader.persistence.catalog.parsers import TextParser
from nautilus_trader.persistence.catalog.scanner import scan
from nautilus_trader.persistence.util import SyncExecutor
from tests.test_kit import PACKAGE_ROOT


# from decimal import Decimal
# from functools import partial
#
# import fsspec.implementations.memory
# import orjson
# from nautilus_trader.adapters.betfair.data import on_market_update
# from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
# from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
# from nautilus_trader.common.providers import InstrumentProvider
# from nautilus_trader.core.datetime import millis_to_nanos


# from numpy import dtype
# from pandas import CategoricalDtype


# from tests.test_kit.stubs import TestStubs


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


@pytest.fixture
def executor():
    return SyncExecutor()


# TODO add some simpler tests


def test_process_files_csv(executor, get_parser):
    files = scan(path=TEST_DATA_DIR, glob_pattern="truefx*.csv", chunk_size=1_000_000)
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


def test_load_text_betfair():
    instrument_provider = BetfairInstrumentProvider.from_instruments([])

    def betfair_parser(chunk):
        update = orjson.loads(chunk)
        results = on_market_update(instrument_provider=instrument_provider, update=update)
        yield from results

    reader = TextReader(
        line_parser=betfair_parser,
        instrument_provider=instrument_provider,
        instrument_provider_update=historical_instrument_provider_loader,
    )
    files = load(
        path=TEST_DATA_DIR,
        reader=reader,
        glob_pattern="**.bz2",
        instrument_provider=instrument_provider,
    )
    assert files == {"1.166564490.bz2": 2698, "1.180305278.bz2": 13797, "1.166811431.bz2": 20871}


# def test_data_loader_parquet():
#     def filename_to_instrument(fn):
#         if "btcusd" in fn:
#             return TestInstrumentProvider.btcusdt_binance()
#         else:
#             raise KeyError()
#
#     def parser(data_type, df, filename):
#         instrument = filename_to_instrument(fn=filename)
#         if data_type == "quote_ticks":
#             df = df.set_index("timestamp")[["bid", "ask", "bid_size", "ask_size"]]
#             wrangler = QuoteTickDataWrangler(data_quotes=df, instrument=instrument)
#         elif data_type == "trade_ticks":
#             wrangler = TradeTickDataWrangler(df=df, instrument=instrument)
#         else:
#             raise TypeError()
#         wrangler.pre_process(0)
#         return wrangler.build_ticks()
#
#     loader = DataLoader(
#         path=TEST_DATA_DIR,
#         parser=ParquetParser(
#             data_type="quote_ticks",
#             parser=parser,
#         ),
#         glob_pattern="*quote*.parquet",
#         instrument_provider=InstrumentProvider(),
#     )
#     assert len(loader.path) == 1
#     values = [x for vals in loader.run() for x in vals if isinstance(x, QuoteTick)]
#     assert len(values) == 451
#
#
# def test_data_loader_csv(catalog):
#     def parse_csv_tick(df, instrument_id, state=None):
#         for _, r in df.iterrows():
#             ts = millis_to_nanos(pd.Timestamp(r["timestamp"]).timestamp())
#             tick = QuoteTick(
#                 instrument_id=instrument_id,
#                 bid=Price.from_str(str(r["bid"])),
#                 ask=Price.from_str(str(r["ask"])),
#                 bid_size=Quantity.from_int(1_000_000),
#                 ask_size=Quantity.from_int(1_000_000),
#                 ts_event=ts,
#                 ts_init=ts,
#             )
#             yield tick
#
#     loader = DataLoader(
#         path=TEST_DATA_DIR,
#         parser=CSVParser(parser=partial(parse_csv_tick, instrument_id=TestStubs.audusd_id())),
#         chunk_size=100 ** 2,
#         glob_pattern="truefx-usd*.csv",
#     )
#     assert len(loader.path) == 1
#     values = [x for vals in loader.run() for x in vals if isinstance(x, QuoteTick)]
#     assert len(values) == 1000
#
#     # Write to parquet
#     catalog.import_from_data_loader(loader=loader)
#     data = catalog.quote_ticks()
#     assert len(data) == 1000
#
#
# def test_data_catalog_from_env():
#     os.environ["NAUTILUS_CATALOG"] = "memory:///"
#     c = DataCatalog.from_env()
#     assert isinstance(c.fs, fsspec.implementations.memory.MemoryFileSystem)
#     assert str(c.path) == "/"
#
#     os.environ["NAUTILUS_CATALOG"] = "file:///data"
#     c = DataCatalog.from_env()
#     assert isinstance(c.fs, fsspec.implementations.local.LocalFileSystem)
#     assert str(c.path) == "/data"
#
#
# def test_data_catalog_instruments_no_partition(loaded_catalog):
#     ds = pq.ParquetDataset(
#         path_or_paths=str(loaded_catalog.path / "betting_instrument.parquet/"),
#         filesystem=loaded_catalog.fs,
#     )
#     partitions = ds.partitions
#     assert not partitions.levels
#
#
# def test_data_catalog_metadata(loaded_catalog):
#     assert ds.parquet_dataset(
#         f"{loaded_catalog.path}/trade_tick.parquet/_metadata", filesystem=loaded_catalog.fs
#     )
#     assert ds.parquet_dataset(
#         f"{loaded_catalog.path}/trade_tick.parquet/_common_metadata", filesystem=loaded_catalog.fs
#     )
#
#
# def test_data_catalog_dataset_types(loaded_catalog):
#     dataset = ds.dataset(
#         str(loaded_catalog.path / "trade_tick.parquet"), filesystem=loaded_catalog.fs
#     )
#     schema = {n: t.__class__.__name__ for n, t in zip(dataset.schema.names, dataset.schema.types)}
#     expected = {
#         "price": "DataType",
#         "size": "DataType",
#         "aggressor_side": "DictionaryType",
#         "match_id": "DataType",
#         "ts_event": "DataType",
#         "ts_init": "DataType",
#     }
#     assert schema == expected
#
#
# def test_data_catalog_parquet():
#     def filename_to_instrument(fn):
#         if "btcusd" in fn:
#             return TestInstrumentProvider.btcusdt_binance()
#         else:
#             raise KeyError()
#
#     def parser(data_type, df, filename):
#         instrument = filename_to_instrument(fn=filename)
#         df = df.set_index("timestamp")
#         if data_type == "quote_ticks":
#             df = df[["bid", "ask", "bid_size", "ask_size"]]
#             wrangler = QuoteTickDataWrangler(data_quotes=df, instrument=instrument)
#         elif data_type == "trade_ticks":
#             wrangler = TradeTickDataWrangler(data=df, instrument=instrument)
#         else:
#             raise TypeError()
#         wrangler.pre_process(0)
#         return wrangler.build_ticks()
#
#     quote_loader = DataLoader(
#         path=TEST_DATA_DIR,
#         parser=ParquetParser(
#             data_type="quote_ticks",
#             parser=parser,
#         ),
#         glob_pattern="*quote*.parquet",
#         instrument_provider=InstrumentProvider(),
#     )
#     trade_loader = DataLoader(
#         path=TEST_DATA_DIR,
#         parser=ParquetParser(
#             data_type="trade_ticks",
#             parser=parser,
#         ),
#         glob_pattern="*trade*.parquet",
#         instrument_provider=InstrumentProvider(),
#     )
#
#     # Write to parquet
#     catalog = DataCatalog.from_uri("memory:///")
#     catalog.import_from_data_loader(loader=quote_loader)
#     catalog.import_from_data_loader(loader=trade_loader)
#     assert len(catalog.quote_ticks(instrument_ids=["BTC/USDT.BINANCE"])) == 451
#     assert len(catalog.trade_ticks(instrument_ids=["BTC/USDT.BINANCE"])) == 2001
#
#
# def test_data_catalog_filter(loaded_catalog):
#     assert len(loaded_catalog.order_book_deltas()) == 2384
#     assert (
#         len(loaded_catalog.order_book_deltas(filter_expr=ds.field("delta_type") == "DELETE")) == 351
#     )
#
#
# def test_data_catalog_parquet_dtypes():
#     # Write trade ticks
#
#     # TODO - fix
#     result = pd.read_parquet(fn).dtypes.to_dict()
#     expected = {
#         "aggressor_side": CategoricalDtype(categories=["UNKNOWN"], ordered=False),
#         "instrument_id": CategoricalDtype(
#             categories=[
#                 "Basketball,,29628709,20191221-001000,ODDS,MATCH_ODDS,1.166564490,237491,0.0.BETFAIR",
#                 "Basketball,,29628709,20191221-001000,ODDS,MATCH_ODDS,1.166564490,60424,0.0.BETFAIR",
#             ],
#             ordered=False,
#         ),
#         "match_id": dtype("O"),
#         "price": dtype("float64"),
#         "size": dtype("float64"),
#         "ts_event": dtype("int64"),
#         "ts_init": dtype("int64"),
#     }
#     assert result == expected
#
#
# def test_data_loader_generic_data(catalog):
#     class NewsEvent(Data):
#         def __init__(self, name, impact, currency, ts_event):
#             super().__init__(ts_event=ts_event, ts_init=ts_event)
#             self.name = name
#             self.impact = impact
#             self.currency = currency
#
#         @staticmethod
#         def to_dict(self):
#             return {
#                 "name": self.name,
#                 "impact": self.impact,
#                 "currency": self.currency,
#                 "ts_event": self.ts_event,
#             }
#
#         @staticmethod
#         def from_dict(data):
#             return NewsEvent(**data)
#
#     register_parquet(
#         NewsEvent,
#         NewsEvent.to_dict,
#         NewsEvent.from_dict,
#         partition_keys=("currency",),
#         force=True,
#     )
#
#     def make_news_event(df, state=None):
#         for _, row in df.iterrows():
#             yield NewsEvent(
#                 name=row["Name"],
#                 impact=row["Impact"],
#                 currency=row["Currency"],
#                 ts_event=millis_to_nanos(pd.Timestamp(row["Start"]).timestamp()),
#             )
#
#     loader = DataLoader(
#         path=TEST_DATA_DIR,
#         parser=CSVParser(parser=make_news_event),
#         glob_pattern="news_events.csv",
#     )
#     catalog.import_from_data_loader(loader=loader)
#     df = catalog.generic_data(cls=NewsEvent, filter_expr=ds.field("currency") == "USD")
#     assert len(df) == 22925
#     data = catalog.generic_data(
#         cls=NewsEvent, filter_expr=ds.field("currency") == "USD", as_nautilus=True
#     )
#     assert len(data) == 22925 and isinstance(data[0], GenericData)
#
#
# def test_data_catalog_append(catalog):
#     instrument_data = orjson.loads(open(TEST_DATA_DIR + "/crypto_instruments.json").read())
#
#     objects = []
#     for data in instrument_data:
#         symbol, venue = data["id"].rsplit(".", maxsplit=1)
#         instrument = CurrencySpot(
#             instrument_id=InstrumentId(symbol=Symbol(symbol), venue=Venue(venue)),
#             base_currency=getattr(currencies, data["base_currency"]),
#             quote_currency=getattr(currencies, data["quote_currency"]),
#             price_precision=data["price_precision"],
#             size_precision=data["size_precision"],
#             price_increment=Price.from_str(data["price_increment"]),
#             size_increment=Quantity.from_str(data["size_increment"]),
#             lot_size=data["lot_size"],
#             max_quantity=data["max_quantity"],
#             min_quantity=data["min_quantity"],
#             max_notional=data["max_notional"],
#             min_notional=data["min_notional"],
#             max_price=data["max_price"],
#             min_price=data["min_price"],
#             margin_init=Decimal(1.0),
#             margin_maint=Decimal(1.0),
#             maker_fee=Decimal(1.0),
#             taker_fee=Decimal(1.0),
#             ts_event=0,
#             ts_init=0,
#         )
#         objects.append(instrument)
#     catalog._write_chunks(chunk=objects[:3])
#     catalog._write_chunks(chunk=objects[3:])
#     assert len(catalog.instruments()) == 6
#
#
# def test_catalog_invalid_partition_key(catalog):
#     register_parquet(
#         NewsEvent,
#         _news_event_to_dict,
#         _news_event_from_dict,
#         partition_keys=("name",),
#         force=True,
#     )
#
#     def make_news_event(df, state=None):
#         for _, row in df.iterrows():
#             yield NewsEvent(
#                 name=row["Name"],
#                 impact=row["Impact"],
#                 currency=row["Currency"],
#                 ts_event=millis_to_nanos(pd.Timestamp(row["Start"]).timestamp()),
#             )
#
#     loader = DataLoader(
#         path=TEST_DATA_DIR,
#         parser=CSVParser(parser=make_news_event),
#         glob_pattern="news_events.csv",
#     )
#     with pytest.raises(ValueError):
#         catalog.import_from_data_loader(loader=loader)
