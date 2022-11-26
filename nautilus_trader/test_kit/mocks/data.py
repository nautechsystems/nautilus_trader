# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import os
from collections.abc import Generator
from functools import partial

import fsspec
import pandas as pd
from fsspec.implementations.memory import MemoryFileSystem

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.base import clear_singleton_instances
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.persistence.external.readers import Reader
from nautilus_trader.trading.filters import NewsEvent


class MockReader(Reader):
    def parse(self, block: bytes) -> Generator:
        yield block


class NewsEventData(NewsEvent):
    """Generic data NewsEvent, needs to be defined here due to `inspect.is_nautilus_class`"""

    pass


def data_catalog_setup():
    """
    Reset the filesystem and ParquetDataCatalog to a clean state
    """
    clear_singleton_instances(ParquetDataCatalog)
    fs = fsspec.filesystem("memory")
    path = "/.nautilus/"
    if not fs.exists(path):
        fs.mkdir(path)
    os.environ["NAUTILUS_PATH"] = f"memory://{path}"
    catalog = ParquetDataCatalog.from_env()
    assert isinstance(catalog.fs, MemoryFileSystem)
    try:
        catalog.fs.rm("/", recursive=True)
    except FileNotFoundError:
        pass
    catalog.fs.mkdir("/.nautilus/catalog/data")
    assert catalog.fs.exists("/.nautilus/catalog/")
    assert not catalog.fs.glob("/.nautilus/catalog/**/*")
    return catalog


def aud_usd_data_loader():
    from nautilus_trader.backtest.data.providers import TestInstrumentProvider
    from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
    from tests.unit_tests.backtest.test_backtest_config import TEST_DATA_DIR

    venue = Venue("SIM")
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=venue)

    def parse_csv_tick(df, instrument_id):
        yield instrument
        for r in df.values:
            ts = secs_to_nanos(pd.Timestamp(r[0]).timestamp())
            tick = QuoteTick(
                instrument_id=instrument_id,
                bid=Price(r[1], 5),
                ask=Price(r[2], 5),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=ts,
                ts_init=ts,
            )
            yield tick

    clock = TestClock()
    logger = Logger(clock)
    catalog = ParquetDataCatalog.from_env()
    instrument_provider = InstrumentProvider(
        venue=venue,
        logger=logger,
    )
    instrument_provider.add(instrument)
    process_files(
        glob_path=f"{TEST_DATA_DIR}/truefx-audusd-ticks.csv",
        reader=CSVReader(
            block_parser=partial(parse_csv_tick, instrument_id=TestIdStubs.audusd_id()),
            as_dataframe=True,
        ),
        instrument_provider=instrument_provider,
        catalog=catalog,
    )
