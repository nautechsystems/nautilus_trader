import inspect
import os
import sys

import pandas as pd
import pytest
from fsspec.implementations.memory import MemoryFileSystem

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.data.wrangling import BarDataWrangler
from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.backtest.loading import load
from nautilus_trader.persistence.backtest.loading import process_files
from nautilus_trader.persistence.backtest.scanner import scan
from nautilus_trader.persistence.catalog import DataCatalog
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.unit_tests.backtest.test_backtest_config import TEST_DATA_DIR


ROOT = "/root"


@pytest.fixture(autouse=True)
def nautilus_dir():
    os.environ["NAUTILUS_DATA"] = f"memory://{ROOT}"


@pytest.fixture(autouse=True, scope="function")
def reset():
    """Cleanup resources before each test run"""
    os.environ["NAUTILUS_CATALOG"] = "memory:///root/"
    catalog = DataCatalog.from_env()
    assert isinstance(catalog.fs, MemoryFileSystem)
    try:
        catalog.fs.rm("/", recursive=True)
    except FileNotFoundError:
        pass
    catalog.fs.mkdir("/root/data")
    assert catalog.fs.exists("/root/")
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
    return BetfairTestStubs.betfair_reader()(instrument_provider)


@pytest.fixture()
def betfair_reader():
    return BetfairTestStubs.betfair_reader()


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
    fs = DataCatalog.from_env().fs
    assert fs.isdir(f"{ROOT}/data/betting_instrument.parquet")


@pytest.fixture(scope="function")
def catalog():
    catalog = DataCatalog(path="/root", fs_protocol="memory")
    return catalog


@pytest.fixture(scope="function")
def loaded_catalog(catalog, load_data):
    return catalog
