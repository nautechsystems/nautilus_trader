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
import hashlib
from collections import defaultdict
from datetime import datetime
from typing import Dict, List, Optional, Set

import orjson

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport millis_to_nanos
from nautilus_trader.core.datetime cimport nanos_to_secs
from nautilus_trader.core.datetime cimport secs_to_nanos
from nautilus_trader.execution.messages cimport ExecutionReport
from nautilus_trader.execution.messages cimport OrderStatusReport
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.venue_type cimport VenueType
from nautilus_trader.model.commands.trading cimport CancelOrder
from nautilus_trader.model.commands.trading cimport SubmitOrder
from nautilus_trader.model.commands.trading cimport UpdateOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.msgbus.message_bus cimport MessageBus

from nautilus_trader.adapters.betfair.client import BetfairClient
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

from nautilus_trader.model.instruments.betting cimport BettingInstrument


cdef class BetfairExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Betfair.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        client not None: BetfairClient,
        AccountId account_id not None,
        Currency base_currency not None,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict market_filter not None,
        bint load_instruments=True,
    ):
        """
        Initialize a new instance of the ``BetfairExecutionClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        client : BetfairClient
            The Betfair HTTPClient.
        account_id : AccountId
            The account ID for the client.
        base_currency : Currency
            The account base currency for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        self._client = client  # type: BetfairClient

        cdef BetfairInstrumentProvider instrument_provider = BetfairInstrumentProvider(
            client=client,
            logger=logger,
            market_filter=market_filter
        )

        super().__init__(
            loop=loop,
            client_id=ClientId(BETFAIR_VENUE.value),
            venue_type=VenueType.EXCHANGE,
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=base_currency,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
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

    cpdef void _start(self) except *:
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    async def _connect(self):
        self._log.info("Connecting to BetfairClient...")
        await self._client.connect()
        self._log.info("BetfairClient login successful.", LogColor.GREEN)

        aws = [
            self._stream.connect(),
            self.connection_account_state(),
            self.check_account_currency(),
        ]
        await asyncio.gather(*aws)

        self.is_connected = True
        self._log.info("Connected.")

    cpdef void _stop(self) except *:
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        # Close socket
        self._log.info("Closing streaming socket...")
        await self._stream.disconnect()

        # Ensure client closed
        self._log.info("Closing APIClient...")
        self._client.disconnect()

        self.is_connected = False
        self._log.info("Disconnected.")

# -- ACCOUNT HANDLERS ------------------------------------------------------------------------------

    async def connection_account_state(self):
        account_details = await self._client.get_account_details()
        account_funds = await self._client.get_account_funds()
        timestamp = self._clock.timestamp_ns()
        account_state = betfair_account_to_account_state(
            account_detail=account_details,
            account_funds=account_funds,
            event_id=self._uuid_factory.generate(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._log.debug(f"Received account state: {account_state}, applying")
        self.apply_account_state(account_state)

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    # TODO (bm) - Do want to throttle updates into a bulk update if they're coming faster than x / sec? Maybe this is
    #  for risk engine? We could use some heuristics about the avg network latency an_check_order_updated add an optional flag for
    #  throttle inserts etc. We actually typically know when the match is happening - so we could do smart buffering

    cpdef void submit_order(self, SubmitOrder command) except *:
        Condition.not_none(command, "command")

        self.create_task(self._submit_order(command))

    async def _submit_order(self, SubmitOrder command):
        self._log.debug(f"Received submit_order {command}")

        self.generate_order_submitted(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
            client_order_id=command.order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )
        self._log.debug(f"Generated _generate_order_submitted")

        instrument = self._cache.instrument(command.instrument_id)
        Condition.not_none(instrument, "instrument")
        client_order_id = command.order.client_order_id

        place_order = order_submit_to_betfair(command=command, instrument=instrument)
        try:
            resp = await self._client.place_orders(**place_order)
        except Exception as e:
            self._log.error(f"submit_order strategy_id={command.strategy_id}, "
                            f"client_order_id={client_order_id} exception={e}")
            return

        assert len(resp['instructionReports']) == 1, f"Should only be a single order, got: {resp['instructionReports']}"

        if resp["status"] == "FAILURE":
            reason = f"{resp['errorCode']}: {resp['instructionReports'][0]['errorCode']}"
            self._log.warning(f"Submit failed - {reason}")
            self.generate_order_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=client_order_id,
                reason=reason,  # type: ignore
                ts_event=self._clock.timestamp_ns(),
            )
            return
        bet_id = resp['instructionReports'][0]['betId']
        self._log.debug(f"Matching venue_order_id: {bet_id} to client_order_id: {client_order_id}")
        self.venue_order_id_to_client_order_id[bet_id] = client_order_id  # type: ignore
        self.generate_order_accepted(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId(bet_id),  # type: ignore
            ts_event=self._clock.timestamp_ns(),
        )
    cpdef void update_order(self, UpdateOrder command) except *:
        Condition.not_none(command, "command")

        self.create_task(self._update_order(command))

    async def _update_order(self, UpdateOrder command):
        self._log.debug(f"Received update_order {command}")
        client_order_id: ClientOrderId = command.client_order_id
        instrument = self._cache.instrument(command.instrument_id)
        Condition.not_none(instrument, "instrument")
        existing_order = self._cache.order(client_order_id)  # type: Order

        # TODO (bm) Should we move this section up a level into cdef?

        self.generate_order_pending_update(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        if existing_order is None:
            self._log.warning(f"Attempting to update order that does not exist in the cache: {command}")
            self.generate_order_update_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=client_order_id,
                venue_order_id=command.venue_order_id,
                response_to=command.id.value,
                reason='ORDER NOT IN CACHE',
                ts_event=self._clock.timestamp_ns(),
            )
            return
        if existing_order.venue_order_id is None:
            self._log.warning(f"Order found does not have `id` set: {existing_order}")
            Condition.not_none(command.strategy_id, "command.strategy_id")
            Condition.not_none(command.instrument_id, "command.instrument_id")
            Condition.not_none(client_order_id, "client_order_id")
            self.generate_order_update_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=client_order_id,
                venue_order_id=VenueOrderId('-1'),
                response_to=command.id.value,
                reason='ORDER MISSING VENUE_ORDER_ID',
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Send order to client
        kw = order_update_to_betfair(
            command=command,
            venue_order_id=existing_order.venue_order_id,
            side=existing_order.side,
            instrument=instrument
        )
        self.pending_update_order_client_ids.add((command.client_order_id, existing_order.venue_order_id))
        resp = await self._client.replace_orders(**kw)

        # Check response
        assert len(resp['instructionReports']) == 1, "Should only be a single order"
        if resp["status"] == "FAILURE":
            reason = f"{resp['errorCode']}: {resp['instructionReports'][0]['errorCode']}"
            self._log.warning(f"Submit failed - {reason}")
            self.generate_order_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Check the venue_order_id that has been deleted currently exists on our order
        deleted_bet_id = resp["instructionReports"][0]["cancelInstructionReport"]["instruction"]["betId"]
        self._log.debug(f"{existing_order}, {deleted_bet_id}")
        assert existing_order.venue_order_id == VenueOrderId(deleted_bet_id)

        instructions = resp["instructionReports"][0]["placeInstructionReport"]
        self.venue_order_id_to_client_order_id[instructions["betId"]] = client_order_id
        self.generate_order_updated(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId(instructions["betId"]),
            quantity=Quantity(instructions["instruction"]['limitOrder']["size"], precision=4),
            price=price_to_probability(instructions["instruction"]['limitOrder']["price"]),
            trigger=None,  # Not applicable for Betfair
            ts_event=self._clock.timestamp_ns(),
            venue_order_id_modified=True,
        )

    cpdef void cancel_order(self, CancelOrder command) except *:
        Condition.not_none(command, "command")

        self.create_task(self._cancel_order(command))

    async def _cancel_order(self, CancelOrder command):
        self._log.debug("Received cancel order")
        self.generate_order_pending_cancel(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        instrument = self._cache.instrument(command.instrument_id)
        Condition.not_none(instrument, "instrument")

        # Format
        cancel_orders = order_cancel_to_betfair(command=command, instrument=instrument)  # type: ignore
        self._log.debug(str(cancel_orders))

        # Send to client
        resp = await self._client.cancel_orders(**cancel_orders)

        # Parse response
        bet_id = resp['instructionReports'][0]['instruction']['betId']
        self._log.debug(f"Matching venue_order_id: {bet_id} to client_order_id: {command.client_order_id}")
        self.venue_order_id_to_client_order_id[VenueOrderId(bet_id)] = command.client_order_id  # type: ignore
        self.generate_order_canceled(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=VenueOrderId(bet_id),  # type: ignore
            ts_event=self._clock.timestamp_ns(),
        )
        self._log.debug("Sent order cancel")

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

    async def check_account_currency(self):
        """
        Check account currency against BetfairClient
        """
        Condition.not_none(self.base_currency, "self.base_currency")
        details = await self._client.get_account_details()
        currency_code = details['currencyCode']
        assert currency_code == self.base_currency.code

# -- DEBUGGING -------------------------------------------------------------------------------------

    def create_task(self, coro):
        self._loop.create_task(self._check_task(coro))

    async def _check_task(self, coro):
        try:
            awaitable = await coro
            return awaitable
        except Exception as e:
            self._log.exception(f"Unhandled exception: {e}")

    cpdef HTTPClient client(self):
        return self._client

    cpdef BetfairInstrumentProvider instrument_provider(self):
        return self._instrument_provider

# -- ORDER STREAM API ------------------------------------------------------------------------------

    cpdef void handle_order_stream_update(self, bytes raw) except *:
        """ Handle an update from the order stream socket """
        cdef dict update = orjson.loads(raw)  # type: dict
        self._loop.create_task(self._handle_order_stream_update(update=update))

    async def _handle_order_stream_update(self, update):
        for market in update.get("oc", []):
            market_id = market["id"]
            for selection in market.get("orc", []):
                if selection.get("fullImage", False):
                    # TODO (bm) - need to replace orders for this selection
                    self._log.warning("Received full order image, SKIPPING!")
                for order_update in selection.get("uo", []):
                    await self._check_order_update(order_update)
                    if order_update["status"] == "E":
                        self._handle_stream_executable_order_update(update=order_update)
                    elif order_update["status"] == "EC":
                        self._handle_stream_execution_complete_order_update(update=order_update)
                    else:
                        self._log.warning(f"Unknown order state: {order_update}")

    async def _check_order_update(self, dict update):
        """
        Ensure we have a client_order_id, instrument and order for this venue order update
        """
        venue_order_id = VenueOrderId(str(update['id']))
        client_order_id = await self.wait_for_order(venue_order_id=venue_order_id, timeout_seconds=10.0)
        if client_order_id is None:
            self._log.warning(f"Can't find client_order_id for {update}")
        order = self._cache.order(client_order_id)
        Condition.not_none(order, "order")
        instrument = self._cache.instrument(order.instrument_id)
        Condition.not_none(instrument, "instrument")

    cpdef void _handle_stream_executable_order_update(self, dict update) except *:
        """
        Handle update containing "E" (executable) order update
        """
        cdef VenueOrderId venue_order_id = VenueOrderId(update['id'])
        cdef ClientOrderId client_order_id = self.venue_order_id_to_client_order_id[VenueOrderId(update['id'])]
        cdef Order order = self._cache.order(client_order_id)
        cdef BettingInstrument instrument = self._cache.instrument(order.instrument_id)

        # Check if this is the first time seeing this order (backtest or replay)
        if venue_order_id in self.venue_order_id_to_client_order_id:
            # We've already sent an accept for this order in self._submit_order
            self._log.debug(f"Skipping order_accept as order exists: venue_order_id={update['id']}")
        else:
            raise RuntimeError()
            # self.generate_order_accepted(
            #     strategy_id=order.strategy_id,
            #     instrument_id=instrument.id,
            #     client_order_id=client_order_id,
            #     venue_order_id=venue_order_id,
            #     ts_event=millis_to_nanos(order_update["pd"]),
            # )

        # Check for any portion executed
        if update["sm"] > 0 and update['sm'] > order.filled_qty:
            execution_id = create_execution_id(update)
            if execution_id not in self.published_executions[client_order_id]:
                self.generate_order_filled(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    venue_position_id=None,  # Can be None
                    execution_id=execution_id,
                    order_side=B2N_ORDER_STREAM_SIDE[update["side"]],
                    order_type=OrderType.LIMIT,
                    last_qty=Quantity(update["sm"], instrument.size_precision),
                    last_px=price_to_probability(update["p"]),
                    # avg_px=Decimal(order['avp']),
                    quote_currency=instrument.quote_currency,
                    commission=Money(0, self.base_currency),
                    liquidity_side=LiquiditySide.NONE,
                    ts_event=millis_to_nanos(update["md"]),
                )
                self.published_executions[client_order_id].append(execution_id)

    cpdef void _handle_stream_execution_complete_order_update(self, dict update) except *:
        """
        Handle "EC" (execution complete) order updates
        """
        cdef VenueOrderId venue_order_id = VenueOrderId(str(update["id"]))
        cdef ClientOrderId client_order_id = self._cache.client_order_id(venue_order_id=venue_order_id)
        cdef Order order = self._cache.order(client_order_id=client_order_id)
        cdef BettingInstrument instrument = self._cache.instrument(order.instrument_id)

        if update["sm"] > 0 and update['sm'] > order.filled_qty:
            self._log.debug("")
            execution_id = create_execution_id(update)
            if execution_id not in self.published_executions[client_order_id]:
                # At least some part of this order has been filled
                self.generate_order_filled(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    venue_position_id=None,  # Can be None
                    execution_id=execution_id,
                    order_side=B2N_ORDER_STREAM_SIDE[update["side"]],
                    order_type=OrderType.LIMIT,
                    last_qty=Quantity(update["sm"], instrument.size_precision),
                    last_px=price_to_probability(update['p']),
                    quote_currency=instrument.quote_currency,
                    # avg_px=order['avp'],
                    commission=Money(0, self.base_currency),
                    liquidity_side=LiquiditySide.TAKER,  # TODO - Fix this?
                    ts_event=millis_to_nanos(update['md']),
                )
                self.published_executions[client_order_id].append(execution_id)

        cancel_qty = update["sc"] + update["sl"] + update["sv"]
        if cancel_qty > 0 and not order.is_completed_c():
            assert update['sm'] + cancel_qty == update["s"], f"Size matched + canceled != total: {update}"
            # If this is the result of a UpdateOrder, we don't want to emit a cancel

            key = (client_order_id, venue_order_id)
            self._log.debug(
                f"cancel key: {key}, pending_update_order_client_ids: {self.pending_update_order_client_ids}")
            if key not in self.pending_update_order_client_ids:
                # The remainder of this order has been canceled
                self.generate_order_canceled(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=millis_to_nanos(update.get("cd") or update.get("ld") or update.get('md')),
                )
        # Market order will not be in self.published_executions
        if client_order_id in self.published_executions:
            # This execution is complete - no need to track this anymore
            del self.published_executions[client_order_id]

    cpdef void _handle_stream_execution_matched_fills(self, dict selection) except *:
        cdef dict trade
        for trade in selection.get("mb", []):
            pass
        for trade in selection.get("ml", []):
            pass

    async def wait_for_order(self, venue_order_id: VenueOrderId, timeout_seconds=10.0):
        """
        We may get an order update from the socket before our submit_order
        response has come back (with our betId).

        As a precaution, wait up to `timeout_seconds` for the betId to be added
        to `self.order_id_to_client_order_id`.
        """
        assert isinstance(venue_order_id, VenueOrderId)
        start = self._clock.timestamp_ns()
        now = start
        while (now - start) < secs_to_nanos(timeout_seconds):
            self._log.debug(f"checking venue_order_id={venue_order_id} in {self.venue_order_id_to_client_order_id}")
            if venue_order_id in self.venue_order_id_to_client_order_id:
                client_order_id = self.venue_order_id_to_client_order_id[venue_order_id]
                self._log.debug(f"Found order in {nanos_to_secs(now - start)} sec: {client_order_id}")
                return client_order_id
            now = self._clock.timestamp_ns()
            await asyncio.sleep(0.1)
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


cpdef ExecutionId create_execution_id(dict uo):
    cdef bytes data = orjson.dumps(
        (uo['id'], uo['p'], uo['s'], uo['side'], uo['pt'], uo['ot'], uo['pd'], uo.get('md'), uo.get('avp'), uo.get('sm'))
    )
    return ExecutionId(hashlib.sha1(data).hexdigest())
