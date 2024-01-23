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

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.endpoints.account.fee_rate import BybitFeeRateEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.fee_rate import BybitFeeRateGetParameters
from nautilus_trader.adapters.bybit.endpoints.account.position_info import BybitPositionInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.position_info import PositionInfoGetParameters

# fmt: off
from nautilus_trader.adapters.bybit.endpoints.account.wallet_balance import BybitWalletBalanceEndpoint
from nautilus_trader.adapters.bybit.endpoints.account.wallet_balance import BybitWalletBalanceGetParameters
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_all_orders import BybitCancelAllOrdersEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_all_orders import BybitCancelAllOrdersPostParameters

# fmt: on
from nautilus_trader.adapters.bybit.endpoints.trade.open_orders import BybitOpenOrdersGetParameters
from nautilus_trader.adapters.bybit.endpoints.trade.open_orders import BybitOpenOrdersHttp
from nautilus_trader.adapters.bybit.endpoints.trade.place_order import BybitPlaceOrderEndpoint
from nautilus_trader.adapters.bybit.endpoints.trade.place_order import BybitPlaceOrderGetParameters
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.account.balance import BybitWalletBalance
from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRate
from nautilus_trader.adapters.bybit.schemas.order import BybitOrder
from nautilus_trader.adapters.bybit.schemas.order import BybitPlaceOrder
from nautilus_trader.adapters.bybit.schemas.position import BybitPositionStruct
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.utils import get_category_from_instrument_type
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


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

        # endpoints
        self._endpoint_fee_rate = BybitFeeRateEndpoint(client, self.base_endpoint)
        self._endpoint_position_info = BybitPositionInfoEndpoint(client, self.base_endpoint)
        self._endpoint_open_orders = BybitOpenOrdersHttp(client, self.base_endpoint)
        self._endpoint_wallet_balance = BybitWalletBalanceEndpoint(client, self.base_endpoint)
        self._endpoint_order = BybitPlaceOrderEndpoint(client, self.base_endpoint)
        self._endpoint_cancel_all_orders = BybitCancelAllOrdersEndpoint(client, self.base_endpoint)

    async def fetch_fee_rate(
        self,
        instrument_type: BybitInstrumentType,
        symbol: str | None = None,
        base_coin: str | None = None,
    ) -> list[BybitFeeRate]:
        response = await self._endpoint_fee_rate.get(
            BybitFeeRateGetParameters(
                category=instrument_type,
                symbol=symbol,
                baseCoin=base_coin,
            ),
        )
        return response.result.list

    async def query_position_info(
        self,
        instrument_type: BybitInstrumentType,
        symbol: str | None = None,
    ) -> list[BybitPositionStruct]:
        # symbol = 'USD'
        response = await self._endpoint_position_info.get(
            PositionInfoGetParameters(
                symbol=BybitSymbol(symbol) if symbol else None,
                settleCoin=self.default_settle_coin if symbol is None else None,
                category=get_category_from_instrument_type(instrument_type),
            ),
        )
        return response.result.list

    # async def close_all_positions(self):
    #     all_positions = await self.query_position_info()
    #     for position in all_positions:
    #         print("Closing position: ")

    async def query_open_orders(
        self,
        instrument_type: BybitInstrumentType,
        symbol: str | None = None,
    ) -> list[BybitOrder]:
        response = await self._endpoint_open_orders.get(
            BybitOpenOrdersGetParameters(
                category=instrument_type,
                symbol=BybitSymbol(symbol) if symbol else None,
                settleCoin=self.default_settle_coin if symbol is None else None,
            ),
        )
        return response.result.list

    async def query_order(
        self,
        instrument_type: BybitInstrumentType,
        symbol: str,
        order_id: str,
    ) -> list[BybitOrder]:
        response = await self._endpoint_open_orders.get(
            BybitOpenOrdersGetParameters(
                category=instrument_type,
                symbol=BybitSymbol(symbol) if symbol else None,
                orderId=order_id,
            ),
        )
        return response.result.list

    async def cancel_all_orders(
        self,
        instrument_type: BybitInstrumentType,
        symbol: str,
    ):
        response = await self._endpoint_cancel_all_orders.post(
            BybitCancelAllOrdersPostParameters(
                category=get_category_from_instrument_type(instrument_type),
                symbol=BybitSymbol(symbol),
            ),
        )
        return response.result.list

    async def query_wallet_balance(
        self,
        coin: str | None = None,
    ) -> tuple[list[BybitWalletBalance], int]:
        response = await self._endpoint_wallet_balance.get(
            BybitWalletBalanceGetParameters(
                accountType="UNIFIED",
            ),
        )
        return response.result.list, response.time

    async def place_order(
        self,
        instrument_type: BybitInstrumentType,
        symbol: str,
        side: BybitOrderSide,
        order_type: BybitOrderType,
        time_in_force: BybitTimeInForce | None = None,
        quantity: str | None = None,
        price: str | None = None,
        order_id: str | None = None,
    ) -> BybitPlaceOrder:
        result = await self._endpoint_order.post(
            parameters=BybitPlaceOrderGetParameters(
                category=get_category_from_instrument_type(instrument_type),
                symbol=BybitSymbol(symbol),
                side=side,
                orderType=order_type,
                qty=quantity,
                price=price,
                orderLinkId=order_id,
            ),
        )
        return result.result
