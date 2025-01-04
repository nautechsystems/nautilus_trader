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

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.common.symbol import BybitSymbol


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient


def enc_hook(obj: Any) -> Any:
    if isinstance(obj, BybitSymbol):
        return str(obj)
    else:
        raise TypeError(f"Objects of type {type(obj)} are not supported")


class BybitHttpEndpoint:
    def __init__(
        self,
        client: BybitHttpClient,
        endpoint_type: BybitEndpointType,
        url_path: str,
    ) -> None:
        self.client = client
        self.endpoint_type = endpoint_type
        self.url_path = url_path

        self.decoder = msgspec.json.Decoder()
        self.encoder = msgspec.json.Encoder(enc_hook=enc_hook)

        self._method_request: dict[BybitEndpointType, Any] = {
            BybitEndpointType.NONE: self.client.send_request,
            BybitEndpointType.MARKET: self.client.send_request,
            BybitEndpointType.ASSET: self.client.sign_request,
            BybitEndpointType.ACCOUNT: self.client.sign_request,
            BybitEndpointType.TRADE: self.client.sign_request,
            BybitEndpointType.POSITION: self.client.sign_request,
            BybitEndpointType.USER: self.client.sign_request,
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
