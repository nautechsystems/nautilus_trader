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

import asyncio
import os
from typing import Any

import lighter

from nautilus_trader.adapters.lighter2.constants import LIGHTER_MAINNET_HTTP_URL
from nautilus_trader.adapters.lighter2.constants import LIGHTER_TESTNET_HTTP_URL
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpResponse


class LighterHttpClient:
    """
    Provides a HTTP client for the Lighter exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    api_key_private_key : str, optional
        The Lighter API private key.
    eth_private_key : str, optional
        The Ethereum private key for signing transactions.
    base_url : str, optional
        The base URL for the HTTP client.
    is_testnet : bool, default False
        If the client should connect to testnet.
    proxy_url : str, optional
        The proxy URL for the HTTP client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        api_key_private_key: str | None = None,
        eth_private_key: str | None = None,
        base_url: str | None = None,
        is_testnet: bool = False,
        proxy_url: str | None = None,
    ) -> None:
        self._loop = loop
        self._clock = clock
        self._log = logger

        # API credentials
        self._api_key_private_key = api_key_private_key or os.environ.get("LIGHTER_API_KEY_PRIVATE_KEY")
        self._eth_private_key = eth_private_key or os.environ.get("LIGHTER_ETH_PRIVATE_KEY")

        # Base URL configuration
        if base_url is None:
            base_url = LIGHTER_TESTNET_HTTP_URL if is_testnet else LIGHTER_MAINNET_HTTP_URL
        self._base_url = base_url
        self._is_testnet = is_testnet

        # HTTP client configuration
        proxy_headers = []
        if proxy_url:
            proxy_headers.append(("proxy-url", proxy_url))

        self._client = HttpClient(
            headers=proxy_headers,
            timeout_secs=30,
            keep_alive=True,
        )

        # Lighter SDK client
        self._lighter_client: lighter.ApiClient | None = None
        self._account_api: lighter.AccountApi | None = None
        self._order_api: lighter.OrderApi | None = None
        self._transaction_api: lighter.TransactionApi | None = None

        # Nonce management
        self._nonce = 0
        self._nonce_lock = asyncio.Lock()

    async def connect(self) -> None:
        """Connect to the Lighter API."""
        try:
            # Initialize Lighter SDK client
            self._lighter_client = lighter.ApiClient()
            
            # Configure base URL if needed
            if hasattr(self._lighter_client, 'configuration'):
                self._lighter_client.configuration.host = self._base_url

            # Initialize API clients
            self._account_api = lighter.AccountApi(self._lighter_client)
            self._order_api = lighter.OrderApi(self._lighter_client)
            self._transaction_api = lighter.TransactionApi(self._lighter_client)

            self._log.info(f"Connected to Lighter HTTP API: {self._base_url}")

        except Exception as e:
            self._log.error(f"Failed to connect to Lighter API: {e}")
            raise

    async def disconnect(self) -> None:
        """Disconnect from the Lighter API."""
        if self._lighter_client:
            try:
                await self._lighter_client.close()
                self._log.info("Disconnected from Lighter HTTP API")
            except Exception as e:
                self._log.error(f"Error disconnecting from Lighter API: {e}")
            finally:
                self._lighter_client = None
                self._account_api = None
                self._order_api = None
                self._transaction_api = None

    async def get_account(self, account_id: str | None = None) -> dict[str, Any]:
        """
        Get account information.

        Parameters
        ----------
        account_id : str, optional
            The account ID. If None, uses default account.

        Returns
        -------
        dict[str, Any]
            The account information.

        """
        if not self._account_api:
            raise RuntimeError("Client not connected")

        try:
            if account_id:
                account = await self._account_api.account(by="id", value=account_id)
            else:
                # Get first account by index
                account = await self._account_api.account(by="index", value="1")
            
            return account.to_dict() if hasattr(account, 'to_dict') else account

        except Exception as e:
            self._log.error(f"Error fetching account: {e}")
            raise

    async def get_order_book(self, instrument_id: str, depth: int = 20) -> dict[str, Any]:
        """
        Get order book for an instrument.

        Parameters
        ----------
        instrument_id : str
            The instrument identifier.
        depth : int, default 20
            The order book depth.

        Returns
        -------
        dict[str, Any]
            The order book data.

        """
        if not self._order_api:
            raise RuntimeError("Client not connected")

        try:
            orderbook = await self._order_api.order_book(
                instrument_id=instrument_id,
                depth=depth
            )
            return orderbook.to_dict() if hasattr(orderbook, 'to_dict') else orderbook

        except Exception as e:
            self._log.error(f"Error fetching order book for {instrument_id}: {e}")
            raise

    async def get_orders(self, instrument_id: str | None = None) -> list[dict[str, Any]]:
        """
        Get orders for account.

        Parameters
        ----------
        instrument_id : str, optional
            Filter by instrument ID.

        Returns
        -------
        list[dict[str, Any]]
            List of orders.

        """
        if not self._order_api:
            raise RuntimeError("Client not connected")

        try:
            if instrument_id:
                orders = await self._order_api.orders(instrument_id=instrument_id)
            else:
                orders = await self._order_api.orders()
            
            if hasattr(orders, '__iter__'):
                return [order.to_dict() if hasattr(order, 'to_dict') else order for order in orders]
            else:
                return [orders.to_dict() if hasattr(orders, 'to_dict') else orders]

        except Exception as e:
            self._log.error(f"Error fetching orders: {e}")
            raise

    async def place_order(
        self,
        instrument_id: str,
        side: str,
        order_type: str,
        quantity: str,
        price: str | None = None,
        **kwargs,
    ) -> dict[str, Any]:
        """
        Place an order.

        Parameters
        ----------
        instrument_id : str
            The instrument identifier.
        side : str
            The order side ('buy' or 'sell').
        order_type : str
            The order type ('limit', 'market', etc.).
        quantity : str
            The order quantity.
        price : str, optional
            The order price (required for limit orders).
        **kwargs
            Additional order parameters.

        Returns
        -------
        dict[str, Any]
            The order response.

        """
        if not self._transaction_api:
            raise RuntimeError("Client not connected")

        try:
            # Get next nonce
            async with self._nonce_lock:
                self._nonce += 1
                nonce = self._nonce

            # Prepare order data
            order_data = {
                'instrument_id': instrument_id,
                'side': side,
                'type': order_type,
                'quantity': quantity,
                'nonce': nonce,
                **kwargs
            }

            if price is not None:
                order_data['price'] = price

            # Submit order transaction
            response = await self._transaction_api.submit_transaction(order_data)
            return response.to_dict() if hasattr(response, 'to_dict') else response

        except Exception as e:
            self._log.error(f"Error placing order: {e}")
            raise

    async def cancel_order(self, order_id: str) -> dict[str, Any]:
        """
        Cancel an order.

        Parameters
        ----------
        order_id : str
            The order ID to cancel.

        Returns
        -------
        dict[str, Any]
            The cancellation response.

        """
        if not self._transaction_api:
            raise RuntimeError("Client not connected")

        try:
            # Get next nonce
            async with self._nonce_lock:
                self._nonce += 1
                nonce = self._nonce

            cancel_data = {
                'order_id': order_id,
                'nonce': nonce,
            }

            response = await self._transaction_api.cancel_order(cancel_data)
            return response.to_dict() if hasattr(response, 'to_dict') else response

        except Exception as e:
            self._log.error(f"Error cancelling order {order_id}: {e}")
            raise

    async def cancel_all_orders(self, instrument_id: str | None = None) -> dict[str, Any]:
        """
        Cancel all orders.

        Parameters
        ----------
        instrument_id : str, optional
            Cancel orders for specific instrument only.

        Returns
        -------
        dict[str, Any]
            The cancellation response.

        """
        if not self._transaction_api:
            raise RuntimeError("Client not connected")

        try:
            # Get next nonce
            async with self._nonce_lock:
                self._nonce += 1
                nonce = self._nonce

            cancel_data = {'nonce': nonce}
            if instrument_id:
                cancel_data['instrument_id'] = instrument_id

            response = await self._transaction_api.cancel_all_orders(cancel_data)
            return response.to_dict() if hasattr(response, 'to_dict') else response

        except Exception as e:
            self._log.error(f"Error cancelling all orders: {e}")
            raise

    async def request(
        self,
        method: str,
        endpoint: str,
        headers: dict[str, str] | None = None,
        body: bytes | None = None,
    ) -> HttpResponse:
        """
        Send a custom HTTP request.

        Parameters
        ----------
        method : str
            The HTTP method.
        endpoint : str
            The API endpoint.
        headers : dict[str, str], optional
            Additional headers.
        body : bytes, optional
            The request body.

        Returns
        -------
        HttpResponse
            The HTTP response.

        """
        url = f"{self._base_url}{endpoint}"
        
        request_headers = headers or {}
        
        # Add authentication headers if credentials are available
        if self._api_key_private_key:
            # Add authentication logic here based on Lighter's requirements
            pass

        return await self._client.request(
            method=method,
            url=url,
            headers=list(request_headers.items()),
            body=body,
        )