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

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.common.enums import BybitKlineInterval

# fmt: off
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoGetParameters

# fmt: on
from nautilus_trader.adapters.bybit.endpoints.market.klines import BybitKlinesEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.klines import BybitKlinesGetParameters
from nautilus_trader.adapters.bybit.endpoints.market.server_time import BybitServerTimeEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.tickers import BybitTickersEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.tickers import BybitTickersGetParameters
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrument
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentList
from nautilus_trader.adapters.bybit.schemas.market.kline import BybitKline
from nautilus_trader.adapters.bybit.schemas.market.server_time import BybitServerTime
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickerList
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.utils import get_category_from_instrument_type
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType


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

        # endpoints
        self._endpoint_instruments = BybitInstrumentsInfoEndpoint(
            client,
            self.base_endpoint,
        )
        self._endpoint_server_time = BybitServerTimeEndpoint(client, self.base_endpoint)
        self._endpoint_klines = BybitKlinesEndpoint(client, self.base_endpoint)
        self._endpoint_tickers = BybitTickersEndpoint(client, self.base_endpoint)

    def _get_url(self, url: str) -> str:
        return self.base_endpoint + url

    async def fetch_tickers(
        self,
        instrument_type: BybitInstrumentType,
        symbol: str | None = None,
        base_coin: str | None = None,
    ) -> BybitTickerList:
        response = await self._endpoint_tickers.get(
            BybitTickersGetParameters(
                category=instrument_type,
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
        instrument_type: BybitInstrumentType,
    ) -> BybitInstrumentList:
        response = await self._endpoint_instruments.get(
            BybitInstrumentsInfoGetParameters(
                category=instrument_type,
            ),
        )
        return response.result.list

    async def fetch_instrument(
        self,
        instrument_type: BybitInstrumentType,
        symbol: str,
    ) -> BybitInstrument:
        response = await self._endpoint_instruments.get(
            BybitInstrumentsInfoGetParameters(
                category=instrument_type,
                symbol=symbol,
            ),
        )
        return response.result.list[0]

    async def fetch_klines(
        self,
        instrument_type: BybitInstrumentType,
        symbol: str,
        interval: BybitKlineInterval,
        limit: int | None = None,
        start: int | None = None,
        end: int | None = None,
    ) -> list[BybitKline]:
        response = await self._endpoint_klines.get(
            parameters=BybitKlinesGetParameters(
                category=get_category_from_instrument_type(instrument_type),
                symbol=symbol,
                interval=interval,
                limit=limit,
                start=start,
                end=end,
            ),
        )
        return response.result.list

    async def request_bybit_bars(
        self,
        instrument_type: BybitInstrumentType,
        bar_type: BarType,
        interval: BybitKlineInterval,
        ts_init: int,
        limit: int = 100,
        start: int | None = None,
        end: int | None = None,
    ) -> list[Bar]:
        all_bars = []
        while True:
            bybit_symbol: BybitSymbol = BybitSymbol(bar_type.instrument_id.symbol.value)
            klines = await self.fetch_klines(
                symbol=bybit_symbol,
                instrument_type=instrument_type,
                interval=interval,
                limit=limit,
                start=start,
                end=end,
            )
            bars: list[Bar] = [kline.parse_to_bar(bar_type, ts_init) for kline in klines]
            all_bars.extend(bars)
            if klines:
                next_start_time = int(klines[-1].startTime) + 1
            else:
                break
            if end is None or ((limit and len(klines) < limit) or next_start_time > end):
                break
            start = next_start_time
        return all_bars

    # async def get_risk_limits(self):
    #     params = {"category": "linear"}
    #     try:
    #         raw: bytes = await self.client.send_request(
    #             http_method=HttpMethod.GET,
    #             url_path=self._get_url("risk-limit"),
    #             payload=params,
    #         )
    #         decoded = self._decoder_risk_limit.decode(raw)
    #         return decoded.result.list
    #     except Exception as e:
    #         print(e)
