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
"""
Define the account HTTP API endpoints.
"""

import datetime

from nautilus_trader.adapters.dydx.common.enums import DYDXMarketType
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderSide
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderType
from nautilus_trader.adapters.dydx.common.enums import DYDXPerpetualPositionStatus
from nautilus_trader.adapters.dydx.endpoints.account.address import DYDXGetAddressEndpoint
from nautilus_trader.adapters.dydx.endpoints.account.address import DYDXGetAddressGetParams

# fmt: off
from nautilus_trader.adapters.dydx.endpoints.account.asset_positions import DYDXGetAssetPositionsEndpoint
from nautilus_trader.adapters.dydx.endpoints.account.asset_positions import DYDXGetAssetPositionsGetParams
from nautilus_trader.adapters.dydx.endpoints.account.fills import DYDXGetFillsEndpoint
from nautilus_trader.adapters.dydx.endpoints.account.fills import DYDXGetFillsGetParams
from nautilus_trader.adapters.dydx.endpoints.account.order import DYDXGetOrderEndpoint
from nautilus_trader.adapters.dydx.endpoints.account.order import DYDXGetOrderGetParams
from nautilus_trader.adapters.dydx.endpoints.account.orders import DYDXGetOrdersEndpoint
from nautilus_trader.adapters.dydx.endpoints.account.orders import DYDXGetOrdersGetParams
from nautilus_trader.adapters.dydx.endpoints.account.perpetual_positions import DYDXGetPerpetualPositionsEndpoint
from nautilus_trader.adapters.dydx.endpoints.account.perpetual_positions import DYDXGetPerpetualPositionsGetParams
from nautilus_trader.adapters.dydx.endpoints.account.subaccount import DYDXGetSubaccountEndpoint
from nautilus_trader.adapters.dydx.endpoints.account.subaccount import DYDXGetSubaccountGetParams
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.adapters.dydx.schemas.account.address import DYDXAddressResponse
from nautilus_trader.adapters.dydx.schemas.account.address import DYDXSubaccountResponse
from nautilus_trader.adapters.dydx.schemas.account.asset_positions import DYDXAssetPositionsResponse
from nautilus_trader.adapters.dydx.schemas.account.fills import DYDXFillsResponse
from nautilus_trader.adapters.dydx.schemas.account.orders import DYDXOrderResponse
from nautilus_trader.adapters.dydx.schemas.account.perpetual_positions import DYDXPerpetualPositionsResponse

# fmt: on
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


class DYDXAccountHttpAPI:
    """
    Define the account HTTP API endpoints.
    """

    def __init__(
        self,
        client: DYDXHttpClient,
        clock: LiveClock,
    ) -> None:
        """
        Define the account HTTP API endpoints.
        """
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock

        self._endpoint_get_address = DYDXGetAddressEndpoint(client)
        self._endpoint_get_subaccount = DYDXGetSubaccountEndpoint(client)
        self._endpoint_get_asset_positions = DYDXGetAssetPositionsEndpoint(client)
        self._endpoint_get_perpetual_positions = DYDXGetPerpetualPositionsEndpoint(client)
        self._endpoint_get_orders = DYDXGetOrdersEndpoint(client)
        self._endpoint_get_order = DYDXGetOrderEndpoint(client)
        self._endpoint_get_fills = DYDXGetFillsEndpoint(client)

    async def get_address_subaccounts(
        self,
        address: str,
    ) -> DYDXAddressResponse | None:
        """
        Fetch the address subaccounts.
        """
        return await self._endpoint_get_address.get(DYDXGetAddressGetParams(address=address))

    async def get_subaccount(
        self,
        address: str,
        subaccount_number: int,
    ) -> DYDXSubaccountResponse | None:
        """
        Fetch the subaccount.
        """
        return await self._endpoint_get_subaccount.get(
            DYDXGetSubaccountGetParams(
                address=address,
                subaccountNumber=subaccount_number,
            ),
        )

    async def get_asset_positions(
        self,
        address: str,
        subaccount_number: int,
    ) -> DYDXAssetPositionsResponse | None:
        """
        Fetch the asset positions.
        """
        return await self._endpoint_get_asset_positions.get(
            DYDXGetAssetPositionsGetParams(
                address=address,
                subaccountNumber=subaccount_number,
            ),
        )

    async def get_perpetual_positions(
        self,
        address: str,
        subaccount_number: int,
        status: list[DYDXPerpetualPositionStatus] | None = None,
        limit: int | None = None,
        created_before_or_at: datetime.datetime | None = None,
    ) -> DYDXPerpetualPositionsResponse | None:
        """
        Fetch the perpetual positions.
        """
        return await self._endpoint_get_perpetual_positions.get(
            DYDXGetPerpetualPositionsGetParams(
                address=address,
                subaccountNumber=subaccount_number,
                status=status,
                limit=limit,
                createdBeforeOrAt=created_before_or_at,
            ),
        )

    async def get_orders(
        self,
        address: str,
        subaccount_number: int,
        limit: int | None = None,
        symbol: str | None = None,
        order_side: DYDXOrderSide | None = None,
        order_type: DYDXOrderType | None = None,
        order_status: list[DYDXOrderStatus] | None = None,
        return_latest_orders: bool | None = None,
    ) -> list[DYDXOrderResponse] | None:
        """
        Fetch the orders.
        """
        return await self._endpoint_get_orders.get(
            DYDXGetOrdersGetParams(
                address=address,
                subaccountNumber=subaccount_number,
                limit=limit,
                ticker=symbol,
                side=order_side,
                type=order_type,
                status=order_status,
                returnLatestOrders=return_latest_orders,
            ),
        )

    async def get_order(
        self,
        address: str,
        subaccount_number: int,
        order_id: str,
    ) -> DYDXOrderResponse | None:
        """
        Fetch a specific order.
        """
        return await self._endpoint_get_order.get(
            order_id=order_id,
            params=DYDXGetOrderGetParams(
                address=address,
                subaccountNumber=subaccount_number,
            ),
        )

    async def get_fills(
        self,
        address: str,
        subaccount_number: int,
        symbol: str | None = None,
        market_type: DYDXMarketType | None = None,
        limit: int | None = None,
        created_before_or_at: datetime.datetime | None = None,
        page: int | None = None,
    ) -> DYDXFillsResponse | None:
        """
        Fetch the fills.
        """
        return await self._endpoint_get_fills.get(
            DYDXGetFillsGetParams(
                address=address,
                subaccountNumber=subaccount_number,
                market=symbol,
                marketType=market_type,
                limit=limit,
                createdBeforeOrAt=created_before_or_at,
                page=page,
            ),
        )
