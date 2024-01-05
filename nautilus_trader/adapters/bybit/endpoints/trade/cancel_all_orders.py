# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.order import BybitCancelAllOrdersResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BybitCancelAllOrdersPostParameters(msgspec.Struct, omit_defaults=True, frozen=False):
    category: str
    symbol: str | None = None
    baseCoin: str | None = None
    settleCoin: str | None = None


class BybitCancelAllOrdersEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "order/cancel-all"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(BybitCancelAllOrdersResponse)

    async def post(
        self,
        parameters: BybitCancelAllOrdersPostParameters,
    ) -> BybitCancelAllOrdersResponse:
        method_type = HttpMethod.POST
        raw = await self._method(method_type, parameters)
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            decoded_raw = raw.decode("utf-8")  # Decoding the bytes object
            raise RuntimeError(
                f"Failed to decode response cancel all orders response: {decoded_raw}",
            ) from e
