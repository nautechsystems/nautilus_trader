from typing import Optional

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.endpoints.account.wallet_balance import BybitWalletBalanceEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.wallet_balance import WalletBalanceGetParameters
from nautilus_trader.adapters.bybit.endpoints.position.position_info import BybitPositionInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.position.position_info import PositionInfoGetParameters
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_all_orders import BybitCancelAllOrdersEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_all_orders import BybitCancelAllOrdersPostParameters
from nautilus_trader.adapters.bybit.endpoints.trade.open_orders import BybitOpenOrdersGetParameters
from nautilus_trader.adapters.bybit.endpoints.trade.open_orders import BybitOpenOrdersHttp
from nautilus_trader.adapters.bybit.endpoints.trade.place_order import BybitPlaceOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.place_order import BybitPlaceOrderGetParameters
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.account.balance import BybitWalletBalance
from nautilus_trader.adapters.bybit.schemas.order import BybitOrder
from nautilus_trader.adapters.bybit.schemas.order import BybitPlaceOrder
from nautilus_trader.adapters.bybit.schemas.position import BybitPositionStruct
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.utils import get_category_from_instrument_type
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.core.correctness import PyCondition


class BybitAccountHttpAPI:
    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
        instrument_type: BybitInstrumentType,
    ):
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.instrument_type = instrument_type
        self.base_endpoint = "/v5/"
        self.default_settle_coin = "USDT"

        # endpoints
        self._endpoint_position_info = BybitPositionInfoEndpoint(client, self.base_endpoint)
        self._endpoint_open_orders = BybitOpenOrdersHttp(client, self.base_endpoint)
        self._endpoint_wallet_balance = BybitWalletBalanceEndpoint(client, self.base_endpoint)
        self._endpoint_order = BybitPlaceOrderEndpoint(client, self.base_endpoint)
        self._endpoint_cancel_all_orders = BybitCancelAllOrdersEndpoint(client, self.base_endpoint)

    async def query_position_info(
        self,
        symbol: Optional[str] = None,
    ) -> list[BybitPositionStruct]:
        # symbol = 'USD'
        response = await self._endpoint_position_info.get(
            PositionInfoGetParameters(
                symbol=BybitSymbol(symbol) if symbol else None,
                settleCoin=self.default_settle_coin if symbol is None else None,
                category=get_category_from_instrument_type(self.instrument_type),
            ),
        )
        return response.result.list

    # async def close_all_positions(self):
    #     all_positions = await self.query_position_info()
    #     for position in all_positions:
    #         print("Closing position: ")

    async def query_open_orders(
        self,
        symbol: Optional[str] = None,
    ) -> list[BybitOrder]:
        response = await self._endpoint_open_orders.get(
            BybitOpenOrdersGetParameters(
                category=get_category_from_instrument_type(self.instrument_type),
                symbol=BybitSymbol(symbol) if symbol else None,
                settleCoin=self.default_settle_coin if symbol is None else None,
            ),
        )
        return response.result.list

    async def query_order(
        self,
        symbol: str,
        order_id: str,
    ) -> list[BybitOrder]:
        response = await self._endpoint_open_orders.get(
            BybitOpenOrdersGetParameters(
                category=get_category_from_instrument_type(self.instrument_type),
                symbol=BybitSymbol(symbol) if symbol else None,
                orderId=order_id,
            ),
        )
        return response.result.list

    async def cancel_all_orders(
        self,
        symbol: str,
    ):
        response = await self._endpoint_cancel_all_orders.post(
            BybitCancelAllOrdersPostParameters(
                category=get_category_from_instrument_type(self.instrument_type),
                symbol=BybitSymbol(symbol),
            ),
        )
        return response.result.list

    async def query_wallet_balance(
        self,
        coin: Optional[str] = None,
    ) -> [list[BybitWalletBalance], int]:
        response = await self._endpoint_wallet_balance._get(
            WalletBalanceGetParameters(
                accountType="UNIFIED",
            ),
        )
        return [response.result.list, response.time]

    async def place_order(
        self,
        symbol: str,
        side: BybitOrderSide,
        order_type: BybitOrderType,
        time_in_force: Optional[BybitTimeInForce] = None,
        quantity: Optional[str] = None,
        price: Optional[str] = None,
        order_id: Optional[str] = None,
    ) -> BybitPlaceOrder:
        result = await self._endpoint_order.post(
            parameters=BybitPlaceOrderGetParameters(
                category=get_category_from_instrument_type(self.instrument_type),
                symbol=BybitSymbol(symbol),
                side=side,
                orderType=order_type,
                # timeInForce=time_in_force,
                qty=quantity,
                price=price,
                orderLinkId=order_id,
            ),
        )
        return result.result
