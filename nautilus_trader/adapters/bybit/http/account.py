from typing import Optional

from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.common.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.endpoints.position.position_info import BybitPositionInfoHttp
from nautilus_trader.adapters.bybit.endpoints.trade.open_orders import BybitOpenOrdersHttp
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.order import BybitOrder
from nautilus_trader.adapters.bybit.schemas.position import BybitPositionStruct
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.core.correctness import PyCondition


class BybitAccountHttpAPI:
    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
        account_type: BybitAccountType,
    ):
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.account_type = account_type
        self.base_endpoint = "/v5/"
        self.default_settle_coin = "USDT"

        # endpoints
        self._endpoint_position_info = BybitPositionInfoHttp(client, self.base_endpoint)
        self._endpoint_open_orders = BybitOpenOrdersHttp(client, self.base_endpoint)

    async def query_position_info(
        self,
        symbol: Optional[str] = None,
    ) -> list[BybitPositionStruct]:
        response = await self._endpoint_position_info._get(
            parameters=self._endpoint_position_info.GetParameters(
                symbol=BybitSymbol(symbol),
                category=self._get_category(),
            ),
        )
        return response.result.list

    async def query_open_orders(
        self,
        symbol: Optional[str] = None,
    ) -> list[BybitOrder]:
        response = await self._endpoint_open_orders._get(
            parameters=self._endpoint_open_orders.GetParameters(
                symbol=BybitSymbol(symbol) if symbol else None,
                category=self._get_category(),
                settleCoin=self.default_settle_coin if symbol is None else None,
            ),
        )
        return response.result.list

    def _get_category(self):
        if self.account_type == BybitAccountType.SPOT:
            return "spot"
        elif self.account_type == BybitAccountType.LINEAR:
            return "linear"
        else:
            raise ValueError(f"Unknown account type: {self.account_type}")
