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
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXTradeMode
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.public.position_tiers import OKXPositionTiersResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXPositionTiersGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instType: OKXInstrumentType
    tdMode: OKXTradeMode
    uly: str | None = None
    instFamily: str | None = None
    instId: str | None = None
    ccy: str | None = None
    tier: str | None = None

    def validate(self) -> None:
        if self.instType in [
            OKXInstrumentType.SWAP,
            OKXInstrumentType.FUTURES,
            OKXInstrumentType.OPTION,
        ]:
            assert (
                self.uly or self.instFamily
            ), "`uly` or `instFamily` is required for SWAP, FUTURES, and OPTION type instruments"

        assert self.tdMode in [
            OKXTradeMode.CROSS,
            OKXTradeMode.ISOLATED,
        ], "the position tiers endpoint is for 'cross' and 'isolated' trade modes only"


class OKXPositionTiersEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/position-tiers"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.PUBLIC,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXPositionTiersResponse)

    async def get(self, params: OKXPositionTiersGetParams) -> OKXPositionTiersResponse:
        # Validate
        params.validate()

        raw = await self._method(HttpMethod.GET, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
