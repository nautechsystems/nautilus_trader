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
from datetime import datetime
from decimal import Decimal
from functools import partial
from typing import Dict, List, Optional

import betfairlightweight
import orjson

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.message cimport Event
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine

from nautilus_trader.model.c_enums.liquidity_side import LiquiditySide

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport OrderId

from nautilus_trader.adapters.betfair.common import B2N_ORDER_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
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
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.order.base import Order


cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class BetfairExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Betfair.
    """

    def __init__(
        self,
        client not None,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
        dict market_filter not None,
    ):
        """
        Initialize a new instance of the `BetfairExecutionClient` class.

        Parameters
        ----------
        client : betfairlightweight.APIClient
            The Betfair client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        self._client = client # type: betfairlightweight.APIClient
        self._client.login()

        cdef BetfairInstrumentProvider instrument_provider = BetfairInstrumentProvider(
            client=client,
            logger=logger,
            load_all=True,
            market_filter=market_filter
        )

        super().__init__(
            BETFAIR_VENUE.value,
            account_id,
            engine,
            instrument_provider,
            clock,
            logger,
            config={
                "name": "BetfairExecClient",
            }
        )
        self._stream = BetfairOrderStreamClient(
            client=self._client, logger=logger, message_handler=self.handle_order_stream_update,
        )
        self.is_connected = False
        self.order_id_to_cl_ord_id = {}  # type: Dict[str, ClientOrderId]

    cpdef void connect(self) except *:
        self._loop.create_task(self._connect())

    async def _connect(self):
        self._log.info("Connecting to Betfair APIClient...")
        resp = self._client.login()
        self._log.info("Betfair APIClient login successful.", LogColor.GREEN)

        aws = [
            self._stream.connect(),
            self.connection_account_state(),
        ]
        await asyncio.gather(*aws)

        self.is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        self._client.client_logout()
        self._log.info("Disconnected.")

    # -- ACCOUNT HANDLERS ------------------------------------------------------------------------------
    async def connection_account_state(self):
        aws = [
            self._loop.run_in_executor(None, self._get_account_details),
            self._loop.run_in_executor(None, self._get_account_funds),
        ]
        result = await asyncio.gather(*aws)
        account_details, account_funds = result
        account_state = betfair_account_to_account_state(
            account_detail=account_details, account_funds=account_funds, event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns()
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
        self._log.debug(f"SubmitOrder received {command}")

        self._generate_order_submitted(
            cl_ord_id=command.order.cl_ord_id, timestamp_ns=self._clock.timestamp_ns(),
        )
        self._log.debug(f"Generated _generate_order_submitted")

        f = self._loop.run_in_executor(None, self._submit_order, command) # type: asyncio.Future
        self._log.debug(f"future: {f}")
        f.add_done_callback(partial(self._post_submit_order, client_order_id=command.order.cl_ord_id))

    def _submit_order(self, SubmitOrder command):
        instrument = self._instrument_provider.find(command.instrument_id)
        kw = order_submit_to_betfair(command=command, instrument=instrument)
        self._log.debug(f"{kw}")
        return self._client.betting.place_orders(**kw)

    def _post_submit_order(self, f: asyncio.Future, client_order_id):
        self._log.debug(f"inside _post_submit_order")
        self._log.debug(f"f: {f}")
        try:
            resp = f.result()
            self._log.debug(f"resp: {resp}")
        except Exception as e:
            self._log.error(str(e))
            return
        assert len(resp['instructionReports']) == 1, "Should only be a single order"
        self._log.debug("Got a single order")
        if resp["status"] == "FAILURE":
            reason = f"{resp['errorCode']}: {resp['instructionReports'][0]['errorCode']}"
            self._log.error(f"Submit failed - {reason}")
            self._generate_order_rejected(
                cl_ord_id=client_order_id,
                reason=reason,
                timestamp_ns=self._clock.timestamp_ns(),
            )
            return
        bet_id = resp['instructionReports'][0]['betId']
        self.order_id_to_cl_ord_id[bet_id] = client_order_id
        self._generate_order_accepted(
            cl_ord_id=client_order_id,
            order_id=OrderId(bet_id),
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cpdef void update_order(self, UpdateOrder command) except *:
        self._log.debug("Received update order")
        instrument = self._instrument_provider._instruments[command.instrument_id]
        existing_order = self._engine.cache.order(command.cl_ord_id) # type: Order
        if existing_order is None:
            self._log.warning(f"Attempting to update order that does not exist in the cache: {command}")
            return
        if existing_order.id == OrderId("NULL"):
            self._log.warning(f"Order found does not have `id` set: {existing_order}")
            return
        self._log.debug(f"existing_order: {existing_order}")
        kw = order_update_to_betfair(
            command=command,
            order_id=existing_order.id,
            side=existing_order.side,
            instrument=instrument
        )
        self._log.debug(f"update kw: {kw}")
        resp = self._client.betting.replace_orders(**kw, async_=True)
        self._log.debug(f"update: {resp}")

    cpdef void cancel_order(self, CancelOrder command) except *:
        self._log.debug("Received cancel order")
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

    # -- Account information ---------------------------------------------------------

    cpdef str get_account_currency(self):
        return self._instrument_provider.get_account_currency()

    # -- Debugging ---------------------------------------------------------

    cpdef object client(self):
        return self._client

    cpdef BetfairInstrumentProvider instrument_provider(self):
        return self._instrument_provider

    cpdef LiveExecutionEngine engine(self):
        return self._engine

    # -- Order stream API ---------------------------------------------------------

    cpdef void handle_order_stream_update(self, bytes raw) except *:
        """ Handle an update from the order stream socket """
        cdef dict update = orjson.loads(raw) # type: dict
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
                    await self._wait_for_order(order['id'], timeout_seconds=5.0)
                    cl_ord_id = self.order_id_to_cl_ord_id[order['id']]
                    order_id = OrderId(order["id"])
                    execution_id = ExecutionId(str(order["id"]))
                    if (
                            order["status"] == "EC" and order["sm"] != 0
                    ):
                        # Execution complete, The entire order has traded or been cancelled
                        self._generate_order_filled(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            execution_id=execution_id,
                            instrument_id=instrument.id,
                            order_side=B2N_ORDER_STREAM_SIDE[order['side']],
                            last_qty=Decimal(order['sm']),
                            last_px=Decimal(order['p']),
                            cum_qty=Decimal(order['s'] - order['sr']),
                            leaves_qty=Decimal(order['sr']),
                            # avg_px=order['avp'],
                            commission_amount=Decimal(0.0),
                            commission_currency=self.get_account_currency(),
                            liquidity_side=LiquiditySide.TAKER,  # TODO - Fix this?
                            timestamp_ns=millis_to_nanos(order['md']),
                        )
                    elif order["sm"] == 0 and any([order[x] != 0 for x in ("sc", "sl", "sv")]):
                        self._generate_order_cancelled(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            timestamp_ns=millis_to_nanos(order['cd']),
                        )
                    # This is a full order, none has traded yet (size_remaining = original placed size)
                    elif order['status'] == "E" and order["sr"] == order["s"]:
                        self._generate_order_accepted(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            timestamp_ns=millis_to_nanos(order['pd']),
                        )
                    # A portion of this order has been filled, size_remaining < placed size, send a fill and an order accept
                    elif order['status'] == "E" and order["sr"] != 0 and order["sr"] < order["s"]:
                        self._generate_order_filled(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            execution_id=execution_id,
                            instrument_id=instrument.id,
                            order_side=B2N_ORDER_STREAM_SIDE[order['side']],
                            last_qty=Decimal(order['sm']),
                            last_px=Decimal(order['p']),
                            cum_qty=Decimal(order['s'] - order['sr']),
                            leaves_qty=Decimal(order['sr']),
                            # avg_px=Decimal(order['avp']),
                            commission_amount=Decimal(0.0),
                            commission_currency=self.get_account_currency(),  # TODO - look up on account
                            liquidity_side=LiquiditySide.NONE,
                            timestamp_ns=millis_to_nanos(order['md']),
                        )
                    else:
                        self._log.error("Unknown order state: {order}")
                        # raise KeyError("Unknown order type", order, None)

                # these values?
                for trade in selection.get("mb", []):
                    # TODO - we can get a matched back without full details. Need to match ourselves??
                    pass
                for trade in selection.get("ml", []):
                    pass

                # TODO - Should be no difference for fullImage at this stage. We just send all updates individually
                if selection.get("fullImage", False):
                    pass

    async def _wait_for_order(self, order_id, timeout_seconds=1.0):
        """
        We may get an order update from the socket before our submit_order response has come back (with our betId).
        As a precaution, wait up to `timeout_seconds` for the betId to be added to `self.order_id_to_cl_ord_id`
        """
        start = self._clock.timestamp_ns()
        now = start
        while (now - start) < secs_to_nanos(timeout_seconds):
            if order_id in self.order_id_to_cl_ord_id:
                self._log.debug(f"Found order in {nanos_to_secs(now - start)} sec ")
                return
            now = self._clock.timestamp_ns()
            await asyncio.sleep(0)
        raise TimeoutError(f"Failed to find order_id: {order_id} after {timeout_seconds} seconds")

    # -- RECONCILIATION -------------------------------------------------------------------------------

    async def generate_order_status_report(self, order: Order) -> Optional[OrderStatusReport]:
        self._log.debug(f"generate_order_status_report: {order}")
        return await generate_order_status_report(self, order)

    async def generate_exec_reports(self, order_id: OrderId, symbol: Symbol,  since: Optional[datetime]=None) -> List[ExecutionReport]:
        self._log.debug(f"generate_exec_reports: {order_id}, {symbol}, {since}")
        return await generate_trades_list(self, order_id,  symbol,  since)

    # -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    def _handle_event_py(self, event: Event):
        self._engine.process(event)

    # -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._engine.process(event)
