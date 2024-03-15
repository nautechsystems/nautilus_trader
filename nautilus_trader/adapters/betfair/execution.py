# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from collections import defaultdict

import msgspec
import pandas as pd
from betfair_parser.exceptions import BetfairError
from betfair_parser.spec.accounts.type_definitions import AccountDetailsResponse
from betfair_parser.spec.betting.enums import ExecutionReportStatus
from betfair_parser.spec.betting.enums import InstructionReportStatus
from betfair_parser.spec.betting.enums import OrderProjection
from betfair_parser.spec.betting.orders import PlaceOrders
from betfair_parser.spec.betting.orders import ReplaceOrders
from betfair_parser.spec.betting.type_definitions import CurrentOrderSummary
from betfair_parser.spec.betting.type_definitions import PlaceExecutionReport
from betfair_parser.spec.common import BetId
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
from nautilus_trader.adapters.betfair.parsing.requests import order_update_to_replace_order_params
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import nanos_to_micros
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
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
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
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
    account_currency : Currency
        The account base currency for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BetfairInstrumentProvider
        The instrument provider.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BetfairHttpClient,
        account_currency: Currency,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BetfairInstrumentProvider,
        request_account_state_period: int = 300,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(BETFAIR_VENUE.value),
            venue=BETFAIR_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.BETTING,
            base_currency=account_currency,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._instrument_provider: BetfairInstrumentProvider = instrument_provider
        self._client: BetfairHttpClient = client
        self.request_account_state_period = request_account_state_period
        self.stream = BetfairOrderStreamClient(
            http_client=self._client,
            message_handler=self.handle_order_stream_update,
        )
        self.venue_order_id_to_client_order_id: dict[VenueOrderId, ClientOrderId] = {}
        self.pending_update_order_client_ids: set[tuple[ClientOrderId, VenueOrderId]] = set()
        self.published_executions: dict[ClientOrderId, list[TradeId]] = defaultdict(list)

        self._strategy_hashes: dict[str, str] = {}
        self._set_account_id(AccountId(f"{BETFAIR_VENUE}-001"))
        AccountFactory.register_calculated_account(BETFAIR_VENUE.value)

    @property
    def instrument_provider(self) -> BetfairInstrumentProvider:
        return self._instrument_provider

    # -- CONNECTION HANDLERS ----------------------------------------------------------------------

    async def _connect(self) -> None:
        self._log.info("Connecting to BetfairHttpClient...")
        await self._client.connect()
        self._log.info("BetfairHttpClient login successful.", LogColor.GREEN)

        # Start scheduled account state updates
        self.create_task(self.account_state_updates())

        # Connections and start-up checks
        aws = [
            self.stream.connect(),
            self.check_account_currency(),
            self.load_venue_id_mapping_from_cache(),
        ]
        await asyncio.gather(*aws)

    async def _disconnect(self) -> None:
        # Close socket
        self._log.info("Closing streaming socket...")
        await self.stream.disconnect()

        # Ensure client closed
        self._log.info("Closing BetfairHttpClient...")
        await self._client.disconnect()

    # -- ERROR HANDLING ---------------------------------------------------------------------------
    async def on_api_exception(self, error: BetfairError) -> None:
        if "INVALID_SESSION_INFORMATION" in error.args[0]:
            # Session is invalid, need to reconnect
            self._log.warning("Invalid session error, reconnecting..")
            await self._client.disconnect()
            await self._connect()
            self._log.info("Reconnected.")

    # -- ACCOUNT HANDLERS -------------------------------------------------------------------------

    async def account_state_updates(self) -> None:
        while True:
            self._log.debug("Requesting account state")
            account_state = await self.request_account_state()
            self._log.debug(f"Received account state: {account_state}")
            self._send_account_state(account_state)
            self._log.debug("Sent account state")
            await asyncio.sleep(self.request_account_state_period)

    async def request_account_state(self) -> AccountState:
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
        return account_state

    async def connection_account_state(self) -> None:
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
        self._log.debug(f"Received account state: {account_state}, sending")
        self._send_account_state(account_state)
        self._log.debug("Initial Account state completed")

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> OrderStatusReport | None:
        self._log.debug(f"Listing current orders for {venue_order_id=} {client_order_id=}")
        assert (
            venue_order_id is not None or client_order_id is not None
        ), "Require one of venue_order_id or client_order_id"
        if venue_order_id is not None:
            bet_id = BetId(venue_order_id.value)
            orders = await self._client.list_current_orders(bet_ids={bet_id})
        else:
            customer_order_ref = make_customer_order_ref(client_order_id)
            orders = await self._client.list_current_orders(
                customer_order_refs={customer_order_ref},
            )

        if not orders:
            self._log.warning(f"Could not find order for {venue_order_id=} {client_order_id=}")
            return None
        # We have a response, check list length and grab first entry
        assert (
            len(orders) == 1
        ), f"More than one order found for {venue_order_id=} {client_order_id=}"
        order: CurrentOrderSummary = orders[0]
        instrument = self._cache.instrument(instrument_id)
        venue_order_id = VenueOrderId(str(order.bet_id))

        report: OrderStatusReport = bet_to_order_status_report(
            order=order,
            account_id=self.account_id,
            instrument_id=instrument.id,
            venue_order_id=venue_order_id,
            client_order_id=self._cache.client_order_id(venue_order_id),
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._log.debug(f"Received {report}.")
        return report

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        ts_init = self._clock.timestamp_ns()
        current_orders: list[CurrentOrderSummary] = await self._client.list_current_orders(
            order_projection=OrderProjection.EXECUTABLE,
            date_range=TimeRange(from_=start, to=end),
        )

        order_status_reports = []
        for order in current_orders:
            instrument_id = betfair_instrument_id(
                market_id=order.market_id,
                selection_id=order.selection_id,
                selection_handicap=order.handicap,
            )
            venue_order_id = VenueOrderId(str(order.bet_id))
            client_order_id = self.venue_order_id_to_client_order_id.get(venue_order_id)
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
        instrument_id: InstrumentId | None = None,
        venue_order_id: VenueOrderId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[FillReport]:
        ts_init = self._clock.timestamp_ns()
        cleared_orders: list[CurrentOrderSummary] = await self._client.list_current_orders(
            order_projection=OrderProjection.ALL,
            date_range=TimeRange(from_=start, to=end),
        )

        fill_reports = []
        for order in cleared_orders:
            instrument_id = betfair_instrument_id(
                market_id=order.market_id,
                selection_id=order.selection_id,
                selection_handicap=order.handicap,
            )
            venue_order_id = VenueOrderId(str(order.bet_id))
            client_order_id = self.venue_order_id_to_client_order_id.get(venue_order_id)
            report = bet_to_fill_report(
                order=order,
                account_id=self.account_id,
                instrument_id=instrument_id,
                venue_order_id=venue_order_id,
                client_order_id=client_order_id,
                ts_init=ts_init,
                report_id=UUID4(),
            )
            if report is not None:
                fill_reports.append(report)

        return fill_reports

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[PositionStatusReport]:
        self._log.warning("Cannot generate `PositionStatusReports`: not yet implemented.")

        return []

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        self._log.debug(f"Received submit_order {command}")

        self.generate_order_submitted(
            command.strategy_id,
            command.instrument_id,
            command.order.client_order_id,
            self._clock.timestamp_ns(),
        )
        self._log.debug("Generated _generate_order_submitted")

        instrument = self._cache.instrument(command.instrument_id)
        PyCondition.not_none(instrument, "instrument")
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

        self._log.debug(f"result={result}")
        for report in result.instruction_reports:
            if result.status == ExecutionReportStatus.FAILURE:
                reason = f"{result.error_code.name} ({result.error_code.__doc__})"
                self._log.warning(f"Submit failed - {reason}")
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
                self.set_venue_id_mapping(venue_order_id, client_order_id)
                self.generate_order_accepted(
                    command.strategy_id,
                    command.instrument_id,
                    client_order_id,
                    venue_order_id,
                    self._clock.timestamp_ns(),
                )
                self._log.debug("Generated order accepted")

    async def _modify_order(self, command: ModifyOrder) -> None:
        self._log.debug(f"Received modify_order {command}")
        client_order_id: ClientOrderId = command.client_order_id
        instrument = self._cache.instrument(command.instrument_id)
        PyCondition.not_none(instrument, "instrument")
        existing_order = self._cache.order(client_order_id)  # type: Order

        if existing_order is None:
            self._log.warning(
                f"Attempting to update order that does not exist in the cache: {command}",
            )
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                client_order_id,
                command.venue_order_id,
                "ORDER NOT IN CACHE",
                self._clock.timestamp_ns(),
            )
            return
        if existing_order.venue_order_id is None:
            self._log.warning(f"Order found does not have `id` set: {existing_order}")
            PyCondition.not_none(command.strategy_id, "command.strategy_id")
            PyCondition.not_none(command.instrument_id, "command.instrument_id")
            PyCondition.not_none(client_order_id, "client_order_id")
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                client_order_id,
                None,
                "ORDER MISSING VENUE_ORDER_ID",
                self._clock.timestamp_ns(),
            )
            return

        # Send order to client
        replace_orders: ReplaceOrders = order_update_to_replace_order_params(
            command=command,
            venue_order_id=existing_order.venue_order_id,
            instrument=instrument,
        )
        self.pending_update_order_client_ids.add(
            (command.client_order_id, existing_order.venue_order_id),
        )
        try:
            result = await self._client.replace_orders(replace_orders)
        except Exception as e:
            if isinstance(e, BetfairError):
                await self.on_api_exception(error=e)
            self._log.warning(f"Modify failed: {e}")
            self.generate_order_modify_rejected(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                existing_order.venue_order_id,
                "client error",
                self._clock.timestamp_ns(),
            )
            return

        self._log.debug(f"result={result}")

        for report in result.instruction_reports:
            if report.status == ExecutionReportStatus.FAILURE:
                reason = f"{result.error_code.name} ({result.error_code.__doc__})"
                self._log.warning(f"replace failed - {reason}")
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
            self.set_venue_id_mapping(venue_order_id, client_order_id)
            self.generate_order_updated(
                command.strategy_id,
                command.instrument_id,
                client_order_id,
                venue_order_id,
                betfair_float_to_quantity(place_instruction.instruction.limit_order.size),
                betfair_float_to_price(place_instruction.instruction.limit_order.price),
                None,  # Not applicable for Betfair
                self._clock.timestamp_ns(),
                True,
            )

    async def _cancel_order(self, command: CancelOrder) -> None:
        self._log.debug(f"Received cancel order: {command}")
        instrument = self._cache.instrument(command.instrument_id)
        PyCondition.not_none(instrument, "instrument")

        # Format
        cancel_orders = order_cancel_to_cancel_order_params(
            command=command,
            instrument=instrument,
        )
        self._log.debug(f"cancel_order {cancel_orders}")

        # Send to client
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
        self._log.debug(f"result={result}")

        # Parse response
        for report in result.instruction_reports:
            venue_order_id = VenueOrderId(str(report.instruction.bet_id))
            if report.status == InstructionReportStatus.FAILURE:
                reason = f"{report.error_code.name}: {report.error_code.__doc__}"
                self._log.warning(f"cancel failed - {reason}")
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
            self.set_venue_id_mapping(venue_order_id, command.client_order_id)
            self.generate_order_canceled(
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                venue_order_id,
                self._clock.timestamp_ns(),
            )
            self._log.debug("Sent order cancel")

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

    # -- CACHE  -----------------------------------------------------------------------------------

    async def load_venue_id_mapping_from_cache(self) -> None:
        self._log.info("Loading venue_id mapping from cache")
        raw = self._cache.get("betfair_execution_client.venue_order_id_to_client_order_id") or b"{}"
        self._log.info(f"venue_id_mapping: {raw.decode()=}")
        self.venue_order_id_to_client_order_id = msgspec.json.decode(raw)

    def set_venue_id_mapping(
        self,
        venue_order_id: VenueOrderId,
        client_order_id: ClientOrderId,
    ) -> None:
        self._log.debug(f"Updating venue_id_mapping: {venue_order_id=} {client_order_id=}")
        self.venue_order_id_to_client_order_id[venue_order_id] = client_order_id
        self._log.debug("Updating venue_id_mapping in cache")
        raw = msgspec.json.encode(
            {k.value: v.value for k, v in self.venue_order_id_to_client_order_id.items()},
        )
        self._cache.add("betfair_execution_client.venue_order_id_to_client_order_id", raw)

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
            pass
        elif isinstance(update, Status):
            self._handle_status_message(update=update)
        else:
            raise RuntimeError

    async def _handle_order_stream_update(self, order_change_message: OCM) -> None:
        for market in order_change_message.oc or []:
            if market.orc is not None:
                for selection in market.orc:
                    if selection.uo is not None:
                        for unmatched_order in selection.uo:
                            await self._check_order_update(unmatched_order=unmatched_order)
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
                        continue

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
                    if order is not None:
                        continue  # Order exists
                    self._log.error(f"UNKNOWN ORDER NOT IN CACHE: {unmatched_order=} ")
                    raise RuntimeError(f"UNKNOWN ORDER NOT IN CACHE: {unmatched_order=}")
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
                    if not matched:
                        self._log.error(f"UNKNOWN FILL: {instrument_id=} {matched_order}")
                        raise RuntimeError(f"UNKNOWN FILL: {instrument_id=} {matched_order}")

    async def _check_order_update(self, unmatched_order: UnmatchedOrder) -> None:
        """
        Ensure we have a client_order_id, instrument and order for this venue order
        update.
        """
        venue_order_id = VenueOrderId(str(unmatched_order.id))
        client_order_id = await self.wait_for_order(
            venue_order_id=venue_order_id,
            timeout_seconds=10.0,
        )
        if client_order_id is None:
            self._log.warning(f"Can't find client_order_id for {unmatched_order}")
            return
        PyCondition.type(client_order_id, ClientOrderId, "client_order_id")
        order = self._cache.order(client_order_id)
        PyCondition.not_none(order, "order")
        instrument = self._cache.instrument(order.instrument_id)
        PyCondition.not_none(instrument, "instrument")

    def _handle_stream_executable_order_update(self, unmatched_order: UnmatchedOrder) -> None:
        """
        Handle update containing 'E' (executable) order update.
        """
        venue_order_id = VenueOrderId(str(unmatched_order.id))

        # Check if this is the first time seeing this order (backtest or replay)
        if venue_order_id in self.venue_order_id_to_client_order_id:
            # We've already sent an accept for this order in self._submit_order
            self._log.info(f"Skipping order_accept as order exists: {venue_order_id=}")

        client_order_id = self.venue_order_id_to_client_order_id[venue_order_id]
        order = self._cache.order(client_order_id)
        instrument = self._cache.instrument(order.instrument_id)

        # Check for any portion executed
        if unmatched_order.sm > 0 and unmatched_order.sm > order.filled_qty:
            trade_id = order_to_trade_id(unmatched_order)
            if trade_id not in self.published_executions[client_order_id]:
                fill_qty = unmatched_order.sm - order.filled_qty
                fill_price = self._determine_fill_price(
                    unmatched_order=unmatched_order,
                    order=order,
                )
                self.generate_order_filled(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    venue_position_id=None,  # Can be None
                    trade_id=trade_id,
                    order_side=OrderSideParser.to_nautilus(unmatched_order.side),
                    order_type=OrderType.LIMIT,
                    last_qty=betfair_float_to_quantity(fill_qty),
                    last_px=betfair_float_to_price(fill_price),
                    quote_currency=instrument.quote_currency,
                    commission=Money(0, self.base_currency),
                    liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                    ts_event=millis_to_nanos(unmatched_order.md),
                )
                self.published_executions[client_order_id].append(trade_id)

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
                price = (new_price - (prev_price * (prev_size / total_size))) / (
                    new_size / total_size
                )
                self._log.debug(
                    f"Calculating fill price {prev_price=} {prev_size=} {new_price=} {new_size=} == {price=}",
                )
                return price

    def _handle_stream_execution_complete_order_update(
        self,
        unmatched_order: UnmatchedOrder,
    ) -> None:
        """
        Handle 'EC' (execution complete) order updates.
        """
        venue_order_id = VenueOrderId(str(unmatched_order.id))
        client_order_id = self._cache.client_order_id(venue_order_id=venue_order_id)
        PyCondition.not_none(client_order_id, "client_order_id")
        order = self._cache.order(client_order_id=client_order_id)
        instrument = self._cache.instrument(order.instrument_id)
        assert instrument

        if unmatched_order.sm > 0 and unmatched_order.sm > order.filled_qty:
            trade_id = order_to_trade_id(unmatched_order)
            if trade_id not in self.published_executions[client_order_id]:
                fill_qty = unmatched_order.sm - order.filled_qty
                fill_price = self._determine_fill_price(
                    unmatched_order=unmatched_order,
                    order=order,
                )
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
                    last_qty=betfair_float_to_quantity(fill_qty),
                    last_px=betfair_float_to_price(fill_price),
                    quote_currency=instrument.quote_currency,
                    # avg_px=order['avp'],
                    commission=Money(0, self.base_currency),
                    liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                    ts_event=millis_to_nanos(unmatched_order.md),
                )
                self.published_executions[client_order_id].append(trade_id)

        cancel_qty = unmatched_order.sc + unmatched_order.sl + unmatched_order.sv
        if cancel_qty > 0 and not order.is_closed:
            assert (
                unmatched_order.sm + cancel_qty == unmatched_order.s
            ), f"Size matched + canceled != total: {unmatched_order}"
            # If this is the result of a ModifyOrder, we don't want to emit a cancel

            key = (client_order_id, venue_order_id)
            self._log.debug(
                f"cancel key: {key}, pending_update_order_client_ids: {self.pending_update_order_client_ids}",
            )
            if key not in self.pending_update_order_client_ids:
                # The remainder of this order has been canceled
                cancelled_ts = unmatched_order.cd or unmatched_order.ld or unmatched_order.md
                cancelled_ts = (
                    millis_to_nanos(cancelled_ts)
                    if cancelled_ts is not None
                    else self._clock.timestamp_ns()
                )
                self.generate_order_canceled(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=cancelled_ts,
                )
                if venue_order_id in self.venue_order_id_to_client_order_id:
                    del self.venue_order_id_to_client_order_id[venue_order_id]
        # Market order will not be in self.published_executions
        if client_order_id in self.published_executions:
            # This execution is complete - no need to track this anymore
            del self.published_executions[client_order_id]

    async def wait_for_order(
        self,
        venue_order_id: VenueOrderId,
        timeout_seconds: float = 10.0,
    ) -> ClientOrderId | None:
        """
        We may get an order update from the socket before our submit_order response has
        come back (with our bet_id).

        As a precaution, wait up to `timeout_seconds` for the bet_id to be added
        to `self.order_id_to_client_order_id`.

        """
        assert isinstance(venue_order_id, VenueOrderId)
        start = self._clock.timestamp_ns()
        now = start
        while (now - start) < secs_to_nanos(timeout_seconds):
            # self._log.debug(
            #     f"checking venue_order_id={venue_order_id} in {self.venue_order_id_to_client_order_id}"
            # )
            if venue_order_id in self.venue_order_id_to_client_order_id:
                client_order_id = self.venue_order_id_to_client_order_id[venue_order_id]
                self._log.debug(
                    f"Found order in {nanos_to_micros(now - start)}us: {client_order_id}",
                )
                return client_order_id
            now = self._clock.timestamp_ns()
            await asyncio.sleep(0.1)
        self._log.warning(
            f"Failed to find venue_order_id: {venue_order_id} "
            f"after {timeout_seconds} seconds"
            f"\nexisting: {self.venue_order_id_to_client_order_id})",
        )
        return None

    def _handle_status_message(self, update: Status) -> None:
        if update.is_error and update.connection_closed:
            self._log.warning(str(update))
            if update.error_code == StatusErrorCode.MAX_CONNECTION_LIMIT_EXCEEDED:
                raise RuntimeError("No more connections available")
            else:
                self._log.info("Attempting reconnect")
                self._loop.create_task(self.stream.connect())
