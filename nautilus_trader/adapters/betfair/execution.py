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

import asyncio
from datetime import datetime

import pandas as pd
from betfair_parser.exceptions import BetfairError
from betfair_parser.spec.accounts.type_definitions import AccountDetailsResponse
from betfair_parser.spec.betting.enums import ExecutionReportStatus
from betfair_parser.spec.betting.enums import InstructionReportErrorCode
from betfair_parser.spec.betting.enums import InstructionReportStatus
from betfair_parser.spec.betting.enums import OrderProjection
from betfair_parser.spec.betting.orders import CancelOrders
from betfair_parser.spec.betting.orders import PlaceOrders
from betfair_parser.spec.betting.orders import ReplaceOrders
from betfair_parser.spec.betting.type_definitions import CancelExecutionReport
from betfair_parser.spec.betting.type_definitions import CurrentOrderSummary
from betfair_parser.spec.betting.type_definitions import MarketVersion
from betfair_parser.spec.betting.type_definitions import PlaceExecutionReport
from betfair_parser.spec.common import OrderStatus as BetfairOrderStatus
from betfair_parser.spec.common import TimeRange
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import Order as UnmatchedOrder
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming import StatusErrorCode
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.adapters.betfair.common import OrderSideParser
from nautilus_trader.adapters.betfair.common import is_rate_limit_error
from nautilus_trader.adapters.betfair.common import is_session_error
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.constants import BETFAIR_FILL_CACHE_SWEEP_TIMER
from nautilus_trader.adapters.betfair.constants import BETFAIR_FILL_CACHE_TTL_NS
from nautilus_trader.adapters.betfair.constants import BETFAIR_ORDER_STATUS_EXECUTABLE
from nautilus_trader.adapters.betfair.constants import BETFAIR_ORDER_STATUS_EXECUTION_COMPLETE
from nautilus_trader.adapters.betfair.constants import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_RATE_LIMIT_RETRY_DELAY_SECS
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data_types import BetfairOrderVoided
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.parsing.common import FillQtyResult
from nautilus_trader.adapters.betfair.parsing.common import betfair_instrument_id
from nautilus_trader.adapters.betfair.parsing.requests import bet_to_fill_report
from nautilus_trader.adapters.betfair.parsing.requests import bet_to_order_status_report
from nautilus_trader.adapters.betfair.parsing.requests import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing.requests import make_customer_order_ref
from nautilus_trader.adapters.betfair.parsing.requests import make_customer_order_ref_legacy
from nautilus_trader.adapters.betfair.parsing.requests import order_cancel_to_cancel_order_params
from nautilus_trader.adapters.betfair.parsing.requests import order_submit_to_place_order_params
from nautilus_trader.adapters.betfair.parsing.requests import order_to_trade_id
from nautilus_trader.adapters.betfair.parsing.requests import order_update_to_cancel_order_params
from nautilus_trader.adapters.betfair.parsing.requests import order_update_to_replace_order_params
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.events import TimeEvent
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import as_utc_timestamp
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.nautilus_pyo3 import FifoCache
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


class BetfairExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Betfair.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BetfairHttpClient
        The Betfair HttpClient.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BetfairInstrumentProvider
        The instrument provider.
    config : BetfairExecClientConfig
        The configuration for the client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BetfairHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BetfairInstrumentProvider,
        config: BetfairExecClientConfig,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(BETFAIR_VENUE.value),
            venue=BETFAIR_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.BETTING,
            base_currency=Currency.from_str(config.account_currency),
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        self._instrument_provider: BetfairInstrumentProvider = instrument_provider
        self._set_account_id(AccountId(f"{BETFAIR_VENUE}-001"))

        if config.calculate_account_state:
            AccountFactory.register_calculated_account(BETFAIR_VENUE.value)

        # Configuration
        self.config = config
        self._log.info(f"{config.account_currency=}", LogColor.BLUE)
        self._log.info(f"{config.calculate_account_state=}", LogColor.BLUE)
        self._log.info(f"{config.request_account_state_secs=}", LogColor.BLUE)
        self._log.info(f"{config.reconcile_market_ids_only=}", LogColor.BLUE)
        self._log.info(f"{config.stream_market_ids_filter=}", LogColor.BLUE)
        self._log.info(f"{config.ignore_external_orders=}", LogColor.BLUE)
        self._log.info(f"{config.use_market_version=}", LogColor.BLUE)

        # Include filter for order stream updates (None = process all markets)
        self._stream_market_ids_filter: set[str] | None = (
            set(config.stream_market_ids_filter) if config.stream_market_ids_filter else None
        )

        # Clients
        self._client: BetfairHttpClient = client
        self._stream = BetfairOrderStreamClient(
            http_client=self._client,
            message_handler=self.handle_order_stream_update,
            certs_dir=config.certs_dir,
            heartbeat_ms=config.stream_heartbeat_ms,
        )
        self._is_reconnecting = (
            False  # Necessary for coordination, as the clients rely on each other
        )

        # Async tasks
        self._update_account_task: asyncio.Task | None = None

        # Maps customer_order_ref (rfo) to client_order_id for stream order matching.
        # Betfair streams include rfo which lets us match orders without waiting for bet_id.
        self._customer_order_refs: dict[str, ClientOrderId] = {}

        # Tracks orders with pending updates to ensure state consistency during asynchronous processing
        self._pending_update_keys: set[tuple[ClientOrderId, VenueOrderId]] = set()

        # Tracks published trade IDs to avoid duplicate fills (bounded FIFO cache)
        self._published_executions: FifoCache = FifoCache()

        # Hot caches:
        # Track fill state separately from order state since Betfair only provides
        # cumulative matched sizes (sm). This lets us calculate incremental fills
        # while avoiding race conditions with delayed order state updates.
        self._cache_filled_qty: dict[ClientOrderId, Quantity] = {}
        self._cache_filled_completed_ns: dict[ClientOrderId, int] = {}
        self._cache_avg_px: dict[ClientOrderId, float] = {}

        # Tracks orders for which a terminal event (cancel/expire) has been generated
        # to prevent duplicate events from race conditions with multiple event sources
        self._terminal_orders: FifoCache = FifoCache()

        # Tracks venue_order_ids that have been replaced via modify price operations
        # to ignore late stream updates for the old (replaced) order
        self._replaced_venue_order_ids: FifoCache = FifoCache()

    @property
    def instrument_provider(self) -> BetfairInstrumentProvider:
        """
        Return the instrument provider for the client.

        Returns
        -------
        BetfairInstrumentProvider

        """
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._client.connect()

        # Sync before stream connects to prevent duplicate fills from full image
        self._sync_fill_caches_from_orders()

        self._clock.set_timer_ns(
            name=BETFAIR_FILL_CACHE_SWEEP_TIMER,
            interval_ns=BETFAIR_FILL_CACHE_TTL_NS,
            start_time_ns=0,
            stop_time_ns=0,
            callback=self._on_fill_cache_sweep_timer,
        )

        self._log.debug(
            "Connecting to stream, checking account currency and loading venue ID mapping...",
        )
        aws = [
            self._stream.connect(),
            self._check_account_currency(),
        ]
        await asyncio.gather(*aws)

        self._log.debug("Starting 'update_account_state' task")

        # Request one initial update
        account_state = await self.request_account_state()
        self._send_account_state(account_state)

        if self.config.request_account_state_secs:
            self._update_account_task = self.create_task(self._update_account_state())

    async def _reconnect(self) -> None:
        self._log.info("Reconnecting to Betfair")
        self._is_reconnecting = True

        try:
            if self._update_account_task:
                self._update_account_task.cancel()
                self._update_account_task = None

            await self._client.reconnect()
            self._sync_fill_caches_from_orders()
            await self._stream.reconnect()

            account_state = await self.request_account_state()
            self._send_account_state(account_state)

            if self.config.request_account_state_secs:
                self._update_account_task = self.create_task(self._update_account_state())
        except Exception as e:
            self._log.error(f"Reconnection failed: {e}")
        finally:
            self._is_reconnecting = False

    async def _disconnect(self) -> None:
        if BETFAIR_FILL_CACHE_SWEEP_TIMER in self._clock.timer_names:
            self._clock.cancel_timer(BETFAIR_FILL_CACHE_SWEEP_TIMER)

        if self._update_account_task:
            self._log.debug("Canceling task 'update_account_task'")
            self._update_account_task.cancel()
            self._update_account_task = None

        self._log.info("Closing streaming socket")
        await self._stream.disconnect()

        self._log.info("Closing BetfairHttpClient")
        await self._client.disconnect()

    async def on_api_exception(self, error: BetfairError) -> None:
        if is_rate_limit_error(error):
            # Rate limit hit, log warning but don't reconnect since the client's
            # rate limiter will prevent further bursts.
            self._log.warning(f"Betfair rate limit hit: {error}")
            return

        if is_session_error(error):
            if self._is_reconnecting:
                # Avoid multiple reconnection attempts when multiple INVALID_SESSION_INFORMATION errors
                # are received at "the same time" from the Betfair API. Simultaneous reconnection attempts
                # will result in MAX_CONNECTION_LIMIT_EXCEEDED errors.
                self._log.info("Reconnect already in progress")
                return

            try:
                # Session is invalid, need to reconnect
                self._log.warning("Invalid session error, reconnecting...")
                await self._reconnect()
            except Exception as e:
                self._log.exception("Reconnection failed", e)
        else:
            # Other Betfair API errors (PERMISSION_DENIED, INSUFFICIENT_FUNDS, etc.)
            self._log.error(f"Betfair API error: {error}")

    def _sync_fill_caches_from_orders(self) -> None:
        orders = self._cache.orders(venue=BETFAIR_VENUE)
        synced_count = 0

        for order in orders:
            if order.is_closed:
                self._terminal_orders.add(order.client_order_id.value)
            else:
                # Register both truncations for pre-existing orders since we don't know
                # which version was used when they were placed
                self._customer_order_ref_add_with_legacy(order.client_order_id)

            if order.filled_qty > 0:
                self._cache_filled_qty[order.client_order_id] = order.filled_qty
                self._cache_avg_px[order.client_order_id] = order.avg_px

                if order.is_closed:
                    self._cache_filled_completed_ns[order.client_order_id] = (
                        self._clock.timestamp_ns()
                    )

                for trade_id in order.trade_ids:
                    if trade_id.value not in self._published_executions:
                        self._published_executions.add(trade_id.value)

                synced_count += 1

        if synced_count > 0:
            self._log.info(
                f"Synced fill caches from {synced_count} order(s) with existing fills",
                LogColor.BLUE,
            )

    def _try_mark_terminal_order(self, client_order_id: ClientOrderId) -> bool:
        key = client_order_id.value
        if key in self._terminal_orders:
            return False

        self._terminal_orders.add(key)
        return True

    async def _check_account_currency(self) -> None:
        PyCondition.not_none(self.base_currency, "self.base_currency")

        details: AccountDetailsResponse = await self._client.get_account_details()
        currency_code = details.currency_code
        self._log.debug(f"Account {currency_code=}, {self.base_currency.code=}")

        PyCondition.equal(
            currency_code,
            self.base_currency.code,
            "currency_code",
            "base_currency.code",
        )
        self._log.debug("Base currency matches client details")

    async def _update_account_state(self) -> None:
        try:
            while True:
                try:
                    await asyncio.sleep(self.config.request_account_state_secs)
                    account_state = await self.request_account_state()
                    self._send_account_state(account_state)
                except BetfairError as e:
                    await self.on_api_exception(error=e)
                except Exception as e:
                    # Log and continue on transient errors to keep the update loop running
                    self._log.exception("Error updating account state", e)
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'update_account_state'")
        except Exception as e:
            self._log.exception("Fatal error in account state update loop", e)

    async def request_account_state(self) -> AccountState:
        self._log.debug("Requesting account state")

        account_details = await self._client.get_account_details()
        account_funds = await self._client.get_account_funds()
        timestamp = self._clock.timestamp_ns()
        account_state: AccountState = betfair_account_to_account_state(
            account_detail=account_details,
            account_funds=account_funds,
            event_id=UUID4(),
            reported=True,
            ts_event=timestamp,
            ts_init=timestamp,
            fallback_currency=self.base_currency,
        )
        self._log.debug(f"Received account state: {account_state}")

        return account_state

    def _get_market_version(self, instrument: BettingInstrument) -> MarketVersion | None:
        if not self.config.use_market_version:
            return None

        version = instrument.info.get("version")
        if version is not None:
            return MarketVersion(version=version)

        return None

    def _market_ids_filter(self) -> set[str] | None:
        if (
            self.config.instrument_config
            and self.config.reconcile_market_ids_only
            and self.config.instrument_config.market_ids
        ):
            return set(self.config.instrument_config.market_ids)
        return None

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        return await self._generate_order_status_report_impl(command, retry=True)

    def _select_order_from_multiple(
        self,
        orders: list[CurrentOrderSummary],
        command: GenerateOrderStatusReport,
    ) -> CurrentOrderSummary:
        # Select the appropriate order when multiple orders share the same customer_order_ref,
        # this can happen after replace operations or due to client_order_id collision after
        # 32-char truncation.
        # Strategy: (1) filter by instrument, (2) prefer EXECUTABLE, (3) use most recent.
        if command.instrument_id is not None:
            matching_orders = []

            for o in orders:
                try:
                    order_instrument_id = betfair_instrument_id(
                        market_id=o.market_id,
                        selection_id=o.selection_id,
                        selection_handicap=o.handicap if o.handicap not in (None, 0) else None,
                    )
                    if order_instrument_id == command.instrument_id:
                        matching_orders.append(o)
                except Exception:
                    # If we can't parse instrument, include it to be safe
                    matching_orders.append(o)

            if matching_orders:
                orders = matching_orders

        if len(orders) == 1:
            return orders[0]

        executable_orders = [o for o in orders if o.status == BetfairOrderStatus.EXECUTABLE]
        if executable_orders:
            if len(executable_orders) == 1:
                return executable_orders[0]

            executable_orders.sort(key=lambda o: pd.Timestamp(o.placed_date), reverse=True)
            order = executable_orders[0]
            self._log.warning(
                f"Multiple EXECUTABLE orders for {command.client_order_id=}, "
                f"using most recent: bet_id={order.bet_id}",
            )
            return order

        orders.sort(key=lambda o: pd.Timestamp(o.placed_date), reverse=True)
        order = orders[0]
        self._log.info(
            f"Multiple orders for {command.client_order_id=} (likely replaced), "
            f"using most recent: bet_id={order.bet_id} status={order.status}",
        )
        return order

    async def _generate_order_status_report_impl(
        self,
        command: GenerateOrderStatusReport,
        retry: bool,
    ) -> OrderStatusReport | None:
        self._log.debug(
            f"Listing current orders for {command.venue_order_id=} {command.client_order_id=}",
        )
        assert command.venue_order_id is not None or command.client_order_id is not None, (
            "Require one of venue_order_id or client_order_id"
        )

        try:
            if command.venue_order_id is not None:
                bet_id = command.venue_order_id.value
                orders = await self._client.list_current_orders(bet_ids={bet_id})
            else:
                # Try new truncation first, fall back to legacy for pre-existing orders
                customer_order_ref = make_customer_order_ref(command.client_order_id)
                orders = await self._client.list_current_orders(
                    customer_order_refs={customer_order_ref},
                )
                if not orders:
                    legacy_ref = make_customer_order_ref_legacy(command.client_order_id)
                    if legacy_ref != customer_order_ref:
                        orders = await self._client.list_current_orders(
                            customer_order_refs={legacy_ref},
                        )
        except BetfairError as e:
            await self.on_api_exception(error=e)

            if retry and is_rate_limit_error(e):
                await asyncio.sleep(BETFAIR_RATE_LIMIT_RETRY_DELAY_SECS)
                return await self._generate_order_status_report_impl(command, retry=False)

            # Retry once after session reconnection
            if retry and is_session_error(e):
                return await self._generate_order_status_report_impl(command, retry=False)

            # Non-session errors or retry exhausted - re-raise
            raise

        if not orders:
            self._log.warning(
                f"Could not find order for {command.venue_order_id=} {command.client_order_id=}",
            )
            return None

        if len(orders) == 1:
            order = orders[0]
        else:
            order = self._select_order_from_multiple(orders, command)

        venue_order_id = VenueOrderId(str(order.bet_id))
        client_order_id = self._cache.client_order_id(venue_order_id)

        # Pass cached fill state to handle stale API responses during reconciliation
        cached_filled_qty = self._cache_filled_qty.get(client_order_id) if client_order_id else None
        cached_avg_px = self._cache_avg_px.get(client_order_id) if client_order_id else None

        report: OrderStatusReport = bet_to_order_status_report(
            order=order,
            account_id=self.account_id,
            instrument_id=command.instrument_id,
            venue_order_id=venue_order_id,
            client_order_id=client_order_id,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            cached_filled_qty=cached_filled_qty,
            cached_avg_px=cached_avg_px,
        )

        self._confirm_fill_cache_cleanup(client_order_id, order.size_matched)
        self._sweep_expired_fill_cache()

        self._log.debug(f"Received {report}")
        return report

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        return await self._generate_order_status_reports_impl(command, retry=True)

    async def _generate_order_status_reports_impl(
        self,
        command: GenerateOrderStatusReports,
        retry: bool,
    ) -> list[OrderStatusReport]:
        try:
            current_orders: list[CurrentOrderSummary] = await self._client.list_current_orders(
                order_projection=(
                    OrderProjection.EXECUTABLE if command.open_only else OrderProjection.ALL
                ),
                date_range=TimeRange(
                    from_=_to_utc_datetime(command.start),
                    to=_to_utc_datetime(command.end),
                ),
                market_ids=self._market_ids_filter(),
            )
        except BetfairError as e:
            await self.on_api_exception(error=e)

            if retry and is_rate_limit_error(e):
                await asyncio.sleep(BETFAIR_RATE_LIMIT_RETRY_DELAY_SECS)
                return await self._generate_order_status_reports_impl(command, retry=False)

            # Retry once after session reconnection
            if retry and is_session_error(e):
                return await self._generate_order_status_reports_impl(command, retry=False)

            # Non-session errors or retry exhausted - re-raise
            raise

        ts_init = self._clock.timestamp_ns()

        order_status_reports: list[OrderStatusReport] = []

        for order in current_orders:
            instrument_id = betfair_instrument_id(
                market_id=order.market_id,
                selection_id=order.selection_id,
                selection_handicap=order.handicap or None,
            )
            venue_order_id = VenueOrderId(str(order.bet_id))
            client_order_id = self._cache.client_order_id(venue_order_id)

            # Pass cached fill state to handle stale API responses during reconciliation
            cached_filled_qty = (
                self._cache_filled_qty.get(client_order_id) if client_order_id else None
            )
            cached_avg_px = self._cache_avg_px.get(client_order_id) if client_order_id else None

            report = bet_to_order_status_report(
                order=order,
                account_id=self.account_id,
                instrument_id=instrument_id,
                venue_order_id=venue_order_id,
                client_order_id=client_order_id,
                ts_init=ts_init,
                report_id=UUID4(),
                cached_filled_qty=cached_filled_qty,
                cached_avg_px=cached_avg_px,
            )
            order_status_reports.append(report)

            self._confirm_fill_cache_cleanup(client_order_id, order.size_matched)

        self._sweep_expired_fill_cache()

        return order_status_reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        return await self._generate_fill_reports_impl(command, retry=True)

    async def _generate_fill_reports_impl(
        self,
        command: GenerateFillReports,
        retry: bool,
    ) -> list[FillReport]:
        try:
            cleared_orders: list[CurrentOrderSummary] = await self._client.list_current_orders(
                order_projection=OrderProjection.ALL,
                date_range=TimeRange(
                    from_=_to_utc_datetime(command.start),
                    to=_to_utc_datetime(command.end),
                ),
                market_ids=self._market_ids_filter(),
            )
        except BetfairError as e:
            await self.on_api_exception(error=e)

            if retry and is_rate_limit_error(e):
                await asyncio.sleep(BETFAIR_RATE_LIMIT_RETRY_DELAY_SECS)
                return await self._generate_fill_reports_impl(command, retry=False)

            # Retry once after session reconnection
            if retry and is_session_error(e):
                return await self._generate_fill_reports_impl(command, retry=False)

            # Non-session errors or retry exhausted - re-raise
            raise

        ts_init = self._clock.timestamp_ns()

        fill_reports: list[FillReport] = []

        for order in cleared_orders:
            if order.size_matched == 0.0:
                # No executions, skip
                continue
            instrument_id = betfair_instrument_id(
                market_id=order.market_id,
                selection_id=order.selection_id,
                selection_handicap=order.handicap or None,
            )
            venue_order_id = VenueOrderId(str(order.bet_id))
            client_order_id = self._cache.client_order_id(venue_order_id)
            report = bet_to_fill_report(
                order=order,
                account_id=self.account_id,
                instrument_id=instrument_id,
                venue_order_id=venue_order_id,
                client_order_id=client_order_id,
                base_currency=self.base_currency,
                ts_init=ts_init,
                report_id=UUID4(),
            )
            fill_reports.append(report)

        return fill_reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        self._log.info("Skipping generate_position_status_reports; not implemented for Betfair")

        return []

    def _generate_order_denied_from_command(self, command: SubmitOrder, reason: str) -> None:
        self._try_mark_terminal_order(command.order.client_order_id)

        self.generate_order_denied(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.order.client_order_id,
            reason=reason,
            ts_event=self._clock.timestamp_ns(),
        )

    async def _query_account(self, command: QueryAccount) -> None:
        try:
            account_state = await self.request_account_state()
            self._send_account_state(account_state)
        except BetfairError as e:
            await self.on_api_exception(error=e)
            # User command - no retry needed, they can manually retry if needed

    async def _submit_order(self, command: SubmitOrder) -> None:
        instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            reason = f"INSTRUMENT_NOT_FOUND: {command.instrument_id}"
            self._log.error(f"Cannot submit order: no instrument found for {command.instrument_id}")
            self._generate_order_denied_from_command(command, reason)
            return

        if command.order.is_quote_quantity:
            reason = "UNSUPPORTED_QUOTE_QUANTITY"
            self._log.error(
                f"Cannot submit order {command.order.client_order_id}: {reason}",
            )
            self._generate_order_denied_from_command(command, reason)
            return

        self.generate_order_submitted(
            command.strategy_id,
            command.instrument_id,
            command.order.client_order_id,
            self._clock.timestamp_ns(),
        )

        client_order_id = command.order.client_order_id
        self._customer_order_ref_add(client_order_id)

        place_orders: PlaceOrders = order_submit_to_place_order_params(
            command=command,
            instrument=instrument,
            market_version=self._get_market_version(instrument),
        )

        try:
            result: PlaceExecutionReport = await self._client.place_orders(place_orders)
        except Exception as e:
            await self._handle_submit_error(command, client_order_id, e)
            return

        self._log.debug(f"{result=}")

        # Handle result-level failures when no instruction reports are present
        if not result.instruction_reports:
            if result.status == ExecutionReportStatus.FAILURE:
                reason = self._format_error_reason(result.error_code)
                self._log.warning(f"Submit failed (result-level): {reason}")

                self._customer_order_ref_remove_for_client_id(client_order_id)
                self._try_mark_terminal_order(client_order_id)

                self.generate_order_rejected(
                    command.strategy_id,
                    command.instrument_id,
                    client_order_id,
                    reason,
                    self._clock.timestamp_ns(),
                )
                self._log.debug("Generated _generate_order_rejected")
            return

        for report in result.instruction_reports:
            if report.status in {ExecutionReportStatus.FAILURE, InstructionReportStatus.FAILURE}:
                reason = self._format_error_reason(report.error_code, result.error_code)
                self._log.warning(f"Submit failed: {reason}")

                self._customer_order_ref_remove_for_client_id(client_order_id)
                self._try_mark_terminal_order(client_order_id)

                self.generate_order_rejected(
                    command.strategy_id,
                    command.instrument_id,
                    client_order_id,
                    reason,
                    self._clock.timestamp_ns(),
                )
                self._log.debug("Generated _generate_order_rejected")
                return
            else:
                venue_order_id = VenueOrderId(str(report.bet_id))
                self._log.debug(
                    f"Matching venue_order_id: {venue_order_id} to client_order_id: {client_order_id}",
                )

                # Check before caching so the cache check in _should_skip works correctly
                skip_acceptance = self._should_skip_order_acceptance(client_order_id)

                # Always cache venue_order_id for stream resolution, even if skipping acceptance
                self._cache.add_venue_order_id(client_order_id, venue_order_id)

                if skip_acceptance:
                    continue

                self.generate_order_accepted(
                    command.strategy_id,
                    command.instrument_id,
                    client_order_id,
                    venue_order_id,
                    self._clock.timestamp_ns(),
                )
                self._log.debug("Generated order accepted")

    async def _handle_submit_error(
        self,
        command: SubmitOrder,
        client_order_id: ClientOrderId,
        error: Exception,
    ) -> None:
        if isinstance(error, BetfairError):
            # Betfair responded with an explicit error - order was not placed
            await self.on_api_exception(error=error)
            self._customer_order_ref_remove_for_client_id(client_order_id)
            self._try_mark_terminal_order(client_order_id)
            self.generate_order_rejected(
                command.strategy_id,
                command.instrument_id,
                client_order_id,
                str(error),
                self._clock.timestamp_ns(),
            )
            return

        # Network error - order may have been placed on venue.
        # Leave SUBMITTED and keep rfo so stream can confirm.
        self._log.warning(
            f"Network error placing {client_order_id!r}, "
            f"order may have been placed on venue (leaving SUBMITTED): {error}",
        )

    async def _modify_order(self, command: ModifyOrder) -> None:
        existing_order: Order | None = self._cache.order(client_order_id=command.client_order_id)
        if existing_order is None:
            self._log.warning(
                f"Attempting to update order that does not exist in the cache: {command}",
            )
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                command.venue_order_id,
                "ORDER NOT IN CACHE",
                self._clock.timestamp_ns(),
            )
            return

        if existing_order.venue_order_id is None:
            self._log.warning(f"Order found does not have `id` set: {existing_order}")
            PyCondition.not_none(command.strategy_id, "command.strategy_id")
            PyCondition.not_none(command.instrument_id, "command.instrument_id")
            PyCondition.not_none(command.client_order_id, "client_order_id")

            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                None,
                "ORDER MISSING VENUE_ORDER_ID",
                self._clock.timestamp_ns(),
            )
            return

        # Size and Price cannot be modified in a single operation, so we cannot guarantee
        # an atomic amend (pass or fail).
        if command.quantity not in (None, existing_order.quantity) and command.price not in (
            None,
            existing_order.price,
        ):
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                "CANNOT MODIFY PRICE AND SIZE AT THE SAME TIME",
                self._clock.timestamp_ns(),
            )
            return

        if command.price is not None and command.price != existing_order.price:
            await self._modify_price(command, existing_order)
        elif command.quantity is not None and command.quantity != existing_order.quantity:
            await self._modify_quantity(command, existing_order)
        else:
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                "NOP",
                self._clock.timestamp_ns(),
            )

    async def _modify_price(self, command: ModifyOrder, existing_order: Order) -> None:
        instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot modify order: no instrument found for {command.instrument_id}")
            return

        replace_orders: ReplaceOrders = order_update_to_replace_order_params(
            command=command,
            venue_order_id=existing_order.venue_order_id,
            instrument=instrument,
            market_version=self._get_market_version(instrument),
        )
        pending_key = (command.client_order_id, existing_order.venue_order_id)
        self._pending_update_keys.add(pending_key)

        try:
            result = await self._client.replace_orders(replace_orders)
        except Exception as e:
            # Ensure we remove pending key on exception
            self._pending_update_keys.discard(pending_key)
            if isinstance(e, BetfairError):
                await self.on_api_exception(error=e)

            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                str(e),
                self._clock.timestamp_ns(),
            )
            return

        self._log.debug(f"{result=}")

        if not result.instruction_reports:
            self._pending_update_keys.discard(pending_key)
            self._log.warning(f"Empty instruction_reports for replace: {result}")

            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                "Empty response from Betfair",
                self._clock.timestamp_ns(),
            )
            return

        for report in result.instruction_reports:
            if report.status in {ExecutionReportStatus.FAILURE, InstructionReportStatus.FAILURE}:
                # Ensure we remove pending key on API-level failure
                self._pending_update_keys.discard(pending_key)

                reason = self._format_error_reason(report.error_code, result.error_code)
                self._log.warning(f"Replace failed: {reason}")

                # Use modify_rejected, not order_rejected (original order is still accepted)
                self.generate_order_modify_rejected(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    existing_order.venue_order_id,
                    reason,
                    self._clock.timestamp_ns(),
                )
                return

            # Check the venue_order_id that has been deleted currently exists on our order
            deleted_bet_id = report.cancel_instruction_report.instruction.bet_id
            self._log.debug(f"{existing_order}, {deleted_bet_id}")
            err = f"{deleted_bet_id} != {existing_order.venue_order_id}"
            assert existing_order.venue_order_id == VenueOrderId(str(deleted_bet_id)), err

            place_instruction = report.place_instruction_report
            venue_order_id = VenueOrderId(str(place_instruction.bet_id))
            self._cache.add_venue_order_id(command.client_order_id, venue_order_id, overwrite=True)

            # Clear pending key now that new venue_order_id is cached.
            # Stream updates for the new order can now be matched via venue_order_id.
            self._pending_update_keys.discard(pending_key)

            # Mark old venue_order_id as replaced to ignore late stream updates
            self._replaced_venue_order_ids.add(existing_order.venue_order_id.value)

            self.generate_order_updated(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                venue_order_id,
                betfair_float_to_quantity(place_instruction.instruction.limit_order.size),
                betfair_float_to_price(place_instruction.instruction.limit_order.price),
                None,  # Not applicable for Betfair
                self._clock.timestamp_ns(),
                True,
            )

    async def _modify_quantity(self, command: ModifyOrder, existing_order: Order) -> None:
        instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot modify order: no instrument found for {command.instrument_id}")
            return

        # Check before subtraction to avoid ValueError from Quantity
        if command.quantity >= existing_order.quantity:
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                f"Cannot increase quantity: {command.quantity} >= {existing_order.quantity}",
                self._clock.timestamp_ns(),
            )
            return

        size_reduction = existing_order.quantity - command.quantity

        cancel_orders: CancelOrders = order_update_to_cancel_order_params(
            command=command,
            instrument=instrument,
            size_reduction=size_reduction,
        )

        try:
            result: CancelExecutionReport = await self._client.cancel_orders(cancel_orders)
        except Exception as e:
            if isinstance(e, BetfairError):
                await self.on_api_exception(error=e)

            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                str(e),
                self._clock.timestamp_ns(),
            )
            return

        self._log.debug(f"{result=}")

        if not result.instruction_reports:
            self._log.warning(f"Empty instruction_reports for size reduction: {result}")

            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                "Empty response from Betfair",
                self._clock.timestamp_ns(),
            )
            return

        for report in result.instruction_reports:
            if report.status in {ExecutionReportStatus.FAILURE, InstructionReportStatus.FAILURE}:
                reason = self._format_error_reason(report.error_code, result.error_code)
                self._log.warning(f"Size reduction failed: {reason}")

                # Use modify_rejected, not order_rejected (original order is still accepted)
                self.generate_order_modify_rejected(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    existing_order.venue_order_id,
                    reason,
                    self._clock.timestamp_ns(),
                )
                return

            self.generate_order_updated(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,  # Use existing order's venue_order_id, not command's
                betfair_float_to_quantity(existing_order.quantity - report.size_cancelled),
                betfair_float_to_price(existing_order.price),
                None,  # Not applicable for Betfair
                self._clock.timestamp_ns(),
                False,
            )

    async def _cancel_order(self, command: CancelOrder) -> None:
        instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot cancel order: no instrument found for {command.instrument_id}")
            return

        cancel_orders = order_cancel_to_cancel_order_params(
            command=command,
            instrument=instrument,
        )
        self._log.debug(f"cancel_orders: {cancel_orders}")

        try:
            result = await self._client.cancel_orders(cancel_orders)
        except Exception as e:
            if isinstance(e, BetfairError):
                await self.on_api_exception(error=e)

            self.generate_order_cancel_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                command.venue_order_id,
                str(e),
                self._clock.timestamp_ns(),
            )
            return

        self._log.debug(f"{result=}")

        if not result.instruction_reports:
            self._log.warning(f"Empty instruction_reports for cancel: {result}")

            self.generate_order_cancel_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                command.venue_order_id,
                "Empty response from Betfair",
                self._clock.timestamp_ns(),
            )
            return

        for report in result.instruction_reports:
            venue_order_id = VenueOrderId(str(report.instruction.bet_id))
            if (
                report.status == InstructionReportStatus.FAILURE
                and report.error_code != InstructionReportErrorCode.BET_TAKEN_OR_LAPSED
            ):
                reason = f"{report.error_code.name}: {report.error_code.__doc__}"
                self._log.warning(f"Cancel failed: {reason}")

                self.generate_order_cancel_rejected(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    venue_order_id,
                    reason,
                    self._clock.timestamp_ns(),
                )
                return

            self._log.debug(
                f"Matching venue_order_id: {venue_order_id} to client_order_id: {command.client_order_id}",
            )
            self._cache.add_venue_order_id(command.client_order_id, venue_order_id, overwrite=True)

            # Guard against duplicate cancel events (stream may have already processed)
            if not self._try_mark_terminal_order(command.client_order_id):
                self._log.debug(
                    f"Order {command.client_order_id!r} already terminal, skipping cancel event",
                )
                return

            self.generate_order_canceled(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                venue_order_id,
                self._clock.timestamp_ns(),
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        open_orders = self._cache.orders_open(
            instrument_id=command.instrument_id,
            side=command.order_side,
        )

        for order in open_orders:
            command = CancelOrder(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                command_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )

            self.cancel_order(command)

    def _should_skip_order_acceptance(self, client_order_id: ClientOrderId) -> bool:
        if client_order_id.value in self._terminal_orders:
            self._log.debug(f"Order {client_order_id!r} already terminal, skipping acceptance")
            return True

        existing_venue_order_id = self._cache.venue_order_id(client_order_id)
        if existing_venue_order_id is not None:
            self._log.debug(
                f"Stream already cached {existing_venue_order_id!r} for {client_order_id!r}, "
                f"skipping acceptance",
            )
            return True

        return False

    def _resolve_client_order_id(
        self,
        unmatched_order: UnmatchedOrder,
    ) -> ClientOrderId | None:
        # Primary: rfo match (works before bet_id is known)
        if unmatched_order.rfo:
            client_order_id = self._customer_order_refs.get(unmatched_order.rfo)
            if client_order_id is not None:
                return client_order_id

        # Secondary: venue_order_id via cache
        venue_order_id = VenueOrderId(str(unmatched_order.id))
        return self._cache.client_order_id(venue_order_id)

    def _customer_order_ref_add(self, client_order_id: ClientOrderId) -> None:
        customer_order_ref = make_customer_order_ref(client_order_id)
        self._customer_order_refs[customer_order_ref] = client_order_id

    def _customer_order_ref_add_with_legacy(self, client_order_id: ClientOrderId) -> None:
        # Register both new and legacy truncations for backwards compatibility
        self._customer_order_refs[make_customer_order_ref(client_order_id)] = client_order_id
        self._customer_order_refs[make_customer_order_ref_legacy(client_order_id)] = client_order_id

    def _customer_order_ref_remove(self, customer_order_ref: str) -> None:
        self._customer_order_refs.pop(customer_order_ref, None)

    def _customer_order_ref_remove_for_client_id(self, client_order_id: ClientOrderId) -> None:
        # Remove both truncations in case either was registered
        self._customer_order_refs.pop(make_customer_order_ref(client_order_id), None)
        self._customer_order_refs.pop(make_customer_order_ref_legacy(client_order_id), None)

    def _cleanup_terminal_order(
        self,
        unmatched_order: UnmatchedOrder,
        client_order_id: ClientOrderId,
    ) -> None:
        # Don't cleanup if there's a pending replace - the new order uses the same rfo
        has_pending_update = any(cid == client_order_id for cid, _ in self._pending_update_keys)
        if has_pending_update:
            return

        self._customer_order_ref_remove_for_client_id(client_order_id)

    def handle_order_stream_update(self, raw: bytes) -> None:
        """
        Handle an update from the order stream socket.
        """
        self._log.debug(f"[RECV]: {raw.decode()}")
        update = stream_decode(raw)

        if isinstance(update, OCM):
            self.create_task(self._handle_order_stream_update(update))
        elif isinstance(update, Connection):
            self._log.info(f"Connection opened, connection_id={update.connection_id}")
        elif isinstance(update, Status):
            self._handle_status_message(update=update)
        else:
            raise RuntimeError(f"Cannot handle order stream update: {update}")

    async def _handle_order_stream_update(self, order_change_message: OCM) -> None:
        for market in order_change_message.oc or []:
            if market.orc is None:
                continue

            if self._stream_market_ids_filter and market.id not in self._stream_market_ids_filter:
                continue

            for selection in market.orc:
                if selection.uo is None:
                    continue

                instrument_id = betfair_instrument_id(
                    market_id=market.id,
                    selection_id=selection.id,
                    selection_handicap=selection.hc,
                )
                instrument = self._cache.instrument(instrument_id)
                if instrument is None:
                    self._log.warning(
                        f"Instrument {instrument_id} not loaded, "
                        f"dropping {len(selection.uo)} update(s) for {instrument_id} (may occur during startup)",
                    )
                    continue

                for unmatched_order in selection.uo:
                    self._process_order_update(unmatched_order, instrument)

        if self._contains_full_image(order_change_message):
            self.check_cache_against_order_image(order_change_message)

    def _process_order_update(
        self,
        unmatched_order: UnmatchedOrder,
        instrument: BettingInstrument,
    ) -> None:
        client_order_id = self._resolve_client_order_id(unmatched_order)
        if client_order_id is None:
            self._log_skipped_external_order(unmatched_order, instrument.id)
            return

        # Any stream update for a SUBMITTED order confirms it exists on venue.
        # Guard on venue_order_id not yet cached to avoid duplicate acceptance
        # when the HTTP response path has already emitted one.
        order = self._cache.order(client_order_id)
        if (
            order is not None
            and order.status == OrderStatus.SUBMITTED
            and self._cache.venue_order_id(client_order_id) is None
        ):
            venue_order_id = VenueOrderId(str(unmatched_order.id))
            self._cache.add_venue_order_id(client_order_id, venue_order_id)
            self.generate_order_accepted(
                order.strategy_id,
                instrument.id,
                client_order_id,
                venue_order_id,
                self._clock.timestamp_ns(),
            )

        if unmatched_order.status == BETFAIR_ORDER_STATUS_EXECUTABLE:
            self._handle_stream_executable_order_update(
                unmatched_order,
                client_order_id,
                instrument,
            )
        elif unmatched_order.status == BETFAIR_ORDER_STATUS_EXECUTION_COMPLETE:
            self._handle_stream_execution_complete_order_update(
                unmatched_order,
                client_order_id,
                instrument,
            )
        else:
            self._log.warning(f"Unknown order status: {unmatched_order.status}")

    def _log_skipped_external_order(
        self,
        unmatched_order: UnmatchedOrder,
        instrument_id: InstrumentId,
    ) -> None:
        msg = (
            f"Skipping external order: bet_id={unmatched_order.id}, "
            f"rfo={unmatched_order.rfo!r}, instrument_id={instrument_id}"
        )
        if self.config.ignore_external_orders:
            self._log.debug(msg)
        else:
            self._log.warning(msg)

    def _contains_full_image(self, order_change_message: OCM) -> bool:
        return any(
            selection.full_image
            for market in (order_change_message.oc or [])
            if market.orc
            for selection in market.orc
        )

    def check_cache_against_order_image(self, order_change_message: OCM) -> None:
        for market in order_change_message.oc or []:
            if self._stream_market_ids_filter and market.id not in self._stream_market_ids_filter:
                continue

            for selection in market.orc or []:
                instrument_id = betfair_instrument_id(
                    market_id=market.id,
                    selection_id=selection.id,
                    selection_handicap=selection.hc,
                )
                orders = self._cache.orders(instrument_id=instrument_id)
                venue_orders = {o.venue_order_id: o for o in orders}

                for unmatched_order in selection.uo or []:
                    self._check_unmatched_order_known(unmatched_order, venue_orders)

                self._check_fills_known(selection, orders, instrument_id)

    def _check_unmatched_order_known(
        self,
        unmatched_order: UnmatchedOrder,
        venue_orders: dict[VenueOrderId, Order],
    ) -> None:
        venue_order_id = VenueOrderId(str(unmatched_order.id))
        order = venue_orders.get(venue_order_id)
        if order is not None:
            return

        # Check if we know this order via rfo (stream arrived before HTTP response)
        client_order_id = self._resolve_client_order_id(unmatched_order)
        if client_order_id is None and not self.config.ignore_external_orders:
            self._log.error(f"Unknown order not in cache: {unmatched_order=}")

    def _check_fills_known(
        self,
        selection,
        orders: list[Order],
        instrument_id: InstrumentId,
    ) -> None:
        matched_orders = [(OrderSide.SELL, lay) for lay in (selection.ml or [])] + [
            (OrderSide.BUY, back) for back in (selection.mb or [])
        ]

        for side, matched_order in matched_orders:
            # We don't get much information from Betfair here, try our best to match
            price = betfair_float_to_price(matched_order.price)
            quantity = betfair_float_to_quantity(matched_order.size)
            matched = False

            for order in orders:
                for event in order.events:
                    if isinstance(event, OrderFilled) and (
                        order.side == side and order.price == price and quantity <= order.quantity
                    ):
                        matched = True

            if not matched and not self.config.ignore_external_orders:
                self._log.error(f"Unknown fill: {instrument_id=}, {matched_order=}")

    def _process_order_fill(  # noqa: C901
        self,
        unmatched_order: UnmatchedOrder,
        client_order_id: ClientOrderId,
        instrument: BettingInstrument,
    ) -> Order | None:
        venue_order_id = VenueOrderId(str(unmatched_order.id))

        order = self._cache.order(client_order_id=client_order_id)
        if order is None:
            self._log.error(
                f"Cannot handle update: order not found for {client_order_id!r}",
            )
            return None

        sm_qty = betfair_float_to_quantity(unmatched_order.sm) if unmatched_order.sm else None
        if sm_qty is None:
            return order

        # Cache venue_order_id if stream fill arrived before HTTP response (matched via rfo).
        # Only cache when processing a fill so HTTP response emits acceptance for no-fill updates.
        if order.status == OrderStatus.SUBMITTED:
            self._log.debug(
                f"Stream fill arrived before HTTP response for {client_order_id!r}",
            )
            self._cache.add_venue_order_id(client_order_id, venue_order_id)

        # Compute baseline using cache (same logic as _determine_fill_qty)
        cache_filled_qty = self._cache_filled_qty.get(client_order_id)
        baseline_qty = (
            max(cache_filled_qty, order.filled_qty) if cache_filled_qty else order.filled_qty
        )

        if sm_qty <= baseline_qty:
            self._log.debug(
                f"Fill skipped: sm_qty={sm_qty} <= baseline_qty={baseline_qty} "
                f"for {client_order_id!r}, bet_id={unmatched_order.id}",
            )
            return order

        trade_id = order_to_trade_id(unmatched_order)
        if trade_id.value in self._published_executions:
            self._log.debug(
                f"Fill skipped: duplicate trade_id={trade_id!r} for {client_order_id!r}",
            )
            return order

        result = self._determine_fill_qty(unmatched_order, order)
        if result.fill_qty == 0:
            self._log.warning(
                f"Fill size zero: sm={unmatched_order.sm}, "
                f"baseline_qty={baseline_qty} for {client_order_id!r}",
            )
            return order

        if result.total_matched_qty > order.quantity:
            self._log.warning(
                f"Rejecting potential overfill for {client_order_id!r}: "
                f"order.quantity={order.quantity}, total_matched={result.total_matched_qty}, "
                f"sm={unmatched_order.sm}",
            )
            return order

        try:
            fill_price = self._determine_fill_price(unmatched_order, order)
            if fill_price <= 0:
                self._log.warning(
                    f"Skipping fill with invalid price={fill_price} for {client_order_id!r}",
                )
                return order
            last_px = betfair_float_to_price(fill_price)
        except ValueError as e:
            self._log.warning(
                f"Skipping fill: invalid price conversion, "
                f"client_order_id={client_order_id!r}, error={e}",
            )
            return order

        ts_event = self._get_matched_timestamp(unmatched_order)

        self.generate_order_filled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=None,
            trade_id=trade_id,
            order_side=OrderSideParser.to_nautilus(unmatched_order.side),
            order_type=OrderType.LIMIT,
            last_qty=result.fill_qty,
            last_px=last_px,
            quote_currency=instrument.quote_currency,
            commission=Money(0, self.base_currency),
            liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            ts_event=ts_event,
        )

        avg_px = self._compute_avg_px_for_cache(
            unmatched_order.avp,
            fill_price,
            result.fill_qty,
            result.total_matched_qty,
            order.client_order_id,
        )
        self._update_fill_cache(result.total_matched_qty, avg_px, order)
        self._published_executions.add(trade_id.value)

        if result.total_matched_qty >= order.quantity:
            self._try_mark_terminal_order(client_order_id)

        return order

    def _handle_stream_executable_order_update(
        self,
        unmatched_order: UnmatchedOrder,
        client_order_id: ClientOrderId,
        instrument: BettingInstrument,
    ) -> None:
        self._process_order_fill(unmatched_order, client_order_id, instrument)

    def _handle_stream_execution_complete_order_update(  # noqa: C901
        self,
        unmatched_order: UnmatchedOrder,
        client_order_id: ClientOrderId,
        instrument: BettingInstrument,
    ) -> None:
        order = self._process_order_fill(unmatched_order, client_order_id, instrument)
        if order is None:
            self._cleanup_terminal_order(unmatched_order, client_order_id)
            return

        venue_order_id = VenueOrderId(str(unmatched_order.id))

        # Publish void event if matched bets were voided (e.g., VAR decision)
        if unmatched_order.sv and unmatched_order.sv > 0:
            self._log.info(
                f"{client_order_id!r} voided: size_voided={unmatched_order.sv}",
            )
            voided = BetfairOrderVoided(
                instrument_id=instrument.id,
                client_order_id=client_order_id.value,
                venue_order_id=venue_order_id.value,
                size_voided=unmatched_order.sv,
                price=unmatched_order.p,
                size=unmatched_order.s,
                side=unmatched_order.side,
                avg_price_matched=unmatched_order.avp,
                size_matched=unmatched_order.sm,
                reason=None,
                ts_event=self._get_canceled_timestamp(unmatched_order),
                ts_init=self._clock.timestamp_ns(),
            )
            custom_data = CustomData(
                DataType(BetfairOrderVoided, {"instrument_id": instrument.id}),
                voided,
            )
            self._handle_data(custom_data)

        # Check for cancel
        cancel_qty = self._get_cancel_quantity(unmatched_order)
        if cancel_qty > 0 and not order.is_closed:
            key = (client_order_id, venue_order_id)
            self._log.debug(
                f"cancel key: {key}, pending_update_order_client_ids: {self._pending_update_keys}",
            )
            # If this is the result of a ModifyOrder, we don't want to emit a cancel
            if key not in self._pending_update_keys:
                # Skip late cancel updates for orders that have been replaced
                if venue_order_id.value in self._replaced_venue_order_ids:
                    self._log.debug(
                        f"Skipping cancel for replaced venue_order_id={venue_order_id!r}",
                    )
                    return
                # Guard against duplicate terminal events from race conditions
                if not self._try_mark_terminal_order(client_order_id):
                    self._log.debug(f"Skipping duplicate cancel for {client_order_id!r}")

                    self._cleanup_terminal_order(unmatched_order, client_order_id)
                    return

                # The remainder of this order has been canceled
                canceled_ts = self._get_canceled_timestamp(unmatched_order)

                self.generate_order_canceled(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=canceled_ts,
                )
            else:
                # Cancel originates from a ReplaceOrders amend that we initiated.
                # Suppress the synthetic cancel event - the order is being modified, not canceled.
                # Don't discard pending_key or cleanup rfo here - the HTTP response handler
                # will discard the key after caching the new venue_order_id. This ensures
                # stream updates for the replacement order can still be matched via rfo
                # if they arrive before the HTTP response.
                return
        # Check for lapse
        elif unmatched_order.lapse_status_reason_code is not None:
            # This order has lapsed. No lapsed size was found in the above check for cancel,
            # so we assume size lapsed None implies the entire order.
            self._log.info(
                f"{client_order_id!r}, {venue_order_id!r} lapsed on cancel: "
                f"lapse_status={unmatched_order.lapse_status_reason_code}, "
                f"size_lapsed={unmatched_order.sl}",
            )

            order = self._cache.order(client_order_id)
            if order is None:
                self._log.error(f"Cannot handle lapse: {client_order_id!r} not found in cache")
                self._cleanup_terminal_order(unmatched_order, client_order_id)
                return

            # Check if order is still open before generating a cancel
            if order.is_open:
                # Skip late lapse updates for orders that have been replaced
                if venue_order_id.value in self._replaced_venue_order_ids:
                    self._log.debug(
                        f"Skipping lapse for replaced venue_order_id={venue_order_id!r}",
                    )
                    return

                # Guard against duplicate terminal events from race conditions
                if not self._try_mark_terminal_order(client_order_id):
                    self._log.debug(f"Skipping duplicate lapse cancel for {client_order_id!r}")
                    self._cleanup_terminal_order(unmatched_order, client_order_id)
                    return

                canceled_ts = self._get_canceled_timestamp(unmatched_order)

                self.generate_order_canceled(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=canceled_ts,
                )

        # Do not clear _published_executions here to prevent duplicate fills from
        # late 'EC' messages or after reconnects. Cache persists for client lifetime.

        self._cleanup_terminal_order(unmatched_order, client_order_id)

    def _handle_status_message(self, update: Status) -> None:
        if update.is_error:
            if update.error_code == StatusErrorCode.MAX_CONNECTION_LIMIT_EXCEEDED:
                raise RuntimeError("No more connections available")
            elif update.error_code == StatusErrorCode.SUBSCRIPTION_LIMIT_EXCEEDED:
                raise RuntimeError("Subscription request limit exceeded")

            self._log.warning(f"Betfair API error: {update.error_message}")

            if update.connection_closed:
                self._log.warning("Betfair connection closed")
                if self._is_reconnecting:
                    self._log.info("Reconnect already in progress")
                    return
                self.create_task(self._reconnect())

    def _determine_fill_price(self, unmatched_order: UnmatchedOrder, order: Order) -> float:
        if not unmatched_order.avp:
            # No average price matched, assume filled at limit price
            return unmatched_order.p

        # Use max of cache and order state to handle reconciliation
        cache_filled_qty = self._cache_filled_qty.get(order.client_order_id)
        cache_avg_px = self._cache_avg_px.get(order.client_order_id)

        if cache_filled_qty is not None and cache_filled_qty >= order.filled_qty:
            prev_qty = cache_filled_qty
            prev_avg_price = cache_avg_px
        elif order.filled_qty > 0:
            prev_qty = order.filled_qty
            prev_avg_price = order.avg_px
        else:
            # No previous fills, this is first fill
            return unmatched_order.avp

        new_avg_price = float(unmatched_order.avp)
        if prev_avg_price == new_avg_price:
            # Matched at same price
            return unmatched_order.avp

        # Calculate individual fill price from weighted average
        prev_size = float(prev_qty)
        new_size = float(unmatched_order.sm) - prev_size
        total_size = prev_size + new_size

        if new_size == 0 or total_size == 0:
            self._log.warning(
                f"Avoided division by zero: {prev_avg_price=} {prev_size=} "
                f"{new_avg_price=} {new_size=}",
            )
            return prev_avg_price

        price = (new_avg_price - (prev_avg_price * (prev_size / total_size))) / (
            new_size / total_size
        )

        if price <= 0:
            self._log.warning(
                f"Calculated fill price {price} is invalid, "
                f"falling back to avp={unmatched_order.avp}: "
                f"{prev_avg_price=} {prev_size=} {new_avg_price=} {new_size=}",
            )
            return unmatched_order.avp

        self._log.debug(
            f"Calculating fill price: {prev_avg_price=} {prev_size=} "
            f"{new_avg_price=} {new_size=} == {price=}",
        )
        return price

    def _determine_fill_qty(
        self,
        unmatched_order: UnmatchedOrder,
        order: Order,
    ) -> FillQtyResult:
        total_matched_qty = betfair_float_to_quantity(unmatched_order.sm or 0.0)

        # Use max of cache and order state to handle startup/reconnect
        cache_filled_qty = self._cache_filled_qty.get(order.client_order_id)
        baseline_qty = (
            max(cache_filled_qty, order.filled_qty) if cache_filled_qty else order.filled_qty
        )

        if total_matched_qty <= baseline_qty:
            return FillQtyResult(Quantity.zero(BETFAIR_QUANTITY_PRECISION), total_matched_qty)

        fill_qty = total_matched_qty - baseline_qty
        return FillQtyResult(fill_qty, total_matched_qty)

    def _compute_avg_px_for_cache(
        self,
        avp: float | None,
        fill_price: float,
        fill_qty: Quantity,
        total_matched_qty: Quantity,
        client_order_id: ClientOrderId,
    ) -> float:
        if avp is not None:
            return avp

        cache_filled_qty = self._cache_filled_qty.get(client_order_id)
        cache_avg_px = self._cache_avg_px.get(client_order_id)
        if cache_filled_qty is None or cache_avg_px is None:
            return fill_price

        # Compute weighted average when avp is missing
        prev_value = float(cache_filled_qty) * cache_avg_px
        new_value = float(fill_qty) * fill_price
        return (prev_value + new_value) / float(total_matched_qty)

    def _update_fill_cache(
        self,
        total_matched_qty: Quantity,
        avg_px: float,
        order: Order,
    ) -> None:
        # Always retain the cache entry, even for fully filled orders.
        # Cleanup is deferred to HTTP reconciliation confirmation to avoid
        # a race where the API returns stale fill qty after the stream
        # has already applied all fills.
        self._cache_filled_qty[order.client_order_id] = total_matched_qty
        self._cache_avg_px[order.client_order_id] = avg_px

        if total_matched_qty >= order.quantity:
            self._cache_filled_completed_ns[order.client_order_id] = self._clock.timestamp_ns()

    def _confirm_fill_cache_cleanup(
        self,
        client_order_id: ClientOrderId | None,
        api_size_matched: float,
    ) -> None:
        if client_order_id is None:
            return

        cached_qty = self._cache_filled_qty.get(client_order_id)
        if cached_qty is None:
            return

        api_qty = Quantity(api_size_matched, BETFAIR_QUANTITY_PRECISION)
        if api_qty >= cached_qty:
            self._evict_fill_cache(client_order_id)

    def _on_fill_cache_sweep_timer(self, event: TimeEvent) -> None:
        self._log.debug(f"Fill cache sweep timer fired: {event}")
        self._sweep_expired_fill_cache()

    def _sweep_expired_fill_cache(self) -> None:
        ts_now = self._clock.timestamp_ns()
        expired = [
            cid
            for cid, ts in self._cache_filled_completed_ns.items()
            if (ts_now - ts) > BETFAIR_FILL_CACHE_TTL_NS
        ]
        for cid in expired:
            self._evict_fill_cache(cid)

    def _evict_fill_cache(self, client_order_id: ClientOrderId) -> None:
        self._cache_filled_qty.pop(client_order_id, None)
        self._cache_filled_completed_ns.pop(client_order_id, None)
        self._cache_avg_px.pop(client_order_id, None)

    def _get_matched_timestamp(self, unmatched_order: UnmatchedOrder) -> int:
        if unmatched_order.md is None:
            self._log.warning("Matched timestamp was `None` from Betfair, using current time")
            return self._clock.timestamp_ns()
        return millis_to_nanos(unmatched_order.md)

    def _get_canceled_timestamp(self, unmatched_order: UnmatchedOrder) -> int:
        canceled_ms = unmatched_order.cd or unmatched_order.ld or unmatched_order.md
        return millis_to_nanos(canceled_ms) if canceled_ms else self._clock.timestamp_ns()

    def _get_cancel_quantity(self, unmatched_order: UnmatchedOrder) -> float:
        return (unmatched_order.sc or 0) + (unmatched_order.sl or 0) + (unmatched_order.sv or 0)

    def _handle_data(self, data: Data) -> None:
        self._msgbus.send(endpoint="DataEngine.process", msg=data)

    def _format_error_reason(self, error_code, result_error_code=None) -> str:
        parts = []
        if error_code is not None:
            parts.append(f"{error_code.name} ({error_code.__doc__})")
        if result_error_code is not None and result_error_code != error_code:
            parts.append(f"result={result_error_code.name} ({result_error_code.__doc__})")
        if parts:
            return ", ".join(parts)
        return "UNKNOWN_ERROR"


def _to_utc_datetime(timestamp: datetime | pd.Timestamp | None) -> datetime | None:
    if timestamp is None:
        return None

    if isinstance(timestamp, pd.Timestamp):
        return ensure_pydatetime_utc(timestamp)

    return as_utc_timestamp(timestamp)
