import inspect
import os
import sys
from functools import partial

import fsspec.implementations.memory
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
from nautilus_trader.persistence.catalog.core import DataCatalog
from nautilus_trader.persistence.catalog.loading import load
from nautilus_trader.persistence.catalog.loading import process_files
from nautilus_trader.persistence.catalog.metadata import load_processed_raw_files
from nautilus_trader.persistence.catalog.parsers import TextReader
from nautilus_trader.persistence.catalog.scanner import scan
from nautilus_trader.persistence.util import get_catalog_fs
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.unit_tests.backtest.test_backtest_config import TEST_DATA_DIR


ROOT = "/root"


@pytest.fixture(autouse=True)
def nautilus_dir():
    os.environ["NAUTILUS_DATA"] = f"memory://{ROOT}"


@pytest.fixture(autouse=True, scope="function")
def test_reset():
    """Cleanup resources before each test run"""
    fs = get_catalog_fs()
    assert isinstance(fs, fsspec.implementations.memory.MemoryFileSystem)
    try:
        fs.rm("/", recursive=True)
    except FileNotFoundError:
        pass
    fs.mkdir(f"{ROOT}/data")
    assert fs.exists(ROOT)
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
        reader = TextReader(
            line_parser=partial(parse_betfair, instrument_provider=instrument_provider),
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


@pytest.fixture(scope="function")
def load_data(betfair_reader):
    instrument_provider = BetfairInstrumentProvider.from_instruments([])

    load(
        path=TEST_DATA_DIR,
        reader=betfair_reader(instrument_provider),
        glob_pattern="1.166564490*",
        instrument_provider=instrument_provider,
    )
    fs = get_catalog_fs()
    assert fs.isdir(f"{ROOT}/data/betting_instrument.parquet")


@pytest.fixture(scope="function")
def catalog():
    catalog = DataCatalog(path="/root/data", fs_protocol="memory")
    catalog.fs = get_catalog_fs()
    return catalog


@pytest.fixture(scope="function")
def loaded_catalog(catalog, load_data):
    return catalog
