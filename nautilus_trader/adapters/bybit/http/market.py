from typing import Optional

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.common.enums import BybitKlineInterval
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoGetParameters
from nautilus_trader.adapters.bybit.endpoints.market.klines import BybitKlinesEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.klines import BybitKlinesGetParameters
from nautilus_trader.adapters.bybit.endpoints.market.server_time import BybitServerTimeEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrument
from nautilus_trader.adapters.bybit.schemas.market.kline import BybitKline
from nautilus_trader.adapters.bybit.schemas.market.server_time import BybitServerTime
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.utils import get_category_from_instrument_type
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType


class BybitMarketHttpAPI:
    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
        instrument_type: BybitInstrumentType,
    ):
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/v5/market/"
        self.instrument_type = instrument_type

        # endpoints
        self._endpoint_instruments = BybitInstrumentsInfoEndpoint(
            client,
            self.base_endpoint,
            instrument_type,
        )
        self._endpoint_server_time = BybitServerTimeEndpoint(client, self.base_endpoint)
        self._endpoint_klines = BybitKlinesEndpoint(client, self.base_endpoint)

    def _get_url(self, url: str):
        return self.base_endpoint + url

    async def fetch_server_time(self) -> BybitServerTime:
        response = await self._endpoint_server_time.get()
        return response.result

    async def fetch_instruments(self) -> list[BybitInstrument]:
        response = await self._endpoint_instruments.get(
            BybitInstrumentsInfoGetParameters(
                category=get_category_from_instrument_type(self.instrument_type),
            ),
        )
        return response.result.list

    async def fetch_klines(
        self,
        symbol: str,
        interval: BybitKlineInterval,
        limit: Optional[int] = None,
        start: Optional[int] = None,
        end: Optional[int] = None,
    ):
        response = await self._endpoint_klines.get(
            parameters=BybitKlinesGetParameters(
                category=get_category_from_instrument_type(self.instrument_type),
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
        bar_type: BarType,
        interval: BybitKlineInterval,
        ts_init: int,
        limit: Optional[int] = 100,
        start: Optional[int] = None,
        end: Optional[int] = None,
    ):
        all_bars: list[BybitKline] = []
        while True:
            klines = await self.fetch_klines(
                symbol=BybitSymbol(bar_type.instrument_id.symbol.value),
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

    async def get_risk_limits(self):
        params = {"category": "linear"}
        try:
            raw: bytes = await self.client.send_request(
                http_method=HttpMethod.GET,
                url_path=self._get_url("risk-limit"),
                payload=params,
            )
            decoded = self._decoder_risk_limit.decode(raw)
            return decoded.result.list
        except Exception as e:
            print(e)
