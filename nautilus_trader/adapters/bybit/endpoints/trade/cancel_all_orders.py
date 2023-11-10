import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.order import BybitCancelAllOrdersResponse
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class BybitCancelAllOrdersPostParameters(msgspec.Struct, omit_defaults=True, frozen=False):
    category: BybitInstrumentType
    symbol: str = None
    baseCoin: str = None
    settleCoin: str = None


class BybitCancelAllOrdersEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ):
        url_path = base_endpoint + "order/cancel-all"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(BybitCancelAllOrdersResponse)

    async def post(
        self, parameters: BybitCancelAllOrdersPostParameters
    ) -> BybitCancelAllOrdersResponse:
        method_type = HttpMethod.POST
        raw = await self._method(method_type, parameters)
        try:
            return self._resp_decoder.decode(raw)
        except Exception:
            raise RuntimeError(f"Failed to decode response cancel all orders response: {raw}")
