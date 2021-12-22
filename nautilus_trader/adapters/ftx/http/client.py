# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
import hmac
import urllib.parse
from typing import Any, Dict, List, Optional

import orjson
import pandas as pd
from aiohttp import ClientResponse
from aiohttp import ClientResponseError

from nautilus_trader.adapters.ftx.http.error import FTXClientError
from nautilus_trader.adapters.ftx.http.error import FTXServerError
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.network.http import HttpClient


class FTXHttpClient(HttpClient):
    """
    Provides an `FTX` asynchronous HTTP client.
    """

    BASE_URL = "https://ftx.com/api/"

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        key=None,
        secret=None,
        base_url=None,
        subaccount_name=None,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
        )
        self._clock = clock
        self._key = key
        self._secret = secret
        self._base_url = base_url or self.BASE_URL
        self._subaccount_name = subaccount_name

    @property
    def api_key(self) -> str:
        return self._key

    def _prepare_params(self, params: Dict[str, str]) -> str:
        return "&".join([k + "=" + v for k, v in params.items()])

    async def _sign_request(
        self,
        http_method: str,
        url_path: str,
        payload: Dict[str, str] = None,
    ) -> Any:
        ts: int = self._clock.timestamp_ms()
        signature_payload: str = f"{ts}{http_method}/api/{url_path}"
        if payload:
            signature_payload += "?" + self._prepare_params(payload)
        signature = hmac.new(
            self._secret.encode(), signature_payload.encode(), "sha256"
        ).hexdigest()
        headers = {
            "FTX-KEY": self._key,
            "FTX-SIGN": signature,
            "FTX-TS": str(ts),
        }

        if self._subaccount_name:
            headers["FTX-SUBACCOUNT"] = urllib.parse.quote(self._subaccount_name)

        return await self._send_request(
            http_method=http_method,
            url_path=url_path,
            headers=headers,
            payload=payload,
        )

    async def _send_request(
        self,
        http_method: str,
        url_path: str,
        headers: Dict[str, Any] = None,
        payload: Dict[str, str] = None,
    ) -> Any:
        # TODO(cs): Uncomment for development
        print(f"{http_method} {url_path} {headers} {payload}")
        if payload is None:
            payload = {}
        try:
            resp: ClientResponse = await self.request(
                method=http_method,
                url=self._base_url + url_path,
                headers=headers,
                params=self._prepare_params(payload),
            )
        except ClientResponseError as ex:
            await self._handle_exception(ex)
            return

        try:
            data = orjson.loads(resp.data)
            if not data["success"]:
                return data["error"]
            return data["result"]
        except orjson.JSONDecodeError:
            self._log.error(f"Could not decode data to JSON: {resp.data}.")

    async def _handle_exception(self, error: ClientResponseError) -> None:
        if error.status < 400:
            return
        elif 400 <= error.status < 500:
            raise FTXClientError(
                status=error.status,
                message=error.message,
                headers=error.headers,
            )
        else:
            raise FTXServerError(
                status=error.status,
                message=error.message,
                headers=error.headers,
            )

    async def list_futures(self) -> List[dict]:
        return await self._send_request(http_method="GET", url_path="futures")

    async def list_markets(self) -> List[dict]:
        return await self._send_request(http_method="GET", url_path="markets")

    async def get_orderbook(self, market: str, depth: int = None) -> dict:
        payload: Dict[str, str] = {}
        if depth is not None:
            payload = {"depth": str(depth)}

        return await self._send_request(
            http_method="GET",
            url_path=f"markets/{market}/orderbook",
            payload=payload,
        )

    async def get_trades(self, market: str) -> dict:
        return await self._send_request(
            http_method="GET",
            url_path=f"markets/{market}/trades",
        )

    async def get_account_info(self) -> Dict[str, Any]:
        return await self._sign_request(http_method="GET", url_path="account")

    async def get_open_orders(self, market: str = None) -> List[dict]:
        return await self._sign_request(
            http_method="GET",
            url_path="orders",
            payload={"market": market},
        )

    async def get_order_history(
        self,
        market: str = None,
        side: str = None,
        order_type: str = None,
        start_time: int = None,
        end_time: int = None,
    ) -> List[dict]:
        payload: Dict[str, str] = {}
        if market is not None:
            payload["market"] = market
        if side is not None:
            payload["side"] = side
        if order_type is not None:
            payload["order_type"] = order_type
        if start_time is not None:
            payload["start_time"] = str(start_time)
        if end_time is not None:
            payload["end_time"] = str(end_time)
        return await self._sign_request(
            http_method="GET",
            url_path="orders/history",
            payload=payload,
        )

    async def get_conditional_order_history(
        self,
        market: str = None,
        side: str = None,
        type: str = None,
        order_type: str = None,
        start_time: float = None,
        end_time: float = None,
    ) -> List[dict]:
        payload: Dict[str, str] = {}
        if market is not None:
            payload["market"] = market
        if side is not None:
            payload["side"] = side
        if type is not None:
            payload["type"] = type
        if order_type is not None:
            payload["order_type"] = order_type
        if start_time is not None:
            payload["start_time"] = str(start_time)
        if end_time is not None:
            payload["end_time"] = str(end_time)

        return await self._sign_request(
            http_method="GET",
            url_path="conditional_orders/history",
            payload=payload,
        )

    async def modify_order(
        self,
        existing_order_id: Optional[str] = None,
        existing_client_order_id: Optional[str] = None,
        price: Optional[str] = None,
        size: Optional[str] = None,
        client_order_id: Optional[str] = None,
    ) -> dict:
        assert (existing_order_id is None) ^ (
            existing_client_order_id is None
        ), "Must supply exactly one ID for the order to modify"
        assert (price is None) or (size is None), "Must modify price or size of order"

        url_path = (
            f"orders/{existing_order_id}/modify"
            if existing_order_id is not None
            else f"orders/by_client_id/{existing_client_order_id}/modify"
        )

        payload: Dict[str, str] = {}
        if price is not None:
            payload["price"] = price
        if size is not None:
            payload["size"] = size
        if client_order_id is not None:
            payload["client_order_id"] = client_order_id

        return await self._sign_request(
            http_method="POST",
            url_path=url_path,
            payload=payload,
        )

    async def get_conditional_orders(self, market: str = None) -> List[dict]:
        payload: Dict[str, str] = {}
        if market is not None:
            payload["price"] = market

        return await self._sign_request(
            http_method="GET",
            url_path="conditional_orders",
            payload=payload,
        )

    async def place_order(
        self,
        market: str,
        side: str,
        size: str,
        type: str,
        client_id: str,
        price: str = None,
        reduce_only: bool = False,
        ioc: bool = False,
        post_only: bool = False,
    ) -> dict:
        payload: Dict[str, Any] = {
            "market": market,
            "side": side,
            "size": size,
            "type": type,
            "reduce_only": reduce_only,
            "ioc": ioc,
            "post_only": post_only,
            "client_id": client_id,
        }
        if price is not None:
            payload["price"] = price

        return await self._sign_request(
            http_method="POST",
            url_path="orders",
            payload=payload,
        )

    async def place_conditional_order(
        self,
        market: str,
        side: str,
        size: float,
        type: str = "stop",
        limit_price: float = None,
        reduce_only: bool = False,
        cancel: bool = True,
        trigger_price: float = None,
        trail_value: float = None,
    ) -> dict:
        """
        To send a Stop Market order, set type='stop' and supply a trigger_price
        To send a Stop Limit order, also supply a limit_price
        To send a Take Profit Market order, set type='trailing_stop' and supply a trigger_price
        To send a Trailing Stop order, set type='trailing_stop' and supply a trail_value
        """
        assert type in ("stop", "take_profit", "trailing_stop")
        assert (
            type not in ("stop", "take_profit") or trigger_price is not None
        ), "Need trigger prices for stop losses and take profits"
        assert type not in ("trailing_stop",) or (
            trigger_price is None and trail_value is not None
        ), "Trailing stops need a trail value and cannot take a trigger price"

        return await self._post(
            "conditional_orders",
            {
                "market": market,
                "side": side,
                "triggerPrice": trigger_price,
                "size": size,
                "reduceOnly": reduce_only,
                "type": "stop",
                "cancelLimitOnTrigger": cancel,
                "orderPrice": limit_price,
            },
        )

    async def cancel_order(self, order_id: str) -> dict:
        return await self._sign_request(
            http_method="DELETE",
            url_path=f"orders/{order_id}",
        )

    async def cancel_orders(
        self,
        market_name: str = None,
        conditional_orders: bool = False,
        limit_orders: bool = False,
    ) -> dict:
        payload: Dict[str, Any] = {
            "conditional_orders": conditional_orders,
            "limit_orders": limit_orders,
        }
        if market_name is not None:
            payload["market_name"] = market_name

        return await self._sign_request(
            http_method="DELETE",
            url_path="orders",
            payload=payload,
        )

    async def get_fills(self) -> List[dict]:
        return await self._get("fills")

    async def get_balances(self) -> List[dict]:
        return await self._get("wallet/balances")

    async def get_deposit_address(self, ticker: str) -> dict:
        return await self._get(f"wallet/deposit_address/{ticker}")

    async def get_positions(self, show_avg_price: bool = False) -> List[dict]:
        return await self._get("positions", {"showAvgPrice": show_avg_price})

    async def get_position(self, name: str, show_avg_price: bool = False) -> dict:
        positions = await self.get_positions(show_avg_price)
        return next(filter(lambda x: x["future"] == name, positions), None)

    async def get_all_trades(
        self, market: str, start_time: float = None, end_time: float = None
    ) -> List:
        ids = set()
        limit = 100
        results = []
        while True:
            response = await self._get(
                f"markets/{market}/trades",
                {
                    "end_time": end_time,
                    "start_time": start_time,
                },
            )
            deduped_trades = [r for r in response if r["id"] not in ids]
            results.extend(deduped_trades)
            ids |= {r["id"] for r in deduped_trades}
            print(f"Adding {len(response)} trades with end time {end_time}")
            if len(response) == 0:
                break
            end_time = min(pd.Timestamp(t["time"]) for t in response).timestamp()
            if len(response) < limit:
                break
        return results
