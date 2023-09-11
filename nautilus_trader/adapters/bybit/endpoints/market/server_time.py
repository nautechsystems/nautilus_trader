import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.market.server_time import BybitServerTimeResponseStruct
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
        self._get_resp_decoder = msgspec.json.Decoder(BybitServerTimeResponseStruct)

    async def _get(self) -> BybitServerTimeResponseStruct:
        method_type = HttpMethod.GET
        raw = await self._method(method_type)
        return self._get_resp_decoder.decode(raw)
