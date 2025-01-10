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

from typing import TYPE_CHECKING

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersLinearResponse
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersOptionResponse
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersResponse
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersSpotResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient


class BybitTickersGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    category: BybitProductType | None = None
    symbol: str | None = None
    baseCoin: str | None = None


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

    async def get(self, params: BybitTickersGetParams) -> BybitTickersResponse:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        try:
            if params.category == BybitProductType.SPOT:
                return self._response_decoder_spot.decode(raw)
            elif params.category in (BybitProductType.LINEAR, BybitProductType.INVERSE):
                return self._response_decoder_linear.decode(raw)
            elif params.category == BybitProductType.OPTION:
                return self._response_decoder_option.decode(raw)
            else:
                raise RuntimeError(
                    f"Unsupported product type: {params.category}",
                )
        except Exception as e:
            decoder_raw = raw.decode("utf-8")
            raise RuntimeError(
                f"Failed to decode Bybit tickers response: {decoder_raw}",
            ) from e
