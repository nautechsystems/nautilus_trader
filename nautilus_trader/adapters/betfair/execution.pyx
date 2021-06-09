# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import datetime
from functools import partial
from typing import Dict, List, Optional, Set

import betfairlightweight
import orjson

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport VenueOrderId

from nautilus_trader.adapters.betfair.common import B2N_ORDER_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.parsing import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing import generate_order_status_report
from nautilus_trader.adapters.betfair.parsing import generate_trades_list
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_update_to_betfair
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import nanos_to_secs
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order


cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class BetfairExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Betfair.
    """

    def __init__(
        self,
        client not None,
        AccountId account_id not None,
        Currency base_currency not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
        dict market_filter not None,
        bint load_instruments=True,
    ):
        """
        Initialize a new instance of the ``BetfairExecutionClient`` class.

        Parameters
        ----------
        client : betfairlightweight.APIClient
            The Betfair client.
        account_id : AccountId
            The account identifier for the client.
        base_currency : Currency
            The account base currency for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        self._client = client  # type: betfairlightweight.APIClient
        self._client.login()

        cdef BetfairInstrumentProvider instrument_provider = BetfairInstrumentProvider(
            client=client,
            logger=logger,
            load_all=load_instruments,
            market_filter=market_filter
        )

        super().__init__(
            client_id=ClientId(BETFAIR_VENUE.value),
            venue_type=VenueType.EXCHANGE,
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=base_currency,
            engine=engine,
            instrument_provider=instrument_provider,
            clock=clock,
            logger=logger,
            config={
                "name": "BetfairExecClient",
                "calculate_account_state": True,
            }
        )

        self.venue = BETFAIR_VENUE
        self._stream = BetfairOrderStreamClient(
            client=self._client,
            logger=logger,
            message_handler=self.handle_order_stream_update,
        )
        self.is_connected = False
        self.venue_order_id_to_client_order_id = {}  # type: Dict[str, ClientOrderId]
        self.pending_update_order_client_ids = set()  # type: Set[(ClientOrderId, VenueOrderId)]
        self.published_executions = defaultdict(list)  # type: Dict[ClientOrderId, ExecutionId]
        self._account_currency = None

    cpdef void connect(self) except *:
        self._loop.create_task(self._connect())

    async def _connect(self):
        self._log.info("Connecting to Betfair APIClient...")
        self._log.info("Betfair APIClient login successful.", LogColor.GREEN)

        aws = [
            self._stream.connect(),
            self.connection_account_state(),
        ]
        await asyncio.gather(*aws)

        self.is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """ Disconnect the client """
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        # Close socket
        self._log.info("Closing streaming socket...")
        await self._stream.disconnect()

        # Ensure client closed
        self._log.info("Closing APIClient...")
        self._client.client_logout()

        self.is_connected = False
        self._log.info("Disconnected.")

# -- ACCOUNT HANDLERS ------------------------------------------------------------------------------

    async def connection_account_state(self):
        aws = [
            self._loop.run_in_executor(None, self._get_account_details),
            self._loop.run_in_executor(None, self._get_account_funds),
        ]
        result = await asyncio.gather(*aws)
        account_details, account_funds = result
        timestamp_ns = self._clock.timestamp_ns()
        account_state = betfair_account_to_account_state(
            account_detail=account_details,
            account_funds=account_funds,
            event_id=self._uuid_factory.generate(),
            ts_updated_ns=timestamp_ns,
            timestamp_ns=timestamp_ns,
        )
        self._handle_event(account_state)

    cpdef dict _get_account_details(self):
        self._log.debug("Sending get_account_details request")
        return self._client.account.get_account_details()

    cpdef dict _get_account_funds(self):
        self._log.debug("Sending get_account_funds request")
        return self._client.account.get_account_funds()

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    # TODO - #  Do want to throttle updates into a bulk update if they're coming faster than x / sec? Maybe this is for risk engine?
    #  We could use some heuristics about the avg network latency and add an optional flag for throttle inserts etc.

    cpdef void submit_order(self, SubmitOrder command) except *:
        self._log.debug(f"Received {command}")

        self.generate_order_submitted(
            client_order_id=command.order.client_order_id,
            ts_submitted_ns=self._clock.timestamp_ns(),
        )
        self._log.debug(f"Generated _generate_order_submitted")

        f = self._loop.run_in_executor(None, self._submit_order, command)  # type: asyncio.Future
        self._log.debug(f"future: {f}")
        f.add_done_callback(partial(self._post_submit_order, client_order_id=command.order.client_order_id))

    def _submit_order(self, SubmitOrder command):
        instrument = self._instrument_provider.find(command.instrument_id)
        kw = order_submit_to_betfair(command=command, instrument=instrument)
        self._log.debug(f"{kw}")
        return self._client.betting.place_orders(**kw)

    def _post_submit_order(self, f: asyncio.Future, client_order_id):
        self._log.debug(f"inside _post_submit_order for {client_order_id}")
        try:
            resp = f.result()
            self._log.debug(f"resp: {resp}")
        except Exception as e:
            self._log.warning(str(e))
            return
        assert len(resp['instructionReports']) == 1, "Should only be a single order"

        if resp["status"] == "FAILURE":
            reason = f"{resp['errorCode']}: {resp['instructionReports'][0]['errorCode']}"
            self._log.warning(f"Submit failed - {reason}")
            self.generate_order_rejected(
                client_order_id=client_order_id,
                reason=reason,
                ts_rejected_ns=self._clock.timestamp_ns(),
            )
            return
        bet_id = resp['instructionReports'][0]['betId']
        self._log.debug(f"Matching venue_order_id: {bet_id} to client_order_id: {client_order_id}")
        self.venue_order_id_to_client_order_id[bet_id] = client_order_id
        self.generate_order_accepted(
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId(bet_id),
            ts_accepted_ns=self._clock.timestamp_ns(),
        )

    cpdef void update_order(self, UpdateOrder command) except *:
        self._log.debug(f"Received {command}")
        self.generate_order_pending_replace(
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            ts_pending_ns=self._clock.timestamp_ns(),
        )
        f = self._loop.run_in_executor(None, self._update_order, command)  # type: asyncio.Future
        self._log.debug(f"future: {f}")
        f.add_done_callback(partial(self._post_update_order, client_order_id=command.client_order_id))

    def _update_order(self, UpdateOrder command):
        existing_order = self._engine.cache.order(command.client_order_id)  # type: Order
        if existing_order is None:
            self._log.warning(f"Attempting to update order that does not exist in the cache: {command}")
            return
        if existing_order.venue_order_id == VenueOrderId("NULL"):
            self._log.warning(f"Order found does not have `id` set: {existing_order}")
            return
        self._log.debug(f"existing_order: {existing_order}")
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_update_to_betfair(
            command=command,
            venue_order_id=existing_order.venue_order_id,
            side=existing_order.side,
            instrument=instrument
        )
        self._log.debug(f"update kw: {kw}")
        self.pending_update_order_client_ids.add((command.client_order_id, existing_order.venue_order_id))
        return self._client.betting.replace_orders(**kw)

    def _post_update_order(self, f: asyncio.Future, client_order_id: ClientOrderId):
        Condition.type(client_order_id, ClientOrderId, "client_order_id")

        self._log.debug(f"inside _post_update_order for {client_order_id}")
        try:
            resp = f.result()
            self._log.debug(f"resp: {resp}")
        except Exception as e:
            self._log.warning(str(e))
            return

        assert len(resp['instructionReports']) == 1, "Should only be a single order"
        if resp["status"] == "FAILURE":
            reason = f"{resp['errorCode']}: {resp['instructionReports'][0]['errorCode']}"
            self._log.warning(f"Submit failed - {reason}")
            self.generate_order_rejected(
                client_order_id=client_order_id,
                reason=reason,
                ts_rejected_ns=self._clock.timestamp_ns(),
            )
            return
        # Check the venue_order_id that has been deleted currently exists on our order
        existing_order = self._engine.cache.order(client_order_id)  # type: Order
        deleted_bet_id = resp["instructionReports"][0]["cancelInstructionReport"]["instruction"]["betId"]
        self._log.debug(f"{existing_order}, {deleted_bet_id}")
        assert existing_order.venue_order_id == VenueOrderId(deleted_bet_id)

        instructions = resp["instructionReports"][0]["placeInstructionReport"]
        self.venue_order_id_to_client_order_id[instructions["betId"]] = client_order_id
        self.generate_order_updated(
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId(instructions["betId"]),
            quantity=Quantity(instructions["instruction"]['limitOrder']["size"], precision=4),
            price=price_to_probability(instructions["instruction"]['limitOrder']["price"]),
            ts_updated_ns=self._clock.timestamp_ns(),
            venue_order_id_modified=True,
        )

    cpdef void cancel_order(self, CancelOrder command) except *:
        self._log.debug("Received cancel order")
        self.generate_order_pending_cancel(
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            ts_pending_ns=self._clock.timestamp_ns(),
        )
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_cancel_to_betfair(command=command, instrument=instrument)
        resp = self._client.betting.cancel_orders(**kw)
        self._log.debug(f"cancel: {resp}")

    # cpdef void bulk_submit_order(self, list commands):
    # betfair allows up to 200 inserts per request
    #     raise NotImplementedError

    # cpdef void bulk_submit_update(self, list commands):
    # betfair allows up to 60 updates per request
    #     raise NotImplementedError

    # cpdef void bulk_submit_delete(self, list commands):
    # betfair allows up to 60 cancels per request
    #     raise NotImplementedError

# -- ACCOUNT ---------------------------------------------------------------------------------------

    cpdef Currency get_account_currency(self):
        if not self._account_currency:
            self._account_currency = Currency.from_str(self._instrument_provider.get_account_currency())
        return self._account_currency

# -- DEBUGGING -------------------------------------------------------------------------------------

    cpdef object client(self):
        return self._client

    cpdef BetfairInstrumentProvider instrument_provider(self):
        return self._instrument_provider

    cpdef LiveExecutionEngine engine(self):
        return self._engine

# -- ORDER STREAM API ------------------------------------------------------------------------------

    cpdef void handle_order_stream_update(self, bytes raw) except *:
        """ Handle an update from the order stream socket """
        cdef dict update = orjson.loads(raw)  # type: dict
        self._loop.create_task(self._handle_order_stream_update(update=update))

    async def _handle_order_stream_update(self, update):
        for market in update.get("oc", []):
            market_id = market["id"]
            for selection in market.get("orc", []):
                instrument = self._instrument_provider.get_betting_instrument(
                    market_id=market_id,
                    selection_id=str(selection["id"]),
                    handicap=str(selection.get("hc", "0.0")),
                )
                for order in selection.get("uo", []):
                    self._log.debug(f"order_update: {order}")
                    client_order_id = await self.wait_for_order(order['id'], timeout_seconds=10.0)
                    if client_order_id is None:
                        continue
                    venue_order_id = VenueOrderId(order["id"])

                    # "E" = Executable (live / working)
                    if order["status"] == "E":
                        # Check if this is the first time seeing this order (backtest or replay)
                        if venue_order_id.value in self.venue_order_id_to_client_order_id:
                            # We've already sent an accept for this order in self._post_submit_order
                            self._log.debug(f"Skipping order_accept as order exists: {venue_order_id}")
                        else:
                            self.generate_order_accepted(
                                client_order_id=client_order_id,
                                venue_order_id=venue_order_id,
                                ts_accepted_ns=millis_to_nanos(order["pd"]),
                            )

                        # Check for any portion executed
                        if order["sm"] != 0:
                            execution_id = ExecutionId(str(order["md"]))  # Use matched date as execution id
                            if execution_id not in self.published_executions[client_order_id]:
                                self.generate_order_filled(
                                    client_order_id=client_order_id,
                                    venue_order_id=venue_order_id,
                                    execution_id=execution_id,
                                    position_id=None,  # Assigned in engine
                                    instrument_id=instrument.id,
                                    order_side=B2N_ORDER_STREAM_SIDE[order["side"]],
                                    last_qty=Quantity(order["sm"], instrument.size_precision),
                                    last_px=price_to_probability(order["p"]),
                                    # avg_px=Decimal(order['avp']),
                                    quote_currency=instrument.quote_currency,
                                    commission=Money(0, self.get_account_currency()),
                                    liquidity_side=LiquiditySide.NONE,
                                    ts_filled_ns=millis_to_nanos(order["md"]),
                                )
                                self.published_executions[client_order_id].append(execution_id)

                    # Execution complete, this order is fulled match or canceled
                    elif order["status"] == "EC":
                        if order["sm"] != 0:
                            execution_id = ExecutionId(str(order["md"]))  # Use matched date as execution id
                            if execution_id not in self.published_executions[client_order_id]:
                                # At least some part of this order has been filled
                                self.generate_order_filled(
                                    client_order_id=client_order_id,
                                    venue_order_id=venue_order_id,
                                    execution_id=execution_id,
                                    position_id=None,  # Assigned in engine
                                    instrument_id=instrument.id,
                                    order_side=B2N_ORDER_STREAM_SIDE[order["side"]],
                                    last_qty=Quantity(order["sm"], instrument.size_precision),
                                    last_px=price_to_probability(order['p']),
                                    quote_currency=instrument.quote_currency,
                                    # avg_px=order['avp'],
                                    commission=Money(0, self.get_account_currency()),
                                    liquidity_side=LiquiditySide.TAKER,  # TODO - Fix this?
                                    ts_filled_ns=millis_to_nanos(order['md']),
                                )
                        if any([order[x] != 0 for x in ("sc", "sl", "sv")]):
                            cancel_qty = sum([order[k] for k in ("sc", "sl", "sv")])
                            assert order['sm'] + cancel_qty == order["s"], f"Size matched + canceled != total: {order}"
                            # If this is the result of a UpdateOrder, we don't want to emit a cancel
                            key = (ClientOrderId(order.get("rfo")), VenueOrderId(order["id"]))
                            self._log.debug(f"cancel key: {key}, pending_update_order_client_ids: {self.pending_update_order_client_ids}")
                            if key not in self.pending_update_order_client_ids:
                                # The remainder of this order has been canceled
                                self.generate_order_canceled(
                                    client_order_id=client_order_id,
                                    venue_order_id=venue_order_id,
                                    ts_canceled_ns=millis_to_nanos(order.get("cd") or order.get("ld") or order.get('md')),
                                )
                        # Market order will not be in self.published_executions
                        if client_order_id in self.published_executions:
                            # This execution is complete - no need to track this anymore
                            del self.published_executions[client_order_id]

                    else:
                        self._log.warning("Unknown order state: {order}")
                        # raise KeyError("Unknown order type", order, None)

                # these values?
                # for trade in selection.get("mb", []):
                #     # TODO - we can get a matched back without full details.
                #     #  Need to match ourselves??
                #     pass
                # for trade in selection.get("ml", []):
                #     pass

                # TODO - Should be no difference for fullImage at this stage.
                #  We just send all updates individually.
                if selection.get("fullImage", False):
                    pass

    async def wait_for_order(self, venue_order_id, timeout_seconds=10.0):
        """
        We may get an order update from the socket before our submit_order
        response has come back (with our betId).

        As a precaution, wait up to `timeout_seconds` for the betId to be added
        to `self.order_id_to_client_order_id`.
        """
        start = self._clock.timestamp_ns()
        now = start
        while (now - start) < secs_to_nanos(timeout_seconds):
            if venue_order_id in self.venue_order_id_to_client_order_id:
                self._log.debug(f"Found order in {nanos_to_secs(now - start)} sec ")
                return self.venue_order_id_to_client_order_id[venue_order_id]
            now = self._clock.timestamp_ns()
            await asyncio.sleep(0)
        self._log.warning(f"Failed to find venue_order_id: {venue_order_id} "
                          f"after {timeout_seconds} seconds"
                          f"\nexisting: {self.venue_order_id_to_client_order_id})")

# -- RECONCILIATION -------------------------------------------------------------------------------

    async def generate_order_status_report(self, order: Order) -> Optional[OrderStatusReport]:
        self._log.debug(f"generate_order_status_report: {order}")
        return await generate_order_status_report(self, order)

    async def generate_exec_reports(
            self,
            venue_order_id: VenueOrderId,
            symbol: Symbol,
            since: Optional[datetime]=None
    ) -> List[ExecutionReport]:
        self._log.debug(f"generate_exec_reports: {venue_order_id}, {symbol}, {since}")
        return await generate_trades_list(self, venue_order_id, symbol, since)

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._engine.process(event)
