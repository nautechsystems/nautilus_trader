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

from nautilus_trader.adapters.bybit.endpoints.user.query_api import BybitQueryApiEndpoint
from nautilus_trader.adapters.bybit.endpoints.user.update_sub_api import BybitUpdateSubApiEndpoint
from nautilus_trader.adapters.bybit.endpoints.user.update_sub_api import BybitUpdateSubApiPostParams
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.core.correctness import PyCondition


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
    from nautilus_trader.adapters.bybit.schemas.user.query_api import BybitApiInfo
    from nautilus_trader.adapters.bybit.schemas.user.update_sub_api import BybitUpdateSubApiResult
    from nautilus_trader.common.component import LiveClock


class BybitUserHttpAPI:
    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/v5"

        self._endpoint_query_api = BybitQueryApiEndpoint(client, self.base_endpoint)
        self._endpoint_update_sub_api = BybitUpdateSubApiEndpoint(client, self.base_endpoint)

    async def query_api(self) -> BybitApiInfo:
        response = await self._endpoint_query_api.get()
        return response.result

    async def update_sub_api(
        self,
        api_key: str | None = None,
        read_only: int = 0,
        ips: str | None = None,
    ) -> BybitUpdateSubApiResult:
        response = await self._endpoint_update_sub_api.post(
            BybitUpdateSubApiPostParams(
                api_key=api_key,
                read_only=read_only,
                ips=ips,
            ),
        )
        return response.result
