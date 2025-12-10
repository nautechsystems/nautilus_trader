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

from __future__ import annotations

import asyncio
import hashlib
import json
from collections import defaultdict
from datetime import datetime
from decimal import Decimal
from typing import TYPE_CHECKING, Any, Callable, Optional

from nautilus_trader.adapters.lighter.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter.constants import (
    LIGHTER_MAINNET_WS_BASE,
    LIGHTER_TESTNET_WS_BASE,
    LIGHTER_VENUE,
)
from nautilus_trader.adapters.lighter.signer import LighterSigner, SignerError
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient, WebSocketClientError, WebSocketConfig
from nautilus_trader.execution.messages import (
    CancelOrder,
    CancelAllOrders,
    GenerateOrderStatusReport,
    GenerateOrderStatusReports,
    SubmitOrder,
)
from nautilus_trader.execution.reports import FillReport, OrderStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import (
    AccountType,
    LiquiditySide,
    OmsType,
    OrderSide,
    OrderStatus,
    OrderType,
    TimeInForce,
)
from nautilus_trader.model.identifiers import (
    ClientId,
    ClientOrderId,
    InstrumentId,
    TradeId,
    Venue,
    VenueOrderId,
)
from nautilus_trader.model.objects import Money, Price, Quantity
from nautilus_trader.model.orders import Order

from nautilus_trader.core.uuid import UUID4


if TYPE_CHECKING:
    from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider


class _LighterUserStream:
    """
    Minimal private WebSocket client for account order updates.
    """

    def __init__(
        self,
        *,
        clock: LiveClock,
        loop: asyncio.AbstractEventLoop,
        url: str,
        account_index: int,
        auth_provider: Callable[[], str | None],
        handler: Callable[[dict[str, Any]], None],
        on_reconnect: Callable[[], None] | None = None,
    ) -> None:
        self._clock = clock
        self._loop = loop
        self._url = url
        self._account_index = account_index
        self._auth_provider = auth_provider
        self._handler = handler
        self._on_reconnect = on_reconnect
        self._client: WebSocketClient | None = None

    async def connect(self) -> None:
        config = WebSocketConfig(
            url=self._url,
            handler=self._on_message,
            heartbeat=15,
            headers=[],
        )
        self._client = await WebSocketClient.connect(
            config=config,
            post_reconnection=self._handle_reconnect,
        )
        await self._subscribe()

    async def close(self) -> None:
        if self._client is None:
            return
        await self._client.disconnect()
        self._client = None

    def _handle_reconnect(self) -> None:
        if self._client is None:
            return
        self._loop.create_task(self._subscribe())
        if self._on_reconnect:
            self._on_reconnect()

    async def _subscribe(self) -> None:
        if self._client is None:
            return

        token = self._auth_provider()
        if not token:
            return

        msg = {
            "type": "subscribe",
            "channel": f"account_all_orders/{self._account_index}",
            "auth": token,
        }
        try:
            await self._client.send_text(json.dumps(msg).encode("utf-8"))
        except WebSocketClientError:
            return

    def _on_message(self, payload: bytes) -> None:
        try:
            message = json.loads(payload)
        except Exception:
            return

        if isinstance(message, dict):
            self._handler(message)


class LighterExecutionClient(LiveExecutionClient):
    """
    Minimal execution client for Lighter (submit + cancel only).

    Uses the native signer to produce tx_info payloads and the Rust HTTP client
    (via PyO3) to post `sendTx`. Private WS order streams are not yet wired.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: Any,
        ws_client: Any,
        signer: LighterSigner,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: LighterInstrumentProvider,
        config: LighterExecClientConfig,
        name: str,
    ) -> None:
        self._http_client = http_client
        self._ws_client = ws_client
        self._signer = signer
        self._config = config
        self._instrument_provider = instrument_provider
        self._client_order_indices: dict[str, int] = {}
        self._strategy_order_ids: dict[str, set[str]] = defaultdict(set)
        self._filled_qty_cache: dict[str, Quantity] = {}
        self._filled_quote_cache: dict[str, Decimal] = {}
        self._auth_token: Optional[str] = None
        self._auth_token_expiry_ns: int = 0
        self._user_stream: _LighterUserStream | None = None

        super().__init__(
            loop=loop,
            client_id=ClientId(name),
            venue=LIGHTER_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

    # ---------------------------------------------------------------------------------------------
    # Connection lifecycle
    # ---------------------------------------------------------------------------------------------

    async def _connect(self) -> None:
        # Ensure auth token is primed for reconciliation calls.
        self._ensure_auth_token()
        await self._reconcile_open_orders()
        await self._start_user_stream()

    async def _disconnect(self) -> None:
        if self._user_stream is not None:
            await self._user_stream.close()

    # ---------------------------------------------------------------------------------------------
    # Command handlers
    # ---------------------------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order
        instrument = self._require_instrument(order.instrument_id)

        market_index = self._instrument_provider.market_index_for(order.instrument_id)
        if market_index is None:
            raise ValueError(f"Missing market index for {order.instrument_id}")

        price_int = self._price_to_int(instrument, order.price)
        size_int = self._size_to_int(instrument, order.quantity)
        coi = self._client_order_index(order.client_order_id.value)

        signed = await self._execute_with_retry(
            lambda nonce: self._signer.sign_create_order(
                market_index=market_index,
                client_order_index=coi,
                base_amount_int=size_int,
            price_int=price_int,
            is_ask=getattr(order.side, "is_sell", order.side == OrderSide.SELL),
            order_type=_map_order_type(order.order_type),
            time_in_force=_map_time_in_force(order.time_in_force),
            nonce=nonce,
            reduce_only=getattr(order, "reduce_only", getattr(order, "is_reduce_only", False)),
            trigger_price=self._price_to_int(instrument, order.stop_price) if getattr(order, "stop_price", None) else 0,
            order_expiry=_expiry_from_order(order),
        ),
        op_name="submit",
    )
        self._strategy_order_ids[order.strategy_id.value].add(order.client_order_id.value)
        self._log.info(f"Submitted order {order.client_order_id} tx={signed.tx_hash}")

    async def _cancel_order(self, command: CancelOrder) -> None:
        order_id = getattr(command, "order_id", None)
        instrument_id = getattr(order_id, "instrument_id", getattr(command, "instrument_id", None))
        if instrument_id is None:
            raise ValueError("CancelOrder missing instrument_id")
        instrument = self._require_instrument(instrument_id)
        market_index = self._instrument_provider.market_index_for(instrument.id)
        if market_index is None:
            raise ValueError(f"Missing market index for {instrument.id}")

        client_order_id = getattr(order_id, "client_order_id", getattr(command, "client_order_id", None))
        coi_value = client_order_id.value if hasattr(client_order_id, "value") else str(client_order_id)
        coi = self._client_order_index(coi_value)

        signed = await self._execute_with_retry(
            lambda nonce: self._signer.sign_cancel_order(
                market_index=market_index,
                order_index=coi,
                nonce=nonce,
            ),
            op_name="cancel",
        )
        strategy_id = getattr(order_id, "strategy_id", getattr(command, "strategy_id", None))
        if strategy_id is not None:
            key = strategy_id.value if hasattr(strategy_id, "value") else str(strategy_id)
            self._strategy_order_ids[key].discard(coi_value)
        self._log.info(f"Canceled order {client_order_id} tx={signed.tx_hash}")

    # ---------------------------------------------------------------------------------------------
    # Reports
    # ---------------------------------------------------------------------------------------------

    async def generate_order_status_report(self, command):
        reports, _ = await self._build_reports(instrument_id=command.instrument_id)
        for report in reports:
            if command.client_order_id and report.client_order_id == command.client_order_id:
                return report
            if command.venue_order_id and report.venue_order_id == command.venue_order_id:
                return report
        return None

    async def generate_order_status_reports(self, command):
        reports, _ = await self._build_reports(
            instrument_id=command.instrument_id,
            open_only=command.open_only,
        )
        return reports

    async def generate_fill_reports(self, command):
        _, fills = await self._build_reports(instrument_id=command.instrument_id)
        return fills

    async def generate_position_status_reports(self, command):  # pragma: no cover - PR4
        return []

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        # Placeholder: loop through active orders via REST until WS schema is known.
        token = self._ensure_auth_token()
        if not token:
            self._log.warning("Cannot cancel all orders without auth token")
            return

        strategy_id = command.strategy_id.value if command.strategy_id else None
        strategy_orders = self._strategy_order_ids.get(strategy_id, set()) if strategy_id else None
        if strategy_orders is not None and not strategy_orders:
            # After restart we may not have local state; fall back to canceling all for the instrument.
            self._log.warning("No tracked orders for strategy; cancel-all will not filter by strategy")
            strategy_orders = None
        instrument_ids = [command.instrument_id] if command.instrument_id else []
        if not instrument_ids:
            # Fallback: cancel per loaded instrument.
            instrument_ids = list(self._instrument_provider._market_index_by_instrument.keys())  # type: ignore[attr-defined]

        for instrument_id in instrument_ids:
            market_index = self._instrument_provider.market_index_for(instrument_id)  # type: ignore[arg-type]
            if market_index is None:
                continue

            resp = await self._http_client.account_active_orders(  # type: ignore[attr-defined]
                account_index=self._config.resolved_account_index or 0,
                market_id=market_index,
                auth_token=token,
            )
            orders = resp["orders"] if isinstance(resp, dict) else resp.orders
            for order in orders or []:
                client_order_id = None
                try:
                    is_ask = order.get("is_ask") if isinstance(order, dict) else getattr(order, "is_ask", None)
                    if command.order_side and is_ask is not None:
                        if command.order_side == OrderSide.BUY and is_ask is True:
                            continue
                        if command.order_side == OrderSide.SELL and is_ask is False:
                            continue

                    client_order_id = str(
                        order.get("client_order_id")
                        if isinstance(order, dict)
                        else getattr(order, "client_order_id", "")
                    )
                    if not client_order_id:
                        continue
                    if strategy_orders is not None and client_order_id not in strategy_orders:
                        # Respect per-strategy scope; skip untracked orders.
                        continue
                    coi = self._client_order_index(client_order_id)
                    await self._execute_with_retry(
                        lambda nonce: self._signer.sign_cancel_order(
                            market_index=market_index,
                            order_index=coi,
                            nonce=nonce,
                        ),
                        op_name="cancel_all",
                    )
                except Exception as exc:  # pragma: no cover - best-effort
                    cid = client_order_id or "<unknown>"
                    self._log.warning(f"cancel_all_orders failed for {cid}: {exc}")

    # ---------------------------------------------------------------------------------------------
    # Helpers
    # ---------------------------------------------------------------------------------------------

    async def _fetch_nonce(self) -> int:
        # Token is optional for nextNonce but provided if available
        token = self._ensure_auth_token()
        resp = await self._http_client.next_nonce(  # type: ignore[attr-defined]
            account_index=self._config.resolved_account_index or 0,
            api_key_index=self._config.api_key_index,
            auth_token=token,
        )
        return resp["nonce"] if isinstance(resp, dict) else resp.nonce  # PyO3 returns pyobj

    async def _post_send_tx(self, tx_type: int, tx_info: str) -> dict[str, Any]:
        resp = await self._http_client.send_tx(  # type: ignore[attr-defined]
            tx_type=tx_type,
            tx_info=tx_info,
            price_protection=True,
        )
        parsed = {
            "code": _get(resp, "code"),
            "message": _get(resp, "message"),
            "tx_hash": _get(resp, "tx_hash"),
        }
        if parsed["code"] not in (None, 200):
            raise RuntimeError(f"sendTx failed code={parsed['code']} message={parsed.get('message')}")
        return parsed

    async def _execute_with_retry(self, signer_fn, *, op_name: str) -> Any:
        last_error: Exception | None = None
        for attempt in range(self._config.max_retries):
            nonce = await self._fetch_nonce()
            try:
                signed = signer_fn(nonce)
                if asyncio.iscoroutine(signed):
                    signed = await signed
            except Exception as exc:
                last_error = exc
                self._log.warning(f"{op_name} signing failed (attempt {attempt + 1}): {exc}")
                await asyncio.sleep(self._config.retry_delay_ms / 1000)
                continue

            try:
                await self._post_send_tx(signed.tx_type, signed.tx_info)
                return signed
            except Exception as exc:
                last_error = exc
                lower = str(exc).lower()
                if "nonce" in lower:
                    self._log.info(f"{op_name} retrying after nonce error: {exc}")
                else:
                    self._log.warning(f"{op_name} sendTx failed (attempt {attempt + 1}): {exc}")
                await asyncio.sleep(self._config.retry_delay_ms / 1000)

        raise last_error or RuntimeError(f"{op_name} failed after retries")

    def _ensure_auth_token(self) -> Optional[str]:
        now = self._clock.timestamp_ns()
        if self._auth_token and now < self._auth_token_expiry_ns:
            return self._auth_token
        try:
            token = self._signer.auth_token()
        except SignerError as exc:
            self._log.warning(f"Failed to refresh auth token: {exc}")
            return None

        # signer default expiry is 10 minutes; refresh 2 minutes early.
        self._auth_token = token
        self._auth_token_expiry_ns = now + (8 * 60 * 1_000_000_000)
        return token

    def _client_order_index(self, client_order_id: str) -> int:
        """
        Convert an arbitrary client order ID into a deterministic int64 the signer accepts.

        Prefers numeric IDs; otherwise uses a stable blake2b hash (64-bit) derived from the string.
        """
        if client_order_id in self._client_order_indices:
            return self._client_order_indices[client_order_id]

        if client_order_id.isdigit():
            value = int(client_order_id)
        else:
            digest = hashlib.blake2b(client_order_id.encode("utf-8"), digest_size=8).digest()
            # Clamp to signed 63-bit range (positive) to keep signer happy.
            value = int.from_bytes(digest, "big") & ((1 << 63) - 1)
        if value == 0:
            value = 1

        self._client_order_indices[client_order_id] = value
        return value

    async def _build_reports(
        self,
        *,
        instrument_id=None,
        open_only: bool | None = None,
    ) -> tuple[list[OrderStatusReport], list[FillReport]]:
        token = self._ensure_auth_token()
        if not token:
            self._log.warning("Cannot build reports without auth token")
            return [], []

        instruments = (
            [instrument_id] if instrument_id else list(self._instrument_provider._market_index_by_instrument.keys())  # type: ignore[attr-defined]
        )
        status_reports: list[OrderStatusReport] = []
        fill_reports: list[FillReport] = []

        for instrument_key in instruments:
            instrument = self._require_instrument(instrument_key)
            market_index = self._instrument_provider.market_index_for(instrument.id)
            if market_index is None:
                continue

            resp = await self._http_client.account_active_orders(  # type: ignore[attr-defined]
                account_index=self._config.resolved_account_index or 0,
                market_id=market_index,
                auth_token=token,
            )
            orders = resp["orders"] if isinstance(resp, dict) else resp.orders
            if not orders:
                continue

            reports, fills = self._parse_active_orders(
                orders,
                instrument=instrument,
                open_only=open_only,
            )
            status_reports.extend(reports)
            fill_reports.extend(fills)

        return status_reports, fill_reports

    def _parse_active_orders(
        self,
        orders: list[Any],
        *,
        instrument,
        open_only: bool | None = None,
    ) -> tuple[list[OrderStatusReport], list[FillReport]]:
        reports: list[OrderStatusReport] = []
        fills: list[FillReport] = []

        for order in orders:
            order_status = _order_status(order)
            if open_only and order_status not in {OrderStatus.ACCEPTED, OrderStatus.PARTIALLY_FILLED}:
                continue

            status_report, fill_report = self._active_order_to_reports(order, instrument)
            if status_report:
                reports.append(status_report)
            if fill_report:
                fills.append(fill_report)

        return reports, fills

    def _active_order_to_reports(self, order: Any, instrument: Any) -> tuple[OrderStatusReport | None, FillReport | None]:
        order_index = _get(order, "order_index", "order_id")
        client_order_id = _get(order, "client_order_id", "client_order_index") or str(order_index)
        if not order_index:
            return None, None

        quantity = Quantity.from_str(str(_get(order, "initial_base_amount", default="0")))
        if quantity <= Quantity.zero():
            self._log.warning("Skipping order with non-positive quantity for %s", instrument.id)
            return None, None
        filled_qty = Quantity.from_str(str(_get(order, "filled_base_amount", default="0")))
        remaining_qty = Quantity.from_str(str(_get(order, "remaining_base_amount", default="0")))
        if filled_qty == Quantity.zero():
            filled_qty_calc = quantity - remaining_qty if quantity > remaining_qty else Quantity.zero()
            filled_qty = filled_qty_calc if isinstance(filled_qty_calc, Quantity) else Quantity.from_str(str(filled_qty_calc))

        price_raw = _get(order, "price")
        price = Price.from_str(str(price_raw)) if price_raw not in (None, "") else None

        ts_created = _to_nanos(_get(order, "created_at", "timestamp"), self._clock.timestamp_ns())
        ts_last = _to_nanos(_get(order, "updated_at", "timestamp"), self._clock.timestamp_ns())

        order_status = _order_status(order, filled_qty=filled_qty, quantity=quantity)
        order_side = OrderSide.SELL if bool(_get(order, "is_ask")) else OrderSide.BUY
        order_type = _order_type(order)
        tif = _tif(order)

        avg_px = None
        filled_quote = _get(order, "filled_quote_amount")
        if filled_quote is not None and filled_qty > Quantity.zero():
            try:
                avg_px = Decimal(str(filled_quote)) / Decimal(str(filled_qty))
            except Exception:
                avg_px = None

        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=instrument.id,
            venue_order_id=VenueOrderId(str(order_index)),
            order_side=order_side,
            order_type=order_type,
            time_in_force=tif,
            order_status=order_status,
            quantity=quantity,
            filled_qty=filled_qty,
            report_id=UUID4(),
            ts_accepted=ts_created,
            ts_last=ts_last,
            ts_init=self._clock.timestamp_ns(),
            client_order_id=ClientOrderId(str(client_order_id)) if client_order_id else None,
            price=price,
            avg_px=avg_px,
            reduce_only=bool(_get(order, "reduce_only", default=False)),
            cancel_reason=None,
        )

        fill_report = self._build_fill_report(
            order_index=str(order_index),
            client_order_id=client_order_id,
            instrument=instrument,
            order_side=order_side,
            price=price,
            filled_quote=_decimal_or_none(filled_quote),
            filled_qty=filled_qty,
            ts_event=ts_last,
        )

        return report, fill_report

    def _build_fill_report(
        self,
        *,
        order_index: str,
        client_order_id: str | None,
        instrument: Any,
        order_side: OrderSide,
        price: Price | None,
        filled_quote: Decimal | None,
        filled_qty: Quantity,
        ts_event: int,
    ) -> FillReport | None:
        prev_filled = self._filled_qty_cache.get(order_index, Quantity.zero())
        prev_quote = self._filled_quote_cache.get(order_index, Decimal("0"))
        if filled_qty <= prev_filled:
            return None

        if filled_quote is None or filled_quote <= Decimal("0"):
            return None

        last_qty = filled_qty - prev_filled
        quote_delta = filled_quote - prev_quote if filled_quote is not None else Decimal("0")
        px_dec: Decimal | None = None
        if last_qty > Quantity.zero() and quote_delta > Decimal("0"):
            px_dec = quote_delta / Decimal(str(last_qty))
        elif filled_quote is not None and filled_qty > Quantity.zero():
            px_dec = filled_quote / Decimal(str(filled_qty))

        px = _price_from_decimal(instrument, px_dec) if px_dec is not None else price or Price.from_str("0")
        self._filled_qty_cache[order_index] = filled_qty
        if filled_quote is not None:
            self._filled_quote_cache[order_index] = filled_quote

        return FillReport(
            account_id=self.account_id,
            instrument_id=instrument.id,
            venue_order_id=VenueOrderId(order_index),
            trade_id=TradeId(str(UUID4())),
            order_side=order_side,
            last_qty=last_qty,
            last_px=px,
            commission=Money(0, instrument.quote_currency),
            liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            report_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
            client_order_id=ClientOrderId(str(client_order_id)) if client_order_id else None,
        )

    def _require_instrument(self, instrument_id) -> Any:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            raise ValueError(f"Instrument not loaded: {instrument_id}")
        return instrument

    @staticmethod
    def _price_scale(instrument: Any) -> int:
        if hasattr(instrument, "price_precision"):
            return int(getattr(instrument, "price_precision", 0))
        inc = getattr(instrument, "price_increment", None)
        if inc:
            try:
                return max(0, -Decimal(str(inc)).as_tuple().exponent)
            except Exception:
                return 0
        return 0

    def _price_to_int(self, instrument: Any, price: Price | None) -> int:
        if price is None:
            return 0
        converter = getattr(instrument, "price_to_int", None)
        if callable(converter):
            try:
                return converter(price)
            except Exception:
                pass
        scale = self._price_scale(instrument)
        return int(Decimal(str(price)) * (Decimal(10) ** scale))

    @staticmethod
    def _size_scale(instrument: Any) -> int:
        if hasattr(instrument, "size_precision"):
            return int(getattr(instrument, "size_precision", 0))
        inc = getattr(instrument, "size_increment", None)
        if inc:
            try:
                return max(0, -Decimal(str(inc)).as_tuple().exponent)
            except Exception:
                return 0
        return 0

    def _size_to_int(self, instrument: Any, quantity: Quantity) -> int:
        converter = getattr(instrument, "size_to_int", None)
        if callable(converter):
            try:
                return converter(quantity)
            except Exception:
                pass
        scale = self._size_scale(instrument)
        return int(Decimal(str(quantity)) * (Decimal(10) ** scale))

    async def _reconcile_open_orders(self) -> None:
        reports, fills = await self._build_reports()
        for report in reports:
            self._send_order_status_report(report)
        for fill in fills:
            self._send_fill_report(fill)
        if reports or fills:
            self._log.info("Reconciled %d open orders (fills=%d)", len(reports), len(fills))

    # ---------------------------------------------------------------------------------------------
    # WebSocket handling
    # ---------------------------------------------------------------------------------------------

    async def _start_user_stream(self) -> None:
        if self._user_stream is not None:
            return

        base_url = (
            self._config.base_url_ws
            or (LIGHTER_TESTNET_WS_BASE if self._config.testnet else LIGHTER_MAINNET_WS_BASE)
        )
        account_index = self._config.resolved_account_index
        if account_index is None:
            self._log.warning("Cannot start user stream without account index")
            return

        self._user_stream = _LighterUserStream(
            clock=self._clock,
            loop=self._loop,
            url=base_url,
            account_index=account_index,
            auth_provider=self._ensure_auth_token,
            handler=self._handle_user_stream_message,
            on_reconnect=lambda: self._loop.create_task(self._reconcile_open_orders()),
        )
        try:
            await self._user_stream.connect()
        except Exception as exc:  # pragma: no cover - defensive
            self._log.exception("Failed to connect user stream", exc)
            self._user_stream = None

    def _handle_user_stream_message(self, message: dict[str, Any]) -> None:
        msg_type = message.get("type")
        if msg_type not in {"update/account_all_orders", "update/account_orders"}:
            return

        orders_by_market = message.get("orders") or {}
        if not isinstance(orders_by_market, dict):
            return

        for market_index_str, orders in orders_by_market.items():
            try:
                market_index = int(market_index_str)
            except Exception:
                continue

            instrument = self._instrument_for_market_index(market_index)
            if instrument is None:
                self._log.warning("Skipping WS order update for unknown market %s", market_index)
                continue

            if not isinstance(orders, list):
                continue

            reports, fills = self._parse_active_orders(orders, instrument=instrument)
            for report in reports:
                self._send_order_status_report(report)
            for fill in fills:
                self._send_fill_report(fill)

    def _instrument_for_market_index(self, market_index: int):
        lookup = getattr(self._instrument_provider, "_market_index_by_instrument", {})  # type: ignore[attr-defined]
        for instrument_key, idx in lookup.items():
            if idx == market_index:
                try:
                    instrument_id = InstrumentId.from_str(instrument_key)
                except Exception:
                    continue

                instrument = self._instrument_provider.find(instrument_id)
                if instrument:
                    return instrument
        return None


def _get(order: Any, *keys: str, default=None):
    for key in keys:
        if isinstance(order, dict) and key in order:
            return order[key]
        if hasattr(order, key):
            try:
                return getattr(order, key)
            except Exception:  # pragma: no cover - defensive
                continue
    return default


def _map_order_type(order_type: OrderType) -> int:
    mapping = {
        OrderType.MARKET: 1,
        OrderType.STOP_MARKET: 2,
        OrderType.STOP_LIMIT: 3,
    }
    return mapping.get(order_type, 0)


def _map_time_in_force(time_in_force: TimeInForce) -> int:
    mapping = {
        TimeInForce.IOC: 0,
        TimeInForce.GTC: 1,
        TimeInForce.FOK: 0,
    }
    return mapping.get(time_in_force, 1)


def _order_status(order: Any, *, filled_qty: Quantity | None = None, quantity: Quantity | None = None) -> OrderStatus:
    raw = (_get(order, "status", default="") or "").lower()
    if raw == "partial":
        return OrderStatus.PARTIALLY_FILLED
    if raw in {"filled", "success"}:
        return OrderStatus.FILLED
    if raw in {"cancelled", "canceled"}:
        return OrderStatus.CANCELED
    if raw == "expired":
        return OrderStatus.EXPIRED
    if raw == "rejected":
        return OrderStatus.REJECTED
    if raw == "open":
        return OrderStatus.ACCEPTED
    if filled_qty is not None and quantity is not None:
        if filled_qty > Quantity.zero() and filled_qty < quantity:
            return OrderStatus.PARTIALLY_FILLED
        if filled_qty >= quantity:
            return OrderStatus.FILLED
    return OrderStatus.ACCEPTED


def _order_type(order: Any) -> OrderType:
    raw = (_get(order, "type", default="") or "").lower()
    if raw in {"market"}:
        return OrderType.MARKET
    if raw in {"stop", "stop-loss", "stop_market"}:
        return OrderType.STOP_MARKET
    if raw in {"stop-limit", "stop_limit"}:
        return OrderType.STOP_LIMIT
    return OrderType.LIMIT


def _tif(order: Any) -> TimeInForce:
    raw = (_get(order, "time_in_force", default="") or "").lower()
    if raw in {"immediate-or-cancel", "ioc"}:
        return TimeInForce.IOC
    if raw in {"fill-or-kill", "fok"}:
        return TimeInForce.FOK
    return TimeInForce.GTC


def _expiry_from_order(order: Order) -> int:
    if order.expire_time is None:
        return -1
    if isinstance(order.expire_time, datetime):
        return int(order.expire_time.timestamp() * 1000)
    return -1


def _to_nanos(raw_ts: Any, fallback: int) -> int:
    try:
        value = int(raw_ts)
    except Exception:
        return fallback

    if value > 10_000_000_000_000_000:  # already ns
        return value
    if value > 10_000_000_000_000:  # ms
        return value * 1_000
    if value > 10_000_000_000:  # seconds with ms?
        return value * 1_000_000
    return value * 1_000_000_000


def _decimal_or_none(value: Any) -> Decimal | None:
    try:
        if value is None or value == "":
            return None
        return Decimal(str(value))
    except Exception:
        return None


def _price_from_decimal(instrument: Any, px: Decimal) -> Price:
    precision = getattr(instrument, "price_precision", 8) or 8
    formatted = format(px, f".{precision}f")
    return Price.from_str(formatted)
