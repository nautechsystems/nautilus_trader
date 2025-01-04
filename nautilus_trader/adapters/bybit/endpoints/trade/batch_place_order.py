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
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.common.enums import BybitTpSlMode
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerDirection
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.schemas.order import BybitBatchPlaceOrderResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient


class BybitBatchPlaceOrder(
    msgspec.Struct,
    omit_defaults=True,
    frozen=True,
    kw_only=True,
):
    symbol: str
    isLeverage: int | None = None
    side: BybitOrderSide
    orderType: BybitOrderType
    qty: str
    price: str | None = None
    marketUnit: str | None = None
    triggerDirection: BybitTriggerDirection | None = None
    orderFilter: str | None = None
    triggerPrice: str | None = None
    triggerBy: BybitTriggerType | None = None
    orderIv: str | None = None
    timeInForce: BybitTimeInForce | None = None
    positionIdx: int | None = None
    orderLinkId: str | None = None
    takeProfit: str | None = None
    stopLoss: str | None = None
    tpTriggerBy: BybitTriggerType | None = None
    slTriggerBy: BybitTriggerType | None = None
    reduceOnly: bool | None = None
    closeOnTrigger: bool | None = None
    smpType: str | None = None
    mmp: bool | None = None
    tpslMode: BybitTpSlMode | None = None  # Must be PARTIAL for Limit orders
    tpLimitPrice: str | None = None  # tpslMode must be PARTIAL, tpOrderType must be LIMIT
    slLimitPrice: str | None = None  # tpslMode must be PARTIAL, slOrderType must be LIMIT
    tpOrderType: BybitOrderType | None = None  # MARKET for takeProfit, LIMIT with tpLimitPrice
    slOrderType: BybitOrderType | None = None  # MARKET for stopLoss, LIMIT with slLimitPrice


class BybitBatchPlaceOrderPostParams(msgspec.Struct, omit_defaults=True, frozen=True):
    category: BybitProductType
    request: list[BybitBatchPlaceOrder]


class BybitBatchPlaceOrderEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/order/create-batch"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(BybitBatchPlaceOrderResponse)

    async def post(self, params: BybitBatchPlaceOrderPostParams) -> BybitBatchPlaceOrderResponse:
        method_type = HttpMethod.POST
        raw = await self._method(method_type, params)
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()}",
            ) from e
