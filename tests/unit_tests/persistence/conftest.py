import inspect
import os
import sys

import fsspec.implementations.memory
import numpy as np
import orjson
import pandas as pd
import pytest

from nautilus_trader.adapters.betfair.parsing import on_market_update
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
from nautilus_trader.data.wrangling import BarDataWrangler
from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.loading import process_files
from nautilus_trader.persistence.catalog.metadata import PROCESSED_FILES_FN
from nautilus_trader.persistence.catalog.metadata import load_processed_raw_files
from nautilus_trader.persistence.catalog.parsers import TextReader
from nautilus_trader.persistence.catalog.scanner import scan
from nautilus_trader.persistence.util import get_catalog_fs
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.unit_tests.backtest.test_backtest_config import TEST_DATA_DIR


@pytest.fixture(autouse=True)
def nautilus_dir():
    os.environ["NAUTILUS_DATA"] = "memory:///"


@pytest.fixture(autouse=True, scope="function")
def test_reset():
    """Cleanup resources before each test run"""
    fs = get_catalog_fs()
    assert isinstance(fs, fsspec.implementations.memory.MemoryFileSystem)
    for f in list(fs.glob("**/*")):
        fs.rm(f)
    if fs.exists(PROCESSED_FILES_FN):
        fs.rm(PROCESSED_FILES_FN)
    assert not load_processed_raw_files()
    yield


@pytest.fixture()
def get_parser():
    def inner(name):
        mappings = {
            name: obj
            for name, obj in inspect.getmembers(sys.modules[__name__])
            if inspect.isfunction(obj)
        }
        if name in mappings:
            return mappings[name]
        raise KeyError

    return inner


@pytest.fixture()
def parser(request, get_parser):
    return get_parser(request.param)


@pytest.fixture()
def sample_df():
    return pd.DataFrame({"value": np.random.random(5), "instrument_id": ["a", "a", "a", "b", "b"]})


def parse_text(x):
    # Mock actual parsing
    yield TradeTick(
        instrument_id=TestStubs.audusd_id(),
        price=Price.from_int(1),
        size=Quantity.from_int(1),
        aggressor_side=AggressorSide.BUY,
        match_id="1",
        ts_event=0,
        ts_init=0,
    )


def parse_csv_quotes(data):
    if data is None:
        return
    data.loc[:, "timestamp"] = pd.to_datetime(data["timestamp"])
    wrangler = QuoteTickDataWrangler(
        instrument=TestInstrumentProvider.default_fx_ccy(
            "AUD/USD"
        ),  # Normally we would properly parse this
        data_quotes=data.set_index("timestamp"),
    )
    wrangler.pre_process(0)
    yield from wrangler.build_ticks()


def parse_json_bytes(data):
    yield data


def parse_betfair(line, instrument_provider):
    yield from on_market_update(instrument_provider=instrument_provider, update=orjson.loads(line))


@pytest.fixture()
def betfair_reader():
    def inner(instrument_provider):
        def betfair_parser(chunk):
            update = orjson.loads(chunk)
            results = on_market_update(instrument_provider=instrument_provider, update=update)
            yield from results

        reader = TextReader(
            line_parser=betfair_parser,
            instrument_provider=instrument_provider,
            instrument_provider_update=historical_instrument_provider_loader,
        )
        return reader

    return inner


@pytest.fixture()
def betfair_nautilus_objects(betfair_reader):
    instrument_provider = BetfairInstrumentProvider.from_instruments([])
    files = scan(
        path=TEST_DATA_DIR + "/betfair",
        glob_pattern="**.bz2",
    )

    data = []

    def append(*args, **kwargs):
        if kwargs["chunk"]:
            data.extend(kwargs["chunk"])

    process_files(
        files=files,
        reader=betfair_reader(instrument_provider=instrument_provider),
        instrument_provider=instrument_provider,
        output_func=append,
    )
    return data


# TODO (cs)
@pytest.mark.skip(reason="Not implemented")
def parse_csv_bars(data):
    if data is None:
        return
    data.loc[:, "timestamp"] = pd.to_datetime(data["timestamp"])
    wrangler = BarDataWrangler(
        BarType(
            instrument_id=TestInstrumentProvider.default_fx_ccy("AUD/USD").id,
            spec=BarSpecification(),
        ),
        2,
        2,
        data=data.set_index("timestamp"),
    )
    yield from wrangler.build_bars_all()
