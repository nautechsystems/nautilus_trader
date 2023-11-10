import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.market.server_time import BybitServerTimeResponse
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class BybitServerTimeEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ):
        url_path = base_endpoint + "time"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.MARKET,
            url_path=url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BybitServerTimeResponse)

    async def get(self) -> BybitServerTimeResponse:
        method_type = HttpMethod.GET
        raw = await self._method(method_type)
        try:
            return self._get_resp_decoder.decode(raw)
        except Exception:
            raise RuntimeError(f"Failed to decode response server time response: {raw}")
