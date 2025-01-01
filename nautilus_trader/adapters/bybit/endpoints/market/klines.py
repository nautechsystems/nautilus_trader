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
from nautilus_trader.adapters.bybit.common.enums import BybitKlineInterval
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.schemas.market.kline import BybitKlinesResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient


class BybitKlinesGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    category: str
    symbol: str
    interval: BybitKlineInterval
    start: int | None = None
    end: int | None = None
    limit: int | None = None


class BybitKlinesEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "kline"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.MARKET,
            url_path=url_path,
        )
        self._response_decoder = msgspec.json.Decoder(BybitKlinesResponse)

    async def get(
        self,
        params: BybitKlinesGetParams,
    ) -> BybitKlinesResponse:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._response_decoder.decode(raw)
