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

import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEndpointType
from nautilus_trader.adapters.okx.common.enums import OKXMarginMode
from nautilus_trader.adapters.okx.common.enums import OKXPositionSide
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.trade import OKXClosePositionResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXClosePositionPostParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instId: str
    mgnMode: OKXMarginMode
    posSide: OKXPositionSide = OKXPositionSide.NET
    ccy: str | None = None
    autoCxl: bool = False  # cancel pending orders for this instrument else error if pendings exist
    clOrdId: str | None = None
    tag: str | None = None

    def validate(self) -> None:
        pass


class OKXClosePositionEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/close-position"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXClosePositionResponse)

    async def post(self, params: OKXClosePositionPostParams) -> OKXClosePositionResponse:
        # Validate
        params.validate()

        method_type = HttpMethod.POST
        raw = await self._method(method_type, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
