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
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentsLinearResponse
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentsOptionResponse
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentsSpotResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BybitInstrumentsInfoGetParameters(msgspec.Struct, omit_defaults=True, frozen=False):
    category: BybitInstrumentType | None = None
    symbol: str | None = None
    status: str | None = None


class BybitInstrumentsInfoEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "instruments-info"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.MARKET,
            url_path=url_path,
        )
        self._response_decoder_instrument_linear = msgspec.json.Decoder(
            BybitInstrumentsLinearResponse,
        )
        self._response_decoder_instrument_spot = msgspec.json.Decoder(BybitInstrumentsSpotResponse)
        self._response_decoder_instrument_option = msgspec.json.Decoder(
            BybitInstrumentsOptionResponse,
        )

    async def get(
        self,
        parameters: BybitInstrumentsInfoGetParameters,
    ) -> BybitInstrumentsLinearResponse | (
        BybitInstrumentsSpotResponse | BybitInstrumentsOptionResponse
    ):
        method_type = HttpMethod.GET
        raw = await self._method(method_type, parameters)
        if parameters.category == BybitInstrumentType.LINEAR:
            return self._response_decoder_instrument_linear.decode(raw)
        elif parameters.category == BybitInstrumentType.SPOT:
            return self._response_decoder_instrument_spot.decode(raw)
        elif parameters.category == BybitInstrumentType.OPTION:
            return self._response_decoder_instrument_option.decode(raw)
        else:
            raise ValueError("Invalid account type")
