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

from betfair_parser.endpoints import ENDPOINTS
from betfair_parser.spec.accounts.operations import GetAccountDetails
from betfair_parser.spec.accounts.operations import GetAccountFunds
from betfair_parser.spec.accounts.type_definitions import AccountDetailsResponse
from betfair_parser.spec.accounts.type_definitions import AccountFundsResponse
from betfair_parser.spec.betting.enums import BetStatus
from betfair_parser.spec.betting.enums import GroupBy
from betfair_parser.spec.betting.enums import MarketProjection
from betfair_parser.spec.betting.enums import MarketSort
from betfair_parser.spec.betting.enums import OrderBy
from betfair_parser.spec.betting.enums import OrderProjection
from betfair_parser.spec.betting.enums import Side
from betfair_parser.spec.betting.enums import SortDir
from betfair_parser.spec.betting.listings import ListMarketCatalogue
from betfair_parser.spec.betting.orders import CancelOrders
from betfair_parser.spec.betting.orders import ListClearedOrders
from betfair_parser.spec.betting.orders import ListCurrentOrders
from betfair_parser.spec.betting.orders import PlaceOrders
from betfair_parser.spec.betting.orders import ReplaceOrders
from betfair_parser.spec.betting.type_definitions import CancelExecutionReport
from betfair_parser.spec.betting.type_definitions import ClearedOrderSummary
from betfair_parser.spec.betting.type_definitions import ClearedOrderSummaryReport
from betfair_parser.spec.betting.type_definitions import CurrentOrderSummary
from betfair_parser.spec.betting.type_definitions import CurrentOrderSummaryReport
from betfair_parser.spec.betting.type_definitions import MarketCatalogue
from betfair_parser.spec.betting.type_definitions import MarketFilter
from betfair_parser.spec.betting.type_definitions import PlaceExecutionReport
from betfair_parser.spec.betting.type_definitions import ReplaceExecutionReport
from betfair_parser.spec.betting.type_definitions import RunnerId
from betfair_parser.spec.common import BetId
from betfair_parser.spec.common import CustomerOrderRef
from betfair_parser.spec.common import CustomerStrategyRef
from betfair_parser.spec.common import EventId
from betfair_parser.spec.common import EventTypeId
from betfair_parser.spec.common import MarketId
from betfair_parser.spec.common import Request
from betfair_parser.spec.common import TimeRange
from betfair_parser.spec.identity import KeepAlive
from betfair_parser.spec.identity import Login
from betfair_parser.spec.identity import LoginResponse
from betfair_parser.spec.identity import LoginStatus
from betfair_parser.spec.navigation import Menu
from betfair_parser.spec.navigation import Navigation

import nautilus_trader
from nautilus_trader.common.component import Logger
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.nautilus_pyo3 import HttpResponse
from nautilus_trader.core.rust.common import LogColor


class BetfairHttpClient:
    """
    Provides a HTTP client for Betfair.
    """

    def __init__(
        self,
        username: str,
        password: str,
        app_key: str,
    ) -> None:
        # Config
        self.username = username
        self.password = password
        self.app_key = app_key

        # Client
        self._client = HttpClient()
        self._headers: dict[str, str] = {}
        self._log = Logger(name=type(self).__name__)
        self.reset_headers()

    async def _request(self, method: HttpMethod, request: Request) -> HttpResponse:
        url = ENDPOINTS.url_for_request(request)
        headers = self._headers
        body = request.body()
        if isinstance(body, str):
            body = body.encode()
        self._log.debug(f"[REQ] {method} {url} {body.decode()}")
        response: HttpResponse = await self._client.request(
            method,
            url,
            headers=headers,
            body=body,
        )
        if url not in SKIP_LOG_URLS:
            self._log.debug(f"[RESP] {response.body.decode()}")
        return response

    async def _post(self, request: Request) -> Request.return_type:
        response: HttpResponse = await self._request(HttpMethod.POST, request)
        return request.parse_response(response.body, raise_errors=True)

    async def _get(self, request: Request) -> Request.return_type:
        response: HttpResponse = await self._request(HttpMethod.GET, request)
        return request.parse_response(response.body, raise_errors=True)

    @property
    def session_token(self) -> str | None:
        return self._headers.get("X-Authentication")

    def update_headers(self, login_resp: LoginResponse) -> None:
        self._headers.update(
            {
                "X-Authentication": login_resp.token,
                "X-Application": login_resp.product,
            },
        )

    def reset_headers(self) -> None:
        self._headers = {
            "Accept": "application/json",
            "Content-Type": "application/x-www-form-urlencoded",
            "User-Agent": nautilus_trader.NAUTILUS_USER_AGENT,
            "X-Application": self.app_key,
        }

    async def connect(self) -> None:
        if self.session_token is not None:
            self._log.warning("Session token exists (already connected), skipping")
            return

        self._log.info("Connecting (Betfair login)")
        request = Login.with_params(username=self.username, password=self.password)
        resp: LoginResponse = await self._post(request)
        if resp.status != LoginStatus.SUCCESS:
            raise RuntimeError(f"Login not successful: {resp.status.value}")
        self._log.info("Login success", color=LogColor.GREEN)
        self.update_headers(login_resp=resp)

    async def reconnect(self) -> None:
        self._log.info("Reconnecting...")
        self.reset_headers()
        await self.connect()

    async def disconnect(self) -> None:
        self._log.info("Disconnecting...")
        self.reset_headers()
        self._log.info("Disconnected", color=LogColor.GREEN)

    async def keep_alive(self) -> None:
        """
        Renew authentication.
        """
        resp: KeepAlive.return_type = await self._post(KeepAlive())
        if resp.status == "SUCCESS":
            self.update_headers(resp)

    async def list_navigation(self) -> Navigation:
        """
        List the tree (navigation) of all betfair markets.
        """
        navigation: Navigation = await self._get(request=Menu())
        return navigation

    async def list_market_catalogue(
        self,
        filter_: MarketFilter,
        market_projection: list[MarketProjection] | None = None,
        sort: MarketSort | None = None,
        max_results: int = 1000,
        locale: str | None = None,
    ) -> list[MarketCatalogue]:
        """
        Return specific data about markets.
        """
        assert 0 < max_results <= 1000
        resp: ListMarketCatalogue.return_type = await self._post(
            request=ListMarketCatalogue.with_params(
                filter=filter_,
                market_projection=market_projection,
                sort=sort,
                max_results=max_results,
                locale=locale,
            ),
        )
        return resp

    async def get_account_details(self) -> AccountDetailsResponse:
        return await self._post(request=GetAccountDetails.with_params())

    async def get_account_funds(self, wallet: str | None = None) -> AccountFundsResponse:
        return await self._post(request=GetAccountFunds.with_params(wallet=wallet))

    async def place_orders(self, request: PlaceOrders) -> PlaceExecutionReport:
        return await self._post(request)

    async def replace_orders(self, request: ReplaceOrders) -> ReplaceExecutionReport:
        return await self._post(request)

    async def cancel_orders(self, request: CancelOrders) -> CancelExecutionReport:
        return await self._post(request)

    async def list_current_orders(
        self,
        bet_ids: set[BetId] | None = None,
        market_ids: set[str] | None = None,
        order_projection: OrderProjection | None = None,
        customer_order_refs: set[CustomerOrderRef] | None = None,
        customer_strategy_refs: set[CustomerStrategyRef] | None = None,
        date_range: TimeRange | None = None,
        order_by: OrderBy | None = None,
        sort_dir: SortDir | None = None,
        from_record: int | None = None,
        record_count: int | None = None,
        include_item_description: bool | None = None,
    ) -> list[CurrentOrderSummary]:
        current_orders: list[CurrentOrderSummary] = []
        more_available = True
        index = from_record or 0
        while more_available:
            from_record = index
            request = ListCurrentOrders.with_params(
                bet_ids=bet_ids,
                market_ids=market_ids,
                order_projection=order_projection,
                customer_order_refs=customer_order_refs,
                customer_strategy_refs=customer_strategy_refs,
                date_range=date_range,
                order_by=order_by,
                sort_dir=sort_dir,
                from_record=from_record,
                record_count=record_count,
                include_item_description=include_item_description,
            )
            resp: CurrentOrderSummaryReport = await self._post(request=request)
            current_orders.extend(resp.current_orders)
            more_available = resp.more_available
            index = len(current_orders)
        return current_orders

    async def list_cleared_orders(
        self,
        bet_status: BetStatus,
        event_type_ids: set[EventTypeId] | None = None,
        event_ids: set[EventId] | None = None,
        market_ids: set[MarketId] | None = None,
        runner_ids: set[RunnerId] | None = None,
        bet_ids: set[BetId] | None = None,
        customer_order_refs: set[CustomerOrderRef] | None = None,
        customer_strategy_refs: set[CustomerStrategyRef] | None = None,
        side: Side | None = None,
        settled_date_range: TimeRange | None = None,
        group_by: GroupBy | None = None,
        include_item_description: bool | None = None,
        locale: str | None = None,
        from_record: int | None = None,
        record_count: int | None = None,
    ) -> list[ClearedOrderSummary]:
        cleared_orders: list[ClearedOrderSummary] = []
        more_available = True
        index = from_record or 0
        while more_available:
            from_record = index
            request = ListClearedOrders.with_params(
                bet_status=bet_status,
                event_type_ids=event_type_ids,
                event_ids=event_ids,
                market_ids=market_ids,
                runner_ids=runner_ids,
                bet_ids=bet_ids,
                customer_order_refs=customer_order_refs,
                customer_strategy_refs=customer_strategy_refs,
                side=side,
                settled_date_range=settled_date_range,
                group_by=group_by,
                include_item_description=include_item_description,
                locale=locale,
                from_record=from_record,
                record_count=record_count,
            )
            resp: ClearedOrderSummaryReport = await self._post(request=request)
            cleared_orders.extend(resp.cleared_orders)
            more_available = resp.more_available
            index = len(cleared_orders)
        return cleared_orders


SKIP_LOG_URLS = {
    "https://identitysso.betfair.com/api/login",
    "https://api.betfair.com/exchange/betting/rest/v1/en/navigation/menu.json",
    "https://api.betfair.com/exchange/betting/json-rpc/v1/SportsAPING/v1.0/listMarketCatalogue",
}
