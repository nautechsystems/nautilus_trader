import pathlib
import sys
from functools import partial
from typing import Callable

import fsspec.implementations.local
import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
from nautilus_trader.persistence.catalog.parsers import CSVReader
from nautilus_trader.persistence.catalog.parsers import RawFile
from nautilus_trader.persistence.catalog.parsers import TextReader
from nautilus_trader.persistence.catalog.scanner import scan
from tests.test_kit import PACKAGE_ROOT


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


def test_parse_raw_file_single_chunk():
    fs = fsspec.implementations.local.LocalFileSystem()
    rf = RawFile(fs=fs, path=TEST_DATA_DIR + "/betfair/1.166811431.bz2", chunk_size=-1)
    data = list(rf.iter_raw())
    assert len(data) == 1
    assert len(data[0]) == 151707


def test_parse_raw_file_multiple_chunks():
    fs = fsspec.implementations.local.LocalFileSystem()
    rf = RawFile(fs=fs, path=TEST_DATA_DIR + "/betfair/1.166811431.bz2", chunk_size=100_000)
    data = list(rf.iter_raw())
    assert len(data) == 2
    assert len(data[0]) == 100_000
    assert len(data[1]) == 51707


@pytest.mark.parametrize(
    "glob, parser, expected",
    [
        ("binance*.txt", "parse_text", {"binance-btcusdt-instrument.txt": 1}),
    ],
    indirect=["parser"],
)
def test_text_parser(glob, parser, expected):
    reader = TextReader(line_parser=parser)
    files = scan(path=TEST_DATA_DIR, glob_pattern=glob)
    results = {}
    for f in files:
        f.reader = reader
        data = []
        for chunk in f.iter_parsed():
            data.extend(chunk)
        results[f.name] = len(data)
    assert results == expected


@pytest.mark.parametrize(
    "glob, parser, expected",
    [
        (
            "truefx*.csv",
            "parse_csv_quotes",
            {"truefx-audusd-ticks.csv": 100000, "truefx-usdjpy-ticks.csv": 1000},
        ),
        # TODO (bm)
        # ("fxcm*.csv", "parse_csv_quotes", {}),
        # ("binance*.csv", "parse_csv_quotes", {}),
    ],
    indirect=["parser"],
)
def test_csv_quoter_parser(glob, parser, expected):
    files = scan(path=TEST_DATA_DIR, glob_pattern=glob)

    results = {}
    for f in files:
        f.reader = CSVReader(chunk_parser=parser, as_dataframe=True)
        data = []
        for chunk in f.iter_parsed():
            data.extend(chunk)
        results[f.name] = len(data)
    assert results == expected


@pytest.mark.parametrize(
    "glob, parser, expected",
    [
        ("betfair/*.bz2", "parse_betfair", {"1.166811431.bz2": 16027, "1.180305278.bz2": 12448}),
    ],
    indirect=["parser"],
)
def test_byte_parser(glob, parser: Callable, expected):
    provider = BetfairInstrumentProvider.from_instruments([])
    reader = TextReader(
        line_parser=partial(parser, instrument_provider=provider),
        instrument_provider=provider,
        instrument_provider_update=historical_instrument_provider_loader,
    )

    files = scan(path=TEST_DATA_DIR, glob_pattern=glob)
    results = {}
    for f in files:
        f.reader = reader
        data = []
        for chunk in f.iter_parsed():
            data.extend(chunk)
        results[f.name] = len(data)
    assert results == expected


# def test_byte_parser():
#     files = scan(path=TEST_DATA_DIR, glob_pattern="*.json")
#     results = {}
#     for f in files:
#         f.parser = ByteParser()
#         data = list(f.iter_parsed())
#         results[f.name] = len(data)
#     expected = {}
#     assert results == expected
#
#
# def test_parquet_parser():
#     files = scan(path=TEST_DATA_DIR, glob_pattern="*.parquet")
#     results = {}
#     for f in files:
#         f.parser = ParquetParser()
#         data = list(f.iter_parsed())
#         results[f.name] = len(data)
#     expected = {}
#     assert results == expected
