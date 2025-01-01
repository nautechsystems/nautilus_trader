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

from typing import Literal

from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXMarginMode
from nautilus_trader.adapters.okx.common.enums import OKXOrderSide
from nautilus_trader.adapters.okx.common.enums import OKXOrderStatus
from nautilus_trader.adapters.okx.common.enums import OKXOrderType
from nautilus_trader.adapters.okx.common.enums import OKXPositionSide
from nautilus_trader.adapters.okx.common.enums import OKXSelfTradePreventionMode
from nautilus_trader.adapters.okx.common.enums import OKXTradeMode
from nautilus_trader.adapters.okx.common.enums import OKXTransactionType
from nautilus_trader.adapters.okx.endpoints.trade.amend_order import OKXAmendOrderAttachAlgoOrds
from nautilus_trader.adapters.okx.endpoints.trade.amend_order import OKXAmendOrderEndpoint
from nautilus_trader.adapters.okx.endpoints.trade.amend_order import OKXAmendOrderPostParams
from nautilus_trader.adapters.okx.endpoints.trade.cancel_order import OKXCancelOrderEndpoint
from nautilus_trader.adapters.okx.endpoints.trade.cancel_order import OKXCancelOrderPostParams
from nautilus_trader.adapters.okx.endpoints.trade.close_position import OKXClosePositionEndpoint
from nautilus_trader.adapters.okx.endpoints.trade.close_position import OKXClosePositionPostParams
from nautilus_trader.adapters.okx.endpoints.trade.fills import OKXFillsEndpoint
from nautilus_trader.adapters.okx.endpoints.trade.fills import OKXFillsGetParams
from nautilus_trader.adapters.okx.endpoints.trade.fills_history import OKXFillsHistoryEndpoint
from nautilus_trader.adapters.okx.endpoints.trade.fills_history import OKXFillsHistoryGetParams
from nautilus_trader.adapters.okx.endpoints.trade.order_details import OKXOrderDetailsEndpoint
from nautilus_trader.adapters.okx.endpoints.trade.order_details import OKXOrderDetailsGetParams
from nautilus_trader.adapters.okx.endpoints.trade.orders_history import OKXOrderHistoryEndpoint
from nautilus_trader.adapters.okx.endpoints.trade.orders_history import OKXOrderHistoryGetParams
from nautilus_trader.adapters.okx.endpoints.trade.orders_pending import OKXOrdersPendingEndpoint
from nautilus_trader.adapters.okx.endpoints.trade.orders_pending import OKXOrdersPendingGetParams
from nautilus_trader.adapters.okx.endpoints.trade.place_order import OKXPlaceOrderAttachAlgoOrds
from nautilus_trader.adapters.okx.endpoints.trade.place_order import OKXPlaceOrderEndpoint
from nautilus_trader.adapters.okx.endpoints.trade.place_order import OKXPlaceOrderPostParams
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.trade import OKXAmendOrderResponse
from nautilus_trader.adapters.okx.schemas.trade import OKXCancelOrderResponse
from nautilus_trader.adapters.okx.schemas.trade import OKXClosePositionResponse
from nautilus_trader.adapters.okx.schemas.trade import OKXFillsHistoryResponse
from nautilus_trader.adapters.okx.schemas.trade import OKXFillsResponse
from nautilus_trader.adapters.okx.schemas.trade import OKXOrderDetailsResponse
from nautilus_trader.adapters.okx.schemas.trade import OKXPlaceOrderResponse
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


class OKXTradeHttpAPI:
    def __init__(
        self,
        client: OKXHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/api/v5/trade"

        self._endpoint_order_details = OKXOrderDetailsEndpoint(client, self.base_endpoint)
        self._endpoint_order_history = OKXOrderHistoryEndpoint(client, self.base_endpoint)
        self._endpoint_orders_pending = OKXOrdersPendingEndpoint(client, self.base_endpoint)
        self._endpoint_place_order = OKXPlaceOrderEndpoint(client, self.base_endpoint)
        self._endpoint_amend_order = OKXAmendOrderEndpoint(client, self.base_endpoint)
        self._endpoint_cancel_order = OKXCancelOrderEndpoint(client, self.base_endpoint)
        self._endpoint_close_position = OKXClosePositionEndpoint(client, self.base_endpoint)
        self._endpoint_fills_history = OKXFillsHistoryEndpoint(client, self.base_endpoint)
        self._endpoint_fills = OKXFillsEndpoint(client, self.base_endpoint)

    async def fetch_order_details(
        self,
        instId: str,
        ordId: str | None = None,
        clOrdId: str | None = None,
    ) -> OKXOrderDetailsResponse:
        response = await self._endpoint_order_details.get(
            OKXOrderDetailsGetParams(
                instId=instId,
                ordId=ordId,
                clOrdId=clOrdId,
            ),
        )
        return response

    async def fetch_order_history(
        self,
        instType: OKXInstrumentType,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
        ordType: OKXOrderType | None = None,
        state: OKXOrderStatus | None = None,
        category: (
            Literal["twap", "adl", "full_liquidation", "partial_liquidation", "delivery", "ddh"]
            | None
        ) = None,
        after: str | None = None,
        before: str | None = None,
        begin: str | None = None,
        end: str | None = None,
        limit: str | None = None,
    ) -> OKXOrderDetailsResponse:  # same response struct as order details
        response = await self._endpoint_order_history.get(
            OKXOrderHistoryGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
                ordType=ordType,
                state=state,
                category=category,
                after=after,
                before=before,
                begin=begin,
                end=end,
                limit=limit,
            ),
        )
        return response

    async def fetch_orders_pending(
        self,
        instType: OKXInstrumentType,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
        ordType: OKXOrderType | None = None,
        state: OKXOrderStatus | None = None,
        category: (
            Literal["twap", "adl", "full_liquidation", "partial_liquidation", "delivery", "ddh"]
            | None
        ) = None,
        after: str | None = None,
        before: str | None = None,
        begin: str | None = None,
        end: str | None = None,
        limit: str | None = None,
    ) -> OKXOrderDetailsResponse:  # same response struct as order details
        response = await self._endpoint_orders_pending.get(
            OKXOrdersPendingGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
                ordType=ordType,
                state=state,
                category=category,
                after=after,
                before=before,
                begin=begin,
                end=end,
                limit=limit,
            ),
        )
        return response

    async def fetch_fills(
        self,
        instType: OKXInstrumentType | None = None,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
        ordId: str | None = None,
        subType: OKXTransactionType | None = None,
        after: str | None = None,
        before: str | None = None,
        begin: str | None = None,
        end: str | None = None,
        limit: str | None = None,
    ) -> OKXFillsResponse:
        response = await self._endpoint_fills.get(
            OKXFillsGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
                ordId=ordId,
                subType=subType,
                after=after,
                before=before,
                begin=begin,
                end=end,
                limit=limit,
            ),
        )
        return response

    async def fetch_fills_history(
        self,
        instType: OKXInstrumentType,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
        ordId: str | None = None,
        subType: OKXTransactionType | None = None,
        after: str | None = None,
        before: str | None = None,
        begin: str | None = None,
        end: str | None = None,
        limit: str | None = None,
    ) -> OKXFillsHistoryResponse:
        response = await self._endpoint_fills_history.get(
            OKXFillsHistoryGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
                ordId=ordId,
                subType=subType,
                after=after,
                before=before,
                begin=begin,
                end=end,
                limit=limit,
            ),
        )
        return response

    async def place_order(
        self,
        instId: str,
        tdMode: OKXTradeMode,
        side: OKXOrderSide,
        ordType: OKXOrderType,
        sz: str,
        ccy: str | None = None,
        clOrdId: str | None = None,
        tag: str | None = None,
        posSide: OKXPositionSide = OKXPositionSide.NET,
        px: str | None = None,
        reduceOnly: bool = False,
        stpMode: OKXSelfTradePreventionMode = OKXSelfTradePreventionMode.CANCEL_MAKER,
        attachAlgoOrds: list[OKXPlaceOrderAttachAlgoOrds] | None = None,
    ) -> OKXPlaceOrderResponse:
        response = await self._endpoint_place_order.post(
            OKXPlaceOrderPostParams(
                instId=instId,
                tdMode=tdMode,
                side=side,
                ordType=ordType,
                sz=sz,
                ccy=ccy,
                clOrdId=clOrdId,
                tag=tag,
                posSide=posSide,
                px=px,
                reduceOnly=reduceOnly,
                stpMode=stpMode,
                attachAlgoOrds=attachAlgoOrds,
            ),
        )
        return response

    async def amend_order(
        self,
        instId: str,
        cxlOnFail: bool = False,  # if should automatically cancel when amendment fails
        ordId: str | None = None,
        clOrdId: str | None = None,
        reqId: str | None = None,  # client order id for the amended order
        newSz: str | None = None,  # newSz should include amount filled for partially filled orders
        newPx: str | None = None,
        attachAlgoOrds: list[OKXAmendOrderAttachAlgoOrds] | None = None,
    ) -> OKXAmendOrderResponse:
        response = await self._endpoint_amend_order.post(
            OKXAmendOrderPostParams(
                instId=instId,
                cxlOnFail=cxlOnFail,
                ordId=ordId,
                clOrdId=clOrdId,
                reqId=reqId,
                newSz=newSz,
                newPx=newPx,
                attachAlgoOrds=attachAlgoOrds,
            ),
        )
        return response

    async def cancel_order(
        self,
        instId: str,
        ordId: str | None = None,
        clOrdId: str | None = None,
    ) -> OKXCancelOrderResponse:
        response = await self._endpoint_cancel_order.post(
            OKXCancelOrderPostParams(
                instId=instId,
                ordId=ordId,
                clOrdId=clOrdId,
            ),
        )
        return response

    async def close_position(
        self,
        instId: str,
        mgnMode: OKXMarginMode,
        posSide: OKXPositionSide = OKXPositionSide.NET,
        ccy: str | None = None,
        autoCxl: bool = False,  # cancel pending orders for this instrument else error if pendings
        clOrdId: str | None = None,
        tag: str | None = None,
    ) -> OKXClosePositionResponse:
        response = await self._endpoint_close_position.post(
            OKXClosePositionPostParams(
                instId=instId,
                mgnMode=mgnMode,
                posSide=posSide,
                ccy=ccy,
                autoCxl=autoCxl,
                clOrdId=clOrdId,
                tag=tag,
            ),
        )
        return response
