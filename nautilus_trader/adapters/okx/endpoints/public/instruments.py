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
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrumentsSpotResponse
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrumentsSwapResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXInstrumentsGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instType: OKXInstrumentType
    uly: str | None = None
    instFamily: str | None = None
    instId: str | None = None

    def validate(self) -> None:
        if self.instType == OKXInstrumentType.OPTION:
            assert (
                self.uly or self.instFamily
            ), "`uly` or `instFamily` is required for OPTION type instruments"


class OKXInstrumentsEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/instruments"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.PUBLIC,
            url_path=url_path,
        )
        self._resp_decoder_spot = msgspec.json.Decoder(OKXInstrumentsSpotResponse)
        self._resp_decoder_swap = msgspec.json.Decoder(OKXInstrumentsSwapResponse)

    async def get(
        self,
        params: OKXInstrumentsGetParams,
    ) -> OKXInstrumentsSpotResponse | OKXInstrumentsSwapResponse:
        # Validate
        params.validate()

        raw = await self._method(HttpMethod.GET, params)  # , ratelimiter_keys=[self.url_path])
        try:
            if params.instType in [OKXInstrumentType.SPOT, OKXInstrumentType.MARGIN]:
                return self._resp_decoder_spot.decode(raw)
            elif params.instType == OKXInstrumentType.SWAP:
                return self._resp_decoder_swap.decode(raw)
            else:
                raise ValueError(
                    f"Invalid (or not implemented) instrument type, was {params.instType}",
                )
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
