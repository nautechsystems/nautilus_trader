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

from typing import Any

from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.endpoints.account.fee_rate import BybitFeeRateEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.fee_rate import BybitFeeRateGetParams
from nautilus_trader.adapters.bybit.endpoints.account.position_info import BybitPositionInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.position_info import PositionInfoGetParams

# fmt: off
from nautilus_trader.adapters.bybit.endpoints.account.wallet_balance import BybitWalletBalanceEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.wallet_balance import BybitWalletBalanceGetParams
from nautilus_trader.adapters.bybit.endpoints.trade.amend_order import BybitAmendOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.amend_order import BybitAmendOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_amend_order import BybitBatchAmendOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrder
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_place_order import BybitBatchPlaceOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_all_orders import BybitCancelAllOrdersEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_all_orders import BybitCancelAllOrdersPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_order import BybitCancelOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_order import BybitCancelOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.open_orders import BybitOpenOrdersEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.open_orders import BybitOpenOrdersGetParams
from nautilus_trader.adapters.bybit.endpoints.trade.order_history import BybitOrderHistoryEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.order_history import BybitOrderHistoryGetParams
from nautilus_trader.adapters.bybit.endpoints.trade.place_order import BybitPlaceOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.place_order import BybitPlaceOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.trade_history import BybitTradeHistoryEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.trade_history import BybitTradeHistoryGetParams
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.account.balance import BybitWalletBalance
from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRate
from nautilus_trader.adapters.bybit.schemas.order import BybitAmendOrder
from nautilus_trader.adapters.bybit.schemas.order import BybitCancelOrder
from nautilus_trader.adapters.bybit.schemas.order import BybitOrder
from nautilus_trader.adapters.bybit.schemas.order import BybitPlaceOrderResponse
from nautilus_trader.adapters.bybit.schemas.position import BybitPositionStruct
from nautilus_trader.adapters.bybit.schemas.trade import BybitExecution
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.orders import Order


# fmt: on


class BybitAccountHttpAPI:
    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/v5"
        self.default_settle_coin = "USDT"

        self._endpoint_fee_rate = BybitFeeRateEndpoint(client, self.base_endpoint)
        self._endpoint_wallet_balance = BybitWalletBalanceEndpoint(client, self.base_endpoint)
        self._endpoint_position_info = BybitPositionInfoEndpoint(client, self.base_endpoint)
        self._endpoint_open_orders = BybitOpenOrdersEndpoint(client, self.base_endpoint)
        self._endpoint_order_history = BybitOrderHistoryEndpoint(client, self.base_endpoint)
        self._endpoint_trade_history = BybitTradeHistoryEndpoint(client, self.base_endpoint)
        self._endpoint_place_order = BybitPlaceOrderEndpoint(client, self.base_endpoint)
        self._endpoint_amend_order = BybitAmendOrderEndpoint(client, self.base_endpoint)
        self._endpoint_cancel_order = BybitCancelOrderEndpoint(client, self.base_endpoint)
        self._endpoint_cancel_all_orders = BybitCancelAllOrdersEndpoint(client, self.base_endpoint)
        self._endpoint_batch_place_order = BybitBatchPlaceOrderEndpoint(client, self.base_endpoint)
        self._endpoint_batch_amend_order = BybitBatchAmendOrderEndpoint(client, self.base_endpoint)
        self._endpoint_batch_cancel_order = BybitBatchCancelOrderEndpoint(
            client,
            self.base_endpoint,
        )

    async def fetch_fee_rate(
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
        base_coin: str | None = None,
    ) -> list[BybitFeeRate]:
        response = await self._endpoint_fee_rate.get(
            BybitFeeRateGetParams(
                category=product_type,
                symbol=symbol,
                baseCoin=base_coin,
            ),
        )
        return response.result.list

    async def query_wallet_balance(
        self,
        coin: str | None = None,
    ) -> tuple[list[BybitWalletBalance], int]:
        response = await self._endpoint_wallet_balance.get(
            BybitWalletBalanceGetParams(
                accountType="UNIFIED",
            ),
        )
        return response.result.list, response.time

    async def query_position_info(
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
    ) -> list[BybitPositionStruct]:
        match product_type:
            case BybitProductType.INVERSE:
                settle_coin = None
            case _:
                settle_coin = self.default_settle_coin if symbol is None else None

        response = await self._endpoint_position_info.get(
            PositionInfoGetParams(
                symbol=symbol,
                settleCoin=settle_coin,
                category=product_type.value,
            ),
        )
        return response.result.list

    async def query_open_orders(
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
    ) -> list[BybitOrder]:
        match product_type:
            case BybitProductType.INVERSE:
                settle_coin = None
            case _:
                settle_coin = self.default_settle_coin if symbol is None else None

        response = await self._endpoint_open_orders.get(
            BybitOpenOrdersGetParams(
                category=product_type,
                symbol=symbol,
                settleCoin=settle_coin,
            ),
        )
        return response.result.list

    async def query_order_history(
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
    ) -> list[BybitOrder]:
        match product_type:
            case BybitProductType.INVERSE:
                settle_coin = None
            case _:
                settle_coin = self.default_settle_coin if symbol is None else None

        response = await self._endpoint_order_history.get(
            BybitOrderHistoryGetParams(
                category=product_type,
                symbol=symbol,
                settleCoin=settle_coin,
            ),
        )
        return response.result.list

    async def query_trade_history(
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
    ) -> list[BybitExecution]:
        response = await self._endpoint_trade_history.get(
            BybitTradeHistoryGetParams(
                category=product_type,
                symbol=symbol,
            ),
        )
        return response.result.list

    async def query_order(
        self,
        product_type: BybitProductType,
        symbol: str | None,
        client_order_id: str | None,
        order_id: str | None,
    ) -> list[BybitOrder]:
        response = await self._endpoint_open_orders.get(
            BybitOpenOrdersGetParams(
                category=product_type,
                symbol=symbol,
                orderLinkId=client_order_id,
                orderId=order_id,
            ),
        )
        return response.result.list

    async def place_order(
        self,
        product_type: BybitProductType,
        symbol: str,
        side: BybitOrderSide,
        quantity: str,
        quote_quantity: bool,
        order_type: BybitOrderType,
        price: str | None = None,
        time_in_force: BybitTimeInForce | None = None,
        client_order_id: str | None = None,
    ) -> BybitPlaceOrderResponse:
        market_unit = "baseCoin" if not quote_quantity else "quoteCoin"
        result = await self._endpoint_place_order.post(
            params=BybitPlaceOrderPostParams(
                category=product_type,
                symbol=symbol,
                side=side,
                orderType=order_type,
                qty=quantity,
                marketUnit=market_unit,
                price=price,
                timeInForce=time_in_force,
                orderLinkId=client_order_id,
            ),
        )
        return result

    async def amend_order(
        self,
        product_type: BybitProductType,
        symbol: str,
        client_order_id: str | None = None,
        venue_order_id: str | None = None,
        trigger_price: str | None = None,
        quantity: str | None = None,
        price: str | None = None,
    ) -> BybitAmendOrder:
        response = await self._endpoint_amend_order.post(
            BybitAmendOrderPostParams(
                category=product_type,
                symbol=symbol,
                orderId=venue_order_id,
                orderLinkId=client_order_id,
                triggerPrice=trigger_price,
                qty=quantity,
                price=price,
            ),
        )
        return response.result

    async def cancel_order(
        self,
        product_type: BybitProductType,
        symbol: str,
        client_order_id: str | None = None,
        venue_order_id: str | None = None,
        order_filter: str | None = None,
    ) -> BybitCancelOrder:
        response = await self._endpoint_cancel_order.post(
            BybitCancelOrderPostParams(
                category=product_type,
                symbol=symbol,
                orderId=venue_order_id,
                orderLinkId=client_order_id,
                orderFilter=order_filter,
            ),
        )
        return response.result

    async def cancel_all_orders(
        self,
        product_type: BybitProductType,
        symbol: str,
    ) -> list[Any]:
        response = await self._endpoint_cancel_all_orders.post(
            BybitCancelAllOrdersPostParams(
                category=product_type,
                symbol=symbol,
            ),
        )
        return response.result.list

    async def batch_cancel_orders(
        self,
        product_type: BybitProductType,
        symbol: str,
        orders: list[Order],
    ) -> list[Any]:
        request: list[BybitBatchCancelOrder] = []

        for order in orders:
            request.append(
                BybitBatchCancelOrder(
                    symbol=symbol,
                    orderId=order.venue_order_id.value if order.venue_order_id else None,
                    orderLinkId=order.client_order_id.value,
                ),
            )
        response = await self._endpoint_batch_cancel_order.post(
            BybitBatchCancelOrderPostParams(
                category=product_type,
                request=request,
            ),
        )
        return response.result
