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
"""Execution client for Coinbase."""

import asyncio
import json
from decimal import Decimal

import nautilus_pyo3
from nautilus_trader.adapters.coinbase.constants import COINBASE_VENUE
from nautilus_trader.adapters.coinbase.providers import CoinbaseInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder


class CoinbaseExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Coinbase Advanced Trade API.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.CoinbaseHttpClient
        The Coinbase HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : CoinbaseInstrumentProvider
        The instrument provider.
    ws_client : nautilus_pyo3.CoinbaseWebSocketClient, optional
        The WebSocket client for real-time updates.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.CoinbaseHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: CoinbaseInstrumentProvider,
        ws_client: nautilus_pyo3.CoinbaseWebSocketClient | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(COINBASE_VENUE.value),
            venue=COINBASE_VENUE,
            oms_type=None,  # Will be set by the engine
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._client = client
        self._ws_client = ws_client
        
        # WebSocket message handling task
        self._ws_task: asyncio.Task | None = None
        
        # Order tracking
        self._client_order_id_to_venue_order_id: dict[ClientOrderId, VenueOrderId] = {}

    async def _connect(self) -> None:
        """Connect to Coinbase."""
        self._log.info("Connecting to Coinbase execution...")

        # Load account information
        await self._update_account_state()

        # Connect WebSocket if available
        if self._ws_client:
            await self._ws_client.connect()
            # Subscribe to user channel for order updates
            await self._ws_client.subscribe([], "user", True)
            self._ws_task = self._loop.create_task(self._handle_ws_messages())
            self._log.info("WebSocket connected for execution")

        self._log.info("Connected to Coinbase execution")

    async def _disconnect(self) -> None:
        """Disconnect from Coinbase."""
        self._log.info("Disconnecting from Coinbase execution...")

        # Cancel WebSocket task
        if self._ws_task and not self._ws_task.done():
            self._ws_task.cancel()
            try:
                await self._ws_task
            except asyncio.CancelledError:
                pass

        # Disconnect WebSocket
        if self._ws_client:
            await self._ws_client.disconnect()
            self._log.info("WebSocket disconnected for execution")

        self._log.info("Disconnected from Coinbase execution")

    async def _handle_ws_messages(self) -> None:
        """Handle incoming WebSocket messages."""
        try:
            while True:
                message = await self._ws_client.receive_message()
                if message is None:
                    self._log.warning("WebSocket connection closed")
                    break

                try:
                    data = json.loads(message)
                    await self._handle_ws_message(data)
                except json.JSONDecodeError as e:
                    self._log.error(f"Failed to decode WebSocket message: {e}")
                except Exception as e:
                    self._log.error(f"Error handling WebSocket message: {e}")

        except asyncio.CancelledError:
            self._log.debug("WebSocket message handler cancelled")
        except Exception as e:
            self._log.error(f"WebSocket message handler error: {e}")

    async def _handle_ws_message(self, data: dict) -> None:
        """Handle a parsed WebSocket message."""
        channel = data.get("channel")
        
        if channel == "user":
            await self._handle_user_update(data)
        else:
            self._log.debug(f"Unhandled channel: {channel}")

    async def _handle_user_update(self, data: dict) -> None:
        """Handle user channel updates (orders, fills)."""
        # Order update handling would go here
        self._log.debug(f"User update: {data}")

    async def _update_account_state(self) -> None:
        """Update account state from Coinbase."""
        try:
            accounts_json = await self._client.list_accounts()
            accounts_data = json.loads(accounts_json)

            balances = []
            for account in accounts_data.get("accounts", []):
                currency_code = account.get("currency")
                available = account.get("available_balance", {}).get("value", "0")
                
                if currency_code and Decimal(available) > 0:
                    currency = Currency.from_str(currency_code)
                    total = Money(Decimal(available), currency)
                    free = Money(Decimal(available), currency)
                    locked = Money(Decimal(0), currency)
                    
                    balance = AccountBalance(
                        total=total,
                        locked=locked,
                        free=free,
                    )
                    balances.append(balance)

            # Generate account state event
            if balances:
                account_id = AccountId(f"{COINBASE_VENUE.value}-001")
                self.generate_account_state(
                    balances=balances,
                    margins=[],
                    reported=True,
                    ts_event=self._clock.timestamp_ns(),
                )
                self._log.info(f"Updated account state with {len(balances)} balances")

        except Exception as e:
            self._log.error(f"Failed to update account state: {e}")

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> OrderStatusReport | None:
        """Generate an order status report."""
        try:
            if venue_order_id:
                order_json = await self._client.get_order(venue_order_id.value)
                order_data = json.loads(order_json)
                return self._parse_order_status_report(order_data, instrument_id)
        except Exception as e:
            self._log.error(f"Failed to generate order status report: {e}")
        return None

    def _parse_order_status_report(self, order_data: dict, instrument_id: InstrumentId) -> OrderStatusReport:
        """Parse Coinbase order data into an OrderStatusReport."""
        # This is a simplified implementation
        venue_order_id = VenueOrderId(order_data["order_id"])
        client_order_id = ClientOrderId(order_data.get("client_order_id", ""))
        
        # Map Coinbase status to Nautilus status
        status_map = {
            "OPEN": OrderStatus.ACCEPTED,
            "FILLED": OrderStatus.FILLED,
            "CANCELLED": OrderStatus.CANCELED,
            "EXPIRED": OrderStatus.EXPIRED,
            "FAILED": OrderStatus.REJECTED,
        }
        order_status = status_map.get(order_data["status"], OrderStatus.PENDING_UPDATE)

        return OrderStatusReport(
            account_id=AccountId(f"{COINBASE_VENUE.value}-001"),
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            order_side=OrderSide.BUY if order_data["side"] == "BUY" else OrderSide.SELL,
            order_type=OrderType.LIMIT,  # Simplified
            time_in_force=None,
            order_status=order_status,
            filled_qty=Decimal(order_data.get("filled_size", "0")),
            ts_accepted=0,
            ts_last=0,
            ts_init=self._clock.timestamp_ns(),
            report_id=UUID4(),
        )

    async def _submit_order(self, command: SubmitOrder) -> None:
        """Submit an order to Coinbase."""
        try:
            order = command.order
            product_id = order.instrument_id.symbol.value.replace("/", "-")
            
            # Build order request
            request = {
                "client_order_id": order.client_order_id.value,
                "product_id": product_id,
                "side": "BUY" if order.side == OrderSide.BUY else "SELL",
            }

            if isinstance(order, MarketOrder):
                request["order_configuration"] = {
                    "market_market_ioc": {
                        "base_size": str(order.quantity),
                    }
                }
            elif isinstance(order, LimitOrder):
                request["order_configuration"] = {
                    "limit_limit_gtc": {
                        "base_size": str(order.quantity),
                        "limit_price": str(order.price),
                        "post_only": order.is_post_only,
                    }
                }
            else:
                self._log.error(f"Unsupported order type: {type(order)}")
                return

            # Submit order
            request_json = json.dumps(request)
            response_json = await self._client.create_order(request_json)
            response = json.loads(response_json)

            if response.get("success"):
                venue_order_id = VenueOrderId(response["order_id"])
                self._client_order_id_to_venue_order_id[order.client_order_id] = venue_order_id
                self._log.info(f"Order submitted: {order.client_order_id} -> {venue_order_id}")
            else:
                error = response.get("failure_reason", "Unknown error")
                self._log.error(f"Order submission failed: {error}")

        except Exception as e:
            self._log.error(f"Failed to submit order: {e}")

    async def _cancel_order(self, command: CancelOrder) -> None:
        """Cancel an order on Coinbase."""
        try:
            venue_order_id = self._client_order_id_to_venue_order_id.get(command.client_order_id)
            if not venue_order_id:
                self._log.error(f"No venue order ID found for {command.client_order_id}")
                return

            response_json = await self._client.cancel_orders([venue_order_id.value])
            response = json.loads(response_json)
            
            self._log.info(f"Order cancelled: {command.client_order_id}")

        except Exception as e:
            self._log.error(f"Failed to cancel order: {e}")

    async def _modify_order(self, command: ModifyOrder) -> None:
        """Modify an order on Coinbase."""
        # Coinbase doesn't support order modification directly
        # Would need to cancel and replace
        self._log.warning("Order modification not supported, cancel and resubmit instead")

