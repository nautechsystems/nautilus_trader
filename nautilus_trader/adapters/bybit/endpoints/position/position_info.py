from typing import Optional

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.position import BybitPositionResponseStruct
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class PositionInfoGetParameters(msgspec.Struct, omit_defaults=True, frozen=False):
    category: str = None
    symbol: Optional[BybitSymbol] = None
    settleCoin: str = None


class BybitPositionInfoEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ):
        url_path = base_endpoint + "position/list"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.ACCOUNT,
            url_path=url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BybitPositionResponseStruct)

    async def get(self, parameters: PositionInfoGetParameters) -> BybitPositionResponseStruct:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, parameters)
        try:
            return self._get_resp_decoder.decode(raw)
        except Exception:
            raise RuntimeError(f"Failed to decode response position info response: {raw}")
