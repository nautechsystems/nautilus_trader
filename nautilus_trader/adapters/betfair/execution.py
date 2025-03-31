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
import traceback
from collections import defaultdict

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
from betfair_parser.spec.betting.type_definitions import PlaceExecutionReport
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
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.parsing.common import betfair_instrument_id
from nautilus_trader.adapters.betfair.parsing.requests import bet_to_fill_report
from nautilus_trader.adapters.betfair.parsing.requests import bet_to_order_status_report
from nautilus_trader.adapters.betfair.parsing.requests import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing.requests import make_customer_order_ref
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
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
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
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
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
        self.check_order_timeout_secs = 10.0
        self._log.info(f"{config.account_currency=}", LogColor.BLUE)
        self._log.info(f"{config.calculate_account_state=}", LogColor.BLUE)
        self._log.info(f"{config.request_account_state_secs=}", LogColor.BLUE)
        self._log.info(f"{self.check_order_timeout_secs=}", LogColor.BLUE)

        # Clients
        self._client: BetfairHttpClient = client
        self._stream = BetfairOrderStreamClient(
            http_client=self._client,
            message_handler=self.handle_order_stream_update,
            certs_dir=config.certs_dir,
        )
        self._is_reconnecting = (
            False  # Necessary for coordination, as the clients rely on each other
        )

        # Async tasks
        self._update_account_task: asyncio.Task | None = None

        # Hot caches:
        # Tracks filled quantities separately from order state since Betfair only provides
        # cumulative matched sizes (sm). This lets us calculate incremental fills
        # while avoiding race conditions with delayed order state updates.
        self._filled_qty_cache: dict[ClientOrderId, Quantity] = {}

        # Tracks orders with pending updates to ensure state consistency during asynchronous processing
        self._pending_update_order_client_ids: set[tuple[ClientOrderId, VenueOrderId]] = set()

        # Stores published executions per order to avoid duplicates and support reconciliation
        self._published_executions: dict[ClientOrderId, list[TradeId]] = defaultdict(list)

    @property
    def instrument_provider(self) -> BetfairInstrumentProvider:
        """
        Return the instrument provider for the client.

        Returns
        -------
        BetfairInstrumentProvider

        """
        return self._instrument_provider

    # -- CONNECTION HANDLERS ----------------------------------------------------------------------

    async def _connect(self) -> None:
        await self._client.connect()

        # Connections and start-up checks
        self._log.debug(
            "Connecting to stream, checking account currency and loading venue ID mapping...",
        )
        aws = [
            self._stream.connect(),
            self.check_account_currency(),
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

        if self._update_account_task:
            self._update_account_task.cancel()
            self._update_account_task = None

        await self._client.reconnect()
        await self._stream.reconnect()

        account_state = await self.request_account_state()
        self._send_account_state(account_state)

        if self.config.request_account_state_secs:
            self._update_account_task = self.create_task(self._update_account_state())

        self._is_reconnecting = False

    async def _disconnect(self) -> None:
        # Cancel tasks
        if self._update_account_task:
            self._log.debug("Canceling task 'update_account_task'")
            self._update_account_task.cancel()
            self._update_account_task = None

        self._log.info("Closing streaming socket")
        await self._stream.disconnect()

        self._log.info("Closing BetfairHttpClient")
        await self._client.disconnect()

    # -- ERROR HANDLING ---------------------------------------------------------------------------
    async def on_api_exception(self, error: BetfairError) -> None:
        if "INVALID_SESSION_INFORMATION" in error.args[0] or "NO_SESSION" in error.args[0]:
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
            except Exception:
                self._log.error(f"Reconnection failed: {traceback.format_exc()}")

    # -- ACCOUNT HANDLERS -------------------------------------------------------------------------

    async def _update_account_state(self) -> None:
        try:
            while True:
                try:
                    await asyncio.sleep(self.config.request_account_state_secs)
                    account_state = await self.request_account_state()
                    self._send_account_state(account_state)
                except BetfairError as e:
                    self._log.warning(str(e))
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'update_account_state'")
        except Exception as e:
            self._log.exception("Error updating account state", e)

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
        )
        self._log.debug(f"Received account state: {account_state}")

        return account_state

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

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
        self._log.debug(
            f"Listing current orders for {command.venue_order_id=} {command.client_order_id=}",
        )
        assert (
            command.venue_order_id is not None or command.client_order_id is not None
        ), "Require one of venue_order_id or client_order_id"

        try:
            if command.venue_order_id is not None:
                bet_id = command.venue_order_id.value
                orders = await self._client.list_current_orders(bet_ids={bet_id})
            else:
                customer_order_ref = make_customer_order_ref(command.client_order_id)
                orders = await self._client.list_current_orders(
                    customer_order_refs={customer_order_ref},
                )
        except BetfairError as e:
            self._log.warning(str(e))
            return None

        if not orders:
            self._log.warning(
                f"Could not find order for {command.venue_order_id=} {command.client_order_id=}",
            )
            return None

        # We have a response, check list length and grab first entry
        assert (
            len(orders) == 1
        ), f"More than one order found for {command.venue_order_id=} {command.client_order_id=}"
        order: CurrentOrderSummary = orders[0]
        venue_order_id = VenueOrderId(str(order.bet_id))

        report: OrderStatusReport = bet_to_order_status_report(
            order=order,
            account_id=self.account_id,
            instrument_id=command.instrument_id,
            venue_order_id=venue_order_id,
            client_order_id=self._cache.client_order_id(venue_order_id),
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._log.debug(f"Received {report}")
        return report

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        current_orders: list[CurrentOrderSummary] = await self._client.list_current_orders(
            order_projection=(
                OrderProjection.EXECUTABLE if command.open_only else OrderProjection.ALL
            ),
            date_range=TimeRange(from_=command.start, to=command.end),
            market_ids=self._market_ids_filter(),
        )

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
            report = bet_to_order_status_report(
                order=order,
                account_id=self.account_id,
                instrument_id=instrument_id,
                venue_order_id=venue_order_id,
                client_order_id=client_order_id,
                ts_init=ts_init,
                report_id=UUID4(),
            )
            order_status_reports.append(report)

        return order_status_reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        cleared_orders: list[CurrentOrderSummary] = await self._client.list_current_orders(
            order_projection=OrderProjection.ALL,
            date_range=TimeRange(from_=command.start, to=command.end),
            market_ids=self._market_ids_filter(),
        )

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

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot submit order: no instrument found for {command.instrument_id}")
            return

        self.generate_order_submitted(
            command.strategy_id,
            command.instrument_id,
            command.order.client_order_id,
            self._clock.timestamp_ns(),
        )

        client_order_id = command.order.client_order_id

        place_orders: PlaceOrders = order_submit_to_place_order_params(
            command=command,
            instrument=instrument,
        )
        try:
            result: PlaceExecutionReport = await self._client.place_orders(place_orders)
        except Exception as e:
            if isinstance(e, BetfairError):
                await self.on_api_exception(error=e)
            self._log.warning(f"Submit failed: {e}")
            self.generate_order_rejected(
                command.strategy_id,
                command.instrument_id,
                client_order_id,
                "client error",
                self._clock.timestamp_ns(),
            )
            return

        self._log.debug(f"{result=}")

        for report in result.instruction_reports or []:
            if result.status == ExecutionReportStatus.FAILURE:
                reason = f"{result.error_code.name} ({result.error_code.__doc__})"
                self._log.warning(f"Submit failed: {reason}")
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
                self._cache.add_venue_order_id(client_order_id, venue_order_id)
                self.generate_order_accepted(
                    command.strategy_id,
                    command.instrument_id,
                    client_order_id,
                    venue_order_id,
                    self._clock.timestamp_ns(),
                )
                self._log.debug("Generated order accepted")

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

        # Send order to client
        replace_orders: ReplaceOrders = order_update_to_replace_order_params(
            command=command,
            venue_order_id=existing_order.venue_order_id,
            instrument=instrument,
        )
        self._pending_update_order_client_ids.add(
            (command.client_order_id, existing_order.venue_order_id),
        )
        try:
            result = await self._client.replace_orders(replace_orders)
        except Exception as e:
            if isinstance(e, BetfairError):
                await self.on_api_exception(error=e)
            self._log.warning(f"Modify failed (px): {e}")
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                "client error",
                self._clock.timestamp_ns(),
            )
            return

        self._log.debug(f"{result=}")

        for report in result.instruction_reports or []:
            if report.status in {ExecutionReportStatus.FAILURE, InstructionReportStatus.FAILURE}:
                reason = f"{result.error_code.name} ({result.error_code.__doc__})"
                self._log.warning(f"Replace failed: {reason}")
                self.generate_order_rejected(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
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

        size_reduction = existing_order.quantity - command.quantity
        if size_reduction <= 0:
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                f"Insufficient remaining quantity: {size_reduction}",
                self._clock.timestamp_ns(),
            )
            return

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
            self._log.warning(f"Modify failed (qty): {e}")
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                "client error",
                self._clock.timestamp_ns(),
            )
            return

        self._log.debug(f"{result=}")

        for report in result.instruction_reports or []:
            if report.status in {ExecutionReportStatus.FAILURE, InstructionReportStatus.FAILURE}:
                reason = f"{result.error_code.name} ({result.error_code.__doc__})"
                self._log.warning(f"Size reduction failed: {reason}")
                self.generate_order_rejected(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    reason,
                    self._clock.timestamp_ns(),
                )
                return

            self.generate_order_updated(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                command.venue_order_id,
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
            self._log.warning(f"Cancel failed: {e}")
            self.generate_order_cancel_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                command.venue_order_id,
                "client error",
                self._clock.timestamp_ns(),
            )
            return

        self._log.debug(f"{result=}")

        for report in result.instruction_reports or []:
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

    # -- ACCOUNT ----------------------------------------------------------------------------------

    async def check_account_currency(self) -> None:
        """
        Check account currency against `BetfairHttpClient`.
        """
        self._log.debug("Checking account currency")
        PyCondition.not_none(self.base_currency, "self.base_currency")
        details: AccountDetailsResponse = await self._client.get_account_details()
        currency_code = details.currency_code
        self._log.debug(f"Account {currency_code=}, {self.base_currency.code=}")
        assert currency_code == self.base_currency.code
        self._log.debug("Base currency matches client details")

    # -- DEBUGGING --------------------------------------------------------------------------------

    def client(self) -> BetfairHttpClient:
        return self._client

    # -- ORDER STREAM API -------------------------------------------------------------------------

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

    async def _handle_order_stream_update(self, order_change_message: OCM) -> None:  # noqa: C901
        for market in order_change_message.oc or []:
            if market.orc is not None:
                for selection in market.orc:
                    if selection.uo is not None:
                        for unmatched_order in selection.uo:
                            if not await self._check_order_update(unmatched_order=unmatched_order):
                                if not self.config.ignore_external_orders:
                                    venue_order_id = VenueOrderId(str(unmatched_order.id))
                                    self._log.warning(
                                        f"Failed to find ClientOrderId for {venue_order_id!r} "
                                        f"after {self.check_order_timeout_secs} seconds",
                                    )
                                    self._log.warning(
                                        f"Unknown order for this node: {unmatched_order}",
                                    )
                                return
                            if unmatched_order.status == "E":
                                self._handle_stream_executable_order_update(
                                    unmatched_order=unmatched_order,
                                )
                            elif unmatched_order.status == "EC":
                                self._handle_stream_execution_complete_order_update(
                                    unmatched_order=unmatched_order,
                                )
                            else:
                                self._log.warning(f"Unknown order state: {unmatched_order}")
                    if selection.full_image:
                        self.check_cache_against_order_image(order_change_message)

    def check_cache_against_order_image(self, order_change_message: OCM) -> None:
        for market in order_change_message.oc or []:
            for selection in market.orc or []:
                instrument_id = betfair_instrument_id(
                    market_id=market.id,
                    selection_id=selection.id,
                    selection_handicap=selection.hc,
                )
                orders = self._cache.orders(instrument_id=instrument_id)
                venue_orders = {o.venue_order_id: o for o in orders}
                for unmatched_order in selection.uo or []:
                    # We can match on venue_order_id here
                    order = venue_orders.get(VenueOrderId(str(unmatched_order.id)))
                    if order is None and not self.config.ignore_external_orders:
                        self._log.error(f"Unknown order not in cache: {unmatched_order=} ")
                matched_orders = [(OrderSide.SELL, lay) for lay in (selection.ml or [])] + [
                    (OrderSide.BUY, back) for back in (selection.mb or [])
                ]
                for side, matched_order in matched_orders:
                    # We don't get much information from Betfair here, try our best to match order
                    price = betfair_float_to_price(matched_order.price)
                    quantity = betfair_float_to_quantity(matched_order.size)
                    matched = False
                    for order in orders:
                        for event in order.events:
                            if isinstance(event, OrderFilled) and (
                                order.side == side
                                and order.price == price
                                and quantity <= order.quantity
                            ):
                                matched = True
                    if not matched and not self.config.ignore_external_orders:
                        self._log.error(f"Unknown fill: {instrument_id=}, {matched_order=}")

    async def _check_order_update(self, unmatched_order: UnmatchedOrder) -> bool:
        # We may get an order update from the socket before our submit_order response has
        # come back (with our bet_id).
        #
        # As a precaution, wait up to `check_order_timeout_seconds` for the bet_id to be added
        # to cache.
        venue_order_id = VenueOrderId(str(unmatched_order.id))
        client_order_id = await self._wait_for_order(venue_order_id, self.check_order_timeout_secs)
        if client_order_id is None:
            return False

        self._log.debug(f"Found {client_order_id!r} for {venue_order_id!r}")

        # Check order exists
        order = self._cache.order(client_order_id=client_order_id)
        if order is None:
            self._log.error(f"Cannot find order for {client_order_id!r}")
            return False

        # Check instrument exists
        instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {order.instrument_id}")
            return False

        return True

    async def _wait_for_order(
        self,
        venue_order_id: VenueOrderId,
        timeout_secs: float,
    ) -> ClientOrderId | None:
        try:
            timeout_ns = secs_to_nanos(timeout_secs)
            start = self._clock.timestamp_ns()
            now = start
            while (now - start) < timeout_ns:
                client_order_id = self._cache.client_order_id(venue_order_id)
                if client_order_id:
                    return client_order_id
                await asyncio.sleep(0.01)
                now = self._clock.timestamp_ns()
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'wait_for_order'")
        return None

    def _handle_stream_executable_order_update(self, unmatched_order: UnmatchedOrder) -> None:
        # Handle update containing 'E' (executable) order update
        venue_order_id = VenueOrderId(str(unmatched_order.id))
        client_order_id = self._cache.client_order_id(venue_order_id=venue_order_id)
        if client_order_id is None:
            self._log.error(
                f"Cannot handle update: ClientOrderId not found for {venue_order_id!r}",
            )
            return

        order = self._cache.order(client_order_id=client_order_id)
        order = self._cache.order(client_order_id=client_order_id)
        if order is None:
            self._log.error(
                f"Cannot handle update: order not found for {client_order_id!r}",
            )
            return

        instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot handle update: no instrument found for {order.instrument_id}",
            )
            return

        # Check for any portion executed
        if unmatched_order.sm and unmatched_order.sm > order.filled_qty:
            trade_id = order_to_trade_id(unmatched_order)
            if trade_id not in self._published_executions[client_order_id]:
                fill_qty = self._determine_fill_qty(unmatched_order, order)
                fill_price = self._determine_fill_price(unmatched_order, order)
                ts_event = self._get_matched_timestamp(unmatched_order)

                self.generate_order_filled(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    venue_position_id=None,  # Can be None
                    trade_id=trade_id,
                    order_side=OrderSideParser.to_nautilus(unmatched_order.side),
                    order_type=OrderType.LIMIT,
                    last_qty=fill_qty,
                    last_px=betfair_float_to_price(fill_price),
                    quote_currency=instrument.quote_currency,
                    commission=Money(0, self.base_currency),
                    liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                    ts_event=ts_event,
                )
                self._published_executions[client_order_id].append(trade_id)

    def _handle_stream_execution_complete_order_update(  # noqa: C901 (too complex)
        self,
        unmatched_order: UnmatchedOrder,
    ) -> None:
        """
        Handle 'EC' (execution complete) order updates.
        """
        venue_order_id = VenueOrderId(str(unmatched_order.id))
        client_order_id = self._cache.client_order_id(venue_order_id=venue_order_id)
        if client_order_id is None:
            self._log.error(
                f"Cannot handle update: ClientOrderId not found for {venue_order_id!r}",
            )
            return

        order = self._cache.order(client_order_id=client_order_id)
        if order is None:
            self._log.error(
                f"Cannot handle update: order not found for {client_order_id!r}",
            )
            return

        instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot handle update: no instrument found for {order.instrument_id}",
            )
            return

        # Check for fill
        if unmatched_order.sm and unmatched_order.sm > order.filled_qty:
            trade_id = order_to_trade_id(unmatched_order)
            if trade_id not in self._published_executions[client_order_id]:
                fill_qty = self._determine_fill_qty(unmatched_order, order)
                fill_price = self._determine_fill_price(unmatched_order, order)
                ts_event = self._get_matched_timestamp(unmatched_order)

                # At least some part of this order has been filled
                self.generate_order_filled(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    venue_position_id=None,  # Can be None
                    trade_id=trade_id,
                    order_side=OrderSideParser.to_nautilus(unmatched_order.side),
                    order_type=OrderType.LIMIT,
                    last_qty=fill_qty,
                    last_px=betfair_float_to_price(fill_price),
                    quote_currency=instrument.quote_currency,
                    # avg_px=order['avp'],
                    commission=Money(0, self.base_currency),
                    liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                    ts_event=ts_event,
                )
                self._published_executions[client_order_id].append(trade_id)

        # Check for cancel
        cancel_qty = self._get_cancel_quantity(unmatched_order)
        if cancel_qty > 0 and not order.is_closed:
            key = (client_order_id, venue_order_id)
            self._log.debug(
                f"cancel key: {key}, pending_update_order_client_ids: {self._pending_update_order_client_ids}",
            )
            # If this is the result of a ModifyOrder, we don't want to emit a cancel
            if key not in self._pending_update_order_client_ids:
                # The remainder of this order has been canceled
                canceled_ts = self._get_canceled_timestamp(unmatched_order)
                self.generate_order_canceled(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=canceled_ts,
                )
        # Check for lapse
        elif unmatched_order.lapse_status_reason_code is not None:
            # This order has lapsed. No lapsed size was found in the above check for cancel,
            # so we assume size lapsed None implies the entire order.
            self._log.info(
                f"{client_order_id!r}, {venue_order_id!r} lapsed on cancel: "
                f"lapse_status={unmatched_order.lapse_status_reason_code}, "
                f"size_lapsed={unmatched_order.sl}",
            )

            order = self._cache.order(order.client_order_id)
            if order is None:
                self._log.error("Cannot handle cancel: {order.client_order_id!r} not found")
                return

            # Check if order is still open before generating a cancel.
            # Note: A race condition exists where a closing event might still be en route
            # to the execution engine. Running with this for now to avoid the complexity
            # of another hot cache to deal with the lapsed bet sequencing.
            if order.is_open:
                canceled_ts = self._get_canceled_timestamp(unmatched_order)
                self.generate_order_canceled(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=canceled_ts,
                )

        # Market order will not be in self.published_executions
        # This execution is complete - no need to track this anymore
        self._published_executions.pop(client_order_id, None)

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
            # We don't have any specifics about the fill, assume it was filled at our price
            return unmatched_order.p
        if order.filled_qty == 0:
            # New fill, simply return average price
            return unmatched_order.avp
        else:
            new_price = betfair_float_to_price(unmatched_order.avp)
            prev_price = order.avg_px
            if prev_price == new_price:
                # Matched at same price
                return unmatched_order.avp
            else:
                avg_price = betfair_float_to_price(order.avg_px)
                prev_price = betfair_float_to_price(avg_price)
                prev_size = order.filled_qty
                new_price = betfair_float_to_price(unmatched_order.avp)
                new_size = unmatched_order.sm - prev_size
                total_size = prev_size + new_size

                # Check for division by zero
                if new_size == 0 or total_size == 0:
                    # In case there's no new size, return the previous price
                    self._log.warning(
                        f"Avoided division by zero: {prev_price=} {prev_size=} {new_price=} {new_size=}",
                    )
                    return prev_price

                price = (new_price - (prev_price * (prev_size / total_size))) / (
                    new_size / total_size
                )
                self._log.debug(
                    f"Calculating fill price: {prev_price=} {prev_size=} {new_price=} {new_size=} == {price=}",
                )
                return price

    def _determine_fill_qty(self, unmatched_order: UnmatchedOrder, order: Order) -> Quantity:
        prev_filled_qty = self._filled_qty_cache.get(order.client_order_id)
        fill_qty = betfair_float_to_quantity((unmatched_order.sm or 0) - (prev_filled_qty or 0))

        total_matched_qty = betfair_float_to_quantity(unmatched_order.sm)

        if total_matched_qty >= order.quantity:
            self._filled_qty_cache.pop(order.client_order_id, None)  # Done
        else:
            self._filled_qty_cache[order.client_order_id] = total_matched_qty

        return fill_qty

    def _get_matched_timestamp(self, unmatched_order: UnmatchedOrder) -> int:
        if unmatched_order.md is None:
            self._log.warning("Matched timestamp was `None` from Betfair")
            matched_ms = 0
        else:
            matched_ms = unmatched_order.md
        return millis_to_nanos(matched_ms)

    def _get_canceled_timestamp(self, unmatched_order: UnmatchedOrder) -> int:
        canceled_ms = unmatched_order.cd or unmatched_order.ld or unmatched_order.md
        return millis_to_nanos(canceled_ms) if canceled_ms else self._clock.timestamp_ns()

    def _get_cancel_quantity(self, unmatched_order: UnmatchedOrder) -> float:
        return (unmatched_order.sc or 0) + (unmatched_order.sl or 0) + (unmatched_order.sv or 0)
