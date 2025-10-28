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
from typing import Any

import pandas as pd

from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitPositionIdx
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.common.enums import BybitTpSlMode
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerDirection
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerType
from nautilus_trader.adapters.bybit.endpoints.account.fee_rate import BybitFeeRateEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.fee_rate import BybitFeeRateGetParams
from nautilus_trader.adapters.bybit.endpoints.account.info import BybitAccountInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.position_info import BybitPositionInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.position_info import PositionInfoGetParams
from nautilus_trader.adapters.bybit.endpoints.account.set_leverage import BybitSetLeverageEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.set_leverage import BybitSetLeveragePostParams
from nautilus_trader.adapters.bybit.endpoints.account.set_margin_mode import BybitSetMarginModeEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.set_margin_mode import BybitSetMarginModePostParams
from nautilus_trader.adapters.bybit.endpoints.account.switch_mode import BybitSwitchModeEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.switch_mode import BybitSwitchModePostParams
from nautilus_trader.adapters.bybit.endpoints.account.wallet_balance import BybitWalletBalanceEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.wallet_balance import BybitWalletBalanceGetParams
from nautilus_trader.adapters.bybit.endpoints.trade.amend_order import BybitAmendOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.amend_order import BybitAmendOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_amend_order import BybitBatchAmendOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrder
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_place_order import BybitBatchPlaceOrder
from nautilus_trader.adapters.bybit.endpoints.trade.batch_place_order import BybitBatchPlaceOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.batch_place_order import BybitBatchPlaceOrderPostParams
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
from nautilus_trader.adapters.bybit.endpoints.trade.set_trading_stop import BybitSetTradingStopEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.set_trading_stop import BybitSetTradingStopPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.trade_history import BybitTradeHistoryEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.trade_history import BybitTradeHistoryGetParams
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.order import BybitBatchCancelOrderResponse
from nautilus_trader.adapters.bybit.schemas.order import BybitBatchPlaceOrderResponse
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.common.enums import BybitMarginMode
    from nautilus_trader.adapters.bybit.common.enums import BybitPositionMode
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
    from nautilus_trader.adapters.bybit.schemas.account.balance import BybitWalletBalance
    from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRate
    from nautilus_trader.adapters.bybit.schemas.account.info import BybitAccountInfo
    from nautilus_trader.adapters.bybit.schemas.account.set_leverage import BybitSetLeverageResponse
    from nautilus_trader.adapters.bybit.schemas.account.set_margin_mode import BybitSetMarginModeResponse
    from nautilus_trader.adapters.bybit.schemas.account.switch_mode import BybitSwitchModeResponse
    from nautilus_trader.adapters.bybit.schemas.order import BybitAmendOrder
    from nautilus_trader.adapters.bybit.schemas.order import BybitCancelOrderResponse
    from nautilus_trader.adapters.bybit.schemas.order import BybitOrder
    from nautilus_trader.adapters.bybit.schemas.order import BybitPlaceOrderResponse
    from nautilus_trader.adapters.bybit.schemas.order import BybitSetTradingStopResponse
    from nautilus_trader.adapters.bybit.schemas.position import BybitPositionStruct
    from nautilus_trader.adapters.bybit.schemas.trade import BybitExecution
    from nautilus_trader.common.component import LiveClock




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
        self._endpoint_set_trading_stop = BybitSetTradingStopEndpoint(client, self.base_endpoint)
        self._endpoint_amend_order = BybitAmendOrderEndpoint(client, self.base_endpoint)
        self._endpoint_cancel_order = BybitCancelOrderEndpoint(client, self.base_endpoint)
        self._endpoint_cancel_all_orders = BybitCancelAllOrdersEndpoint(client, self.base_endpoint)
        self._endpoint_batch_place_order = BybitBatchPlaceOrderEndpoint(client, self.base_endpoint)
        self._endpoint_batch_amend_order = BybitBatchAmendOrderEndpoint(client, self.base_endpoint)
        self._endpoint_batch_cancel_order = BybitBatchCancelOrderEndpoint(
            client,
            self.base_endpoint,
        )
        self._endpoint_account_info = BybitAccountInfoEndpoint(client, self.base_endpoint)
        self._endpoint_set_margin_mode = BybitSetMarginModeEndpoint(client, self.base_endpoint)
        self._endpoint_set_leverage = BybitSetLeverageEndpoint(client, self.base_endpoint)
        self._endpoint_switch_mode = BybitSwitchModeEndpoint(client, self.base_endpoint)

    async def fetch_account_info(self) -> BybitAccountInfo:
        response = await self._endpoint_account_info.get()
        return response.result

    async def set_margin_mode(self, margin_mode: BybitMarginMode) -> BybitSetMarginModeResponse:
        response = await self._endpoint_set_margin_mode.post(
            BybitSetMarginModePostParams(
                setMarginMode=margin_mode,
            ),
        )
        return response

    async def set_leverage(
        self,
        category: BybitProductType,
        symbol: str,
        buy_leverage: str,
        sell_leverage: str,
    ) -> BybitSetLeverageResponse:
        response = await self._endpoint_set_leverage.post(
            BybitSetLeveragePostParams(
                category=category,
                symbol=symbol,
                buyLeverage=buy_leverage,
                sellLeverage=sell_leverage,
            ),
        )
        return response

    async def switch_mode(
        self,
        category: BybitProductType,
        mode: BybitPositionMode,
        symbol: str | None = None,
        coin: str | None = None,
    ) -> BybitSwitchModeResponse:
        response = await self._endpoint_switch_mode.post(
            BybitSwitchModePostParams(
                category=category,
                symbol=symbol,
                coin=coin,
                mode=mode,
            ),
        )
        return response

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

        all_positions = []
        cursor = None

        while True:
            response = await self._endpoint_position_info.get(
                PositionInfoGetParams(
                    symbol=symbol,
                    settleCoin=settle_coin,
                    category=product_type.value,
                    limit=200,
                    cursor=cursor,
                ),
            )
            all_positions.extend(response.result.list)

            if hasattr(response.result, "nextPageCursor") and response.result.nextPageCursor:
                cursor = response.result.nextPageCursor
            else:
                break

        return all_positions

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

        all_orders = []
        cursor = None

        while True:
            response = await self._endpoint_open_orders.get(
                BybitOpenOrdersGetParams(
                    category=product_type,
                    symbol=symbol,
                    settleCoin=settle_coin,
                    limit=50,
                    cursor=cursor,
                ),
            )
            all_orders.extend(response.result.list)

            if hasattr(response.result, "nextPageCursor") and response.result.nextPageCursor:
                cursor = response.result.nextPageCursor
            else:
                break

        return all_orders

    async def query_order_history(  # noqa: C901 (too complex)
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
        open_only: bool | None = None,
        start: pd.Timestamp | None = None,
    ) -> list[BybitOrder]:
        # Don't filter by settleCoin for order history
        # This ensures we get orders for all settlement coins (USDT, USDC, etc.)
        settle_coin = None

        # openOnly is unintuitively 0 for true (see docs https://bybit-exchange.github.io/docs/v5/order/open-order)
        # openOnly=0 means "open orders only", openOnly=1 means "closed orders", None defaults to 0

        # Convert start timestamp to milliseconds if provided
        start_time_ms = None
        if start is not None:
            start_time_ms = int(start.timestamp() * 1000)

        # Default to 3 days of history for closed orders to avoid excessive API calls
        import time

        three_days_ago_ms = int((time.time() - 3 * 24 * 60 * 60) * 1000)

        # When open_only=False, we need BOTH open and closed orders for proper reconciliation
        if open_only is False:
            all_orders = []

            # Query open orders with pagination (openOnly=0)
            # Note: Bybit API returns open + recent closed orders even with openOnly=0
            cursor = None

            while True:
                open_response = await self._endpoint_order_history.get(
                    BybitOrderHistoryGetParams(
                        category=product_type,
                        symbol=symbol,
                        openOnly=0,  # Gets open + recent closed orders
                        settleCoin=settle_coin,
                        startTime=start_time_ms,  # Use provided start time if available
                        limit=50,  # Max 50 per request (default is 20)
                        cursor=cursor,
                    ),
                )
                all_orders.extend(open_response.result.list)

                # Check if there are more pages
                if (
                    hasattr(open_response.result, "nextPageCursor")
                    and open_response.result.nextPageCursor
                ):
                    cursor = open_response.result.nextPageCursor
                else:
                    break

            # Query closed orders with pagination (openOnly=1)
            cursor = None

            while True:
                closed_response = await self._endpoint_order_history.get(
                    BybitOrderHistoryGetParams(
                        category=product_type,
                        symbol=symbol,
                        openOnly=1,  # Get closed orders (filled, canceled, rejected)
                        settleCoin=settle_coin,
                        startTime=start_time_ms
                        or three_days_ago_ms,  # Use provided start time or default to 3 days
                        limit=50,  # Max 50 per request (default is 20)
                        cursor=cursor,
                    ),
                )
                all_orders.extend(closed_response.result.list)

                # Check if there are more pages
                if (
                    hasattr(closed_response.result, "nextPageCursor")
                    and closed_response.result.nextPageCursor
                ):
                    cursor = closed_response.result.nextPageCursor
                else:
                    break

            # De-duplicate orders by orderId (some orders may appear in both queries)
            seen_order_ids = set()
            deduplicated_orders = []
            for order in all_orders:
                if order.orderId not in seen_order_ids:
                    seen_order_ids.add(order.orderId)
                    deduplicated_orders.append(order)

            return deduplicated_orders

        # For open_only=True or None, query with openOnly=0
        # Note: Bybit API returns open + recent closed orders even with openOnly=0
        # This is expected behavior - the API includes recently closed orders for context
        if open_only is True:
            open_only_param = 0  # Gets open + recent closed orders
        else:
            open_only_param = None  # Use API default (also returns open + recent closed)

        all_orders = []
        cursor = None

        while True:
            response = await self._endpoint_order_history.get(
                BybitOrderHistoryGetParams(
                    category=product_type,
                    symbol=symbol,
                    openOnly=open_only_param,
                    settleCoin=settle_coin,
                    startTime=start_time_ms,  # Use provided start time if available
                    limit=50,  # Max 50 per request (default is 20)
                    cursor=cursor,
                ),
            )
            all_orders.extend(response.result.list)

            # Check if there are more pages
            if hasattr(response.result, "nextPageCursor") and response.result.nextPageCursor:
                cursor = response.result.nextPageCursor
            else:
                break

        return all_orders

    async def query_trade_history(
        self,
        product_type: BybitProductType,
        symbol: str | None = None,
        start_time: int | None = None,
        end_time: int | None = None,
    ) -> list[BybitExecution]:
        all_executions = []
        cursor = None

        while True:
            response = await self._endpoint_trade_history.get(
                BybitTradeHistoryGetParams(
                    category=product_type,
                    symbol=symbol,
                    startTime=start_time,
                    endtime=end_time,
                    limit=100,
                    cursor=cursor,
                ),
            )
            all_executions.extend(response.result.list)

            if hasattr(response.result, "nextPageCursor") and response.result.nextPageCursor:
                cursor = response.result.nextPageCursor
            else:
                break

        return all_executions

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
        reduce_only: bool | None = None,
        is_leverage: bool | None = None,
        tpsl_mode: BybitTpSlMode | None = None,
        close_on_trigger: bool | None = None,
        tp_order_type: BybitOrderType | None = None,
        sl_order_type: BybitOrderType | None = None,
        trigger_direction: BybitTriggerDirection | None = None,
        trigger_type: BybitTriggerType | None = None,
        trigger_price: str | None = None,
        sl_trigger_price: str | None = None,
        tp_trigger_price: str | None = None,
        tp_limit_price: str | None = None,
        sl_limit_price: str | None = None,
    ) -> BybitPlaceOrderResponse:
        return await self._endpoint_place_order.post(
            params=BybitPlaceOrderPostParams(
                category=product_type,
                symbol=symbol,
                side=side,
                orderType=order_type,
                qty=quantity,
                marketUnit="baseCoin" if not quote_quantity else "quoteCoin",
                price=price,
                timeInForce=time_in_force,
                orderLinkId=client_order_id,
                isLeverage=int(is_leverage) if is_leverage is not None else None,
                reduceOnly=reduce_only,
                closeOnTrigger=close_on_trigger,
                tpslMode=tpsl_mode if product_type != BybitProductType.SPOT else None,
                triggerPrice=trigger_price,
                triggerDirection=trigger_direction,
                triggerBy=trigger_type,
                takeProfit=tp_trigger_price if product_type == BybitProductType.SPOT else None,
                stopLoss=sl_trigger_price if product_type == BybitProductType.SPOT else None,
                slTriggerBy=trigger_type if product_type != BybitProductType.SPOT else None,
                tpTriggerBy=trigger_type if product_type != BybitProductType.SPOT else None,
                tpLimitPrice=tp_limit_price if product_type != BybitProductType.SPOT else None,
                slLimitPrice=sl_limit_price if product_type != BybitProductType.SPOT else None,
                tpOrderType=tp_order_type,
                slOrderType=sl_order_type,
            ),
        )

    async def set_trading_stop(
        self,
        product_type: BybitProductType,
        symbol: str,
        take_profit: str | None = None,
        stop_loss: str | None = None,
        tp_order_type: BybitOrderType | None = None,
        sl_order_type: BybitOrderType | None = None,
        trigger_type: BybitTriggerType | None = None,
        trailing_offset: str | None = None,  # By price
        tpsl_mode: BybitTpSlMode | None = None,
        tp_quantity: str | None = None,
        sl_quantity: str | None = None,
        tp_limit_price: str | None = None,
        sl_limit_price: str | None = None,
        active_price: str | None = None,
    ) -> BybitSetTradingStopResponse:
        position_idx = BybitPositionIdx.ONE_WAY  # TODO
        return await self._endpoint_set_trading_stop.post(
            BybitSetTradingStopPostParams(
                category=product_type,
                symbol=symbol,
                positionIdx=position_idx,
                takeProfit=take_profit,
                stopLoss=stop_loss,
                trailingStop=trailing_offset,
                slTriggerBy=trigger_type if product_type != BybitProductType.SPOT else None,
                tpTriggerBy=trigger_type if product_type != BybitProductType.SPOT else None,
                activePrice=active_price,
                tpslMode=tpsl_mode,
                tpSize=tp_quantity,
                slSize=sl_quantity,
                tpLimitPrice=tp_limit_price,
                slLimitPrice=sl_limit_price,
                tpOrderType=tp_order_type,
                slOrderType=sl_order_type,
            ),
        )

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
    ) -> BybitCancelOrderResponse:
        response = await self._endpoint_cancel_order.post(
            BybitCancelOrderPostParams(
                category=product_type,
                symbol=symbol,
                orderId=venue_order_id,
                orderLinkId=client_order_id,
                orderFilter=order_filter,
            ),
        )
        return response

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

    async def batch_place_orders(
        self,
        product_type: BybitProductType,
        submit_orders: list[BybitBatchPlaceOrder],
    ) -> BybitBatchPlaceOrderResponse:
        response = await self._endpoint_batch_place_order.post(
            BybitBatchPlaceOrderPostParams(
                category=product_type,
                request=submit_orders,
            ),
        )
        return response

    async def batch_cancel_orders(
        self,
        product_type: BybitProductType,
        cancel_orders: list[BybitBatchCancelOrder],
    ) -> BybitBatchCancelOrderResponse:
        response = await self._endpoint_batch_cancel_order.post(
            BybitBatchCancelOrderPostParams(
                category=product_type,
                request=cancel_orders,
            ),
        )
        return response
