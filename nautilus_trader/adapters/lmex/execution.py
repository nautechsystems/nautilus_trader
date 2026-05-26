# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
LMEX live execution client.

Handles order submission, cancellation, fill events, and account state
reconciliation.
"""

from __future__ import annotations

import asyncio
from decimal import Decimal
from typing import Any

import msgspec

from nautilus_trader.adapters.lmex.config import LmexExecClientConfig
from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
from nautilus_trader.adapters.lmex.constants import LMEX_WS_TOPIC_NOTIFICATIONS
from nautilus_trader.adapters.lmex.enums import LmexOrderStatus
from nautilus_trader.adapters.lmex.http.account import LmexAccountHttpAPI
from nautilus_trader.adapters.lmex.http.client import LmexHttpClient
from nautilus_trader.adapters.lmex.http.error import LmexClientError
from nautilus_trader.adapters.lmex.http.error import LmexServerError
from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider
from nautilus_trader.adapters.lmex.schemas.order import LmexFill
from nautilus_trader.adapters.lmex.schemas.order import LmexOpenOrder
from nautilus_trader.adapters.lmex.schemas.ws import LmexWsMsg
from nautilus_trader.adapters.lmex.schemas.ws import LmexWsOrderEvent
from nautilus_trader.adapters.lmex.schemas.ws import LmexWsOrderEventMsg
from nautilus_trader.adapters.lmex.websocket.client import LmexWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.currencies import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def _lmex_status_to_nautilus(status_code: int) -> OrderStatus:
    """
    Map an LMEX integer status code to a NautilusTrader ``OrderStatus``.

    Parameters
    ----------
    status_code : int
        Raw LMEX status code.

    Returns
    -------
    OrderStatus

    Raises
    ------
    ValueError
        If the status code is not recognised.

    """
    mapping: dict[int, OrderStatus] = {
        LmexOrderStatus.ORDER_INSERTED: OrderStatus.ACCEPTED,
        LmexOrderStatus.ORDER_FULLY_TRANSACTED: OrderStatus.FILLED,
        LmexOrderStatus.ORDER_PARTIALLY_TRANSACTED: OrderStatus.PARTIALLY_FILLED,
        LmexOrderStatus.ORDER_CANCELLED: OrderStatus.CANCELED,
        LmexOrderStatus.STATUS_INACTIVE: OrderStatus.CANCELED,
        LmexOrderStatus.TRIGGER_INSERTED: OrderStatus.ACCEPTED,
        LmexOrderStatus.TRIGGER_ACTIVATED: OrderStatus.TRIGGERED,
        LmexOrderStatus.MARKET_UNAVAILABLE: OrderStatus.REJECTED,
        LmexOrderStatus.REJECT_AMEND_ORDER_REJECTION: OrderStatus.REJECTED,
        LmexOrderStatus.FAILED_ERROR: OrderStatus.REJECTED,
    }
    result = mapping.get(status_code)
    if result is None:
        raise ValueError(f"Unknown LMEX order status code: {status_code}")
    return result


class LmexLiveExecutionClient(LiveExecutionClient):
    """
    Provides a live execution client for the LMEX exchange.

    Handles order lifecycle management via REST and (optionally) the private
    WebSocket notification stream.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : LmexHttpClient
        The LMEX HTTP client.
    ws_client : LmexWebSocketClient
        The LMEX WebSocket client (shared with data client or dedicated).
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : LmexInstrumentProvider
        The instrument provider.
    config : LmexExecClientConfig
        Configuration for the client.
    name : str or None, optional
        Custom client ID.

    """

    _dec_ws_envelope = msgspec.json.Decoder(LmexWsMsg)
    _dec_ws_order_events = msgspec.json.Decoder(LmexWsOrderEventMsg)

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: LmexHttpClient,
        ws_client: LmexWebSocketClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: LmexInstrumentProvider,
        config: LmexExecClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or LMEX_VENUE.value),
            venue=LMEX_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,
            base_currency=None,  # multi-currency account
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._http_client = http_client
        self._ws_client = ws_client
        self._config = config
        self._instrument_provider: LmexInstrumentProvider = instrument_provider
        self._account_api = LmexAccountHttpAPI(http_client)

    # ------------------------------------------------------------------
    # Connect / disconnect
    # ------------------------------------------------------------------

    async def _connect(self) -> None:
        """
        Connect to the LMEX execution stream.

        Loads instrument definitions, subscribes to the private notification
        stream (if credentials are configured), and fetches initial account
        state.

        """
        self._log.info("Connecting execution client...")

        await self._instrument_provider.initialize()

        # Subscribe to private order notifications (topic TBD — confirmed in Phase 4)
        if self._http_client.api_key:
            await self._ws_client.subscribe(LMEX_WS_TOPIC_NOTIFICATIONS)

        await self._update_account_state()

        self._log.info("Execution client connected", LogColor.GREEN)

    async def _disconnect(self) -> None:
        """
        Disconnect the execution client.
        """
        self._log.info("Disconnecting execution client...")
        if self._http_client.api_key:
            await self._ws_client.unsubscribe(LMEX_WS_TOPIC_NOTIFICATIONS)
        self._log.info("Execution client disconnected", LogColor.BLUE)

    # ------------------------------------------------------------------
    # Order commands
    # ------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        """
        Submit a new order to LMEX.

        Parameters
        ----------
        command : SubmitOrder
            The order submission command.

        """
        order = command.order
        instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self.generate_order_rejected(
                strategy_id=command.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Instrument {order.instrument_id} not in cache",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        self.generate_order_submitted(
            strategy_id=command.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        side = "BUY" if order.side == OrderSide.BUY else "SELL"
        order_type = "MARKET" if order.order_type == OrderType.MARKET else "LIMIT"
        price: float | None = (
            float(str(order.price)) if order.order_type == OrderType.LIMIT else None
        )

        try:
            resp = await self._account_api.submit_order(
                symbol=order.instrument_id.symbol.value,
                side=side,
                order_type=order_type,
                size=float(str(order.quantity)),
                price=price,
                client_order_id=order.client_order_id.value if order.client_order_id else None,
            )
        except LmexClientError as exc:
            reason = f"{exc.message}" if exc.message else f"HTTP {exc.status}"
            self._log.warning(f"Order rejected by exchange: {reason}")
            self.generate_order_rejected(
                strategy_id=command.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return
        except LmexServerError as exc:
            self._log.error(f"Server error on order submission: {exc}")
            self.generate_order_rejected(
                strategy_id=command.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Server error: HTTP {exc.status}",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        self.generate_order_accepted(
            strategy_id=command.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId(resp.orderID),  # UUID str, capital D
            ts_event=millis_to_nanos(resp.timestamp),
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        """
        Cancel a resting order on LMEX.

        Parameters
        ----------
        command : CancelOrder
            The cancel command.

        """
        venue_order_id = (
            command.venue_order_id.value if command.venue_order_id else None
        )
        try:
            await self._account_api.cancel_order(
                symbol=command.instrument_id.symbol.value,
                order_id=venue_order_id or "",
                client_order_id=(
                    command.client_order_id.value if command.client_order_id else None
                ),
            )
        except LmexClientError as exc:
            self._log.warning(f"Cancel rejected: {exc.message}")
            self.generate_order_cancel_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"{exc.message}" if exc.message else f"HTTP {exc.status}",
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        """
        Cancel all open orders for the given instrument.

        Parameters
        ----------
        command : CancelAllOrders
            The bulk cancel command.

        """
        symbol = command.instrument_id.symbol.value if command.instrument_id else None
        try:
            if symbol:
                await self._account_api.cancel_all_orders(symbol)
            else:
                # Fallback: cancel symbol-by-symbol for all open orders
                open_orders = await self._account_api.get_open_orders()
                symbols_seen: set[str] = set()
                for o in open_orders:
                    if o.symbol not in symbols_seen:
                        symbols_seen.add(o.symbol)
                        await self._account_api.cancel_all_orders(o.symbol)
        except (LmexClientError, LmexServerError) as exc:
            self._log.error(f"cancel_all_orders failed: {exc}")

    async def _modify_order(self, command: ModifyOrder) -> None:
        """
        Modify an existing order.

        LMEX does not offer a native amend endpoint.  This implementation
        falls back to cancel-then-resubmit.

        Parameters
        ----------
        command : ModifyOrder
            The modify command.

        """
        self._log.warning(
            "LMEX does not support native order amendment; "
            "cancelling and resubmitting (cancel-replace)."
        )
        self.generate_order_modify_rejected(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            reason="LMEX does not support order amendment",
            ts_event=self._clock.timestamp_ns(),
        )

    # ------------------------------------------------------------------
    # WebSocket order event handling
    # ------------------------------------------------------------------

    def handle_ws_message(self, raw: bytes) -> None:
        """
        Dispatch an incoming WebSocket message from the private notification stream.

        Should be called by the parent data/WebSocket client for any message
        with topic ``"notificationsApi"``.

        Parameters
        ----------
        raw : bytes
            Raw WebSocket frame.

        """
        try:
            msg = self._dec_ws_order_events.decode(raw)
        except (msgspec.DecodeError, Exception) as exc:
            self._log.warning(f"Failed to decode order event: {exc} | {raw[:200]!r}")
            return

        for event in msg.data:
            self._handle_order_event(event)

    def _handle_order_event(self, event: LmexWsOrderEvent) -> None:
        """
        Translate an LMEX order event into NautilusTrader engine calls.

        Parameters
        ----------
        event : LmexWsOrderEvent
            The parsed order lifecycle event.

        """
        try:
            status = LmexOrderStatus(event.status)
        except ValueError:
            self._log.warning(f"Unknown LMEX order status: {event.status}")
            return

        instrument_id = InstrumentId.from_str(f"{event.symbol}.{LMEX_VENUE.value}")
        instrument = self._cache.instrument(instrument_id)
        venue_order_id = VenueOrderId(str(event.orderId))
        client_order_id = (
            ClientOrderId(event.clOrderId) if event.clOrderId else None
        )

        # Locate the order in the cache for the strategy_id
        order = None
        if client_order_id:
            order = self._cache.order(client_order_id)
        if order is None and venue_order_id:
            order = self._cache.order_by_venue_order_id(venue_order_id)

        strategy_id = order.strategy_id if order else self._cache.strategy_ids()[0] if self._cache.strategy_ids() else None
        if strategy_id is None:
            self._log.warning(
                f"Cannot route order event for {venue_order_id}: no strategy found"
            )
            return

        ts_event = millis_to_nanos(event.timestamp)

        if status in (LmexOrderStatus.ORDER_INSERTED, LmexOrderStatus.TRIGGER_INSERTED):
            # Duplicate accepted — ignore (already generated from REST response)
            pass

        elif status in (LmexOrderStatus.ORDER_FULLY_TRANSACTED,
                        LmexOrderStatus.ORDER_PARTIALLY_TRANSACTED):
            if instrument is None:
                self._log.warning(
                    f"Cannot generate fill: instrument {instrument_id} not cached"
                )
                return

            fill_price = event.avgFillPrice or event.price or 0.0
            fill_qty = event.filledSize or event.size

            commission = Money(
                event.feeAmount or 0.0,
                Currency.from_str(event.feeCurrency or instrument.quote_currency.code),
            )

            self.generate_order_filled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id or ClientOrderId("UNKNOWN"),
                venue_order_id=venue_order_id,
                venue_position_id=None,
                trade_id=TradeId(str(event.tradeId) if event.tradeId else venue_order_id.value),
                order_side=OrderSide.BUY if event.side == "BUY" else OrderSide.SELL,
                order_type=OrderType.LIMIT,
                last_qty=Quantity(fill_qty, instrument.size_precision),
                last_px=Price(fill_price, instrument.price_precision),
                quote_currency=instrument.quote_currency,
                commission=commission,
                liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                ts_event=ts_event,
            )

        elif status in (LmexOrderStatus.ORDER_CANCELLED, LmexOrderStatus.STATUS_INACTIVE):
            if client_order_id:
                self.generate_order_canceled(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=ts_event,
                )

        elif status in (LmexOrderStatus.FAILED_ERROR, LmexOrderStatus.MARKET_UNAVAILABLE):
            if client_order_id:
                self.generate_order_rejected(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    reason=f"LMEX status {event.status}",
                    ts_event=ts_event,
                )

    # ------------------------------------------------------------------
    # Reconciliation reports
    # ------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        """
        Generate a status report for a single order.

        Parameters
        ----------
        command : GenerateOrderStatusReport
            The report request.

        Returns
        -------
        OrderStatusReport or None
            ``None`` if the order cannot be found.

        """
        symbol = command.instrument_id.symbol.value if command.instrument_id else None
        try:
            open_orders = await self._account_api.get_open_orders(symbol=symbol)
        except (LmexClientError, LmexServerError) as exc:
            self._log.error(f"Failed to fetch open orders: {exc}")
            return None

        venue_order_id_val = (
            command.venue_order_id.value if command.venue_order_id else None
        )
        client_order_id_val = (
            command.client_order_id.value if command.client_order_id else None
        )

        for o in open_orders:
            if (
                (venue_order_id_val and str(o.orderId) == venue_order_id_val)
                or (client_order_id_val and o.clOrderId == client_order_id_val)
            ):
                return self._parse_order_status_report(o)

        return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        """
        Generate status reports for all open orders.

        Parameters
        ----------
        command : GenerateOrderStatusReports
            The bulk report request.

        Returns
        -------
        list[OrderStatusReport]

        """
        symbol = command.instrument_id.symbol.value if command.instrument_id else None
        try:
            open_orders = await self._account_api.get_open_orders(symbol=symbol)
        except (LmexClientError, LmexServerError) as exc:
            self._log.error(f"Failed to fetch open orders: {exc}")
            return []

        return [self._parse_order_status_report(o) for o in open_orders]

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        """
        Generate fill reports for completed trades.

        Parameters
        ----------
        command : GenerateFillReports
            The fill report request.

        Returns
        -------
        list[FillReport]

        """
        symbol = command.instrument_id.symbol.value if command.instrument_id else None
        start = int(command.start.timestamp() * 1000) if command.start else None
        end = int(command.end.timestamp() * 1000) if command.end else None

        try:
            fills = await self._account_api.get_fills(
                symbol=symbol,
                start_time=start,
                end_time=end,
            )
        except (LmexClientError, LmexServerError) as exc:
            self._log.error(f"Failed to fetch fills: {exc}")
            return []

        return [self._parse_fill_report(f) for f in fills]

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        """
        Generate position status reports.

        LMEX spot trading does not have persistent positions in the derivatives
        sense.  This returns an empty list for spot instruments.

        Parameters
        ----------
        command : GeneratePositionStatusReports
            The position report request.

        Returns
        -------
        list[PositionStatusReport]

        """
        return []

    async def generate_mass_status(
        self,
        lookback_mins: int | None = None,
    ) -> ExecutionMassStatus | None:
        """
        Generate a full account mass status (open orders + fills).

        Parameters
        ----------
        lookback_mins : int or None
            Lookback window for fills in minutes.

        Returns
        -------
        ExecutionMassStatus or None

        """
        try:
            open_orders = await self._account_api.get_open_orders()
            order_reports = [self._parse_order_status_report(o) for o in open_orders]
        except (LmexClientError, LmexServerError) as exc:
            self._log.error(f"Failed to fetch mass status: {exc}")
            return None

        mass_status = ExecutionMassStatus(
            client_id=self.id,
            account_id=self.account_id,
            venue=LMEX_VENUE,
            ts_init=self._clock.timestamp_ns(),
        )
        for report in order_reports:
            mass_status.add_order_reports([report])

        return mass_status

    # ------------------------------------------------------------------
    # Account state
    # ------------------------------------------------------------------

    async def _update_account_state(self) -> None:
        """
        Fetch wallet balances and publish an account state to the engine.
        """
        try:
            wallet = await self._account_api.get_wallet_balance()
        except (LmexClientError, LmexServerError) as exc:
            self._log.warning(f"Failed to fetch wallet balance: {exc}")
            return

        balances: list[AccountBalance] = []
        for entry in wallet:
            try:
                currency = Currency.from_str(entry.currency)
            except Exception:
                continue

            balances.append(
                AccountBalance(
                    total=Money(entry.total, currency),
                    locked=Money(entry.total - entry.available, currency),
                    free=Money(entry.available, currency),
                )
            )

        self.generate_account_state(
            balances=balances,
            margins=[],
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

    # ------------------------------------------------------------------
    # Private parsing helpers
    # ------------------------------------------------------------------

    def _parse_order_status_report(self, o: LmexOpenOrder) -> OrderStatusReport:
        """
        Convert an ``LmexOpenOrder`` to an ``OrderStatusReport``.

        Parameters
        ----------
        o : LmexOpenOrder
            Open order from the REST API.

        Returns
        -------
        OrderStatusReport

        """
        instrument_id = InstrumentId.from_str(f"{o.symbol}.{LMEX_VENUE.value}")
        instrument = self._cache.instrument(instrument_id)

        price_precision = instrument.price_precision if instrument else 8
        size_precision = instrument.size_precision if instrument else 8

        try:
            order_status = _lmex_status_to_nautilus(o.status)
        except ValueError:
            order_status = OrderStatus.INITIALIZED

        return OrderStatusReport(
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=ClientOrderId(o.clOrderId) if o.clOrderId else None,
            venue_order_id=VenueOrderId(str(o.orderId)),
            order_side=OrderSide.BUY if o.side == "BUY" else OrderSide.SELL,
            order_type=OrderType.LIMIT if o.orderType == "LIMIT" else OrderType.MARKET,
            time_in_force=None,
            order_status=order_status,
            price=Price(o.price, price_precision) if o.price else None,
            quantity=Quantity(o.size, size_precision),
            filled_qty=Quantity(o.filledSize, size_precision),
            avg_px=o.averageFillPrice,
            ts_accepted=millis_to_nanos(o.timestamp),
            ts_last=millis_to_nanos(o.timestamp),
            report_id=None,
            ts_init=self._clock.timestamp_ns(),
        )

    def _parse_fill_report(self, fill: LmexFill) -> FillReport:
        """
        Convert an ``LmexFill`` to a ``FillReport``.

        Parameters
        ----------
        fill : LmexFill
            Fill record from the REST API.

        Returns
        -------
        FillReport

        """
        instrument_id = InstrumentId.from_str(f"{fill.symbol}.{LMEX_VENUE.value}")
        instrument = self._cache.instrument(instrument_id)

        price_precision = instrument.price_precision if instrument else 8
        size_precision = instrument.size_precision if instrument else 8

        try:
            fee_currency = Currency.from_str(fill.feeCurrency)
        except Exception:
            fee_currency = Currency.from_str("USD")

        liquidity_side = (
            LiquiditySide.MAKER if (fill.maker is True) else LiquiditySide.TAKER
        )

        return FillReport(
            account_id=self.account_id,
            instrument_id=instrument_id,
            venue_order_id=VenueOrderId(str(fill.orderId)),
            client_order_id=ClientOrderId(fill.clOrderId) if fill.clOrderId else None,
            trade_id=TradeId(str(fill.tradeId)),
            order_side=OrderSide.BUY if fill.side == "BUY" else OrderSide.SELL,
            last_qty=Quantity(fill.size, size_precision),
            last_px=Price(fill.price, price_precision),
            commission=Money(fill.feeAmount, fee_currency),
            liquidity_side=liquidity_side,
            ts_event=millis_to_nanos(fill.timestamp),
            report_id=None,
            ts_init=self._clock.timestamp_ns(),
        )
