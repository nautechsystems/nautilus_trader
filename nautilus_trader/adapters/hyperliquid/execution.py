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
import math
from collections import deque
from dataclasses import dataclass
from decimal import ROUND_CEILING
from decimal import ROUND_FLOOR
from decimal import Decimal
from time import perf_counter_ns
from typing import Any

from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_POST_ONLY_WOULD_MATCH
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.transformers import transform_order_to_pyo3
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.datadog import enabled as datadog_enabled
from nautilus_trader.datadog import gauge as datadog_gauge
from nautilus_trader.datadog import histogram as datadog_histogram
from nautilus_trader.datadog import increment as datadog_increment
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.functions import order_side_to_pyo3
from nautilus_trader.model.functions import order_type_to_pyo3
from nautilus_trader.model.functions import time_in_force_to_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


DATADOG_WS_HEALTH_INTERVAL = 10.0


@dataclass(slots=True)
class _OrderRttTiming:
    order_command: str
    order_role: str
    instrument_id: str
    strategy_id: str
    order_type: str
    side: str
    reduce_only: bool
    started_ns: int
    ack_recorded: bool = False


def _tag_value(value: object) -> str:
    return str(getattr(value, "value", value))


def _enum_tag(value: object) -> str:
    return str(getattr(value, "name", value))


class HyperliquidExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Hyperliquid decentralized exchange (DEX).

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.HyperliquidHttpClient
        The Hyperliquid HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : HyperliquidInstrumentProvider
        The instrument provider.
    config : HyperliquidExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.
    account_address : str, optional
        The resolved execution account address for REST queries and WebSocket subscriptions.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.HyperliquidHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: HyperliquidInstrumentProvider,
        config: HyperliquidExecClientConfig,
        name: str | None = None,
        account_address: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or HYPERLIQUID_VENUE.value),
            venue=HYPERLIQUID_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Configuration
        self._config = config
        self._client = client
        self._instrument_provider: HyperliquidInstrumentProvider = instrument_provider

        # Log configuration details
        environment = config.environment or nautilus_pyo3.HyperliquidEnvironment.MAINNET
        self._log.info(f"config.environment={environment}", LogColor.BLUE)
        self._log.info(f"config.http_timeout_secs={config.http_timeout_secs}", LogColor.BLUE)
        self._log.info(f"config.normalize_prices={config.normalize_prices}", LogColor.BLUE)
        self._log.info(
            f"config.include_builder_attribution={config.include_builder_attribution}",
            LogColor.BLUE,
        )
        self._log.info(f"{config.proxy_url=}", LogColor.BLUE)

        account_id = AccountId(f"{name or HYPERLIQUID_VENUE.value}-master")
        self._set_account_id(account_id)

        # WebSocket client for order/execution updates (user-level, not product-specific)
        self._ws_client = nautilus_pyo3.HyperliquidWebSocketClient(
            url=config.base_url_ws,
            environment=environment,
            account_id=str(account_id),
            proxy_url=config.proxy_url,
        )
        self._ws_client.set_post_timeout(config.ws_post_timeout_secs)

        # Caches to handle race conditions and duplicate messages
        self._processed_trade_ids: nautilus_pyo3.FifoCache = nautilus_pyo3.FifoCache()
        self._accepted_orders: nautilus_pyo3.FifoCache = nautilus_pyo3.FifoCache()
        self._terminal_orders: nautilus_pyo3.FifoCache = nautilus_pyo3.FifoCache()
        self._pending_filled: set[str] = set()
        # Compatibility view: client_order_id.value to the front pending
        # modify's old venue_order_id.value, when known. None means the modify
        # targeted the stable CLOID before the exchange venue order id was known.
        self._pending_modify_keys: dict[str, str | None] = {}
        # User-intended absolute total qty and price for the front pending
        # modify; the cancel-replace promotion uses these for an accurate
        # OrderUpdated.
        self._pending_modify_target_qty: dict[str, Quantity] = {}
        self._pending_modify_target_price: dict[str, Price] = {}
        self._pending_modify_chains: dict[str, list[dict[str, object]]] = {}
        self._next_modify_generation = 1
        self._cached_venue_order_id_ts: dict[str, int] = {}
        self._superseded_venue_order_ids: dict[str, set[str]] = {}
        self._datadog_order_rtt_timings: dict[str, deque[_OrderRttTiming]] = {}
        self._datadog_ws_health_task: asyncio.Task | None = None
        self._datadog_ws_was_active = False

        # FillReports buffered when fill arrives before order is in cache,
        # drained on OrderAccepted.
        self._pending_fills: dict[str, list[nautilus_pyo3.FillReport]] = {}

        self._fee_refresh_task: asyncio.Task | None = None

        self._account_address = account_address
        if self._account_address is None:
            try:
                self._account_address = nautilus_pyo3.hyperliquid_resolve_execution_account_address(
                    private_key=config.private_key,
                    vault_address=config.vault_address,
                    account_address=config.account_address,
                    environment=environment,
                )
            except Exception as e:
                self._log.warning(f"Could not resolve account address: {e}")

        if self._account_address:
            self._log.info(
                f"Account address (REST/WS subscriptions): {self._account_address}",
                LogColor.BLUE,
            )

    @property
    def hyperliquid_instrument_provider(self) -> HyperliquidInstrumentProvider:
        return self._instrument_provider

    async def _split_outcome(self, outcome: int, amount: Decimal) -> str:
        return await self._client.submit_split_outcome(outcome, amount)

    async def _merge_outcome(self, outcome: int, amount: Decimal | None = None) -> str:
        # `amount=None` serializes as JSON `null`, which the venue treats as the max mergeable balance
        return await self._client.submit_merge_outcome(outcome, amount)

    async def _merge_question(self, question: int, amount: Decimal | None = None) -> str:
        return await self._client.submit_merge_question(question, amount)

    async def _negate_outcome(self, question: int, outcome: int, amount: Decimal) -> str:
        return await self._client.submit_negate_outcome(question, outcome, amount)

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self._instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._client.cache_instrument(inst)

        # Cache spot fill coin mappings for WebSocket fill processing
        spot_fill_coins = self._client.get_spot_fill_coin_mapping()
        self._ws_client.cache_spot_fill_coins(spot_fill_coins)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def _connect(self) -> None:
        self._log.info("Loading instruments...", LogColor.BLUE)
        await self._instrument_provider.initialize()
        self._cache_instruments()
        self._client.set_account_id(str(self.account_id))

        self._log.info(
            f"Loaded {len(self._instrument_provider.list_all())} instruments",
            LogColor.GREEN,
        )

        await self._update_account_state()
        await self._await_account_registered()

        self._sync_cloid_cache()

        instruments = self._instrument_provider.instruments_pyo3()

        await self._ws_client.connect(self._loop, instruments, self._handle_msg)
        self._log.info(f"Connected to WebSocket {self._ws_client.url}", LogColor.BLUE)
        self._start_datadog_ws_health_monitor()

        if self._account_address:
            await self._ws_client.subscribe_order_updates(self._account_address)
            self._log.info(
                f"Subscribed to order updates for {self._account_address}",
                LogColor.BLUE,
            )

            await self._ws_client.subscribe_user_events(self._account_address)
            self._log.info(
                f"Subscribed to user events (includes fills) for {self._account_address}",
                LogColor.BLUE,
            )

    def _start_datadog_ws_health_monitor(self) -> None:
        if not datadog_enabled():
            return
        if self._datadog_ws_health_task is not None and not self._datadog_ws_health_task.done():
            return

        self._datadog_ws_was_active = bool(self._ws_client.is_active())
        self._datadog_ws_health_task = self._loop.create_task(
            self._datadog_ws_health_loop(),
            name="hyperliquid-exec-datadog-ws-health",
        )

    def _stop_datadog_ws_health_monitor(self) -> None:
        if self._datadog_ws_health_task is not None:
            self._datadog_ws_health_task.cancel()
            self._datadog_ws_health_task = None
        self._datadog_ws_was_active = False

    async def _datadog_ws_health_loop(self) -> None:
        try:
            while True:
                active = bool(self._ws_client.is_active())
                datadog_gauge(
                    "adapter.ws.connected",
                    1.0 if active else 0.0,
                    tags=(f"venue:{HYPERLIQUID_VENUE.value}", "adapter:execution"),
                )
                if active and not self._datadog_ws_was_active:
                    datadog_increment(
                        "adapter.ws_reconnect",
                        tags=(f"venue:{HYPERLIQUID_VENUE.value}", "adapter:execution"),
                    )
                self._datadog_ws_was_active = active
                await asyncio.sleep(DATADOG_WS_HEALTH_INTERVAL)
        except asyncio.CancelledError:
            return

    def _sync_cloid_cache(self) -> None:
        orders = self._cache.orders(venue=self.venue)
        if not orders:
            return

        count = 0

        for order in orders:
            if order.is_closed:
                continue

            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
            cloid = nautilus_pyo3.hyperliquid_cloid_from_client_order_id(pyo3_client_order_id)
            self._ws_client.cache_cloid_mapping(cloid, pyo3_client_order_id)
            count += 1

        if count > 0:
            self._log.info(f"Cached cloid mappings for {count} existing order(s)", LogColor.BLUE)

    def _cleanup_cloid_mapping(self, client_order_id: ClientOrderId) -> None:
        self._pending_fills.pop(client_order_id.value, None)
        try:
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(client_order_id.value)
            cloid = nautilus_pyo3.hyperliquid_cloid_from_client_order_id(pyo3_client_order_id)
            self._ws_client.remove_cloid_mapping(cloid)
        except Exception as e:
            self._log.debug(f"Failed to cleanup cloid mapping for {client_order_id!r}: {e}")

    async def _update_account_state(self) -> None:
        pyo3_account_state = await self._client.request_account_state()
        account_state = AccountState.from_dict(pyo3_account_state.to_dict())

        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

        if account_state.balances:
            self._log.info(
                f"Generated account state with {len(account_state.balances)} balance(s)",
            )

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        if not self._ws_client.is_closed():
            self._log.info("Disconnecting WebSocket")
            await self._ws_client.close()
            datadog_gauge(
                "adapter.ws.connected",
                0.0,
                tags=(f"venue:{HYPERLIQUID_VENUE.value}", "adapter:execution"),
            )
            self._stop_datadog_ws_health_monitor()

            # Clear cloid cache to prevent unbounded memory growth
            self._ws_client.clear_cloid_cache()
            self._log.info(
                f"Disconnected from WebSocket {self._ws_client.url}",
                LogColor.BLUE,
            )
        self._stop_datadog_ws_health_monitor()

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        try:
            venue_order_id = command.venue_order_id.value if command.venue_order_id else None
            client_order_id = command.client_order_id.value if command.client_order_id else None

            if venue_order_id is None and client_order_id is None:
                self._log.warning(
                    "Cannot generate order status report without venue_order_id or client_order_id",
                )
                return None

            pyo3_report = await self._client.request_order_status_report(
                venue_order_id=venue_order_id,
                client_order_id=client_order_id,
            )

            if pyo3_report is None:
                self._log.warning(
                    f"No order status report found for client_order_id={command.client_order_id}, "
                    f"venue_order_id={command.venue_order_id}",
                )
                return None

            report = OrderStatusReport.from_pyo3(pyo3_report)
            report.client_order_id = self._resolve_cloid(report.client_order_id)

            if self._is_external_order(report.client_order_id) and report.venue_order_id:
                resolved_id = self._cache.client_order_id(report.venue_order_id)
                if resolved_id:
                    report.client_order_id = resolved_id

            self._promote_replacement_if_inflight_modify(report)

            if self._is_inflight_modify_old_leg_cancel(report):
                self._log.debug(
                    f"Suppressing in-flight modify old-leg CANCELED for "
                    f"{report.client_order_id!r}, venue_order_id={report.venue_order_id!r}",
                )
                return None

            self._log.debug(f"Found order status report: {report}")
            return report
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "OrderStatusReport")
            return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        try:
            instrument_id = command.instrument_id.value if command.instrument_id else None
            pyo3_reports = await self._client.request_order_status_reports(
                instrument_id=instrument_id,
            )

            reports = []

            for pyo3_report in pyo3_reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)

                report.client_order_id = self._resolve_cloid(report.client_order_id)

                if self._is_external_order(report.client_order_id) and report.venue_order_id:
                    resolved_id = self._cache.client_order_id(report.venue_order_id)
                    if resolved_id:
                        report.client_order_id = resolved_id

                self._promote_replacement_if_inflight_modify(report)

                if self._is_inflight_modify_old_leg_cancel(report):
                    self._log.debug(
                        f"Suppressing in-flight modify old-leg CANCELED for "
                        f"{report.client_order_id!r}, venue_order_id={report.venue_order_id!r}",
                    )
                    continue

                reports.append(report)

            self._log_report_receipt(
                len(reports),
                "OrderStatusReport",
                command.log_receipt_level,
                "Generated",
            )
            return reports
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "OrderStatusReports")
            return []

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        try:
            instrument_id = command.instrument_id.value if command.instrument_id else None
            pyo3_reports = await self._client.request_fill_reports(instrument_id=instrument_id)

            reports = []

            for pyo3_report in pyo3_reports:
                report = FillReport.from_pyo3(pyo3_report)

                report.client_order_id = self._resolve_cloid(report.client_order_id)

                if self._is_external_order(report.client_order_id) and report.venue_order_id:
                    resolved_id = self._cache.client_order_id(report.venue_order_id)
                    if resolved_id:
                        report.client_order_id = resolved_id

                reports.append(report)

            self._log_report_receipt(len(reports), "FillReport", LogLevel.INFO, "Generated")
            return reports
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "FillReports")
            return []

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        try:
            instrument_id = command.instrument_id.value if command.instrument_id else None
            pyo3_reports = await self._client.request_position_status_reports(
                instrument_id=instrument_id,
            )

            reports = [PositionStatusReport.from_pyo3(r) for r in pyo3_reports]

            self._log_report_receipt(
                len(reports),
                "PositionStatusReport",
                command.log_receipt_level,
            )

            return reports
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "PositionStatusReports")
            return []

    async def _request_and_process_fills_for_order(
        self,
        order: Any,
        venue_order_id: VenueOrderId,
    ) -> None:
        try:
            pyo3_reports = await self._client.request_fill_reports(
                instrument_id=order.instrument_id.value,
            )

            instrument = self._cache.instrument(order.instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot process fills - instrument {order.instrument_id} not found",
                )
                return

            for pyo3_report in pyo3_reports:
                report = FillReport.from_pyo3(pyo3_report)
                if report.venue_order_id != venue_order_id:
                    continue

                self._log.debug(f"Processing fill for order {order.client_order_id}: {report}")

                self.generate_order_filled(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=venue_order_id,
                    venue_position_id=report.venue_position_id,
                    trade_id=report.trade_id,
                    order_side=order.side,
                    order_type=order.order_type,
                    last_qty=report.last_qty,
                    last_px=report.last_px,
                    quote_currency=instrument.quote_currency,
                    commission=report.commission,
                    liquidity_side=report.liquidity_side,
                    ts_event=report.ts_event,
                )
        except Exception as e:
            self._log.error(f"Failed to request fill reports for {order.client_order_id}: {e}")

    async def _query_account(self, command: QueryAccount) -> None:
        try:
            await self._update_account_state()
        except Exception as e:
            self._log.error(f"Failed to query account state: {e}")

    async def _wait_for_quote(
        self,
        instrument_id: Any,
        timeout_secs: float = 5.0,
        poll_interval_secs: float = 0.1,
    ) -> Any | None:
        elapsed = 0.0
        while elapsed < timeout_secs:
            quote = self._cache.quote_tick(instrument_id)
            if quote is not None:
                return quote
            await asyncio.sleep(poll_interval_secs)
            elapsed += poll_interval_secs
        return None

    def _round_to_significant_figures(self, value: Decimal, sig_figs: int = 5) -> Decimal:
        # Hyperliquid requires max 5 significant figures for prices
        if value == 0:
            return Decimal(0)

        abs_val = abs(float(value))
        # Find order of magnitude (position of first significant digit)
        magnitude = math.floor(math.log10(abs_val))
        # Calculate the shift needed to round to sig_figs
        shift = sig_figs - 1 - magnitude
        factor = Decimal(10) ** shift
        rounded = (value * factor).quantize(Decimal(1)) / factor
        return rounded

    async def _calculate_market_order_price(self, order: Any) -> Any:
        # Default slippage: 0.5% for market orders
        slippage_pct = Decimal("0.005")

        # Get the quote from cache, waiting briefly if not available
        quote = self._cache.quote_tick(order.instrument_id)
        if quote is None:
            self._log.info(
                f"No cached quote for {order.instrument_id}, waiting for quote data...",
            )
            quote = await self._wait_for_quote(order.instrument_id)

        instrument = self._cache.instrument(order.instrument_id)

        if quote is None or instrument is None:
            self._log.error(
                f"Cannot calculate market order price: no cached quote for {order.instrument_id}. "
                "Ensure quote data is subscribed before submitting market orders.",
            )
            raise ValueError(
                f"No cached quote available for {order.instrument_id} to calculate market order price",
            )

        # Use trigger price as base for trigger orders to ensure limit_px
        # satisfies Hyperliquid's constraint (SELL: limit_px <= triggerPx,
        # BUY: limit_px >= triggerPx)
        if (
            order.order_type in (OrderType.STOP_MARKET, OrderType.MARKET_IF_TOUCHED)
            and order.has_trigger_price
        ):
            base_price = Decimal(str(order.trigger_price))
        elif order.side == OrderSide.BUY:
            base_price = Decimal(str(quote.ask_price))
        else:
            base_price = Decimal(str(quote.bid_price))

        # Calculate price with slippage
        if order.side == OrderSide.BUY:
            price = base_price * (Decimal(1) + slippage_pct)
        else:
            price = base_price * (Decimal(1) - slippage_pct)

        # Hyperliquid requires max 5 significant figures AND max decimal places
        price = self._round_to_significant_figures(price, sig_figs=5)

        # TODO: Extract this to Rust
        # Round in the direction that preserves slippage buffer
        quantizer = Decimal(10) ** -instrument.price_precision

        if order.side == OrderSide.BUY:
            price = price.quantize(quantizer, rounding=ROUND_CEILING)
        else:
            price = price.quantize(quantizer, rounding=ROUND_FLOOR)

        # Strip trailing zeros to avoid exceeding 5 significant figures
        price = price.normalize()

        self._log.debug(
            f"Calculated market order price: {price} (base: {base_price}, slippage: {slippage_pct})",
        )

        return nautilus_pyo3.Price.from_decimal(price)

    def _derive_limit_from_trigger(self, order, trigger_price) -> Decimal:
        slippage_pct = Decimal("0.005")
        base_price = Decimal(str(trigger_price))

        if order.side == OrderSide.BUY:
            price = base_price * (Decimal(1) + slippage_pct)
        else:
            price = base_price * (Decimal(1) - slippage_pct)

        price = self._round_to_significant_figures(price, sig_figs=5)

        instrument = self._cache.instrument(order.instrument_id)
        if instrument is not None:
            quantizer = Decimal(10) ** -instrument.price_precision

            if order.side == OrderSide.BUY:
                price = price.quantize(quantizer, rounding=ROUND_CEILING)
            else:
                price = price.quantize(quantizer, rounding=ROUND_FLOOR)

        return price.normalize()

    def _check_time_in_force(self, order) -> bool:
        if order.time_in_force not in (TimeInForce.GTC, TimeInForce.IOC):
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"UNSUPPORTED_TIME_IN_FORCE: {TimeInForce(order.time_in_force).name} not supported by Hyperliquid",
                ts_event=self._clock.timestamp_ns(),
            )
            return False
        return True

    def _track_order_rtt(
        self,
        client_order_id: ClientOrderId,
        *,
        order_command: str,
        order_role: str,
        instrument_id: object,
        strategy_id: object,
        order_type: object,
        side: object,
        reduce_only: bool,
    ) -> _OrderRttTiming | None:
        if not datadog_enabled():
            return None

        timing = _OrderRttTiming(
            order_command=order_command,
            order_role=order_role,
            instrument_id=_tag_value(instrument_id),
            strategy_id=_tag_value(strategy_id),
            order_type=_enum_tag(order_type),
            side=_enum_tag(side),
            reduce_only=reduce_only,
            started_ns=perf_counter_ns(),
        )
        self._datadog_order_rtt_timings.setdefault(client_order_id.value, deque()).append(timing)
        return timing

    def _record_order_rtt_ack(
        self,
        timing: _OrderRttTiming | None,
        *,
        outcome: str,
        keep_for_confirmation: bool = True,
    ) -> None:
        if timing is None:
            return

        timing.ack_recorded = True
        self._emit_order_rtt(timing, phase="ack", outcome=outcome)
        if not keep_for_confirmation:
            self._remove_order_rtt_timing(timing)

    def _record_order_rtt_confirmation(
        self,
        client_order_id: ClientOrderId,
        *,
        outcome: str,
    ) -> None:
        timings = self._datadog_order_rtt_timings.get(client_order_id.value)
        if not timings:
            return

        timing = timings.popleft()
        if not timings:
            self._datadog_order_rtt_timings.pop(client_order_id.value, None)

        self._emit_order_rtt(timing, phase="confirmation", outcome=outcome)

    def _remove_order_rtt_timing(self, timing: _OrderRttTiming) -> None:
        for key, timings in list(self._datadog_order_rtt_timings.items()):
            try:
                timings.remove(timing)
            except ValueError:
                continue

            if not timings:
                self._datadog_order_rtt_timings.pop(key, None)
            return

    def _emit_order_rtt(
        self,
        timing: _OrderRttTiming,
        *,
        phase: str,
        outcome: str,
    ) -> None:
        elapsed_ms = (perf_counter_ns() - timing.started_ns) / 1_000_000
        datadog_histogram(
            "order.rtt_ms",
            elapsed_ms,
            tags=(
                f"venue:{HYPERLIQUID_VENUE.value}",
                f"instrument:{timing.instrument_id}",
                f"strategy:{timing.strategy_id}",
                f"phase:{phase}",
                f"order_command:{timing.order_command}",
                f"order_role:{timing.order_role}",
                f"order_type:{timing.order_type}",
                f"side:{timing.side}",
                f"reduce_only:{str(timing.reduce_only).lower()}",
                f"outcome:{outcome}",
            ),
        )

    def _increment_order_failure_counter(
        self,
        metric_name: str,
        *,
        strategy_id: object,
        instrument_id: object,
        client_order_id: ClientOrderId,
        order_command: str,
        source: str,
        due_post_only: bool | None = None,
    ) -> None:
        order = self._cache.order(client_order_id)
        reduce_only = bool(order and order.is_reduce_only)
        order_role = "close" if reduce_only else "open"
        tags = [
            f"venue:{HYPERLIQUID_VENUE.value}",
            f"instrument:{_tag_value(instrument_id)}",
            f"strategy:{_tag_value(strategy_id)}",
            f"order_command:{order_command}",
            f"order_role:{order_role}",
            f"order_type:{_enum_tag(order.order_type) if order else 'UNKNOWN'}",
            f"side:{order_side_to_str(order.side) if order else 'UNKNOWN'}",
            f"reduce_only:{str(reduce_only).lower()}",
            f"source:{source}",
        ]
        if due_post_only is not None:
            tags.append(f"due_post_only:{str(due_post_only).lower()}")

        datadog_increment(metric_name, tags=tuple(tags))

    def generate_order_rejected(
        self,
        strategy_id,
        instrument_id,
        client_order_id,
        reason,
        ts_event,
        due_post_only=False,
    ) -> None:
        order = self._cache.order(client_order_id)
        self._increment_order_failure_counter(
            "order.rejected",
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_command="close" if order and order.is_reduce_only else "open",
            source="generated",
            due_post_only=due_post_only,
        )
        super().generate_order_rejected(
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            ts_event,
            due_post_only,
        )

    def generate_order_modify_rejected(
        self,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        reason,
        ts_event,
    ) -> None:
        self._increment_order_failure_counter(
            "order.modify_rejected",
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_command="modify",
            source="generated",
        )
        super().generate_order_modify_rejected(
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            reason,
            ts_event,
        )

    def generate_order_cancel_rejected(
        self,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        reason,
        ts_event,
    ) -> None:
        self._increment_order_failure_counter(
            "order.cancel_rejected",
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_command="cancel",
            source="generated",
        )
        super().generate_order_cancel_rejected(
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            reason,
            ts_event,
        )

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Order {order} is already closed")
            return

        if not self._check_time_in_force(order):
            return

        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        timing: _OrderRttTiming | None = None
        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
            pyo3_order_side = order_side_to_pyo3(order.side)
            pyo3_order_type = order_type_to_pyo3(order.order_type)
            pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
            pyo3_time_in_force = time_in_force_to_pyo3(order.time_in_force)

            if order.has_price:
                if self._config.normalize_prices:
                    rounded = self._round_to_significant_figures(order.price.as_decimal())
                    pyo3_price = nautilus_pyo3.Price.from_decimal(rounded)
                else:
                    pyo3_price = nautilus_pyo3.Price.from_str(str(order.price))
            elif order.order_type in (
                OrderType.MARKET,
                OrderType.STOP_MARKET,
                OrderType.MARKET_IF_TOUCHED,
            ):
                pyo3_price = await self._calculate_market_order_price(order)
            else:
                pyo3_price = None

            if order.has_trigger_price:
                if self._config.normalize_prices:
                    rounded = self._round_to_significant_figures(order.trigger_price.as_decimal())
                    pyo3_trigger_price = nautilus_pyo3.Price.from_decimal(rounded)
                else:
                    pyo3_trigger_price = nautilus_pyo3.Price.from_str(str(order.trigger_price))
            else:
                pyo3_trigger_price = None

            # Cache cloid mapping for WebSocket order/fill resolution
            cloid = nautilus_pyo3.hyperliquid_cloid_from_client_order_id(pyo3_client_order_id)
            self._ws_client.cache_cloid_mapping(cloid, pyo3_client_order_id)

            timing = self._track_order_rtt(
                order.client_order_id,
                order_command="close" if order.is_reduce_only else "open",
                order_role="close" if order.is_reduce_only else "open",
                instrument_id=order.instrument_id,
                strategy_id=order.strategy_id,
                order_type=order.order_type,
                side=order_side_to_str(order.side),
                reduce_only=order.is_reduce_only,
            )
            pyo3_report = await self._ws_client.submit_order(
                self._client,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                order_side=pyo3_order_side,
                order_type=pyo3_order_type,
                quantity=pyo3_quantity,
                time_in_force=pyo3_time_in_force,
                price=pyo3_price,
                trigger_price=pyo3_trigger_price,
                post_only=order.is_post_only,
                reduce_only=order.is_reduce_only,
            )
            self._record_order_rtt_ack(timing, outcome="accepted")
        except Exception as e:
            if _is_transport_error(e):
                self._log.warning(
                    f"Submit transport failure for {order.client_order_id} "
                    f"({type(e).__name__}: {e}); awaiting WS reconciliation",
                )
                return

            error_str = str(e)
            due_post_only = HYPERLIQUID_POST_ONLY_WOULD_MATCH in error_str

            self._terminal_orders.add(order.client_order_id.value)
            self._cleanup_cloid_mapping(order.client_order_id)
            self._record_order_rtt_ack(
                timing,
                outcome="rejected",
                keep_for_confirmation=False,
            )
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=error_str,
                ts_event=self._clock.timestamp_ns(),
                due_post_only=due_post_only,
            )
            return

        # Reconcile the venue's immediate response at submit time
        self._process_submit_reports([pyo3_report] if pyo3_report is not None else None)

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        order_list = command.order_list
        orders = order_list.orders

        if not orders:
            self._log.warning("Order list is empty, nothing to submit")
            return

        # Check if all orders are open
        closed_orders = [order for order in orders if order.is_closed]
        if closed_orders:
            self._log.warning(f"Skipping {len(closed_orders)} closed orders in batch")
            orders = [order for order in orders if not order.is_closed]

        if not orders:
            return

        orders = [o for o in orders if self._check_time_in_force(o)]
        if not orders:
            return

        now_ns = self._clock.timestamp_ns()

        for order in orders:
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=now_ns,
            )

            # Cache cloid mapping for WebSocket order/fill resolution
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
            cloid = nautilus_pyo3.hyperliquid_cloid_from_client_order_id(pyo3_client_order_id)
            self._ws_client.cache_cloid_mapping(cloid, pyo3_client_order_id)

        try:
            pyo3_orders = [transform_order_to_pyo3(order) for order in orders]
            pyo3_reports = await self._ws_client.submit_orders(self._client, pyo3_orders)
        except Exception as e:
            if _is_transport_error(e):
                self._log.warning(
                    f"Submit order list transport failure "
                    f"({type(e).__name__}: {e}); awaiting WS reconciliation",
                )
                return

            error_str = str(e)
            due_post_only = HYPERLIQUID_POST_ONLY_WOULD_MATCH in error_str

            for order in orders:
                self._terminal_orders.add(order.client_order_id.value)
                self._cleanup_cloid_mapping(order.client_order_id)
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=error_str,
                    ts_event=self._clock.timestamp_ns(),
                    due_post_only=due_post_only,
                )
            return

        # Deferred trigger children are intentionally absent from pyo3_reports;
        # they stay SUBMITTED until the user-events stream delivers the accept.
        self._process_submit_reports(pyo3_reports)

    def _process_submit_reports(
        self,
        pyo3_reports: list[nautilus_pyo3.OrderStatusReport] | None,
    ) -> None:
        # Same handler as the WS user stream; a missing report (Rust-side build
        # failure) leaves the order SUBMITTED for reconciliation.
        for pyo3_report in pyo3_reports or ():
            try:
                self._handle_order_status_report_pyo3(
                    pyo3_report,
                    record_order_rtt_confirmation=False,
                )
            except Exception as e:
                self._log.warning(
                    f"Failed to process submit response report "
                    f"({type(e).__name__}: {e}); awaiting WS reconciliation",
                )

    async def _modify_order(self, command: ModifyOrder) -> None:  # noqa: C901 (sequence of guard clauses)
        order = self._cache.order(command.client_order_id)

        if order is None:
            self.generate_order_modify_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason="ORDER_NOT_FOUND_IN_CACHE",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        venue_order_id = None
        if order.venue_order_id:
            venue_order_id = order.venue_order_id
        elif command.venue_order_id:
            venue_order_id = command.venue_order_id

        # Use command values if provided, else fall back to current order values
        price = command.price if command.price else (order.price if order.has_price else None)
        # Hyperliquid modify is cancel-replace; subtract filled to avoid overfill.
        target_total_qty = command.quantity if command.quantity else order.quantity
        if target_total_qty <= order.filled_qty:
            self.generate_order_modify_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=venue_order_id,
                reason=(
                    f"MODIFY_QTY_NOT_GREATER_THAN_FILLED "
                    f"(target={target_total_qty}, filled={order.filled_qty})"
                ),
                ts_event=self._clock.timestamp_ns(),
            )
            return
        quantity = target_total_qty - order.filled_qty
        trigger_price = command.trigger_price
        if not trigger_price and order.has_trigger_price:
            trigger_price = order.trigger_price

        # StopMarket/MarketIfTouched have no limit price — derive a
        # slippage-adjusted limit from the trigger, matching submit path
        if price is None and trigger_price is not None:
            price = self._derive_limit_from_trigger(order, trigger_price)

        if price is None:
            self.generate_order_modify_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=venue_order_id,
                reason="PRICE_REQUIRED_FOR_MODIFY",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        modify_generation: int | None = None
        timing: _OrderRttTiming | None = None
        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                command.instrument_id.value,
            )
            # Hyperliquid-owned orders are command-addressed by stable CLOID.
            # The volatile venue order id is only retained for event metadata
            # and old-leg cancel suppression.
            pyo3_venue_order_id = None
            pyo3_order_side = order_side_to_pyo3(order.side)
            pyo3_order_type = order_type_to_pyo3(order.order_type)
            pyo3_price = nautilus_pyo3.Price.from_str(str(price))
            pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(quantity))
            pyo3_time_in_force = time_in_force_to_pyo3(order.time_in_force)
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(
                command.client_order_id.value,
            )

            pyo3_trigger_price = None

            if trigger_price is not None:
                pyo3_trigger_price = nautilus_pyo3.Price.from_str(str(trigger_price))

            # Mark in-flight BEFORE the await so the WS cancel handler sees it regardless of timing.
            # Cleared on non-transport post errors; preserved on transport errors so WS can reconcile.
            modify_generation = self._mark_pending_modify(
                command.client_order_id.value,
                venue_order_id,
                target_total_qty,
                price,
            )
            self._log.info(f"Order modification requested for {command.client_order_id}")

            timing = self._track_order_rtt(
                command.client_order_id,
                order_command="modify",
                order_role="close" if order.is_reduce_only else "open",
                instrument_id=command.instrument_id,
                strategy_id=command.strategy_id,
                order_type=order.order_type,
                side=order_side_to_str(order.side),
                reduce_only=order.is_reduce_only,
            )
            await self._ws_client.modify_order(
                self._client,
                instrument_id=pyo3_instrument_id,
                venue_order_id=pyo3_venue_order_id,
                order_side=pyo3_order_side,
                order_type=pyo3_order_type,
                price=pyo3_price,
                quantity=pyo3_quantity,
                trigger_price=pyo3_trigger_price,
                reduce_only=order.is_reduce_only,
                post_only=order.is_post_only,
                time_in_force=pyo3_time_in_force,
                client_order_id=pyo3_client_order_id,
            )
            self._record_order_rtt_ack(timing, outcome="accepted")

        except Exception as e:
            if _is_transport_error(e):
                # Keep pending state so WS can reconcile target qty if the modify landed
                self._log.warning(
                    f"Modify transport failure for {command.client_order_id} "
                    f"({type(e).__name__}: {e}); awaiting WS reconciliation",
                )
                return
            if modify_generation is not None:
                self._clear_pending_modify_generation(
                    command.client_order_id.value,
                    modify_generation,
                )
            self.generate_order_modify_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )
            self._record_order_rtt_ack(
                timing,
                outcome="rejected",
                keep_for_confirmation=False,
            )

    async def _cancel_order(self, command: CancelOrder) -> None:
        # Try to get venue_order_id from cache first, fall back to command
        order = self._cache.order(command.client_order_id)
        venue_order_id = None
        if order and order.venue_order_id:
            venue_order_id = order.venue_order_id
        elif command.venue_order_id:
            venue_order_id = command.venue_order_id

        timing: _OrderRttTiming | None = None
        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                command.instrument_id.value,
            )
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(command.client_order_id.value)
            # Hyperliquid-owned cancels should not depend on the latest
            # exchange OID ack; route by stable CLOID via client_order_id.
            pyo3_venue_order_id = None

            timing = self._track_order_rtt(
                command.client_order_id,
                order_command="cancel",
                order_role="close" if order and order.is_reduce_only else "open",
                instrument_id=command.instrument_id,
                strategy_id=command.strategy_id,
                order_type=order.order_type if order else "UNKNOWN",
                side=order_side_to_str(order.side) if order else "UNKNOWN",
                reduce_only=bool(order and order.is_reduce_only),
            )
            await self._ws_client.cancel_order(
                self._client,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
            )
            self._log.info(f"Order cancellation requested for {command.client_order_id}")
            self._record_order_rtt_ack(timing, outcome="accepted")
        except Exception as e:
            if _is_transport_error(e):
                self._log.warning(
                    f"Cancel transport failure for {command.client_order_id} "
                    f"({type(e).__name__}: {e}); awaiting WS reconciliation",
                )
                return
            self.generate_order_cancel_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )
            self._record_order_rtt_ack(
                timing,
                outcome="rejected",
                keep_for_confirmation=False,
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        open_orders = self._cache.orders_open(
            venue=self.venue,
            instrument_id=command.instrument_id,
            side=command.order_side,
        )

        if not open_orders:
            instrument_str = (
                f" for {command.instrument_id}" if command.instrument_id is not None else ""
            )
            self._log.info(f"No open orders to cancel{instrument_str}")
            return

        if command.order_side != OrderSide.NO_ORDER_SIDE:
            self._log.info(
                f"Filtering orders by side: {order_side_to_str(command.order_side)}",
            )

        self._log.info(f"Cancelling {len(open_orders)} open order(s)")

        cancel_requests = [
            (
                nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value),
                nautilus_pyo3.ClientOrderId(order.client_order_id.value),
                nautilus_pyo3.VenueOrderId(order.venue_order_id.value)
                if order.venue_order_id
                else None,
            )
            for order in open_orders
        ]

        try:
            errors = await self._ws_client.cancel_orders(self._client, cancel_requests)

            for order, error in zip(open_orders, errors, strict=False):
                if error is None:
                    continue

                self.generate_order_cancel_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=error,
                    ts_event=self._clock.timestamp_ns(),
                )
        except Exception as e:
            if _is_transport_error(e):
                self._log.warning(
                    f"Cancel-all transport failure ({type(e).__name__}: {e}); "
                    "awaiting WS reconciliation",
                )
                return

            for order in open_orders:
                self.generate_order_cancel_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=str(e),
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        if not command.cancels:
            self._log.info("No orders to cancel in batch")
            return

        entries = []

        for cancel_cmd in command.cancels:
            order = self._cache.order(cancel_cmd.client_order_id)
            if not order:
                self._log.warning(
                    f"Cannot cancel order {cancel_cmd.client_order_id}: not found in cache",
                )
                continue

            entries.append(
                (
                    order,
                    (
                        nautilus_pyo3.InstrumentId.from_str(cancel_cmd.instrument_id.value),
                        nautilus_pyo3.ClientOrderId(cancel_cmd.client_order_id.value),
                        nautilus_pyo3.VenueOrderId(order.venue_order_id.value)
                        if order.venue_order_id
                        else None,
                    ),
                ),
            )

        if not entries:
            return

        try:
            errors = await self._ws_client.cancel_orders(
                self._client,
                [request for _, request in entries],
            )

            for (order, _), error in zip(entries, errors, strict=False):
                if error is None:
                    continue

                self.generate_order_cancel_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=error,
                    ts_event=self._clock.timestamp_ns(),
                )
        except Exception as e:
            if _is_transport_error(e):
                self._log.warning(
                    f"Batch cancel transport failure ({type(e).__name__}: {e}); "
                    "awaiting WS reconciliation",
                )
                return

            for order, _ in entries:
                self.generate_order_cancel_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=str(e),
                    ts_event=self._clock.timestamp_ns(),
                )

    def _handle_msg(self, msg: Any) -> None:  # noqa: C901 (too complex)
        try:
            if isinstance(msg, nautilus_pyo3.AccountState):
                self._handle_account_state(msg)
            elif isinstance(msg, nautilus_pyo3.OrderAccepted):
                self._handle_order_accepted_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderCanceled):
                self._handle_order_canceled_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderExpired):
                self._handle_order_expired_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderUpdated):
                self._handle_order_updated_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderRejected):
                self._handle_order_rejected_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderCancelRejected):
                self._handle_order_cancel_rejected_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderModifyRejected):
                self._handle_order_modify_rejected_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderStatusReport):
                self._handle_order_status_report_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.FillReport):
                self._handle_fill_report_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.PositionStatusReport):
                self._handle_position_status_report_pyo3(msg)
            else:
                self._log.debug(f"Received unhandled message type: {type(msg)}")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

    def _handle_account_state(self, msg: nautilus_pyo3.AccountState) -> None:
        account_state = AccountState.from_dict(msg.to_dict())

        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=account_state.is_reported,
            ts_event=account_state.ts_event,
        )

    def _handle_order_accepted_pyo3(self, msg: nautilus_pyo3.OrderAccepted) -> None:
        event = OrderAccepted.from_dict(msg.to_dict())
        key = event.client_order_id.value

        self._record_order_rtt_confirmation(event.client_order_id, outcome="accepted")

        # Check caches to handle race conditions
        if key in self._accepted_orders or key in self._terminal_orders:
            self._log.debug(f"Ignoring duplicate OrderAccepted for {event.client_order_id!r}")
            return

        self._accepted_orders.add(key)
        self._send_order_event(event)

        # Drain any fills that arrived before order was in cache.
        self._drain_fill_buffer(self._pending_fills, key)

    def _handle_order_canceled_pyo3(self, msg: nautilus_pyo3.OrderCanceled) -> None:
        event = OrderCanceled.from_dict(msg.to_dict())
        key = event.client_order_id.value

        if key in self._terminal_orders:
            self._log.debug(f"Ignoring duplicate OrderCanceled for {event.client_order_id!r}")
            return

        # Stale cancel suppression: if the cached venue_order_id has already advanced
        # past the event's venue_order_id, the CANCELED refers to the old leg of a
        # Hyperliquid cancel-replace modify already routed through OrderUpdated.
        cached_voi = self._cache.venue_order_id(event.client_order_id)
        if (
            cached_voi is not None
            and event.venue_order_id is not None
            and event.venue_order_id != cached_voi
        ):
            self._log.debug(
                f"Skipping stale OrderCanceled for {event.venue_order_id!r} "
                f"(cached {cached_voi!r}) on {event.client_order_id!r}",
            )
            return

        # Cancel-before-accept race: the pending marker is set before the modify HTTP
        # call and removed on failure, so an in-flight modify suppresses its old leg
        # CANCELED while a failed modify never falls here.
        if event.venue_order_id is not None and self._pending_modify_matches(
            key,
            event.venue_order_id,
        ):
            self._log.debug(
                f"Suppressing cancel-before-accept for {event.client_order_id!r} "
                f"venue_order_id={event.venue_order_id!r}",
            )
            return

        self._terminal_orders.add(key)
        self._cleanup_cloid_mapping(event.client_order_id)
        self._record_order_rtt_confirmation(event.client_order_id, outcome="canceled")
        self._send_order_event(event)

    def _handle_order_expired_pyo3(self, msg: nautilus_pyo3.OrderExpired) -> None:
        event = OrderExpired.from_dict(msg.to_dict())
        key = event.client_order_id.value

        self._record_order_rtt_confirmation(event.client_order_id, outcome="expired")

        if key in self._terminal_orders:
            self._log.debug(f"Ignoring duplicate OrderExpired for {event.client_order_id!r}")
            return

        self._terminal_orders.add(key)
        self._cleanup_cloid_mapping(event.client_order_id)
        self._send_order_event(event)

    def _handle_order_updated_pyo3(self, msg: nautilus_pyo3.OrderUpdated) -> None:
        event = OrderUpdated.from_dict(msg.to_dict())
        self._record_order_rtt_confirmation(event.client_order_id, outcome="updated")
        self._send_order_event(event)

    def _handle_order_rejected_pyo3(self, msg: nautilus_pyo3.OrderRejected) -> None:
        event = OrderRejected.from_dict(msg.to_dict())
        key = event.client_order_id.value

        self._record_order_rtt_confirmation(event.client_order_id, outcome="rejected")

        if key in self._terminal_orders:
            self._log.debug(f"Ignoring duplicate OrderRejected for {event.client_order_id!r}")
            return

        order = self._cache.order(event.client_order_id)
        self._increment_order_failure_counter(
            "order.rejected",
            strategy_id=event.strategy_id,
            instrument_id=event.instrument_id,
            client_order_id=event.client_order_id,
            order_command="close" if order and order.is_reduce_only else "open",
            source="stream",
            due_post_only=event.due_post_only,
        )
        self._terminal_orders.add(key)
        self._cleanup_cloid_mapping(event.client_order_id)
        self._send_order_event(event)

    def _handle_order_cancel_rejected_pyo3(self, msg: nautilus_pyo3.OrderCancelRejected) -> None:
        event = OrderCancelRejected.from_dict(msg.to_dict())
        self._record_order_rtt_confirmation(event.client_order_id, outcome="rejected")
        self._increment_order_failure_counter(
            "order.cancel_rejected",
            strategy_id=event.strategy_id,
            instrument_id=event.instrument_id,
            client_order_id=event.client_order_id,
            order_command="cancel",
            source="stream",
        )
        self._send_order_event(event)

    def _handle_order_modify_rejected_pyo3(self, msg: nautilus_pyo3.OrderModifyRejected) -> None:
        event = OrderModifyRejected.from_dict(msg.to_dict())
        self._record_order_rtt_confirmation(event.client_order_id, outcome="rejected")
        self._increment_order_failure_counter(
            "order.modify_rejected",
            strategy_id=event.strategy_id,
            instrument_id=event.instrument_id,
            client_order_id=event.client_order_id,
            order_command="modify",
            source="stream",
        )
        self._send_order_event(event)

    def _mark_pending_modify(
        self,
        key: str,
        old_venue_order_id: VenueOrderId | None,
        target_qty: Quantity,
        target_price: Price,
    ) -> int:
        generation = self._next_modify_generation
        self._next_modify_generation += 1
        self._pending_modify_chains.setdefault(key, []).append(
            {
                "generation": generation,
                "old_venue_order_id": old_venue_order_id.value if old_venue_order_id else None,
                "target_qty": target_qty,
                "target_price": target_price,
            },
        )
        self._refresh_pending_modify_view(key)
        return generation

    def _clear_pending_modify(self, key: str) -> None:
        self._pending_modify_chains.pop(key, None)
        self._pending_modify_keys.pop(key, None)
        self._pending_modify_target_qty.pop(key, None)
        self._pending_modify_target_price.pop(key, None)

    def _clear_pending_modify_generation(self, key: str, generation: int) -> None:
        self._ensure_pending_modify_chain(key)
        chain = self._pending_modify_chains.get(key)
        if not chain:
            return

        remaining = [intent for intent in chain if intent.get("generation") != generation]
        if len(remaining) == len(chain):
            return
        if remaining:
            self._pending_modify_chains[key] = remaining
            self._refresh_pending_modify_view(key)
        else:
            self._clear_pending_modify(key)

    def _ensure_pending_modify_chain(self, key: str) -> None:
        if key in self._pending_modify_chains or key not in self._pending_modify_keys:
            return

        self._pending_modify_chains[key] = [
            {
                "generation": 0,
                "old_venue_order_id": self._pending_modify_keys.get(key),
                "target_qty": self._pending_modify_target_qty.get(key),
                "target_price": self._pending_modify_target_price.get(key),
            },
        ]

    def _refresh_pending_modify_view(self, key: str) -> None:
        chain = self._pending_modify_chains.get(key)
        if not chain:
            self._clear_pending_modify(key)
            return

        front = chain[0]
        self._pending_modify_keys[key] = front.get("old_venue_order_id")  # type: ignore[assignment]

        target_qty = front.get("target_qty")
        if target_qty is not None:
            self._pending_modify_target_qty[key] = target_qty  # type: ignore[assignment]
        else:
            self._pending_modify_target_qty.pop(key, None)

        target_price = front.get("target_price")
        if target_price is not None:
            self._pending_modify_target_price[key] = target_price  # type: ignore[assignment]
        else:
            self._pending_modify_target_price.pop(key, None)

    def _pending_modify_matches(self, key: str, venue_order_id: VenueOrderId) -> bool:
        self._ensure_pending_modify_chain(key)
        chain = self._pending_modify_chains.get(key)
        if not chain:
            return False

        venue_value = venue_order_id.value
        return any(
            intent.get("old_venue_order_id") is None
            or intent.get("old_venue_order_id") == venue_value
            for intent in chain
        )

    def _claim_pending_modify_for_replacement(
        self,
        key: str,
        new_venue_order_id: VenueOrderId,
        *,
        report_price: Price | None,
        report_qty: Quantity | None,
    ) -> dict[str, object] | None:
        self._ensure_pending_modify_chain(key)
        chain = self._pending_modify_chains.get(key)
        if not chain:
            return None

        best_index = 0
        best_score = 0
        for index, intent in enumerate(chain):
            score = 0
            if report_price is not None and intent.get("target_price") == report_price:
                score += 2
            if report_qty is not None and intent.get("target_qty") == report_qty:
                score += 1
            if score > best_score:
                best_index = index
                best_score = score

        claimed = None
        for _ in range(best_index + 1):
            claimed = chain.pop(0)

        if chain:
            chain[0]["old_venue_order_id"] = new_venue_order_id.value
            self._refresh_pending_modify_view(key)
        else:
            self._clear_pending_modify(key)

        return claimed

    def _is_stale_venue_order_id(
        self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> bool:
        key = client_order_id.value
        if venue_order_id.value in self._superseded_venue_order_ids.get(key, set()):
            return True

        cached_voi = self._cache.venue_order_id(client_order_id)
        if cached_voi is None or cached_voi == venue_order_id:
            return False

        cached_ts = self._cached_venue_order_id_ts.get(key)
        return cached_ts is not None and ts_event < cached_ts

    def _record_venue_order_id_binding(
        self,
        client_order_id: ClientOrderId,
        new_venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None:
        key = client_order_id.value
        previous = self._cache.venue_order_id(client_order_id)
        if previous is not None and previous != new_venue_order_id:
            self._superseded_venue_order_ids.setdefault(key, set()).add(previous.value)
        self._cache.add_venue_order_id(
            client_order_id,
            new_venue_order_id,
            overwrite=True,
        )
        self._cached_venue_order_id_ts[key] = ts_event

    def _handle_order_status_report_pyo3(  # noqa: C901 (complexity unavoidable)
        self,
        pyo3_report: nautilus_pyo3.OrderStatusReport,
        *,
        record_order_rtt_confirmation: bool = True,
    ) -> None:
        report = OrderStatusReport.from_pyo3(pyo3_report)

        client_order_id = self._resolve_cloid(report.client_order_id)
        report.client_order_id = client_order_id

        if self._is_external_order(client_order_id) and report.venue_order_id:
            resolved_id = self._cache.client_order_id(report.venue_order_id)
            if resolved_id:
                client_order_id = resolved_id
                report.client_order_id = client_order_id

        if self._is_external_order(client_order_id):
            self._send_order_status_report(report)
            return

        order = self._cache.order(client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process order status report - order for {client_order_id!r} not found",
            )
            return

        # At this point client_order_id is guaranteed to be set (external orders return early above)
        assert report.client_order_id is not None

        if order.linked_order_ids is not None:
            report.linked_order_ids = list(order.linked_order_ids)

        if report.order_status == OrderStatus.REJECTED:
            key = report.client_order_id.value
            if record_order_rtt_confirmation:
                self._record_order_rtt_confirmation(report.client_order_id, outcome="rejected")
            if key in self._terminal_orders:
                return

            self._terminal_orders.add(key)
            self._cleanup_cloid_mapping(report.client_order_id)

            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                reason=report.cancel_reason or "Order rejected by exchange",
                ts_event=report.ts_last,
            )
        elif report.order_status == OrderStatus.ACCEPTED:
            key = report.client_order_id.value

            # Cancel-replace detection: if the cached venue_order_id already
            # differs from the report's venue_order_id, this ACCEPTED is the
            # replacement leg of a Hyperliquid modify (cancel-replace), and must
            # be emitted as OrderUpdated rather than a duplicate OrderAccepted.
            # See GH-3827.
            cached_voi = self._cache.venue_order_id(report.client_order_id)
            if (
                cached_voi is not None
                and report.venue_order_id is not None
                and report.venue_order_id != cached_voi
            ):
                if self._is_stale_venue_order_id(
                    report.client_order_id,
                    report.venue_order_id,
                    report.ts_last,
                ):
                    self._log.debug(
                        f"Skipping stale ACCEPTED for {report.venue_order_id!r} "
                        f"(cached {cached_voi!r}) on {report.client_order_id!r}",
                    )
                    return

                if record_order_rtt_confirmation:
                    self._record_order_rtt_confirmation(
                        report.client_order_id,
                        outcome="updated",
                    )

                claim = self._claim_pending_modify_for_replacement(
                    key,
                    report.venue_order_id,
                    report_price=report.price,
                    report_qty=report.quantity,
                )
                update_price = report.price
                if update_price is None and claim is not None:
                    update_price = claim.get("target_price")  # type: ignore[assignment]
                if update_price is None:
                    update_price = self._pending_modify_target_price.get(key)
                if update_price is None and order.has_price:
                    update_price = order.price
                if update_price is None:
                    self._log.warning(
                        f"Cannot emit OrderUpdated for modify {report.client_order_id!r}: "
                        "no price on report or cached order",
                    )
                    return

                # Prefer user target over venue's remaining-only
                # `report.quantity`; fall back when no marker (external modify).
                target_qty = claim.get("target_qty") if claim is not None else None
                update_quantity = target_qty if target_qty is not None else report.quantity

                self._promote_cancel_replace(
                    order,
                    report.venue_order_id,
                    price=update_price,
                    quantity=update_quantity,
                    trigger_price=report.trigger_price,
                    ts_event=report.ts_last,
                )
                return

            if record_order_rtt_confirmation:
                self._record_order_rtt_confirmation(report.client_order_id, outcome="accepted")

            if key in self._accepted_orders or key in self._terminal_orders:
                return
            self._accepted_orders.add(key)

            self.generate_order_accepted(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )

            # Drain any fills that arrived before order was in cache.
            self._drain_fill_buffer(self._pending_fills, key)

        elif report.order_status == OrderStatus.PENDING_CANCEL:
            if order.status == OrderStatus.PENDING_CANCEL:
                self._log.debug(
                    f"Received PENDING_CANCEL status for {report.client_order_id!r} - "
                    "order already in pending cancel state locally",
                )
            else:
                self._log.warning(
                    f"Received PENDING_CANCEL status for {report.client_order_id!r} - "
                    f"order status {order.status_string()}",
                )
        elif report.order_status == OrderStatus.CANCELED:
            key = report.client_order_id.value

            # Stale cancel suppression: if the cached venue_order_id has already
            # been advanced past this report's venue_order_id, the CANCELED
            # refers to the old leg of a Hyperliquid cancel-replace modify that
            # has already been routed through OrderUpdated. See GH-3827.
            cached_voi = self._cache.venue_order_id(report.client_order_id)
            if (
                cached_voi is not None
                and report.venue_order_id is not None
                and report.venue_order_id != cached_voi
            ):
                self._log.debug(
                    f"Skipping stale CANCELED for {report.venue_order_id!r} "
                    f"(cached {cached_voi!r}) on {report.client_order_id!r}",
                )
                return

            # Cancel-before-accept race: for an in-flight modify, Hyperliquid
            # may deliver CANCELED(old_voi) before the replacement ACCEPTED.
            # Suppress the old leg so the later ACCEPTED can route through the
            # OrderUpdated path. The marker is cleared on non-transport modify
            # failure; on transport failure it stays so a landed modify can
            # still reconcile.
            if report.venue_order_id is not None and self._pending_modify_matches(
                key,
                report.venue_order_id,
            ):
                self._log.debug(
                    f"Skipping cancel-before-accept leg for "
                    f"{report.client_order_id!r}, venue_order_id={report.venue_order_id!r}",
                )
                return

            if record_order_rtt_confirmation:
                self._record_order_rtt_confirmation(report.client_order_id, outcome="canceled")

            if key in self._terminal_orders:
                return

            self._terminal_orders.add(key)
            self._cleanup_cloid_mapping(report.client_order_id)

            self.generate_order_canceled(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )
        elif report.order_status == OrderStatus.EXPIRED:
            key = report.client_order_id.value
            if record_order_rtt_confirmation:
                self._record_order_rtt_confirmation(report.client_order_id, outcome="expired")
            if key in self._terminal_orders:
                return

            self._terminal_orders.add(key)
            self._cleanup_cloid_mapping(report.client_order_id)

            self.generate_order_expired(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )
        elif report.order_status == OrderStatus.FILLED:
            key = report.client_order_id.value
            if record_order_rtt_confirmation:
                self._record_order_rtt_confirmation(report.client_order_id, outcome="filled")
            if key in self._terminal_orders:
                return

            self._terminal_orders.add(key)

            # If fill already arrived (order already closed), clean up now
            if order.is_closed:
                self._cleanup_cloid_mapping(report.client_order_id)
            else:
                self._pending_filled.add(key)
                self._log.debug(
                    f"Received FILLED status for {report.client_order_id!r} "
                    f"(order is {order.status_string()}) - fill event expected shortly",
                )
        elif report.order_status == OrderStatus.TRIGGERED:
            # Only STOP_LIMIT, TRAILING_STOP_LIMIT, LIMIT_IF_TOUCHED can be triggered
            if order.order_type not in (
                OrderType.STOP_LIMIT,
                OrderType.TRAILING_STOP_LIMIT,
                OrderType.LIMIT_IF_TOUCHED,
            ):
                self._log.debug(
                    f"Ignoring TRIGGERED status for {order.order_type} order "
                    f"{report.client_order_id!r}",
                )
                return

            self.generate_order_triggered(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )
        elif report.order_status == OrderStatus.PARTIALLY_FILLED:
            # Fills come separately via FillReport events
            self._log.debug(
                f"Received PARTIALLY_FILLED status for {report.client_order_id!r}",
            )
        else:
            self._log.warning(f"Received unhandled OrderStatusReport: {report}")

    def _handle_fill_report_pyo3(  # noqa: C901 (complexity unavoidable)
        self,
        pyo3_report: nautilus_pyo3.FillReport,
    ) -> None:
        report = FillReport.from_pyo3(pyo3_report)

        self._log.debug(
            f"Received fill from WebSocket: venue_order_id={report.venue_order_id}, "
            f"trade_id={report.trade_id}, qty={report.last_qty}, px={report.last_px}",
        )

        trade_id_str = report.trade_id.value
        if trade_id_str in self._processed_trade_ids:
            self._log.debug(f"Skipping duplicate fill: trade_id={report.trade_id}")
            return

        client_order_id = self._resolve_cloid(report.client_order_id)
        report.client_order_id = client_order_id

        if self._is_external_order(client_order_id) and report.venue_order_id:
            resolved_id = self._cache.client_order_id(report.venue_order_id)
            if resolved_id:
                client_order_id = resolved_id
                report.client_order_id = client_order_id

        if self._is_external_order(client_order_id):
            self._processed_trade_ids.add(trade_id_str)
            self._send_fill_report(report)
            return

        self._record_order_rtt_confirmation(client_order_id, outcome="filled")

        order = self._cache.order(client_order_id)
        if order is None:
            self._log.warning(
                f"Buffering fill report - order for {client_order_id!r} not yet in cache, "
                f"will drain on OrderAccepted",
            )
            self._pending_fills.setdefault(client_order_id.value, []).append(pyo3_report)
            return

        instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot process fill report - instrument {order.instrument_id} not found",
            )
            return

        key = order.client_order_id.value

        # A fill on the replacement venue_order_id promotes the binding here so a
        # dropped ACCEPTED cannot strand it; the fill then reconciles below.
        # A delayed earlier-leg fill during a chained modify is a known limitation.
        cached_voi = self._cache.venue_order_id(order.client_order_id)
        if (
            key in self._pending_modify_keys
            and cached_voi is not None
            and report.venue_order_id is not None
            and report.venue_order_id != cached_voi
            and not self._is_stale_venue_order_id(
                order.client_order_id,
                report.venue_order_id,
                report.ts_event,
            )
        ):
            if order.is_closed:
                self._log.error(
                    f"Cannot promote cancel-replace for {order.client_order_id!r}: order is "
                    f"{order.status_string()}, fill on {report.venue_order_id!r} cannot reconcile",
                )
                return

            update_price = self._pending_modify_target_price.get(key)
            if update_price is None and order.has_price:
                update_price = order.price
            if update_price is None:
                self._log.warning(
                    f"Cannot promote cancel-replace for {order.client_order_id!r}: "
                    "no target or cached price",
                )
                return

            claim = self._claim_pending_modify_for_replacement(
                key,
                report.venue_order_id,
                report_price=None,
                report_qty=None,
            )
            target_qty = claim.get("target_qty") if claim is not None else None
            update_quantity = target_qty if target_qty is not None else order.quantity

            self._promote_cancel_replace(
                order,
                report.venue_order_id,
                price=update_price,
                quantity=update_quantity,
                trigger_price=order.trigger_price if order.has_trigger_price else None,
                ts_event=report.ts_event,
            )
            # Fall through so the fill reconciles against the now-advanced binding

        self._processed_trade_ids.add(trade_id_str)

        # If order not yet accepted, generate OrderAccepted first to avoid state transition error
        if key not in self._accepted_orders:
            self._accepted_orders.add(key)

            self.generate_order_accepted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_event,
            )

            # Drain any fills that arrived before order was in cache.
            self._drain_fill_buffer(self._pending_fills, key)

        self.generate_order_filled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=report.venue_order_id,
            venue_position_id=report.venue_position_id,
            trade_id=report.trade_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=report.last_qty,
            last_px=report.last_px,
            quote_currency=instrument.quote_currency,
            commission=report.commission,
            liquidity_side=report.liquidity_side,
            ts_event=report.ts_event,
        )

        # Only clean cloid after FILLED status has been observed (final fill)
        key = order.client_order_id.value
        if key in self._pending_filled:
            self._pending_filled.discard(key)
            self._cleanup_cloid_mapping(order.client_order_id)

    def _drain_fill_buffer(
        self,
        buffer: dict[str, list[nautilus_pyo3.FillReport]],
        key: str,
    ) -> None:
        buffered = buffer.pop(key, None)
        if buffered:
            for pyo3_buffered in buffered:
                self._handle_fill_report_pyo3(pyo3_buffered)

    def _promote_replacement_if_inflight_modify(self, report: OrderStatusReport) -> None:  # noqa: C901
        # During a tracked cancel-replace a query can surface the replacement leg
        # (ACCEPTED, new venue_order_id) before the WS ACCEPTED push or any fill.
        # Promote here so the binding advances without waiting for a signal that may
        # never arrive (dropped ACCEPTED with no fill).
        if report.order_status != OrderStatus.ACCEPTED:
            return

        if report.client_order_id is None or report.venue_order_id is None:
            return

        key = report.client_order_id.value
        if key not in self._pending_modify_keys:
            return

        cached_voi = self._cache.venue_order_id(report.client_order_id)
        if cached_voi is None or report.venue_order_id == cached_voi:
            return

        if self._is_stale_venue_order_id(
            report.client_order_id,
            report.venue_order_id,
            report.ts_last,
        ):
            return

        order = self._cache.order(report.client_order_id)
        if order is None or order.is_closed:
            return

        claim = self._claim_pending_modify_for_replacement(
            key,
            report.venue_order_id,
            report_price=report.price,
            report_qty=report.quantity,
        )
        update_price = report.price
        if update_price is None and claim is not None:
            update_price = claim.get("target_price")  # type: ignore[assignment]
        if update_price is None:
            update_price = self._pending_modify_target_price.get(key)
        if update_price is None and order.has_price:
            update_price = order.price
        if update_price is None:
            self._log.warning(
                f"Cannot promote cancel-replace from query for {report.client_order_id!r}: "
                "no price on report or cached order",
            )
            return

        target_qty = claim.get("target_qty") if claim is not None else None
        update_quantity = target_qty if target_qty is not None else report.quantity

        self._promote_cancel_replace(
            order,
            report.venue_order_id,
            price=update_price,
            quantity=update_quantity,
            trigger_price=report.trigger_price,
            ts_event=report.ts_last,
        )

    def _promote_cancel_replace(
        self,
        order: Order,
        new_venue_order_id: VenueOrderId,
        *,
        price: Price,
        quantity: Quantity,
        trigger_price: Price | None,
        ts_event: int,
    ) -> None:
        # Shared by the ACCEPTED branch and the fill path so a dropped ACCEPTED
        # still rebinds via the fill.
        key = order.client_order_id.value
        previous = self._cache.venue_order_id(order.client_order_id)
        if previous is not None and previous != new_venue_order_id:
            self._superseded_venue_order_ids.setdefault(key, set()).add(previous.value)
        self._cache.add_venue_order_id(
            order.client_order_id,
            new_venue_order_id,
            overwrite=True,
        )
        self._cached_venue_order_id_ts[key] = ts_event
        self.generate_order_updated(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=new_venue_order_id,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            ts_event=ts_event,
            venue_order_id_modified=True,
        )

    def _handle_position_status_report_pyo3(
        self,
        msg: nautilus_pyo3.PositionStatusReport,
    ) -> None:
        report = PositionStatusReport.from_pyo3(msg)
        self._log.debug(f"Received {report}", LogColor.MAGENTA)

    def _is_inflight_modify_old_leg_cancel(self, report: OrderStatusReport) -> bool:
        # Suppress the old leg's CANCELED during a tracked cancel-replace so
        # reconciliation keeps the order alive for the replacement's fill to rebind.
        if report.order_status != OrderStatus.CANCELED:
            return False
        if report.client_order_id is None or report.venue_order_id is None:
            return False
        return self._pending_modify_matches(report.client_order_id.value, report.venue_order_id)

    def _is_external_order(self, client_order_id: ClientOrderId) -> bool:
        return not client_order_id or not self._cache.strategy_id_for_order(client_order_id)

    def _is_cloid_format(self, client_order_id: ClientOrderId) -> bool:
        if not client_order_id:
            return False
        value = client_order_id.value
        # CLOID format: "0x" + 32 hex chars = 34 chars total
        return len(value) == 34 and value.startswith("0x")

    def _resolve_cloid(self, client_order_id: ClientOrderId) -> ClientOrderId:
        if not self._is_cloid_format(client_order_id):
            return client_order_id
        resolved = self._ws_client.get_cloid_mapping(client_order_id.value)
        if resolved:
            # Convert from PyO3 ClientOrderId to model ClientOrderId
            return ClientOrderId(resolved.value)
        return client_order_id


# pyo3 HTTP errors arrive as ValueError carrying the Rust `Display` text
_TRANSPORT_ERROR_PREFIXES = ("transport error:", "IO error:")


def _is_transport_error(exc: BaseException) -> bool:
    if isinstance(exc, (TimeoutError, OSError)):
        return True
    msg = str(exc)
    return msg == "timeout" or msg.startswith(_TRANSPORT_ERROR_PREFIXES)
