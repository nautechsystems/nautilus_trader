from decimal import Decimal
import os
import pathlib

import fsspec
import orjson
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
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.objects import Money
from tests.test_kit import PACKAGE_ROOT


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))
CATALOGUE_DIR = TEST_DATA_DIR + "/catalogue"


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
def catalogue_dir():
    # Ensure we have a catalogue directory, and its cleaned up after use
    fs = fsspec.filesystem("file")
    catalogue = str(pathlib.Path(CATALOGUE_DIR))
    os.environ.update({"NAUTILUS_BACKTEST_DIR": str(catalogue)})
    if fs.exists(catalogue):
        fs.rm(catalogue, recursive=True)
    fs.mkdir(catalogue)
    yield
    fs.rm(catalogue, recursive=True)


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


def test_data_catalogue_import(catalogue_dir, data_loader):
    catalogue = DataCatalog()
    catalogue.import_from_data_loader(loader=data_loader)
    instruments = catalogue.instruments()
    assert len(instruments) == 2


def test_data_catalogue_backtest_data(catalogue_dir, data_loader):
    catalogue = DataCatalog()
    catalogue.import_from_data_loader(loader=data_loader)
    data = catalogue.load_backtest_data()
    assert len(data) == 2698


def test_data_catalogue_backtest_run(catalogue_dir, data_loader):
    catalogue = DataCatalog()
    catalogue.import_from_data_loader(loader=data_loader)
    instruments = catalogue.instruments()
    engine = BacktestEngine()
    engine = catalogue.setup_engine(engine=engine, instruments=instruments)
    engine.add_venue(
        venue=BETFAIR_VENUE,
        venue_type=VenueType.EXCHANGE,
        account_type=AccountType.CASH,
        base_currency=GBP,
        oms_type=OMSType.NETTING,
        starting_balances=[Money(1000, GBP)],
        order_book_level=OrderBookLevel.L2,
    )
    strategy = OrderbookImbalance(
        instrument=instruments[0], max_trade_size=Decimal("50"), order_id_tag="OI"
    )
    engine.run(strategies=[strategy])
