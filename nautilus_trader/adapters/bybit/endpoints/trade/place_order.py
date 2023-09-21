from typing import Optional

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType, BybitOrderSide, BybitOrderType, \
    BybitTriggerType, BybitTimeInForce, BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.order import BybitPlaceOrderResponse
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class PlaceOrderGetParameters(msgspec.Struct,omit_defaults=True,frozen=False):
    category: BybitInstrumentType
    symbol: str
    side: BybitOrderSide
    qty: str
    orderType: Optional[BybitOrderType] = None
    price: Optional[str] = None
    trigger_direction: Optional[int] = None # TODO type this
    trigger_price: Optional[str] = None
    trigger_by: Optional[BybitTriggerType] = None
    timeInForce: Optional[BybitTimeInForce] = None
    orderLinkId: Optional[str] = None


class BybitPlaceOrderEndpoint(BybitHttpEndpoint):

    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ):
        url_path = base_endpoint + "order/create"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(BybitPlaceOrderResponse)

    async def post(self, parameters: PlaceOrderGetParameters) -> BybitPlaceOrderResponse:
        method_type = HttpMethod.POST
        raw = await self._method(method_type, parameters)
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(f"Failed to decode response place order response: {raw}")

