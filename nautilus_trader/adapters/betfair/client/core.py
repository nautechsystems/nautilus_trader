# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import datetime
import pathlib
import ssl
from typing import Dict, List, Optional

import msgspec
from aiohttp import ClientResponse
from aiohttp import ClientResponseError

from nautilus_trader.adapters.betfair.client.enums import MarketProjection
from nautilus_trader.adapters.betfair.client.enums import MarketSort
from nautilus_trader.adapters.betfair.client.exceptions import BetfairAPIError
from nautilus_trader.adapters.betfair.client.exceptions import BetfairError
from nautilus_trader.adapters.betfair.client.util import parse_params
from nautilus_trader.common.logging import Logger
from nautilus_trader.network.http import HttpClient


class BetfairClient(HttpClient):
    """
    Provides a HTTP client for `Betfair`.
    """

    IDENTITY_URL = "https://identitysso-cert.betfair.com/api/"
    BASE_URL = "https://api.betfair.com/exchange/"
    NAVIGATION_URL = BASE_URL + "betting/rest/v1/en/navigation/menu.json"
    ACCOUNT_URL = BASE_URL + "account/json-rpc/v1"
    BETTING_URL = BASE_URL + "betting/json-rpc/v1"
    JSON_RPC_DEFAULTS = {"jsonrpc": "2.0", "id": 1}

    def __init__(
        self,
        username: str,
        password: str,
        app_key: str,
        cert_dir: str,
        loop: asyncio.AbstractEventLoop,
        logger: Logger,
        ssl: bool = True,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
            ssl=ssl or self.ssl_context(cert_dir=cert_dir),
            connector_kwargs={"enable_cleanup_closed": True, "force_close": True},
        )
        self.username = username
        self.password = password
        self.app_key = app_key
        self.session_token: Optional[str] = None

    @property
    def headers(self):
        auth = {"X-Authentication": self.session_token} if self.session_token else {}
        return {
            "Accept-Encoding": "gzip, deflate",
            "Connection": "keep-alive",
            "content-type": "application/json",
            "X-Application": self.app_key,
            **auth,
        }

    @staticmethod
    def ssl_context(cert_dir):
        files = list(pathlib.Path(cert_dir).glob("*"))
        cert_file = next((p for p in files if p.suffix == ".crt"))
        key_file = next((p for p in files if p.suffix == ".key"))
        context = ssl.create_default_context()
        context.load_cert_chain(certfile=cert_file, keyfile=key_file)
        return context

    # For testing purposes, can't mock HttpClient.request due to cython
    async def request(self, method, url, **kwargs) -> ClientResponse:
        return await super().request(method=method, url=url, **kwargs)

    async def rpc_post(
        self, url, method, params: Optional[Dict] = None, data: Optional[Dict] = None
    ) -> Dict:
        data = {**self.JSON_RPC_DEFAULTS, "method": method, **(data or {}), "params": params or {}}
        try:
            resp = await self.request(method="POST", url=url, headers=self.headers, json=data)
            data = msgspec.json.decode(resp.data)
            if "error" in data:
                self._log.error(str(data))
                raise BetfairAPIError(code=data["error"]["code"], message=data["error"]["message"])
            if isinstance(data, dict):
                return data["result"]
            else:
                raise TypeError("Unexpected type:" + str(resp))
        except BetfairError as e:
            self._log.error(str(e))
            raise e
        except ClientResponseError as e:
            self._log.error(f"Err on {method} status={e.status}, message={str(e)}")
            raise e

    async def connect(self):
        await super().connect()
        await self.login()

    async def disconnect(self):
        self._log.info("Disconnecting..")
        self.session_token = None
        await super().disconnect()
        self._log.info("Disconnected.")

    async def login(self):
        self._log.debug("BetfairClient login")
        if self.session_token is not None:
            self._log.warning("Already logged in, returning")
            return
        url = self.IDENTITY_URL + "certlogin"
        data = {"username": self.username, "password": self.password}
        headers = {
            **{k: v for k, v in self.headers.items() if k not in ("content-type",)},
            **{"Content-Type": "application/x-www-form-urlencoded"},
        }
        resp = await self.post(url=url, data=data, headers=headers)
        data = msgspec.json.decode(resp.data)
        if data["loginStatus"] == "SUCCESS":
            self.session_token = data["sessionToken"]

    async def list_navigation(self):
        """
        List the tree (navigation) of all betfair markets.
        """
        resp = await self.get(url=self.NAVIGATION_URL, headers=self.headers)
        return msgspec.json.decode(resp.data)

    async def list_market_catalogue(
        self,
        filter_: dict,
        market_projection: List[MarketProjection] = None,
        sort: str = None,
        max_results: int = 1000,
        locale: str = None,
    ):
        """
        Return specific data about markets.
        """
        assert 0 < max_results <= 1000

        params = parse_params(**locals())

        if "marketProjection" in params:
            assert all([isinstance(m, MarketProjection) for m in params["marketProjection"]])
            params["marketProjection"] = [m.value for m in params["marketProjection"]]
        if "sort" in params:
            assert isinstance(sort, MarketSort)
        resp = await self.rpc_post(
            url=self.BETTING_URL, method="SportsAPING/v1.0/listMarketCatalogue", params=params
        )
        return resp

    async def get_account_details(self):
        resp = await self.rpc_post(
            url=self.ACCOUNT_URL, method="AccountAPING/v1.0/getAccountDetails"
        )
        return resp

    async def get_account_funds(self, wallet: Optional[str] = None):
        params = parse_params(**locals())
        resp = await self.rpc_post(
            url=self.ACCOUNT_URL, method="AccountAPING/v1.0/getAccountFunds", params=params
        )
        return resp

    async def place_orders(
        self,
        market_id: str,
        instructions: list,
        customer_ref: str = None,
        market_version: Optional[Dict] = None,
        customer_strategy_ref: str = None,
    ):
        """
        Place a new order.
        """
        params = parse_params(**locals())
        resp = await self.rpc_post(
            url=self.BETTING_URL, method="SportsAPING/v1.0/placeOrders", params=params
        )
        return resp

    async def replace_orders(
        self,
        market_id: str = None,
        instructions: list = None,
        customer_ref: str = None,
        market_version: Optional[Dict] = None,
    ):
        params = parse_params(**locals())
        resp = await self.rpc_post(
            url=self.BETTING_URL, method="SportsAPING/v1.0/replaceOrders", params=params
        )
        return resp

    async def cancel_orders(
        self,
        market_id: str = None,
        instructions: list = None,
        customer_ref: str = None,
    ):
        params = parse_params(**locals())
        resp = await self.rpc_post(
            url=self.BETTING_URL, method="SportsAPING/v1.0/cancelOrders", params=params
        )
        return resp

    async def list_current_orders(
        self,
        bet_ids: list = None,
        market_ids: list = None,
        order_projection: str = None,
        customer_order_refs: list = None,
        customer_strategy_refs: list = None,
        date_from: datetime.datetime = None,
        date_to: datetime.datetime = None,
        order_by: str = "BY_PLACE_TIME",
        sort_dir: str = None,
        from_record: int = None,
        record_count: int = None,
        include_item_description: bool = None,
    ) -> List[Dict]:
        params = parse_params(**locals())
        current_orders = []
        more_available = True
        index = from_record or 0
        while more_available:
            params["fromRecord"] = index
            resp = await self.rpc_post(
                url=self.BETTING_URL, method="SportsAPING/v1.0/listCurrentOrders", params=params
            )
            order_chunk = resp["currentOrders"]
            current_orders.extend(order_chunk)
            more_available = resp["moreAvailable"]
            index += len(order_chunk)
        return current_orders

    async def list_cleared_orders(
        self,
        bet_status: str = "SETTLED",
        event_type_ids: list = None,
        event_ids: list = None,
        market_ids: list = None,
        runner_ids: list = None,
        bet_ids: list = None,
        customer_order_refs: list = None,
        customer_strategy_refs: list = None,
        side: str = None,
        settled_date_from: datetime.datetime = None,
        settled_date_to: datetime.datetime = None,
        group_by: str = None,
        include_item_description: bool = None,
        locale: str = None,
        from_record: int = None,
        record_count: int = None,
    ) -> List[Dict]:
        params = parse_params(**locals())
        cleared_orders = []
        more_available = True
        index = from_record or 0
        while more_available:
            params["fromRecord"] = index
            if settled_date_from or settled_date_to:
                params["settledDateRange"] = {"from": settled_date_from, "to": settled_date_to}
            resp = await self.rpc_post(
                url=self.BETTING_URL, method="SportsAPING/v1.0/listClearedOrders", params=params
            )
            order_chunk = resp["clearedOrders"]
            cleared_orders.extend(order_chunk)
            more_available = resp["moreAvailable"]
            index += len(order_chunk)
        return cleared_orders
