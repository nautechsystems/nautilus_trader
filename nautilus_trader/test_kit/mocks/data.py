# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import Clock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument
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

    def request_instrument(self, request: RequestInstrument) -> None:
        self._handle_instrument(self.instrument, request.id, request.params)

    def request_instruments(self, request: RequestInstruments) -> None:
        self._handle_instruments(request.venue, self.instruments, request.id, request.params)

    def request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._handle_quote_ticks(
            request.instrument_id,
            self.quote_ticks,
            request.id,
            request.params,
        )

    def request_trade_ticks(self, request: RequestTradeTicks) -> None:
        self._handle_trade_ticks(
            request.instrument_id,
            self.trade_ticks,
            request.id,
            request.params,
        )

    def request_bars(self, request: RequestBars) -> None:
        self._handle_bars(request.bar_type, self.bars, None, request.id, request.params)


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
