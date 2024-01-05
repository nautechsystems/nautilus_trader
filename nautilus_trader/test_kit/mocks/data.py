# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from pathlib import Path
from typing import Literal

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.catalog.singleton import clear_singleton_instances
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading.filters import NewsEvent


_AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class NewsEventData(NewsEvent):
    """
    Generic data NewsEvent.
    """


def setup_catalog(
    protocol: Literal["memory", "file"],
    path: Path | str | None = None,
) -> ParquetDataCatalog:
    if protocol not in ("memory", "file"):
        raise ValueError("`protocol` should only be one of `memory` or `file` for testing")
    if isinstance(path, str):
        path = Path(path)

    clear_singleton_instances(ParquetDataCatalog)

    path = Path.cwd() / "data_catalog" if path is None else path.resolve()

    catalog = ParquetDataCatalog(path=path.as_posix(), fs_protocol=protocol)

    if catalog.fs.exists(catalog.path):
        catalog.fs.rm(catalog.path, recursive=True)

    catalog.fs.mkdir(catalog.path, create_parents=True)

    assert catalog.fs.isdir(catalog.path)
    assert not catalog.fs.glob(f"{catalog.path}/**")

    return catalog


def load_catalog_with_stub_quote_ticks_audusd(catalog: ParquetDataCatalog) -> None:
    clock = TestClock()
    logger = Logger(clock)

    instrument_provider = InstrumentProvider(
        logger=logger,
    )
    instrument_provider.add(_AUDUSD_SIM)

    wrangler = QuoteTickDataWrangler(_AUDUSD_SIM)
    ticks = wrangler.process(TestDataProvider().read_csv_ticks("truefx/audusd-ticks.csv"))
    ticks.sort(key=lambda x: x.ts_init)  # CAUTION: data was not originally sorted
    catalog.write_data([_AUDUSD_SIM])
    catalog.write_data(ticks)
