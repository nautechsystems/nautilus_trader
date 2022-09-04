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
import hmac
import json
import urllib.parse
from typing import Any, Dict, List, Optional

import msgspec
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
        key: Optional[str] = None,
        secret: Optional[str] = None,
        base_url: Optional[str] = None,
        subaccount: Optional[str] = None,
        us: bool = False,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
        )
        self._clock = clock
        self._key = key
        self._secret = secret
        self._base_url = base_url or self.BASE_URL
        self._subaccount = subaccount
        self._us = us
        if self._base_url == self.BASE_URL and us:
            self._base_url = self._base_url.replace("com", "us")
        self._ftx_header = "FTX" if not us else "FTXUS"

    @property
    def base_url(self) -> str:
        return self._base_url

    @property
    def api_key(self) -> str:
        return self._key

    @property
    def api_secret(self) -> str:
        return self._secret

    @staticmethod
    def _prepare_payload(payload: Dict[str, str]) -> Optional[str]:
        return json.dumps(payload, separators=(",", ":")) if payload else None

    @staticmethod
    def _url_encode(params: Dict[str, str]) -> str:
        return "?" + urllib.parse.urlencode(params) if params else ""

    async def _sign_request(
        self,
        http_method: str,
        url_path: str,
        payload: Dict[str, str] = None,
        params: Dict[str, Any] = None,
    ) -> Any:
        ts: int = self._clock.timestamp_ms()

        headers = {}
        query = self._url_encode(params)
        signature_payload: str = f"{ts}{http_method}/api/{url_path}{query}"
        if payload:
            signature_payload += self._prepare_payload(payload)
            headers["Content-Type"] = "application/json"

        signature = hmac.new(
            self._secret.encode(), signature_payload.encode(), "sha256"
        ).hexdigest()

        headers = {
            **headers,
            f"{self._ftx_header}-KEY": self._key,
            f"{self._ftx_header}-SIGN": signature,
            f"{self._ftx_header}-TS": str(ts),
        }

        if self._subaccount:
            headers[f"{self._ftx_header}-SUBACCOUNT"] = urllib.parse.quote(self._subaccount)

        return await self._send_request(
            http_method=http_method,
            url_path=url_path,
            headers=headers,
            payload=payload,
            params=params,
        )

    async def _send_request(
        self,
        http_method: str,
        url_path: str,
        headers: Dict[str, Any] = None,
        payload: Dict[str, str] = None,
        params: Dict[str, str] = None,
    ) -> Any:
        if payload is None:
            payload = {}
        # TODO(cs): Uncomment for development
        # print(f"{http_method} {url_path} {headers} {payload}")
        query = self._url_encode(params)
        try:
            resp: ClientResponse = await self.request(
                method=http_method,
                url=self._base_url + url_path + query,
                headers=headers,
                data=self._prepare_payload(payload),
            )
        except ClientResponseError as e:
            await self._handle_exception(e)
            return

        try:
            data = msgspec.json.decode(resp.data)
            if not data["success"]:
                return data["error"]
            return data["result"]
        except msgspec.MsgspecError:
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

    async def get_trades(self, market: str) -> List[Dict[str, Any]]:
        return await self._send_request(
            http_method="GET",
            url_path=f"markets/{market}/trades",
        )

    async def get_historical_prices(
        self,
        market: str,
        resolution: int,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
    ):
        params: Dict[str, str] = {"resolution": str(resolution)}
        if start_time is not None:
            params["start_time"] = str(start_time)
        if end_time is not None:
            params["end_time"] = str(end_time)
        return await self._send_request(
            http_method="GET",
            url_path=f"markets/{market}/candles",
            params=params,
        )

    async def get_orderbook(self, market: str, depth: int = None) -> Dict[str, Any]:
        payload: Dict[str, str] = {}
        if depth is not None:
            payload = {"depth": str(depth)}

        return await self._send_request(
            http_method="GET",
            url_path=f"markets/{market}/orderbook",
            payload=payload,
        )

    async def get_account_info(self) -> Dict[str, Any]:
        return await self._sign_request(http_method="GET", url_path="account")

    async def list_futures(self) -> List[Dict[str, Any]]:
        return await self._send_request(http_method="GET", url_path="futures")

    async def get_market(self, market: str) -> Dict[str, Any]:
        return await self._send_request(http_method="GET", url_path=f"markets/{market}")

    async def list_markets(self) -> List[Dict[str, Any]]:
        return await self._send_request(http_method="GET", url_path="markets")

    async def get_open_orders(self, market: str = None) -> List[Dict[str, Any]]:
        return await self._sign_request(
            http_method="GET",
            url_path="orders",
            payload={"market": market},
        )

    async def get_open_trigger_orders(self, market: str = None) -> List[Dict[str, Any]]:
        return await self._sign_request(
            http_method="GET",
            url_path="conditional_orders",
            payload={"market": market},
        )

    async def get_order_history(
        self,
        market: str = None,
        side: str = None,
        order_type: str = None,
        start_time: int = None,
        end_time: int = None,
    ) -> List[Dict[str, Any]]:
        payload: Dict[str, str] = {}
        if market is not None:
            payload["market"] = market
        if side is not None:
            payload["side"] = side
        if order_type is not None:
            payload["orderType"] = order_type
        if start_time is not None:
            payload["start_time"] = str(start_time)
        if end_time is not None:
            payload["end_time"] = str(end_time)
        return await self._sign_request(
            http_method="GET",
            url_path="orders/history",
            payload=payload,
        )

    async def get_trigger_order_history(
        self,
        market: str = None,
        side: str = None,
        type: str = None,  # stop, trailing_stop, and take_profit
        order_type: str = None,  # market or limit
        start_time: float = None,
        end_time: float = None,
    ) -> List[Dict[str, Any]]:
        payload: Dict[str, str] = {}
        if market is not None:
            payload["market"] = market
        if side is not None:
            payload["side"] = side
        if type is not None:
            payload["type"] = type
        if order_type is not None:
            payload["orderType"] = order_type
        if start_time is not None:
            payload["start_time"] = str(start_time)
        if end_time is not None:
            payload["end_time"] = str(end_time)
        return await self._sign_request(
            http_method="GET",
            url_path="conditional_orders/history",
            payload=payload,
        )

    async def get_trigger_order_triggers(self, order_id: str) -> Dict[str, Any]:
        return await self._sign_request(
            http_method="GET",
            url_path=f"conditional_orders/{order_id}/triggers",
        )

    async def get_order_status(self, order_id: str) -> Dict[str, Any]:
        return await self._sign_request(
            http_method="GET",
            url_path=f"orders/{order_id}",
        )

    async def get_order_status_by_client_id(self, client_order_id: str) -> Dict[str, Any]:
        return await self._sign_request(
            http_method="GET",
            url_path=f"orders/by_client_id/{client_order_id}",
        )

    async def modify_order(
        self,
        client_order_id: str,
        price: Optional[str] = None,
        size: Optional[str] = None,
    ) -> dict:
        payload: Dict[str, str] = {}
        if price is not None:
            payload["price"] = price
        if size is not None:
            payload["size"] = size

        return await self._sign_request(
            http_method="POST",
            url_path=f"orders/by_client_id/{client_order_id}/modify",
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
        order_type: str,
        client_id: str = None,
        price: Optional[str] = None,
        ioc: bool = False,
        reduce_only: bool = False,
        post_only: bool = False,
    ) -> Dict[str, Any]:
        payload: Dict[str, Any] = {
            "market": market,
            "side": side,
            "price": price,
            "type": order_type,
            "size": size,
            "ioc": ioc,
            "reduceOnly": reduce_only,
            "postOnly": post_only,
            "clientId": client_id,
        }

        return await self._sign_request(
            http_method="POST",
            url_path="orders",
            payload=payload,
        )

    async def place_trigger_order(
        self,
        market: str,
        side: str,
        size: str,
        order_type: str,
        client_id: str = None,
        price: Optional[str] = None,
        trigger_price: Optional[str] = None,
        trail_value: Optional[str] = None,
        reduce_only: bool = False,
    ) -> Dict[str, Any]:
        """
        To place a Stop-Market order, set type='stop' and supply a trigger_price
        To place a Stop-Limit order, also supply a limit_price
        To place a Take-Profit Market order, set type='trailing_stop' and supply a trigger_price
        To place a Trailing-Stop order, set type='trailing_stop' and supply a trail_value
        """
        # assert order_type in ("stop", "take_profit", "trailing_stop")
        # assert (
        #         order_type not in ("stop", "take_profit") or trigger_price is not None
        # ), "Need trigger prices for stop losses and take profits"
        # assert order_type not in ("trailing_stop",) or (
        #     trigger_price is None and trail_value is not None
        # ), "Trailing stops need a trail value and cannot take a trigger price"
        payload: Dict[str, Any] = {
            "market": market,
            "side": side,
            "size": size,
            "type": order_type,
            "clientId": client_id,
            "reduceOnly": reduce_only,
        }
        if price is not None:
            payload["orderPrice"] = price
        if trigger_price is not None:
            payload["triggerPrice"] = trigger_price
        if trail_value is not None:
            payload["trailValue"] = trail_value
        return await self._sign_request(
            http_method="POST",
            url_path="conditional_orders",
            payload=payload,
        )

    async def cancel_order(self, order_id: str) -> Dict[str, Any]:
        return await self._sign_request(
            http_method="DELETE",
            url_path=f"orders/{order_id}",
        )

    async def cancel_order_by_client_id(self, client_order_id: str) -> Dict[str, Any]:
        return await self._sign_request(
            http_method="DELETE",
            url_path=f"orders/by_client_id/{client_order_id}",
        )

    async def cancel_open_trigger_order(self, trigger_id: str) -> str:
        return await self._sign_request(
            http_method="DELETE",
            url_path=f"conditional_orders/{trigger_id}",
        )

    async def cancel_all_orders(self, market: str) -> Dict[str, Any]:
        return await self._sign_request(
            http_method="DELETE",
            url_path="orders",
            payload={"market": market},
        )

    async def get_fills(
        self,
        market: Optional[str] = "ETH-PERP",
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
    ) -> List[dict]:
        payload: Dict[str, Any] = {}
        if market is not None:
            payload["market"] = market
        if start_time is not None:
            payload["start_time"] = str(start_time)
        if end_time is not None:
            payload["end_time"] = str(end_time)
        return await self._sign_request(
            http_method="GET",
            url_path="fills",
            payload=payload,
        )

    async def get_positions(self, show_avg_price: bool = False) -> List[dict]:
        return await self._sign_request(
            http_method="GET",
            url_path="positions",
            params={"showAvgPrice": show_avg_price},
        )

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
            payload: Dict[str, Any] = {}
            if start_time is not None:
                payload["start_time"] = str(start_time)
            if end_time is not None:
                payload["end_time"] = str(end_time)
            response = await self._send_request(
                http_method="GET",
                url_path=f"markets/{market}/trades",
                payload=payload,
            )
            deduped_trades = [r for r in response if r["id"] not in ids]
            results.extend(deduped_trades)
            ids |= {r["id"] for r in deduped_trades}
            # print(f"Adding {len(response)} trades with end time {end_time}")
            if len(response) == 0:
                break
            end_time = min(pd.Timestamp(t["time"]) for t in response).timestamp()
            if len(response) < limit:
                break
        return results
