from typing import Optional

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.order import BybitOpenOrdersResponseStruct
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class BybitOpenOrdersGetParameters(msgspec.Struct, omit_defaults=True, frozen=False):
    category: str = None
    symbol: str = None
    baseCoin: Optional[str] = None
    settleCoin: Optional[str] = None
    orderId: Optional[str] = None
    orderLinkId: Optional[str] = None


class BybitOpenOrdersHttp(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ):
        url_path = base_endpoint + "order/realtime"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.TRADE,
            url_path=url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BybitOpenOrdersResponseStruct)

    async def get(self, parameters: BybitOpenOrdersGetParameters) -> BybitOpenOrdersResponseStruct:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, parameters)
        try:
            return self._get_resp_decoder.decode(raw)
        except Exception:
            raise RuntimeError(f"Failed to decode response open orders response: {raw}")
