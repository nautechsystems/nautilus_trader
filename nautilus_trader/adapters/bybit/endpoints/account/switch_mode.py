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
from nautilus_trader.adapters.bybit.schemas.account.switch_mode import BybitSwitchModeResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.common.enums import BybitPositionMode
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient


class BybitSwitchModePostParams(msgspec.Struct, omit_defaults=True, frozen=True, kw_only=True):
    category: BybitProductType
    symbol: str | None = None
    coin: str | None = None
    mode: BybitPositionMode


class BybitSwitchModeEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ) -> None:
        self.http_method = HttpMethod.POST
        url_path = base_endpoint + "/position/switch-mode"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.POSITION,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(BybitSwitchModeResponse)

    async def post(self, params: BybitSwitchModePostParams) -> BybitSwitchModeResponse:
        raw = await self._method(self.http_method, params)
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()}",
            ) from e
