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

from __future__ import annotations

from typing import TYPE_CHECKING

from nautilus_trader.adapters.bybit.common.enums import BybitKlineInterval
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.symbol import BybitSymbol

# fmt: off
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoGetParams

# fmt: on
from nautilus_trader.adapters.bybit.endpoints.market.klines import BybitKlinesEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.klines import BybitKlinesGetParams
from nautilus_trader.adapters.bybit.endpoints.market.server_time import BybitServerTimeEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.tickers import BybitTickersEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.tickers import BybitTickersGetParams
from nautilus_trader.adapters.bybit.endpoints.market.trades import BybitTradesEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.trades import BybitTradesGetParams
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrument
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentInverse
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentLinear
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentList
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentOption
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentSpot
from nautilus_trader.core.correctness import PyCondition


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
    from nautilus_trader.adapters.bybit.schemas.market.kline import BybitKline
    from nautilus_trader.adapters.bybit.schemas.market.server_time import BybitServerTime
    from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickerList
    from nautilus_trader.adapters.bybit.schemas.market.trades import BybitTrade
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.model.data import Bar
    from nautilus_trader.model.data import BarType
    from nautilus_trader.model.data import TradeTick
    from nautilus_trader.model.identifiers import InstrumentId


class BybitMarketHttpAPI:
    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/v5/market/"

        self._endpoint_instruments = BybitInstrumentsInfoEndpoint(client, self.base_endpoint)
        self._endpoint_server_time = BybitServerTimeEndpoint(client, self.base_endpoint)
        self._endpoint_klines = BybitKlinesEndpoint(client, self.base_endpoint)
        self._endpoint_tickers = BybitTickersEndpoint(client, self.base_endpoint)
        self._endpoint_trades = BybitTradesEndpoint(client, self.base_endpoint)

    def _get_url(self, url: str) -> str:
        return self.base_endpoint + url

    async def fetch_tickers(
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
        base_coin: str | None = None,
    ) -> BybitTickerList:
        response = await self._endpoint_tickers.get(
            BybitTickersGetParams(
                category=product_type,
                symbol=symbol,
                baseCoin=base_coin,
            ),
        )
        return response.result.list

    async def fetch_server_time(self) -> BybitServerTime:
        response = await self._endpoint_server_time.get()
        return response.result

    async def fetch_instruments(
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
        status: str | None = None,
        base_coin: str | None = None,
        limit: int | None = None,
        cursor: str | None = None,
    ) -> BybitInstrumentList:
        response = await self._endpoint_instruments.get(
            BybitInstrumentsInfoGetParams(
                category=product_type,
                symbol=symbol,
                status=status,
                baseCoin=base_coin,
                limit=limit,
                cursor=cursor,
            ),
        )
        return response.result.list

    async def fetch_all_instruments(
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
        status: str | None = None,
        base_coin: str | None = None,
    ) -> BybitInstrumentList:
        """
        Fetch all instruments with pagination from Bybit.
        """
        all_instruments: list[BybitInstrument] = []
        current_cursor = None

        while True:
            response = await self._endpoint_instruments.get(
                BybitInstrumentsInfoGetParams(
                    category=product_type,
                    symbol=symbol,
                    status=status,
                    baseCoin=base_coin,
                    limit=1000,
                    cursor=current_cursor,
                ),
            )
            all_instruments.extend(response.result.list)
            current_cursor = response.result.nextPageCursor

            if not current_cursor or current_cursor == "":
                break

        if product_type == BybitProductType.SPOT:
            return [x for x in all_instruments if isinstance(x, BybitInstrumentSpot)]
        elif product_type == BybitProductType.LINEAR:
            return [x for x in all_instruments if isinstance(x, BybitInstrumentLinear)]
        elif product_type == BybitProductType.INVERSE:
            return [x for x in all_instruments if isinstance(x, BybitInstrumentInverse)]
        elif product_type == BybitProductType.OPTION:
            return [x for x in all_instruments if isinstance(x, BybitInstrumentOption)]
        else:
            raise ValueError(f"Unsupported product type: {product_type}")

    async def fetch_instrument(
        self,
        product_type: BybitProductType,
        symbol: str,
    ) -> BybitInstrument:
        response = await self._endpoint_instruments.get(
            BybitInstrumentsInfoGetParams(
                category=product_type,
                symbol=symbol,
            ),
        )
        return response.result.list[0]

    async def fetch_klines(
        self,
        product_type: BybitProductType,
        symbol: str,
        interval: BybitKlineInterval,
        limit: int | None = None,
        start: int | None = None,
        end: int | None = None,
    ) -> list[BybitKline]:
        response = await self._endpoint_klines.get(
            params=BybitKlinesGetParams(
                category=product_type.value,
                symbol=symbol,
                interval=interval,
                limit=limit,
                start=start,
                end=end,
            ),
        )
        return response.result.list

    async def fetch_public_trades(
        self,
        product_type: BybitProductType,
        symbol: str,
        limit: int | None = None,
    ) -> list[BybitTrade]:
        response = await self._endpoint_trades.get(
            params=BybitTradesGetParams(
                category=product_type.value,
                symbol=symbol,
                limit=limit,
            ),
        )
        return response.result.list

    async def request_bybit_trades(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
        limit: int = 1000,
    ) -> list[Bar]:
        bybit_symbol = BybitSymbol(instrument_id.symbol.value)
        trades = await self.fetch_public_trades(
            symbol=bybit_symbol.raw_symbol,
            product_type=bybit_symbol.product_type,
            limit=limit,
        )
        trade_ticks: list[TradeTick] = [t.parse_to_trade(instrument_id, ts_init) for t in trades]
        return trade_ticks

    async def request_bybit_bars(
        self,
        bar_type: BarType,
        interval: BybitKlineInterval,
        ts_init: int,
        timestamp_on_close: bool,
        limit: int | None = None,
        start: int | None = None,
        end: int | None = None,
    ) -> list[Bar]:
        bybit_symbol = BybitSymbol(bar_type.instrument_id.symbol.value)

        all_bars: list[Bar] = []
        prev_start: int | None = None
        seen_timestamps: set[int] = set()

        while True:
            if prev_start is not None and prev_start == start:
                break
            prev_start = start

            klines = await self.fetch_klines(
                symbol=bybit_symbol.raw_symbol,
                product_type=bybit_symbol.product_type,
                interval=interval,
                limit=1000,  # Limit for data size per page (maximum for the Bybit API)
                start=start,
                end=end,
            )

            if not klines:
                break

            klines.sort(key=lambda k: int(k.startTime))
            new_bars = [
                kline.parse_to_bar(bar_type, ts_init, timestamp_on_close)
                for kline in klines
                if int(kline.startTime) not in seen_timestamps
            ]

            all_bars.extend(new_bars)
            seen_timestamps.update(int(kline.startTime) for kline in klines)

            start = int(klines[-1].startTime) + 1

            if end is not None and start > end:
                break

        if limit is not None and len(all_bars) > limit:
            return all_bars[-limit:]

        return all_bars
