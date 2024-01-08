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
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersLinearResponse
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersOptionResponse
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersResponse
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersSpotResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BybitTickersGetParameters(msgspec.Struct, omit_defaults=True, frozen=False):
    category: BybitInstrumentType = None
    symbol: str = None
    baseCoin: str = None


class BybitTickersEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ):
        url_path = base_endpoint + "tickers"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.MARKET,
            url_path=url_path,
        )
        self._response_decoder_linear = msgspec.json.Decoder(BybitTickersLinearResponse)
        self._response_decoder_option = msgspec.json.Decoder(BybitTickersOptionResponse)
        self._response_decoder_spot = msgspec.json.Decoder(BybitTickersSpotResponse)

    async def get(self, params: BybitTickersGetParameters) -> BybitTickersResponse:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        try:
            if params.category == BybitInstrumentType.LINEAR:
                return self._response_decoder_linear.decode(raw)
            elif params.category == BybitInstrumentType.OPTION:
                return self._response_decoder_option.decode(raw)
            elif params.category == BybitInstrumentType.SPOT:
                return self._response_decoder_spot.decode(raw)
            else:
                raise RuntimeError(
                    f"Unsupported instrument type: {params.category}",
                )
        except Exception as e:
            decoder_raw = raw.decode("utf-8")
            raise RuntimeError(
                f"Failed to decode Bybit tickers response: {decoder_raw}",
            ) from e
