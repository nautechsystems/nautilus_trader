import pathlib
import sys

import pyarrow.dataset as ds
import pyarrow.parquet as pq
import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.data.wrangling import TradeTickDataWrangler
from nautilus_trader.persistence.catalog.loading import load
from nautilus_trader.persistence.catalog.loading import nautilus_chunk_to_dataframes
from nautilus_trader.persistence.catalog.loading import process_files
from nautilus_trader.persistence.catalog.parsers import CSVReader
from nautilus_trader.persistence.catalog.parsers import ParquetReader
from nautilus_trader.persistence.catalog.scanner import scan
from nautilus_trader.persistence.util import SyncExecutor
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.providers import TestInstrumentProvider


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


@pytest.fixture
def executor():
    return SyncExecutor()


def test_nautilus_chunk_to_dataframes(betfair_nautilus_objects):
    dfs = nautilus_chunk_to_dataframes(betfair_nautilus_objects)
    result = {}
    for cls in dfs:
        for ins in dfs[cls]:
            result[(cls.__name__, ins)] = len(dfs[cls][ins])
    expected = {
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


def test_write_parquet():
    raise NotImplementedError


def test_read_and_clear_existing_data():
    raise NotImplementedError


def test_determine_partition_cols():
    raise NotImplementedError


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


@pytest.mark.skip(reason="Not implemented")
def test_data_loader_parquet():
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

    results = load(
        path=TEST_DATA_DIR,
        reader=ParquetReader(
            data_type="quote_ticks",
            parser=parser,
        ),
        glob_pattern="*quote*.parquet",
        instrument_provider=InstrumentProvider(),
    )
    expected = {}
    assert results == expected


def test_load_text_betfair(betfair_reader):
    instrument_provider = BetfairInstrumentProvider.from_instruments([])

    files = load(
        path=TEST_DATA_DIR,
        reader=betfair_reader(instrument_provider=instrument_provider),
        glob_pattern="**.bz2",
        instrument_provider=instrument_provider,
    )
    expected = {
        TEST_DATA_DIR + "/1.166564490.bz2": 2698,
        TEST_DATA_DIR + "/betfair/1.180305278.bz2": 13797,
        TEST_DATA_DIR + "/betfair/1.166811431.bz2": 20871,
    }
    assert files == expected


def test_data_catalog_instruments_no_partition(loaded_catalog):
    ds = pq.ParquetDataset(
        path_or_paths=str(loaded_catalog.path / "betting_instrument.parquet/"),
        filesystem=loaded_catalog.fs,
    )
    partitions = ds.partitions
    assert not partitions.levels


def test_data_catalog_metadata(loaded_catalog):
    assert ds.parquet_dataset(
        f"{loaded_catalog.path}/trade_tick.parquet/_metadata", filesystem=loaded_catalog.fs
    )
    assert ds.parquet_dataset(
        f"{loaded_catalog.path}/trade_tick.parquet/_common_metadata", filesystem=loaded_catalog.fs
    )


def test_data_catalog_dataset_types(loaded_catalog):
    dataset = ds.dataset(
        str(loaded_catalog.path / "trade_tick.parquet"), filesystem=loaded_catalog.fs
    )
    schema = {n: t.__class__.__name__ for n, t in zip(dataset.schema.names, dataset.schema.types)}
    expected = {
        "price": "DataType",
        "size": "DataType",
        "aggressor_side": "DictionaryType",
        "match_id": "DataType",
        "ts_event": "DataType",
        "ts_init": "DataType",
    }
    assert schema == expected


# def test_data_catalog_parquet_dtypes():
#     # Write trade ticks
#
#     # TODO - fix
#     # result = loaded_catalog.trade_ticks().dtypes.to_dict()
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
