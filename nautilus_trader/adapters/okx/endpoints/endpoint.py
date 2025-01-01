# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

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
