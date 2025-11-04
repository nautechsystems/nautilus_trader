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
import json
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.lighter2.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter2.constants import LIGHTER_CLIENT_ID
from nautilus_trader.adapters.lighter2.constants import LIGHTER_ORDER_SIDE_BUY
from nautilus_trader.adapters.lighter2.constants import LIGHTER_ORDER_SIDE_SELL
from nautilus_trader.adapters.lighter2.constants import LIGHTER_ORDER_STATUS_CANCELLED
from nautilus_trader.adapters.lighter2.constants import LIGHTER_ORDER_STATUS_FILLED
from nautilus_trader.adapters.lighter2.constants import LIGHTER_ORDER_STATUS_PARTIALLY_FILLED
from nautilus_trader.adapters.lighter2.constants import LIGHTER_ORDER_STATUS_PENDING
from nautilus_trader.adapters.lighter2.constants import LIGHTER_ORDER_STATUS_REJECTED
from nautilus_trader.adapters.lighter2.constants import LIGHTER_ORDER_TYPE_LIMIT
from nautilus_trader.adapters.lighter2.constants import LIGHTER_ORDER_TYPE_MARKET
from nautilus_trader.adapters.lighter2.constants import LIGHTER_VENUE
from nautilus_trader.adapters.lighter2.http import LighterHttpClient
from nautilus_trader.adapters.lighter2.providers import LighterInstrumentProvider
from nautilus_trader.adapters.lighter2.websocket import LighterWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.bus import MessageBus


class LighterExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Lighter exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : LighterHttpClient
        The Lighter HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    config : LighterExecClientConfig
        The configuration for the client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: LighterHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        config: LighterExecClientConfig,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=LIGHTER_CLIENT_ID,
            venue=LIGHTER_VENUE,
            account_type=AccountType.MARGIN,  # Lighter supports margin trading
            base_currency=None,  # Multi-currency account
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._client = client
        self._config = config

        # WebSocket client for order/account updates
        self._ws_client: LighterWebSocketClient | None = None

        # Instrument provider
        self._instrument_provider = LighterInstrumentProvider(
            client=client,
            logger=logger,
            account_type=config.account_type,
        )

        # Order tracking
        self._venue_order_id_to_client_order_id: dict[VenueOrderId, ClientOrderId] = {}
        self._client_order_id_to_venue_order_id: dict[ClientOrderId, VenueOrderId] = {}

        # Account tracking
        self._account_id: AccountId | None = None

    async def _connect(self) -> None:
        """Connect the client."""
        # Connect HTTP client
        await self._client.connect()

        # Initialize WebSocket client for order updates
        self._ws_client = LighterWebSocketClient(
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
            handler=self._handle_ws_message,
            api_key_private_key=self._config.api_key_private_key,
            eth_private_key=self._config.eth_private_key,
            base_url=self._config.base_url_ws,
            is_testnet=self._config.is_testnet,
            proxy_url=self._config.ws_proxy_url,
        )

        # Connect WebSocket client
        await self._ws_client.connect()

        # Subscribe to account and order updates
        await self._ws_client.subscribe_account_updates()
        await self._ws_client.subscribe_order_updates()

        # Load instruments
        await self._instrument_provider.load_all_async()

        # Get account information
        await self._update_account_state()

        self._log.info("Lighter execution client connected")

    async def _disconnect(self) -> None:
        """Disconnect the client."""
        # Disconnect WebSocket client
        if self._ws_client:
            await self._ws_client.disconnect()
            self._ws_client = None

        # Disconnect HTTP client
        await self._client.disconnect()

        # Clear tracking
        self._venue_order_id_to_client_order_id.clear()
        self._client_order_id_to_venue_order_id.clear()

        self._log.info("Lighter execution client disconnected")

    def _handle_ws_message(self, raw: bytes) -> None:
        """Handle WebSocket message."""
        self._loop.create_task(self._process_ws_message(raw))

    async def _process_ws_message(self, raw: bytes) -> None:
        """Process WebSocket message."""
        try:
            message_str = raw.decode('utf-8')
            message = json.loads(message_str)

            channel = message.get("channel")
            
            if channel == "account":
                await self._handle_account_update(message)
            elif channel == "orders":
                await self._handle_order_update(message)
            else:
                self._log.debug(f"Unhandled channel: {channel}")

        except Exception as e:
            self._log.error(f"Error processing WebSocket message: {e}")

    async def _handle_account_update(self, message: dict[str, Any]) -> None:
        """Handle account update."""
        try:
            data = message.get("data", {})
            
            # Update account balances
            balances = data.get("balances", [])
            for balance in balances:
                currency_code = balance.get("currency")
                total = balance.get("total", 0)
                available = balance.get("available", 0)
                locked = balance.get("locked", 0)

                if currency_code:
                    currency = Currency.from_str(currency_code)
                    
                    # Create account balance
                    account_balance = AccountBalance(
                        total=Money(total, currency),
                        locked=Money(locked, currency),
                        free=Money(available, currency),
                    )
                    
                    self._handle_account_update(account_balance)

        except Exception as e:
            self._log.error(f"Error handling account update: {e}")

    async def _handle_order_update(self, message: dict[str, Any]) -> None:
        """Handle order update."""
        try:
            data = message.get("data", {})
            
            venue_order_id = VenueOrderId(str(data.get("order_id", "")))
            client_order_id = self._venue_order_id_to_client_order_id.get(venue_order_id)
            
            if not client_order_id:
                self._log.warning(f"Unknown venue order ID: {venue_order_id}")
                return

            # Parse order status
            status_str = data.get("status", "")
            status = self._parse_order_status(status_str)

            # Create order status report
            instrument_id_str = data.get("instrument_id", "")
            instrument_id = InstrumentId.from_str(f"{instrument_id_str}.{LIGHTER_VENUE}")
            
            price = Price.from_str(str(data.get("price", 0))) if data.get("price") else None
            quantity = Quantity.from_str(str(data.get("quantity", 0)))
            filled_qty = Quantity.from_str(str(data.get("filled_quantity", 0)))
            
            timestamp_ns = self._clock.timestamp_ns()

            report = OrderStatusReport(
                account_id=self._account_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                order_side=self._parse_order_side(data.get("side", "")),
                order_type=self._parse_order_type(data.get("type", "")),
                quantity=quantity,
                filled_qty=filled_qty,
                avg_px=price,
                order_status=status,
                ts_accepted=timestamp_ns,
                ts_last=timestamp_ns,
            )

            self._handle_order_status_report(report)

            # Handle fills if order is filled or partially filled
            if status in (OrderStatus.FILLED, OrderStatus.PARTIALLY_FILLED):
                await self._handle_trade_report(data, client_order_id, venue_order_id)

        except Exception as e:
            self._log.error(f"Error handling order update: {e}")

    async def _handle_trade_report(
        self, 
        data: dict[str, Any], 
        client_order_id: ClientOrderId, 
        venue_order_id: VenueOrderId
    ) -> None:
        """Handle trade report."""
        try:
            trade_id = TradeId(str(data.get("trade_id", venue_order_id.value)))
            instrument_id_str = data.get("instrument_id", "")
            instrument_id = InstrumentId.from_str(f"{instrument_id_str}.{LIGHTER_VENUE}")
            
            price = Price.from_str(str(data.get("price", 0)))
            quantity = Quantity.from_str(str(data.get("filled_quantity", 0)))
            
            # Determine liquidity side (simplified)
            liquidity_side = LiquiditySide.MAKER  # Default, should be determined from actual data

            timestamp_ns = self._clock.timestamp_ns()

            trade_report = TradeReport(
                account_id=self._account_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                trade_id=trade_id,
                order_side=self._parse_order_side(data.get("side", "")),
                last_qty=quantity,
                last_px=price,
                commission=Money(0, Currency.from_str("USDT")),  # Lighter has fee-less trading
                liquidity_side=liquidity_side,
                ts_event=timestamp_ns,
            )

            self._handle_trade_report(trade_report)

        except Exception as e:
            self._log.error(f"Error handling trade report: {e}")

    async def _update_account_state(self) -> None:
        """Update account state."""
        try:
            account_data = await self._client.get_account()
            account_id_str = account_data.get("account_id", "1")
            self._account_id = AccountId(f"{LIGHTER_VENUE.value}-{account_id_str}")

            self._log.info(f"Updated account state: {self._account_id}")

        except Exception as e:
            self._log.error(f"Error updating account state: {e}")

    def _parse_order_status(self, status_str: str) -> OrderStatus:
        """Parse order status from string."""
        status_map = {
            LIGHTER_ORDER_STATUS_PENDING: OrderStatus.SUBMITTED,
            LIGHTER_ORDER_STATUS_FILLED: OrderStatus.FILLED,
            LIGHTER_ORDER_STATUS_PARTIALLY_FILLED: OrderStatus.PARTIALLY_FILLED,
            LIGHTER_ORDER_STATUS_CANCELLED: OrderStatus.CANCELED,
            LIGHTER_ORDER_STATUS_REJECTED: OrderStatus.REJECTED,
        }
        return status_map.get(status_str, OrderStatus.SUBMITTED)

    def _parse_order_side(self, side_str: str) -> OrderSide:
        """Parse order side from string."""
        if side_str == LIGHTER_ORDER_SIDE_BUY:
            return OrderSide.BUY
        elif side_str == LIGHTER_ORDER_SIDE_SELL:
            return OrderSide.SELL
        else:
            return OrderSide.BUY  # Default

    def _parse_order_type(self, type_str: str) -> OrderType:
        """Parse order type from string."""
        if type_str == LIGHTER_ORDER_TYPE_LIMIT:
            return OrderType.LIMIT
        elif type_str == LIGHTER_ORDER_TYPE_MARKET:
            return OrderType.MARKET
        else:
            return OrderType.LIMIT  # Default

    async def _submit_order(self, command: SubmitOrder) -> None:
        """Submit an order."""
        try:
            order = command.order
            
            # Extract symbol from instrument ID
            symbol = order.instrument_id.symbol.value.replace("-PERP", "")
            
            # Convert order side
            side = LIGHTER_ORDER_SIDE_BUY if order.side == OrderSide.BUY else LIGHTER_ORDER_SIDE_SELL
            
            # Convert order type
            if order.order_type == OrderType.MARKET:
                order_type = LIGHTER_ORDER_TYPE_MARKET
                price = None
            else:
                order_type = LIGHTER_ORDER_TYPE_LIMIT
                price = str(order.price) if order.price else None

            # Submit order
            response = await self._client.place_order(
                instrument_id=symbol,
                side=side,
                order_type=order_type,
                quantity=str(order.quantity),
                price=price,
            )

            # Extract venue order ID
            venue_order_id = VenueOrderId(str(response.get("order_id", "")))
            
            # Track order mapping
            self._venue_order_id_to_client_order_id[venue_order_id] = order.client_order_id
            self._client_order_id_to_venue_order_id[order.client_order_id] = venue_order_id

            self._log.info(f"Submitted order {order.client_order_id} -> {venue_order_id}")

        except Exception as e:
            self._log.error(f"Error submitting order {command.order.client_order_id}: {e}")
            # Generate rejection report
            self._generate_order_rejected(command.order, str(e))

    async def _cancel_order(self, command: CancelOrder) -> None:
        """Cancel an order."""
        try:
            venue_order_id = self._client_order_id_to_venue_order_id.get(command.client_order_id)
            if not venue_order_id:
                self._log.error(f"No venue order ID found for {command.client_order_id}")
                return

            response = await self._client.cancel_order(venue_order_id.value)
            self._log.info(f"Cancelled order {command.client_order_id}")

        except Exception as e:
            self._log.error(f"Error cancelling order {command.client_order_id}: {e}")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        """Cancel all orders."""
        try:
            instrument_id = None
            if command.instrument_id:
                # Extract symbol from instrument ID
                instrument_id = command.instrument_id.symbol.value.replace("-PERP", "")

            response = await self._client.cancel_all_orders(instrument_id)
            self._log.info("Cancelled all orders")

        except Exception as e:
            self._log.error(f"Error cancelling all orders: {e}")

    async def _modify_order(self, command: ModifyOrder) -> None:
        """Modify an order (not supported by Lighter)."""
        self._log.error("Order modification not supported by Lighter")
        # Generate rejection for modification
        self._generate_order_modify_rejected(
            command.strategy_id,
            command.instrument_id,
            command.client_order_id,
            command.venue_order_id,
            "Order modification not supported",
        )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        """Submit an order list (batch orders)."""
        # Lighter supports batch transactions, but we'll implement simple sequential submission
        for order in command.order_list.orders:
            submit_command = SubmitOrder(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                order=order,
                command_id=command.command_id,
                ts_init=command.ts_init,
            )
            await self._submit_order(submit_command)

    # Required abstract method implementations
    async def generate_position_status_reports(
        self, 
        instrument_id: InstrumentId | None = None
    ) -> list[PositionStatusReport]:
        """Generate position status reports."""
        # Lighter positions would be fetched from the API
        # For now, return empty list as this requires specific implementation
        return []

    async def generate_order_status_reports(
        self, 
        instrument_id: InstrumentId | None = None
    ) -> list[OrderStatusReport]:
        """Generate order status reports."""
        try:
            # Get orders from Lighter API
            symbol = None
            if instrument_id:
                symbol = instrument_id.symbol.value.replace("-PERP", "")
            
            orders = await self._client.get_orders(symbol)
            reports = []

            for order_data in orders:
                venue_order_id = VenueOrderId(str(order_data.get("order_id", "")))
                client_order_id = self._venue_order_id_to_client_order_id.get(venue_order_id)
                
                if client_order_id:
                    instrument_id_str = order_data.get("instrument_id", "")
                    instrument_id = InstrumentId.from_str(f"{instrument_id_str}.{LIGHTER_VENUE}")
                    
                    report = OrderStatusReport(
                        account_id=self._account_id,
                        instrument_id=instrument_id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        order_side=self._parse_order_side(order_data.get("side", "")),
                        order_type=self._parse_order_type(order_data.get("type", "")),
                        quantity=Quantity.from_str(str(order_data.get("quantity", 0))),
                        filled_qty=Quantity.from_str(str(order_data.get("filled_quantity", 0))),
                        order_status=self._parse_order_status(order_data.get("status", "")),
                        ts_accepted=self._clock.timestamp_ns(),
                        ts_last=self._clock.timestamp_ns(),
                    )
                    reports.append(report)

            return reports

        except Exception as e:
            self._log.error(f"Error generating order status reports: {e}")
            return []