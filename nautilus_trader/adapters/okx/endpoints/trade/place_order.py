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
from nautilus_trader.adapters.okx.common.enums import OKXOrderSide
from nautilus_trader.adapters.okx.common.enums import OKXOrderType
from nautilus_trader.adapters.okx.common.enums import OKXPositionSide
from nautilus_trader.adapters.okx.common.enums import OKXSelfTradePreventionMode
from nautilus_trader.adapters.okx.common.enums import OKXTakeProfitKind
from nautilus_trader.adapters.okx.common.enums import OKXTradeMode
from nautilus_trader.adapters.okx.common.enums import OKXTriggerType
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.trade import OKXPlaceOrderResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXPlaceOrderAttachAlgoOrds(msgspec.Struct, omit_defaults=True, frozen=True):
    attachAlgoClOrdId: str | None = None
    tpTriggerPx: str | None = None
    tpOrdPx: str | None = None  # assign "-1" for market order (kind must be 'condition')
    tpOrdKind: OKXTakeProfitKind = OKXTakeProfitKind.CONDITION
    slTriggerPx: str | None = None
    slOrdPx: str | None = None  # assign "-1" for market order (kind must be 'condition')
    tpTriggerPxType: OKXTriggerType = OKXTriggerType.LAST
    slTriggerPxType: OKXTriggerType = OKXTriggerType.LAST
    sz: str | None = None
    amendPxOnTriggerType: str = "0"  # enable cost-price SL for split TP's. "0": disable, "1" enable

    def validate(self) -> None:
        if self.tpOrdKind == OKXTakeProfitKind.CONDITION:
            if self.tpTriggerPx:
                assert self.tpOrdPx, (
                    "`tpOrdPx` is required when `tpTriggerPx` is specified for 'condition' "
                    "take-profit orders"
                )
            elif self.tpOrdPx:
                assert self.tpTriggerPx, (
                    "`tpTriggerPx` is required when `tpOrdPx` is specified for 'condition' "
                    "take-profit orders"
                )

        if self.tpOrdKind == OKXTakeProfitKind.LIMIT:
            assert self.tpOrdPx, "`tpOrdPx` is required for 'limit' take-profit orders"
            assert self.tpOrdPx != "-1", (
                "`tpOrdPx` can only be '-1' when the take-profit order kind is 'condition' "
                "(together forming a take-profit market order)"
            )

        if self.slTriggerPx:
            assert self.slOrdPx, "`slOrdPx` is required when `slTriggerPx` is specified"
        if self.slOrdPx:
            assert self.slTriggerPx, "`slTriggerPx` is required when `slOrdPx` is specified"


class OKXPlaceOrderPostParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instId: str
    tdMode: OKXTradeMode
    side: OKXOrderSide
    ordType: OKXOrderType
    sz: str
    ccy: str | None = None
    clOrdId: str | None = None
    tag: str | None = None
    posSide: OKXPositionSide = OKXPositionSide.NET
    px: str | None = None
    reduceOnly: bool = False
    stpMode: OKXSelfTradePreventionMode = OKXSelfTradePreventionMode.CANCEL_MAKER
    attachAlgoOrds: list[OKXPlaceOrderAttachAlgoOrds] | None = None

    def validate(self) -> None:
        if self.attachAlgoOrds:
            for attached_algo_order in self.attachAlgoOrds:
                attached_algo_order.validate()


class OKXPlaceOrderEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/order"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXPlaceOrderResponse)

    async def post(self, params: OKXPlaceOrderPostParams) -> OKXPlaceOrderResponse:
        # Validate
        params.validate()

        method_type = HttpMethod.POST
        raw = await self._method(method_type, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
