import os
import pathlib

import fsspec
import orjson
import pytest

from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
from nautilus_trader.backtest.data_loader import CSVParser
from nautilus_trader.backtest.data_loader import DataCatalogue
from nautilus_trader.backtest.data_loader import DataLoader
from nautilus_trader.backtest.data_loader import ParquetParser
from nautilus_trader.backtest.data_loader import TextParser
from nautilus_trader.backtest.data_loader import parse_timestamp
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


def test_data_loader_json_betting_parser():
    instrument_provider = BetfairInstrumentProvider.from_instruments([])

    parser = TextParser(
        line_parser=lambda x: on_market_update(
            instrument_provider=instrument_provider, update=orjson.loads(x)
        ),
        instrument_provider_update=historical_instrument_provider_loader(
            instrument_provider
        ),
    )
    loader = DataLoader(path=TEST_DATA_DIR, parser=parser, glob_pattern="**.zip")
    assert len(loader.path) == 1

    data = [x for y in loader.run() for x in y]
    assert len(data) == 19100


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
        instrument_provider_update=historical_instrument_provider_loader(
            instrument_provider
        ),
    )
    return DataLoader(
        path=TEST_DATA_DIR,
        parser=parser,
        glob_pattern="1.166564490*",
        instrument_provider=instrument_provider,
    )


def test_data_catalogue_import(catalogue_dir, data_loader):
    catalogue = DataCatalogue()
    catalogue.import_from_data_loader(loader=data_loader)
    instruments = catalogue.instruments()
    assert len(instruments) == 18


def test_data_catalogue_backtest(catalogue_dir, data_loader):
    catalogue = DataCatalogue()
    catalogue.import_from_data_loader(loader=data_loader)
    data = catalogue.load_backtest_data()
    assert len(data) == 1000
