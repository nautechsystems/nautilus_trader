from decimal import Decimal
import os
import pathlib

import fsspec
from numpy import dtype
import orjson
from pandas import CategoricalDtype
import pyarrow.dataset as ds
import pytest

from examples.strategies.orderbook_imbalance import OrderbookImbalance
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
from nautilus_trader.backtest.data_loader import CSVParser
from nautilus_trader.backtest.data_loader import DataCatalog
from nautilus_trader.backtest.data_loader import DataLoader
from nautilus_trader.backtest.data_loader import ParquetParser
from nautilus_trader.backtest.data_loader import TextParser
from nautilus_trader.backtest.data_loader import parse_timestamp
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model.c_enums.account_type import AccountType
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.objects import Money
from tests.test_kit import PACKAGE_ROOT


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))
catalog_DIR = TEST_DATA_DIR + "/catalog"


@pytest.mark.parametrize(
    "glob, num_files",
    [
        ("**.json", 2),
        ("**.txt", 1),
        ("**.parquet", 2),
        ("**.csv", 11),
    ],
)
def test_data_loader_paths(glob, num_files):
    d = DataLoader(path=TEST_DATA_DIR, parser=CSVParser(), glob_pattern=glob)
    assert len(d.path) == num_files


def test_data_loader_stream():
    loader = DataLoader(path=TEST_DATA_DIR, parser=None, glob_pattern="1.166564490.bz2")
    raw = list(loader.stream_bytes())
    assert len(raw) == 6


def test_data_loader_json_betting_parser():
    instrument_provider = BetfairInstrumentProvider.from_instruments([])

    parser = TextParser(
        line_parser=lambda x: on_market_update(
            instrument_provider=instrument_provider, update=orjson.loads(x)
        ),
        instrument_provider_update=historical_instrument_provider_loader,
    )
    loader = DataLoader(
        path=TEST_DATA_DIR,
        parser=parser,
        glob_pattern="**.bz2",
        instrument_provider=instrument_provider,
    )
    assert len(loader.path) == 3

    data = [x for y in loader.run() for x in y]
    assert len(data) == 30829


def test_data_loader_parquet():
    loader = DataLoader(
        path=TEST_DATA_DIR, parser=ParquetParser(), glob_pattern="**.parquet"
    )
    assert len(loader.path) == 2


def test_parse_timestamp():
    assert parse_timestamp(1580453644855000064) == 1580453644855000064
    assert parse_timestamp("2020-01-31T06:54:04.855000064+10:00") == 1580417644855000064
    assert parse_timestamp("2020-01-31 06:54:04.855000064") == 1580453644855000064
    assert parse_timestamp("2020-01-31") == 1580428800000000000


@pytest.fixture(scope="function")
def catalog_dir():
    # Ensure we have a catalog directory, and its cleaned up after use
    fs = fsspec.filesystem("file")
    catalog = str(pathlib.Path(catalog_DIR))
    os.environ.update({"NAUTILUS_BACKTEST_DIR": str(catalog)})
    if fs.exists(catalog):
        fs.rm(catalog, recursive=True)
    fs.mkdir(catalog)
    yield
    fs.rm(catalog, recursive=True)


@pytest.fixture(scope="function")
def data_loader():
    instrument_provider = BetfairInstrumentProvider.from_instruments([])
    parser = TextParser(
        line_parser=lambda x: on_market_update(
            instrument_provider=instrument_provider, update=orjson.loads(x)
        ),
        instrument_provider_update=historical_instrument_provider_loader,
    )
    return DataLoader(
        path=TEST_DATA_DIR,
        parser=parser,
        glob_pattern="1.166564490*",
        instrument_provider=instrument_provider,
    )


@pytest.fixture(scope="function")
def catalog(catalog_dir, data_loader):
    catalog = DataCatalog()
    catalog.import_from_data_loader(loader=data_loader)
    return catalog


def test_data_catalog_import(catalog):
    instruments = catalog.instruments()
    assert len(instruments) == 2


def test_data_catalog_metadata(catalog):
    assert ds.parquet_dataset(f"{catalog.root}/trade_tick.parquet/_metadata")
    assert ds.parquet_dataset(f"{catalog.root}/trade_tick.parquet/_common_metadata")


def test_data_catalog_dataset_types(catalog):
    dataset = ds.dataset(catalog.root / "trade_tick.parquet")
    schema = {
        n: t.__class__.__name__
        for n, t in zip(dataset.schema.names, dataset.schema.types)
    }
    expected = {
        "type": "DictionaryType",
        "price": "DataType",
        "size": "DataType",
        "aggressor_side": "DictionaryType",
        "match_id": "DataType",
        "ts_event_ns": "DataType",
        "ts_recv_ns": "DataType",
        "__index_level_0__": "DataType",
    }
    assert schema == expected


def test_data_catalog_filter(catalog):
    assert len(catalog.order_book_deltas()) == 2384
    assert (
        len(catalog.order_book_deltas(filter_expr=ds.field("delta_type") == "DELETE"))
        == 351
    )


def test_data_catalog_queries(catalog):
    result = catalog.trade_ticks().dtypes.to_dict()
    expected = {
        "aggressor_side": CategoricalDtype(categories=["UNKNOWN"], ordered=False),
        "instrument_id": CategoricalDtype(
            categories=[
                "Basketball||29628709|2019-12-21 00:10:00+00:00|ODDS|MATCH_ODDS|1.166564490|237491|.BETFAIR",
                "Basketball||29628709|2019-12-21 00:10:00+00:00|ODDS|MATCH_ODDS|1.166564490|60424|.BETFAIR",
            ],
            ordered=False,
        ),
        "match_id": dtype("O"),
        "price": dtype("float64"),
        "size": dtype("float64"),
        "ts_event_ns": dtype("int64"),
        "ts_recv_ns": dtype("int64"),
        "type": CategoricalDtype(categories=["TradeTick"], ordered=False),
    }
    assert result == expected


def test_data_catalog_backtest_data_no_filter(catalog):
    data = catalog.load_backtest_data()
    assert len(sum(data.values(), list())) == 2323


def test_data_catalog_backtest_data_filtered(catalog):
    instruments = catalog.instruments(as_nautilus=True)
    engine = BacktestEngine(bypass_logging=True)
    engine = catalog.setup_engine(
        engine=engine,
        instruments=[instruments[1]],
        start_timestamp=1576869877788000000,
    )
    engine.add_venue(
        venue=BETFAIR_VENUE,
        venue_type=VenueType.EXCHANGE,
        account_type=AccountType.CASH,
        base_currency=GBP,
        oms_type=OMSType.NETTING,
        starting_balances=[Money(10000, GBP)],
        order_book_level=BookLevel.L2,
    )
    engine.run()
    # Total events 1045
    assert engine.iteration == 530


def test_data_catalog_backtest_run(catalog):
    instruments = catalog.instruments(as_nautilus=True)
    engine = BacktestEngine()
    engine = catalog.setup_engine(engine=engine, instruments=[instruments[1]])
    engine.add_venue(
        venue=BETFAIR_VENUE,
        venue_type=VenueType.EXCHANGE,
        account_type=AccountType.CASH,
        base_currency=GBP,
        oms_type=OMSType.NETTING,
        starting_balances=[Money(10000, GBP)],
        order_book_level=BookLevel.L2,
    )
    strategy = OrderbookImbalance(
        instrument=instruments[1], max_trade_size=Decimal("50"), order_id_tag="OI"
    )
    engine.run(strategies=[strategy])
