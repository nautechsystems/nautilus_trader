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
from nautilus_trader.adapters.okx.common.enums import OKXTakeProfitKind
from nautilus_trader.adapters.okx.common.enums import OKXTriggerType
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.trade import OKXAmendOrderResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXAmendOrderAttachAlgoOrds(msgspec.Struct, omit_defaults=True, frozen=True):
    attachAlgoId: str | None = None
    attachAlgoClOrdId: str | None = None
    newTpTriggerPx: str | None = None  # if "0", take-profit is deleted
    newTpOrdPx: str | None = None  # if "0", take-profit is deleted
    newTpOrdKind: OKXTakeProfitKind = OKXTakeProfitKind.CONDITION
    newSlTriggerPx: str | None = None
    newSlOrdPx: str | None = None
    newTpTriggerPxType: OKXTriggerType = OKXTriggerType.LAST
    newSlTriggerPxType: OKXTriggerType = OKXTriggerType.LAST
    sz: str | None = None
    amendPxOnTriggerType: str = "0"  # enable cost-price SL for split TP's. "0": disable, "1" enable

    def validate(self) -> None:
        # just handle errors in response -> good initial validation is done in place-order
        pass


class OKXAmendOrderPostParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instId: str
    cxlOnFail: bool = False  # if should automatically cancel when amendment fails
    ordId: str | None = None
    clOrdId: str | None = None
    reqId: str | None = None  # client order id for the amended order
    newSz: str | None = None  # newSz should include amount filled for partially filled orders
    newPx: str | None = None
    attachAlgoOrds: list[OKXAmendOrderAttachAlgoOrds] | None = None

    def validate(self) -> None:
        assert (
            self.ordId or self.clOrdId
        ), "either `ordId` or `clOrdId` is required to amend an order"

        if self.newSz:
            assert float(self.newSz) > 0, "`newSz` must be greater than 0 when provided"

        if self.attachAlgoOrds:
            for attached_algo_order in self.attachAlgoOrds:
                attached_algo_order.validate()


class OKXAmendOrderEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/amend-order"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXAmendOrderResponse)

    async def post(self, params: OKXAmendOrderPostParams) -> OKXAmendOrderResponse:
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
