from typing import Any

import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEndpointType
from nautilus_trader.adapters.okx.common.symbol import OKXSymbol
from nautilus_trader.adapters.okx.http.client import OKXHttpClient


def enc_hook(obj: Any) -> Any:
    if isinstance(obj, OKXSymbol):
        return str(obj)
    raise TypeError(f"Objects of type {type(obj)} are not supported")


class OKXHttpEndpoint:
    def __init__(
        self,
        client: OKXHttpClient,
        endpoint_type: OKXEndpointType,
        url_path: str,
    ) -> None:
        self.client = client
        self.endpoint_type = endpoint_type
        self.url_path = url_path

        self.decoder = msgspec.json.Decoder()
        self.encoder = msgspec.json.Encoder(enc_hook=enc_hook)

        self._method_request: dict[OKXEndpointType, Any] = {
            OKXEndpointType.NONE: self.client.send_request,
            OKXEndpointType.MARKET: self.client.send_request,
            OKXEndpointType.ASSET: self.client.sign_request,
            OKXEndpointType.ACCOUNT: self.client.sign_request,
            OKXEndpointType.TRADE: self.client.sign_request,
            OKXEndpointType.PUBLIC: self.client.send_request,
        }

    async def _method(
        self,
        method_type: Any,
        params: Any | None = None,
        ratelimiter_keys: Any | None = None,
    ) -> bytes:
        payload: dict = self.decoder.decode(self.encoder.encode(params))
        method_call = self._method_request[self.endpoint_type]
        raw: bytes = await method_call(
            http_method=method_type,
            url_path=self.url_path,
            payload=payload,
            ratelimiter_keys=ratelimiter_keys,
        )
        return raw
