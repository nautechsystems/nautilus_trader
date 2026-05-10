# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

import asyncio
import json
import time
from decimal import Decimal
from typing import Any
from nautilus_trader.core.uuid import UUID4

from nautilus_trader.adapters.bullet.config import BulletExecClientConfig
from nautilus_trader.adapters.bullet.constants import BULLET_CLIENT_ID
from nautilus_trader.adapters.bullet.constants import BULLET_POST_ONLY_WOULD_MATCH
from nautilus_trader.adapters.bullet.constants import BULLET_VENUE
from nautilus_trader.adapters.bullet.providers import BulletInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
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
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BulletExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Bullet.xyz perpetuals exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : nautilus_pyo3.BulletHttpClient
        The Bullet HTTP client (for read-only queries).
    order_client : nautilus_pyo3.BulletOrderClient
        The Bullet order client (for signing + submitting orders).
    ws_client : nautilus_pyo3.BulletWebSocketClient
        Dedicated WebSocket connection for order update feed.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BulletInstrumentProvider
        The instrument provider.
    config : BulletExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: nautilus_pyo3.BulletHttpClient,
        order_client: nautilus_pyo3.BulletOrderClient,
        ws_client: nautilus_pyo3.BulletWebSocketClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BulletInstrumentProvider,
        config: BulletExecClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or BULLET_CLIENT_ID.value),
            venue=BULLET_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )
        self._http_client = http_client
        self._order_client = order_client
        self._ws_client = ws_client
        self._config = config

        self._set_account_id(AccountId("BULLET-master"))

        self._reconnect_task: asyncio.Task | None = None

        # Auto-incrementing numeric cloid sent to venue (NT ClientOrderId is not a valid u64).
        # Seeded from time so restarts don't collide with open orders from prior sessions.
        self._next_cloid: int = int(time.time()) % 10_000_000
        # Maps numeric cloid → NT ClientOrderId; set before REST call to avoid WS race
        self._cloid_map: dict[int, ClientOrderId] = {}
        # Reverse map: NT ClientOrderId → numeric cloid (for cancel/amend lookup)
        self._nt_to_cloid: dict[str, int] = {}
        # NT client_order_id values pending a cancel-replace amend. The CANCELED for the old
        # order is suppressed (cloid maps kept intact) so the replacement NEW can be resolved.
        self._pending_amend_cloids: set[str] = set()

    @property
    def instrument_provider(self) -> BulletInstrumentProvider:
        return self._instrument_provider  # type: ignore[return-value]

    async def _connect(self) -> None:
        await self.instrument_provider.load_all_async()
        await self._order_client.connect()

        address = self._order_client.account_address
        self._log.info(f"Connected order client. Account: {address}", LogColor.BLUE)

        await self._update_account_state(address)
        await self._await_account_registered()

        if address:
            instruments = self.instrument_provider.instruments_pyo3()
            await self._ws_client.connect(self._loop, instruments, self._handle_msg)
            await self._ws_client.wait_until_active(10.0)
            await self._ws_client.subscribe_order_updates(address)
            self._log.info(f"Subscribed to order updates for {address}", LogColor.BLUE)

            self._reconnect_task = self._loop.create_task(
                self._reconnect_monitor(address)
            )

    async def _disconnect(self) -> None:
        if self._reconnect_task is not None:
            self._reconnect_task.cancel()
            self._reconnect_task = None
        if self._ws_client.is_connected():
            await self._ws_client.close()
        self._log.info("Bullet execution client disconnected")

    async def _reconnect_monitor(self, address: str) -> None:
        """Refresh account state after WS reconnects (Rust layer handles reconnect + replay)."""
        was_connected = True
        while True:
            try:
                await asyncio.sleep(5)
                now_connected = self._ws_client.is_connected()
                if not was_connected and now_connected:
                    self._log.warning("Execution WS reconnected — refreshing account state")
                    await self._update_account_state(address)
                was_connected = now_connected
            except asyncio.CancelledError:
                break
            except Exception as e:
                self._log.warning(f"Reconnect monitor error: {e}")

    async def _update_account_state(self, address: str | None) -> None:
        if not address:
            self._log.warning("No account address — cannot fetch account state")
            return
        try:
            raw = await self._http_client.account_json(address)
            account = json.loads(raw)
            ts = self._clock.timestamp_ns()

            currency = Currency.from_str("USD")
            total = Money(Decimal(str(account.get("totalWalletBalance", "0"))), currency)
            available = Decimal(str(account.get("availableBalance", "0")))
            free = Money(min(available, Decimal(str(account.get("totalWalletBalance", "0")))), currency)
            locked = total - free

            self.generate_account_state(
                balances=[AccountBalance(total=total, locked=locked, free=free)],
                margins=[],
                reported=True,
                ts_event=ts,
            )
            self._log.info(f"Account state: total={total} free={free}")
        except Exception as e:
            self._log.warning(f"Failed to fetch account state: {e}")

    # ── WS message dispatch ───────────────────────────────────────────────────

    def _handle_msg(self, msg: Any) -> None:
        if not isinstance(msg, str):
            return
        try:
            data = json.loads(msg)
        except Exception:
            return
        event_type = data.get("e")
        if event_type == "ORDER_TRADE_UPDATE":
            self._handle_order_update(data)

    def _handle_order_update(self, data: dict) -> None:  # noqa: C901
        # Flat JSON from Rust to_flat_json(): orderId, status, side, price, origQty,
        # lastFilledQty, lastFilledPrice, T (transaction_time us), E (event_time us)
        order_id: int = data.get("orderId", 0)
        status: str = data.get("status", "")
        side: str = data.get("side", "")
        symbol: str = data.get("s", "")
        last_fill_qty = Decimal(str(data.get("lastFilledQty", "0") or "0"))
        last_fill_price = Decimal(str(data.get("lastFilledPrice", "0") or "0"))
        # E is event_time in microseconds
        ts_event = int(data.get("E", 0)) * 1_000  # µs → ns
        if ts_event == 0:
            ts_event = self._clock.timestamp_ns()

        instrument_id = InstrumentId.from_str(f"{symbol}-PERP.BULLET")
        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.debug(f"Unknown instrument {instrument_id} in order update — skipping")
            return

        venue_order_id = VenueOrderId(str(order_id))

        # Resolve NT client_order_id: check cache first (works after accepted), then cloid map
        client_order_id = self._cache.client_order_id(venue_order_id)
        if client_order_id is None:
            raw_cloid = data.get("clientOrderId")
            if raw_cloid is not None:
                try:
                    client_order_id = self._cloid_map.get(int(raw_cloid))
                except (ValueError, TypeError):
                    pass
        if client_order_id is None:
            self._log.debug(f"Cannot resolve client_order_id for venue order {order_id}")
            return

        order = self._cache.order(client_order_id)
        if order is None:
            self._log.debug(f"Order {client_order_id} not in cache")
            return

        if status == "NEW":
            if client_order_id.value in self._pending_amend_cloids:
                # Replacement order from cancel-replace amend — emit OrderUpdated
                self._pending_amend_cloids.discard(client_order_id.value)
                price_str = data.get("price", "0") or "0"
                qty_str = data.get("origQty", "0") or "0"
                try:
                    new_price = Price(float(price_str), instrument.price_precision)
                    new_qty = Quantity(float(qty_str), instrument.size_precision)
                except Exception:
                    new_price = order.price if hasattr(order, "price") else None
                    new_qty = order.quantity
                self._cache.add_venue_order_id(client_order_id, venue_order_id, overwrite=True)
                self.generate_order_updated(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    quantity=new_qty,
                    price=new_price,
                    trigger_price=None,
                    ts_event=ts_event,
                    venue_order_id_modified=True,
                )
            else:
                self.generate_order_accepted(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=ts_event,
                )

        elif status in ("PARTIALLY_FILLED", "FILLED"):
            if last_fill_qty > 0 and last_fill_price > 0:
                order_side = OrderSide.BUY if side == "BUY" else OrderSide.SELL
                self.generate_order_filled(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    venue_position_id=None,
                    trade_id=TradeId(f"{order_id}-{ts_event}"),
                    order_side=order_side,
                    order_type=order.order_type,
                    last_qty=Quantity(float(last_fill_qty), instrument.size_precision),
                    last_px=Price(float(last_fill_price), instrument.price_precision),
                    quote_currency=instrument.quote_currency,
                    commission=Money(0, instrument.quote_currency),
                    liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                    ts_event=ts_event,
                )
            if status == "FILLED":
                cloid_int = self._nt_to_cloid.pop(client_order_id.value, None)
                if cloid_int is not None:
                    self._cloid_map.pop(cloid_int, None)

        elif status == "CANCELED":
            if client_order_id.value in self._pending_amend_cloids:
                # Suppress: cancel-replace removed the old order; the replacement NEW is en route.
                # Keep cloid maps intact so the replacement can be resolved by clientOrderId.
                self._log.debug(f"Suppressing CANCELED for pending amend: {client_order_id}")
                return
            cloid_int = self._nt_to_cloid.pop(client_order_id.value, None)
            if cloid_int is not None:
                self._cloid_map.pop(cloid_int, None)
            self.generate_order_canceled(
                strategy_id=order.strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )

        elif status == "REJECTED":
            self._cloid_map.pop(order_id, None)
            self._nt_to_cloid.pop(client_order_id.value, None)
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                reason=data.get("cancelReason", "REJECTED"),
                ts_event=ts_event,
            )

    # ── Order submission ──────────────────────────────────────────────────────

    async def _place_order(self, order: object) -> None:
        instrument_id: InstrumentId = order.instrument_id  # type: ignore[attr-defined]

        raw_symbol = instrument_id.symbol.value
        bullet_symbol = raw_symbol.removesuffix("-PERP")
        if bullet_symbol == raw_symbol:
            self._log.error(
                f"Cannot submit order: {instrument_id} is not a Bullet perpetual (-PERP suffix required)"
            )
            return

        instrument = self.instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot submit order: instrument {instrument_id} not found in provider")
            return

        is_buy = order.side.name == "BUY"  # type: ignore[attr-defined]
        is_limit = order.order_type in (  # type: ignore[attr-defined]
            OrderType.LIMIT,
            OrderType.LIMIT_IF_TOUCHED,
            OrderType.STOP_LIMIT,
        )
        price_str = str(order.price) if hasattr(order, "price") and order.price else "1"  # type: ignore[attr-defined]
        qty_str = str(order.quantity)  # type: ignore[attr-defined]

        client_order_id: ClientOrderId = order.client_order_id  # type: ignore[attr-defined]

        # Use an auto-incrementing integer as the venue cloid (NT ClientOrderId is not a u64)
        cloid_int = self._next_cloid
        self._next_cloid += 1
        # Cache mappings before REST call — WS may fire status=NEW before REST returns
        self._cloid_map[cloid_int] = client_order_id
        self._nt_to_cloid[client_order_id.value] = cloid_int

        self.generate_order_submitted(
            strategy_id=order.strategy_id,  # type: ignore[attr-defined]
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        try:
            tx_id = await self._order_client.place_order(
                symbol=bullet_symbol,
                is_buy=is_buy,
                price=price_str,
                qty=qty_str,
                is_limit=is_limit,
                client_order_id=cloid_int,
                reduce_only=getattr(order, "is_reduce_only", False),
            )
            self._log.info(f"Order submitted: tx_id={tx_id} cloid={client_order_id}")
        except Exception as e:
            error_str = str(e)
            self._cloid_map.pop(cloid_int, None)
            self._nt_to_cloid.pop(client_order_id.value, None)
            if BULLET_POST_ONLY_WOULD_MATCH in error_str:
                self._log.warning(f"Post-only order would match: {error_str}")
            self.generate_order_rejected(
                strategy_id=order.strategy_id,  # type: ignore[attr-defined]
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                reason=error_str,
                ts_event=self._clock.timestamp_ns(),
            )

    async def _submit_order(self, command: SubmitOrder) -> None:
        await self._place_order(command.order)

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        for order in command.order_list.orders:
            if not order.is_closed:
                await self._place_order(order)

    async def _cancel_order(self, command: CancelOrder) -> None:
        order = self._cache.order(command.client_order_id)
        if order is None:
            self._log.warning(f"Cannot cancel: order {command.client_order_id} not in cache")
            return

        raw_symbol = order.instrument_id.symbol.value
        bullet_symbol = raw_symbol.removesuffix("-PERP")

        venue_order_id: int | None = None
        if command.venue_order_id:
            try:
                venue_order_id = int(command.venue_order_id.value)
            except (ValueError, AttributeError):
                pass

        client_order_id_int: int | None = self._nt_to_cloid.get(command.client_order_id.value)

        try:
            tx_id = await self._order_client.cancel_order(
                symbol=bullet_symbol,
                venue_order_id=venue_order_id,
                client_order_id=client_order_id_int,
            )
            self._log.info(f"Cancel submitted: tx_id={tx_id}")
        except Exception as e:
            self._log.error(f"Failed to cancel order {command.client_order_id}: {e}")

    async def _modify_order(self, command: ModifyOrder) -> None:
        order = self._cache.order(command.client_order_id)
        if order is None:
            self._log.warning(f"Cannot modify: order {command.client_order_id} not in cache")
            return

        raw_symbol = order.instrument_id.symbol.value
        bullet_symbol = raw_symbol.removesuffix("-PERP")
        is_buy = order.side.name == "BUY"

        venue_order_id: int | None = None
        if hasattr(order, "venue_order_id") and order.venue_order_id:
            try:
                venue_order_id = int(order.venue_order_id.value)
            except (ValueError, AttributeError):
                pass

        client_order_id_int: int | None = self._nt_to_cloid.get(command.client_order_id.value)

        new_price = str(command.price) if command.price else str(order.price)
        new_qty = str(command.quantity) if command.quantity else str(order.quantity)

        try:
            tx_id = await self._order_client.amend_order(
                symbol=bullet_symbol,
                is_buy=is_buy,
                venue_order_id=venue_order_id,
                client_order_id=client_order_id_int,
                new_price=new_price,
                new_qty=new_qty,
                new_client_order_id=client_order_id_int,
            )
            # Track pending cancel-replace so the CANCELED for the old order is suppressed
            # and the replacement's NEW is emitted as OrderUpdated.
            self._pending_amend_cloids.add(command.client_order_id.value)
            self._log.info(f"Amend submitted: tx_id={tx_id}")
        except Exception as e:
            self._log.error(f"Failed to amend order {command.client_order_id}: {e}")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        if command.instrument_id:
            raw_symbol = command.instrument_id.symbol.value
            bullet_symbol = raw_symbol.removesuffix("-PERP")
            try:
                tx_id = await self._order_client.cancel_market_orders(symbol=bullet_symbol)
                self._log.info(f"All market orders canceled: tx_id={tx_id}")
            except Exception as e:
                self._log.error(f"Failed to cancel market orders for {bullet_symbol}: {e}")
        else:
            try:
                tx_id = await self._order_client.cancel_all_orders()
                self._log.info(f"All orders canceled: tx_id={tx_id}")
            except Exception as e:
                self._log.error(f"Failed to cancel all orders: {e}")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        # Group by symbol so each market's cancels go in a single on-chain transaction
        by_symbol: dict[str, list[tuple[int | None, int | None]]] = {}
        for cancel in command.cancels:
            order = self._cache.order(cancel.client_order_id)
            if order is None:
                self._log.warning(f"Cannot batch-cancel: order {cancel.client_order_id} not in cache")
                continue
            raw_symbol = order.instrument_id.symbol.value
            bullet_symbol = raw_symbol.removesuffix("-PERP")
            venue_id: int | None = None
            if cancel.venue_order_id:
                try:
                    venue_id = int(cancel.venue_order_id.value)
                except (ValueError, AttributeError):
                    pass
            cloid_int: int | None = self._nt_to_cloid.get(cancel.client_order_id.value)
            by_symbol.setdefault(bullet_symbol, []).append((venue_id, cloid_int))

        for symbol, pairs in by_symbol.items():
            try:
                tx_id = await self._order_client.batch_cancel_orders(
                    symbol=symbol,
                    orders=pairs,
                )
                self._log.info(f"Batch cancel submitted for {symbol} ({len(pairs)} orders): tx_id={tx_id}")
            except Exception as e:
                self._log.error(f"Failed to batch-cancel orders for {symbol}: {e}")

    # ── Reports ───────────────────────────────────────────────────────────────

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        address = self._order_client.account_address
        if not address or not command.instrument_id:
            return None

        instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            return None

        raw_symbol = instrument.raw_symbol.value
        try:
            raw = await self._http_client.open_orders_json(address, raw_symbol)
            orders = json.loads(raw)
        except Exception as e:
            self._log.warning(f"Failed to fetch open orders for {raw_symbol}: {e}")
            return None

        target_id = command.venue_order_id.value if command.venue_order_id else None
        ts_init = self._clock.timestamp_ns()

        for o in orders:
            if target_id and str(o.get("orderId")) != target_id:
                continue
            try:
                venue_order_id = VenueOrderId(str(o["orderId"]))
                # Prefer cache lookup (works in-session); fall back to command's client_order_id
                client_order_id = (
                    self._cache.client_order_id(venue_order_id) or command.client_order_id
                )
                side = OrderSide.BUY if o.get("side") == "BUY" else OrderSide.SELL
                qty = Quantity(float(o.get("origQty", 0)), instrument.size_precision)
                filled = Quantity(float(o.get("executedQty", 0)), instrument.size_precision)
                price_val = float(o.get("price", 0))
                price = Price(price_val, instrument.price_precision) if price_val else None
                update_ns = int(o.get("updateTime", 0)) * 1_000
                return OrderStatusReport(
                    account_id=self.account_id,
                    instrument_id=command.instrument_id,
                    venue_order_id=venue_order_id,
                    client_order_id=client_order_id,
                    order_side=side,
                    order_type=OrderType.LIMIT if o.get("type") == "LIMIT" else OrderType.MARKET,
                    time_in_force=TimeInForce.GTC,
                    order_status=OrderStatus.ACCEPTED,
                    quantity=qty,
                    filled_qty=filled,
                    price=price,
                    avg_px=Decimal(str(o.get("avgPrice", "0"))) or None,
                    reduce_only=bool(o.get("reduceOnly", False)),
                    report_id=UUID4(),
                    ts_accepted=update_ns,
                    ts_last=update_ns,
                    ts_init=ts_init,
                )
            except Exception as e:
                self._log.warning(f"Failed to parse order {o.get('orderId')}: {e}")

        return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        address = self._order_client.account_address
        if not address:
            return []

        reports: list[OrderStatusReport] = []
        instruments = self.instrument_provider.get_all()
        ts_init = self._clock.timestamp_ns()

        for instrument in instruments.values():
            raw_symbol = instrument.raw_symbol.value
            try:
                raw = await self._http_client.open_orders_json(address, raw_symbol)
                orders = json.loads(raw)
            except Exception as e:
                self._log.warning(f"Failed to fetch open orders for {raw_symbol}: {e}")
                continue

            for o in orders:
                try:
                    venue_order_id = VenueOrderId(str(o["orderId"]))
                    # Prefer cache lookup so in-session orders get their NT client_order_id
                    client_order_id = self._cache.client_order_id(venue_order_id)
                    side = OrderSide.BUY if o.get("side") == "BUY" else OrderSide.SELL
                    qty = Quantity(float(o.get("origQty", 0)), instrument.size_precision)
                    filled = Quantity(float(o.get("executedQty", 0)), instrument.size_precision)
                    price_val = float(o.get("price", 0))
                    price = Price(price_val, instrument.price_precision) if price_val else None
                    update_ns = int(o.get("updateTime", 0)) * 1_000

                    reports.append(
                        OrderStatusReport(
                            account_id=self.account_id,
                            instrument_id=instrument.id,
                            venue_order_id=venue_order_id,
                            client_order_id=client_order_id,
                            order_side=side,
                            order_type=OrderType.LIMIT if o.get("type") == "LIMIT" else OrderType.MARKET,
                            time_in_force=TimeInForce.GTC,
                            order_status=OrderStatus.ACCEPTED,
                            quantity=qty,
                            filled_qty=filled,
                            price=price,
                            avg_px=Decimal(str(o.get("avgPrice", "0"))) or None,
                            reduce_only=bool(o.get("reduceOnly", False)),
                            report_id=UUID4(),
                            ts_accepted=update_ns,
                            ts_last=update_ns,
                            ts_init=ts_init,
                        )
                    )
                except Exception as e:
                    self._log.warning(f"Failed to parse open order {o.get('orderId')}: {e}")

        return reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        # Bullet does not expose a fill history REST endpoint — fills arrive via WS only
        return []

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        address = self._order_client.account_address
        if not address:
            return []

        try:
            raw = await self._http_client.account_json(address)
            account = json.loads(raw)
        except Exception as e:
            self._log.warning(f"Failed to fetch account for position reports: {e}")
            return []

        reports: list[PositionStatusReport] = []
        ts_init = self._clock.timestamp_ns()

        for pos in account.get("positions", []):
            try:
                qty = Decimal(str(pos.get("positionAmt", "0")))
                if qty == 0:
                    continue
                symbol = pos.get("symbol", "")
                instrument_id = InstrumentId.from_str(f"{symbol}-PERP.BULLET")
                instrument = self._cache.instrument(instrument_id)
                if instrument is None:
                    continue
                side = PositionSide.LONG if qty > 0 else PositionSide.SHORT
                entry = Decimal(str(pos.get("entryPrice", "0")))
                update_ns = int(pos.get("updateTime", 0)) * 1_000
                reports.append(
                    PositionStatusReport(
                        account_id=self.account_id,
                        instrument_id=instrument_id,
                        position_side=side,
                        quantity=Quantity(float(abs(qty)), instrument.size_precision),
                        avg_px_open=entry if entry > 0 else None,
                        report_id=UUID4(),
                        ts_last=update_ns,
                        ts_init=ts_init,
                    )
                )
            except Exception as e:
                self._log.warning(f"Failed to parse position {pos.get('symbol')}: {e}")

        return reports
