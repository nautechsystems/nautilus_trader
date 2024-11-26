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

from datetime import datetime
from pathlib import Path
from typing import Literal

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import Clock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.catalog.singleton import clear_singleton_instances
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading.filters import NewsEvent


class MockMarketDataClient(MarketDataClient):
    """
    Provides an implementation of `MarketDataClient` for testing.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.

    """

    def __init__(
        self,
        client_id: ClientId,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
    ):
        super().__init__(
            client_id=client_id,
            venue=Venue(str(client_id)),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        self._set_connected()

        self.instrument: Instrument | None = None
        self.instruments: list[Instrument] = []
        self.quote_ticks: list[QuoteTick] = []
        self.trade_ticks: list[TradeTick] = []
        self.bars: list[Bar] = []

    def request_instrument(
        self,
        instrument_id: InstrumentId,
        correlation_id: UUID4,
        start: datetime | None = None,
        end: datetime | None = None,
        metadata: dict | None = None,
    ) -> None:
        self._handle_instrument(self.instrument, correlation_id, metadata)

    def request_instruments(
        self,
        venue: Venue,
        correlation_id: UUID4,
        start: datetime | None = None,
        end: datetime | None = None,
        metadata: dict | None = None,
    ) -> None:
        self._handle_instruments(venue, self.instruments, correlation_id, metadata)

    def request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: datetime | None = None,
        end: datetime | None = None,
        metadata: dict | None = None,
    ) -> None:
        self._handle_quote_ticks(instrument_id, self.quote_ticks, correlation_id, metadata)

    def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: datetime | None = None,
        end: datetime | None = None,
        metadata: dict | None = None,
    ) -> None:
        self._handle_trade_ticks(instrument_id, self.trade_ticks, correlation_id, metadata)

    def request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: datetime | None = None,
        end: datetime | None = None,
        metadata: dict | None = None,
    ) -> None:
        self._handle_bars(bar_type, self.bars, None, correlation_id, metadata)


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
