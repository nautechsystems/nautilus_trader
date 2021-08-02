import datetime
import os
import pathlib
import sys
from decimal import Decimal
from functools import partial

import fsspec.implementations.memory
import orjson
import pandas as pd
import pyarrow.dataset as ds
import pyarrow.parquet as pq
import pytest
from numpy import dtype
from pandas import CategoricalDtype

from examples.strategies.orderbook_imbalance import OrderbookImbalance
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.data.wrangling import TradeTickDataWrangler
from nautilus_trader.model import currencies
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.data.base import Data
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.instruments.currency import CurrencySpot
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.serialization.arrow.serializer import register_parquet
from nautilus_trader.serialization.arrow.util import is_nautilus_class
from nautilus_trader.serialization.catalog.core import DataCatalog
from nautilus_trader.serialization.catalog.parsers import CSVParser
from nautilus_trader.serialization.catalog.parsers import ParquetParser
from nautilus_trader.serialization.catalog.parsers import TextParser
from nautilus_trader.serialization.catalog.scanner import scan
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


@pytest.mark.parametrize(
    "glob, num_files",
    [
        ("**.json", 3),
        ("**.txt", 1),
        ("**.parquet", 2),
        ("**.csv", 11),
    ],
)
def test_scan_paths(glob, num_files):
    files = scan(path=TEST_DATA_DIR, glob_pattern=glob)
    assert len(files) == num_files


def test_scan_chunks():
    # Total size 17338
    files = scan(path=TEST_DATA_DIR, glob_pattern="1.166564490.bz2", chunk_size=50000)
    raw = list(files[0].iter_chunks())
    assert len(raw) == 5


def test_scan_file_filter():
    files = scan(path=TEST_DATA_DIR, glob_pattern="*.csv")
    assert len(files) == 11

    files = scan(path=TEST_DATA_DIR, glob_pattern="*jpy*.csv")
    assert len(files) == 3
