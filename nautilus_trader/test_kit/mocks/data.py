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

from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.catalog.singleton import clear_singleton_instances
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading.filters import NewsEvent


_AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
_ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class NewsEventData(NewsEvent):
    """
    Represents news event custom data.
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

    path = Path.cwd() / "catalog" if path is None else path.resolve()

    catalog = ParquetDataCatalog(path=path.as_posix(), fs_protocol=protocol)

    if catalog.fs.exists(catalog.path):
        catalog.fs.rm(catalog.path, recursive=True)

    catalog.fs.mkdir(catalog.path, create_parents=True)

    assert catalog.fs.isdir(catalog.path)
    assert not [fn for fn in catalog.fs.glob(f"{catalog.path}/**") if catalog.fs.isfile(fn)]

    return catalog


def load_catalog_with_stub_quote_ticks_audusd(catalog: ParquetDataCatalog) -> None:
    wrangler = QuoteTickDataWrangler(_AUDUSD_SIM)
    ticks = wrangler.process(TestDataProvider().read_csv_ticks("truefx/audusd-ticks.csv"))
    ticks.sort(key=lambda x: x.ts_init)  # CAUTION: data was not originally sorted
    catalog.write_data([_AUDUSD_SIM])
    catalog.write_data(ticks)


def load_catalog_with_stub_trade_ticks_ethusdt(catalog: ParquetDataCatalog) -> None:
    wrangler = TradeTickDataWrangler(_ETHUSDT_BINANCE)
    ticks = wrangler.process(TestDataProvider().read_csv_ticks("binance/ethusdt-trades.csv"))
    # ticks.sort(key=lambda x: x.ts_init)
    catalog.write_data([_ETHUSDT_BINANCE])
    catalog.write_data(ticks)
