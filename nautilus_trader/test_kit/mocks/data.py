# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from collections.abc import Generator
from functools import partial
from pathlib import Path

import pandas as pd

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.persistence.external.readers import Reader
from nautilus_trader.persistence.external.util import clear_singleton_instances
from nautilus_trader.trading.filters import NewsEvent


class MockReader(Reader):
    def parse(self, block: bytes) -> Generator:
        yield block


class NewsEventData(NewsEvent):
    """
    Generic data NewsEvent.
    """


def data_catalog_setup(protocol, path=None) -> ParquetDataCatalog:
    if protocol not in ("memory", "file"):
        raise ValueError("`protocol` should only be one of `memory` or `file` for testing")

    clear_singleton_instances(ParquetDataCatalog)

    path = Path.cwd() / "data_catalog" if path is None else Path(path).resolve()

    catalog = ParquetDataCatalog(path=path.as_posix(), fs_protocol=protocol)

    if catalog.fs.exists(catalog.path):
        catalog.fs.rm(catalog.path, recursive=True)

    catalog.fs.mkdir(catalog.path, create_parents=True)

    assert catalog.fs.isdir(catalog.path)
    assert not catalog.fs.glob(f"{catalog.path}/**")

    return catalog


def aud_usd_data_loader(catalog: ParquetDataCatalog):
    from nautilus_trader.test_kit.providers import TestInstrumentProvider
    from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
    from tests.unit_tests.backtest.test_config import TEST_DATA_DIR

    venue = Venue("SIM")
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=venue)

    def parse_csv_tick(df, instrument_id):
        yield instrument
        for r in df.to_numpy():
            ts = pd.Timestamp(r[0], tz="UTC").value
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
