from typing import Any

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient


# from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class BybitHttpEndpoint:
    def __init__(
        self,
        client: BybitHttpClient,
        endpoint_type: BybitEndpointType,
        url_path: str,
    ):
        self.client = client
        self.endpoint_type = endpoint_type
        self.url_path = url_path

        self.decoder = msgspec.json.Decoder()
        self.encoder = msgspec.json.Encoder()

        self._method_request = {
            BybitEndpointType.NONE: self.client.send_request,
            BybitEndpointType.MARKET_DATA: self.client.send_request,
            BybitEndpointType.USER_DATA: self.client.sign_request,
            BybitEndpointType.ACCOUNT: self.client.sign_request,
        }

    async def _method(
        self,
        method_type: Any,
        parameters: Any,
        ratelimiter_keys: Any = None,
    ) -> bytes:
        payload: dict = self.decoder.decode(self.encoder.encode(parameters))
        # clear payload from nulls
        payload = {k: v for k, v in payload.items() if v is not None}
        if self.methods_desc[method_type] is None:
            raise RuntimeError(
                f"{method_type.name} not available for {self.url_path}",
            )
        raw: bytes = await self._method_request[self.endpoint_type](
            http_method=method_type,
            url_path=self.url_path,
            payload=payload,
            ratelimiter_keys=ratelimiter_keys,
        )
        return raw
