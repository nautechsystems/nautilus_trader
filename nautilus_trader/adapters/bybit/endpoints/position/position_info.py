import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.position import BybitPositionResponseStruct
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class BybitPositionInfoHttp(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BybitEndpointType.USER_DATA,
        }
        url_path = base_endpoint + "position/list"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BybitPositionResponseStruct)

    class GetParameters(msgspec.Struct, omit_defaults=False, frozen=False):
        category: str = None
        symbol: str = None
        settleCoin: str = "USDT"

    async def _get(self, parameters: GetParameters) -> BybitPositionResponseStruct:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, parameters)
        return self._get_resp_decoder.decode(raw)
