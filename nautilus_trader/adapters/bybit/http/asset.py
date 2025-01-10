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

from nautilus_trader.adapters.bybit.endpoints.asset.coin_info import BybitCoinInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.asset.coin_info import BybitCoinInfoGetParams
from nautilus_trader.core.correctness import PyCondition


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
    from nautilus_trader.adapters.bybit.schemas.asset.coin_info import BybitCoinInfo
    from nautilus_trader.common.component import LiveClock


class BybitAssetHttpAPI:
    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/v5"

        self._endpoint_coin_info = BybitCoinInfoEndpoint(client, self.base_endpoint)

    async def fetch_coin_info(
        self,
        coin: str | None = None,
    ) -> list[BybitCoinInfo]:
        response = await self._endpoint_coin_info.get(
            BybitCoinInfoGetParams(
                coin=coin,
            ),
        )
        return response.result.rows
