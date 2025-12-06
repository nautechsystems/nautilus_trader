# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
from datetime import datetime
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.alpaca.config import AlpacaExecClientConfig
from nautilus_trader.adapters.alpaca.constants import ALPACA_VENUE
from nautilus_trader.adapters.alpaca.constants import AlpacaOrderSide
from nautilus_trader.adapters.alpaca.constants import AlpacaOrderStatus
from nautilus_trader.adapters.alpaca.constants import AlpacaOrderType
from nautilus_trader.adapters.alpaca.constants import AlpacaTimeInForce
from nautilus_trader.adapters.alpaca.http.client import AlpacaHttpClient
from nautilus_trader.adapters.alpaca.providers import AlpacaInstrumentProvider
from nautilus_trader.adapters.alpaca.websocket.trading_client import AlpacaTradingWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


class AlpacaExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Alpaca.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : AlpacaHttpClient
        The Alpaca HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : AlpacaInstrumentProvider
        The instrument provider.
    config : AlpacaExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: AlpacaHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: AlpacaInstrumentProvider,
        config: AlpacaExecClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or "ALPACA"),
            venue=ALPACA_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.CASH,
            base_currency=Currency.from_str("USD"),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._http_client = client
        self._config = config

        # Log configuration
        self._log.info(f"Paper trading: {config.paper}", LogColor.BLUE)

        # Create WebSocket client for order updates
        self._ws_client = AlpacaTradingWebSocketClient(
            api_key=config.api_key,
            api_secret=config.api_secret,
            access_token=config.access_token,
            paper=config.paper,
            logger=self._log,
        )

        # Set up callbacks
        self._ws_client.set_on_trade_update(self._handle_trade_update)
        self._ws_client.set_on_error(self._handle_ws_error)

        # Alpaca account number (set on connect)
        self._alpaca_account_id: str | None = None

    async def _connect(self) -> None:
        """Connect the execution client."""
        # Connect HTTP client first (needed for instrument provider)
        await self._http_client.connect()

        # Initialize instrument provider (follows Nautilus convention)
        await self._instrument_provider.initialize()

        # Get account info and set account ID
        account_info = await self._http_client.get_account()
        self._alpaca_account_id = account_info.get("account_number")

        # Set the account ID (must be done before generating account state)
        account_id = AccountId(f"ALPACA-{self._alpaca_account_id}")
        self._set_account_id(account_id)

        # Generate initial account state to register with portfolio
        await self._update_account_state(account_info)

        # Wait for account to be registered with portfolio (follows Nautilus convention)
        await self._await_account_registered()

        # Connect WebSocket for order updates
        await self._ws_client.connect()

        self._log.info(
            f"Alpaca execution client connected (account: {self._alpaca_account_id})",
            LogColor.GREEN,
        )

    async def _update_account_state(self, account_info: dict[str, Any] | None = None) -> None:
        """Update account state from Alpaca account info."""
        if account_info is None:
            account_info = await self._http_client.get_account()

        # Parse account balances from Alpaca response
        # Alpaca returns: cash, buying_power, equity, etc.
        cash = Decimal(str(account_info.get("cash", "0")))
        
        # Create account balance for USD
        usd = Currency.from_str("USD")
        balances = [
            AccountBalance(
                total=Money(cash, usd),
                locked=Money(0, usd),
                free=Money(cash, usd),
            )
        ]

        self.generate_account_state(
            balances=balances,
            margins=[],
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

        self._log.info(f"Account state updated: cash={cash} USD")

    async def _ensure_instrument_loaded(self, instrument_id: InstrumentId) -> None:
        """Ensure an instrument is loaded, loading on-demand if needed."""
        # Check if already in cache
        if self._cache.instrument(instrument_id) is not None:
            return

        # Check if already in provider
        if self._instrument_provider.find(instrument_id) is not None:
            instrument = self._instrument_provider.find(instrument_id)
            self._cache.add_instrument(instrument)
            return

        # Load on-demand from Alpaca
        self._log.info(f"Loading instrument on-demand: {instrument_id}")
        await self._instrument_provider.load_async(instrument_id)

        # Add to cache if successfully loaded
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is not None:
            self._cache.add_instrument(instrument)
            self._log.info(f"Loaded instrument: {instrument_id}", LogColor.GREEN)
        else:
            self._log.warning(f"Failed to load instrument: {instrument_id}")

    async def _disconnect(self) -> None:
        """Disconnect the execution client."""
        await self._ws_client.disconnect()
        await self._http_client.disconnect()
        self._log.info("Alpaca execution client disconnected")

    # -- Order submission ----

    async def _submit_order(self, command: SubmitOrder) -> None:
        """Submit an order to Alpaca."""
        order = command.order
        instrument_id = order.instrument_id
        symbol = instrument_id.symbol.value

        # Ensure instrument is loaded (defensive - should already be loaded by data client)
        await self._ensure_instrument_loaded(instrument_id)

        try:
            # Map order parameters
            side = self._map_order_side(order.side)
            order_type = self._map_order_type(order.order_type)
            tif = self._map_time_in_force(order.time_in_force)

            # Build order params
            params: dict[str, Any] = {
                "symbol": symbol,
                "qty": str(order.quantity),
                "side": side,
                "type": order_type,
                "time_in_force": tif,
                "client_order_id": order.client_order_id.value,
            }

            # Add price for limit orders
            if order.order_type in (OrderType.LIMIT, OrderType.STOP_LIMIT):
                if order.price:
                    params["limit_price"] = str(order.price)

            # Add stop price for stop orders
            if order.order_type in (OrderType.STOP_MARKET, OrderType.STOP_LIMIT):
                if hasattr(order, "trigger_price") and order.trigger_price:
                    params["stop_price"] = str(order.trigger_price)

            # Submit order
            response = await self._http_client.submit_order(**params)

            # Generate accepted event
            venue_order_id = VenueOrderId(response["id"])
            self.generate_order_accepted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=venue_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

            self._log.info(f"Order submitted: {order.client_order_id} -> {venue_order_id}")

        except Exception as e:
            self._log.error(f"Order submission failed: {e}")
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_order(self, command: CancelOrder) -> None:
        """Cancel an order on Alpaca."""
        try:
            venue_order_id = command.venue_order_id

            if venue_order_id:
                await self._http_client.cancel_order(venue_order_id.value)
            else:
                # Try by client order ID
                order = await self._http_client.get_order_by_client_id(
                    command.client_order_id.value
                )
                await self._http_client.cancel_order(order["id"])

            self._log.info(f"Order cancel requested: {command.client_order_id}")

        except Exception as e:
            self._log.error(f"Order cancel failed: {e}")
            self.generate_order_cancel_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _modify_order(self, command: ModifyOrder) -> None:
        """Modify an order on Alpaca."""
        try:
            venue_order_id = command.venue_order_id
            if not venue_order_id:
                raise ValueError("venue_order_id required for modify")

            params: dict[str, Any] = {}
            if command.quantity:
                params["qty"] = str(command.quantity)
            if command.price:
                params["limit_price"] = str(command.price)

            await self._http_client.replace_order(venue_order_id.value, **params)

            self._log.info(f"Order modify requested: {command.client_order_id}")

        except Exception as e:
            self._log.error(f"Order modify failed: {e}")
            self.generate_order_modify_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    # -- Reports ----

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        """Generate order status reports."""
        reports = []

        try:
            status = "open" if command.open_only else "all"
            symbols = [command.instrument_id.symbol.value] if command.instrument_id else None

            orders = await self._http_client.get_orders(
                status=status,
                symbols=symbols,
            )

            for order_data in orders:
                report = self._parse_order_status_report(order_data)
                reports.append(report)

        except Exception as e:
            self._log.error(f"Failed to generate order status reports: {e}")

        return reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        """Generate fill reports."""
        # Alpaca doesn't have a separate fills endpoint
        # Fills are part of order data
        return []

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        """Generate position status reports."""
        reports = []

        try:
            if command.instrument_id:
                positions = [await self._http_client.get_position(command.instrument_id.symbol.value)]
            else:
                positions = await self._http_client.get_positions()

            for pos_data in positions:
                report = self._parse_position_status_report(pos_data)
                reports.append(report)

        except Exception as e:
            self._log.error(f"Failed to generate position status reports: {e}")

        return reports

    # -- WebSocket handlers ----

    def _handle_trade_update(self, data: dict[str, Any]) -> None:
        """Handle trade update from WebSocket."""
        try:
            event = data.get("event")
            order_data = data.get("order", {})

            client_order_id_str = order_data.get("client_order_id")
            if not client_order_id_str:
                return

            client_order_id = ClientOrderId(client_order_id_str)
            venue_order_id = VenueOrderId(order_data.get("id", ""))

            symbol = order_data.get("symbol")
            instrument_id = InstrumentId(
                symbol=Symbol(symbol),
                venue=ALPACA_VENUE,
            )

            # Get cached order for strategy_id
            cached_order = self._cache.order(client_order_id)
            strategy_id = cached_order.strategy_id if cached_order else None

            ts_event = self._clock.timestamp_ns()

            if event == "fill" or event == "partial_fill":
                # Handle fill
                fill_qty = Decimal(str(data.get("qty", order_data.get("filled_qty", 0))))
                fill_price = Decimal(str(data.get("price", order_data.get("filled_avg_price", 0))))

                self.generate_order_filled(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    venue_position_id=None,
                    trade_id=TradeId(str(data.get("execution_id", UUID4().value))),
                    order_side=self._parse_order_side(order_data.get("side", "buy")),
                    order_type=self._parse_order_type(order_data.get("type", "market")),
                    last_qty=Quantity.from_str(str(fill_qty)),
                    last_px=Price.from_str(str(fill_price)),
                    quote_currency=Currency.from_str("USD"),
                    commission=Money(0, Currency.from_str("USD")),
                    liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                    ts_event=ts_event,
                )

            elif event == "canceled":
                self.generate_order_canceled(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=ts_event,
                )

            elif event == "rejected":
                self.generate_order_rejected(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    reason=data.get("message", "Order rejected"),
                    ts_event=ts_event,
                )

            elif event == "expired":
                self.generate_order_expired(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=ts_event,
                )

        except Exception as e:
            self._log.error(f"Error handling trade update: {e}")

    def _handle_ws_error(self, error: str) -> None:
        """Handle WebSocket error."""
        self._log.error(f"Alpaca trading WebSocket error: {error}")

    # -- Parsing helpers ----

    def _parse_order_status_report(self, data: dict[str, Any]) -> OrderStatusReport:
        """Parse order data into OrderStatusReport."""
        instrument_id = InstrumentId(
            symbol=Symbol(data["symbol"]),
            venue=ALPACA_VENUE,
        )

        return OrderStatusReport(
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=ClientOrderId(data.get("client_order_id", "")),
            venue_order_id=VenueOrderId(data["id"]),
            order_side=self._parse_order_side(data.get("side", "buy")),
            order_type=self._parse_order_type(data.get("type", "market")),
            time_in_force=self._parse_time_in_force(data.get("time_in_force", "day")),
            order_status=self._parse_order_status(data.get("status", "new")),
            quantity=Quantity.from_str(str(data.get("qty", 0))),
            filled_qty=Quantity.from_str(str(data.get("filled_qty", 0))),
            avg_px=Price.from_str(str(data.get("filled_avg_price", 0))) if data.get("filled_avg_price") else None,
            price=Price.from_str(str(data.get("limit_price", 0))) if data.get("limit_price") else None,
            trigger_price=Price.from_str(str(data.get("stop_price", 0))) if data.get("stop_price") else None,
            report_id=UUID4(),
            ts_accepted=self._parse_timestamp(data.get("created_at")),
            ts_last=self._parse_timestamp(data.get("updated_at")),
            ts_init=self._clock.timestamp_ns(),
        )

    def _parse_position_status_report(self, data: dict[str, Any]) -> PositionStatusReport:
        """Parse position data into PositionStatusReport."""
        instrument_id = InstrumentId(
            symbol=Symbol(data["symbol"]),
            venue=ALPACA_VENUE,
        )

        qty = Decimal(str(data.get("qty", 0)))
        side = PositionSide.LONG if qty > 0 else PositionSide.SHORT

        return PositionStatusReport(
            account_id=self.account_id,
            instrument_id=instrument_id,
            position_side=side,
            quantity=Quantity.from_str(str(abs(qty))),
            avg_px_open=Price.from_str(str(data.get("avg_entry_price", 0))),
            report_id=UUID4(),
            ts_last=self._clock.timestamp_ns(),
            ts_init=self._clock.timestamp_ns(),
        )

    def _parse_timestamp(self, ts_str: str | None) -> int:
        """Parse timestamp string to nanoseconds."""
        if not ts_str:
            return self._clock.timestamp_ns()
        try:
            dt = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
            return dt_to_unix_nanos(dt)
        except Exception:
            return self._clock.timestamp_ns()

    # -- Mapping helpers ----

    def _map_order_side(self, side: OrderSide) -> str:
        """Map Nautilus OrderSide to Alpaca side string."""
        return "buy" if side == OrderSide.BUY else "sell"

    def _map_order_type(self, order_type: OrderType) -> str:
        """Map Nautilus OrderType to Alpaca type string."""
        mapping = {
            OrderType.MARKET: "market",
            OrderType.LIMIT: "limit",
            OrderType.STOP_MARKET: "stop",
            OrderType.STOP_LIMIT: "stop_limit",
        }
        return mapping.get(order_type, "market")

    def _map_time_in_force(self, tif: TimeInForce) -> str:
        """Map Nautilus TimeInForce to Alpaca TIF string."""
        mapping = {
            TimeInForce.DAY: "day",
            TimeInForce.GTC: "gtc",
            TimeInForce.IOC: "ioc",
            TimeInForce.FOK: "fok",
        }
        return mapping.get(tif, "day")

    def _parse_order_side(self, side: str) -> OrderSide:
        """Parse Alpaca side string to Nautilus OrderSide."""
        return OrderSide.BUY if side.lower() == "buy" else OrderSide.SELL

    def _parse_order_type(self, order_type: str) -> OrderType:
        """Parse Alpaca type string to Nautilus OrderType."""
        mapping = {
            "market": OrderType.MARKET,
            "limit": OrderType.LIMIT,
            "stop": OrderType.STOP_MARKET,
            "stop_limit": OrderType.STOP_LIMIT,
        }
        return mapping.get(order_type.lower(), OrderType.MARKET)

    def _parse_time_in_force(self, tif: str) -> TimeInForce:
        """Parse Alpaca TIF string to Nautilus TimeInForce."""
        mapping = {
            "day": TimeInForce.DAY,
            "gtc": TimeInForce.GTC,
            "ioc": TimeInForce.IOC,
            "fok": TimeInForce.FOK,
        }
        return mapping.get(tif.lower(), TimeInForce.DAY)

    def _parse_order_status(self, status: str) -> OrderStatus:
        """Parse Alpaca status string to Nautilus OrderStatus."""
        mapping = {
            "new": OrderStatus.ACCEPTED,
            "accepted": OrderStatus.ACCEPTED,
            "pending_new": OrderStatus.SUBMITTED,
            "partially_filled": OrderStatus.PARTIALLY_FILLED,
            "filled": OrderStatus.FILLED,
            "canceled": OrderStatus.CANCELED,
            "expired": OrderStatus.EXPIRED,
            "rejected": OrderStatus.REJECTED,
            "pending_cancel": OrderStatus.PENDING_CANCEL,
            "pending_replace": OrderStatus.PENDING_UPDATE,
        }
        return mapping.get(status.lower(), OrderStatus.ACCEPTED)

