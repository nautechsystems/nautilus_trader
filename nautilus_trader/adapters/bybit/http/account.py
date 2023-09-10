from typing import Optional

from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.endpoints.account.wallet_balance import BybitWalletBalanceEndpoint
from nautilus_trader.adapters.bybit.schemas.account import BybitWalletBalance
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.endpoints.position.position_info import BybitPositionInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.open_orders import BybitOpenOrdersHttp
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.order import BybitOrder
from nautilus_trader.adapters.bybit.schemas.position import BybitPositionStruct
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.core.correctness import PyCondition

from nautilus_trader.adapters.bybit.utils import get_category_from_account_type


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
        self._endpoint_position_info = BybitPositionInfoEndpoint(client, self.base_endpoint)
        self._endpoint_open_orders = BybitOpenOrdersHttp(client, self.base_endpoint)
        self._endpoint_wallet_balance = BybitWalletBalanceEndpoint(client, self.base_endpoint)

    async def query_position_info(
        self,
        symbol: Optional[str] = None,
    ) -> list[BybitPositionStruct]:
        # symbol = 'USD'
        response = await self._endpoint_position_info._get(
            parameters=self._endpoint_position_info.GetParameters(
                symbol=BybitSymbol(symbol) if symbol else None,
                settleCoin=self.default_settle_coin if symbol is None else None,
                category=get_category_from_account_type(self.account_type),
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
                category=get_category_from_account_type(self.account_type),
                settleCoin=self.default_settle_coin if symbol is None else None,
            ),
        )
        return response.result.list

    async def query_wallet_balance(
        self,
        coin: Optional[str] = None,
    ) -> list[BybitWalletBalance]:
        response = await self._endpoint_wallet_balance._get(
            parameters=self._endpoint_wallet_balance.GetaParameters(
                accountType='UNIFIED',
            ),
        )
        return response.result.list




