import msgspec
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType, BybitEndpointType, BybitKlineInterval
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.market.kline import BybitKlinesResponse


class BybitKlinesGetParameters(msgspec.Struct,omit_defaults=True,frozen=False):
    category: str
    symbol: str
    interval: BybitKlineInterval
    start: int = None
    end: int = None
    limit: int = None


class BybitKlinesEndpoint(BybitHttpEndpoint):

    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ):
        url_path = base_endpoint + "kline"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.MARKET,
            url_path=url_path,
        )
        self._response_decoder = msgspec.json.Decoder(BybitKlinesResponse)


    async def get(
        self, parameters: BybitKlinesGetParameters
    ) -> BybitKlinesResponse:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, parameters)
        return self._response_decoder.decode(raw)