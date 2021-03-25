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

from datetime import datetime
from decimal import Decimal
from typing import Dict, List, Optional

import betfairlightweight
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.datetime import from_unix_time_ms
from nautilus_trader.core.message cimport Event
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.liquidity_side import LiquiditySide
from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.adapters.betfair.common import B2N_ORDER_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.parsing import generate_order_status_report
from nautilus_trader.adapters.betfair.parsing import generate_trades_list
from nautilus_trader.adapters.betfair.parsing import order_amend_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import Symbol


cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class BetfairExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Betfair.
    """

    def __init__(
        self,
        client not None: betfairlightweight.APIClient,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
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
        cdef BetfairInstrumentProvider instrument_provider = BetfairInstrumentProvider(
            client=client,
            load_all=False,
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

        self._client = client # type: betfairlightweight.APIClient
        self._stream = BetfairOrderStreamClient(
            client=self._client, message_handler=self.handle_order_stream_update,
        )
        self.is_connected = False
        self.order_id_to_cl_ord_id = {}  # type: Dict[str, ClientOrderId]

    cpdef void connect(self) except *:
        self._loop.create_task(self._connect())

    async def _connect(self):
        self._log.info("Connecting...")
        resp = self._client.login()
        print(resp)
        self._log.info("Betfair APIClient login successful.", LogColor.GREEN)

        self._log.info("Loading Instruments.")
        self._instrument_provider.load_all()
        self._log.info(f"Loaded {len(self._instrument_provider._instruments)} Instruments.")

        await self._stream.connect()

        self.is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        self._client.client_lnwsogout()
        self._log.info("Disconnected.")

    # -- COMMAND HANDLERS ------------------------------------------------------------------------------
    # TODO - #  Do want to throttle updates if they're coming faster than x / sec? Maybe this is for risk engine?
    #  We could use some heuristics about the avg network latency and add an optional flag for throttle inserts etc.

    cpdef void submit_order(self, SubmitOrder command) except *:
        aw = self._loop.run_in_executor(None, self._submit_order, command)
        self._log.info("aw:", aw)
        resp = await aw
        self._log.info("resp:", resp)
        assert len(resp['result']['instructionReports']) == 1, "Should only be a single order"
        bet_id = resp['result']['instructionReports'][0]['betId']
        self.order_id_to_cl_ord_id[bet_id] = command.order.cl_ord_id
        self._generate_order_accepted(
            cl_ord_id = command.order.cl_ord_id,
            order_id = bet_id,
            timestamp = self._clock.unix_time(),
        )

    def _submit_order(self, SubmitOrder command):
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_submit_to_betfair(command=command, instrument=instrument)
        self._generate_order_submitted(
            cl_ord_id=command.order.cl_ord_id, timestamp=self._clock.utc_now_c(),
        )
        return self._client.betting.place_orders(**kw)

    # TODO - Does this also take 5s ??
    cpdef void amend_order(self, AmendOrder command) except *:
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_amend_to_betfair(command=command)
        self._client.betting.replace_orders(**kw)

    cpdef void cancel_order(self, CancelOrder command) except *:
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_cancel_to_betfair(command=command)
        self._client.betting.cancel_orders(**kw)

    # cpdef void bulk_submit_order(self, list commands):
    # betfair allows up to 200 inserts per request
    #     raise NotImplementedError

    # cpdef void bulk_submit_amend(self, list commands):
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

    # -- Order stream API ---------------------------------------------------------

    cpdef void handle_order_stream_update(self, dict raw) except *:
        """ Handle an update from the order stream socket """
        for market in raw.get("oc", []):
            market_id = market["id"]
            for selection in market.get("orc", []):
                instrument = self._instrument_provider.get_betting_instrument(
                    market_id=market_id,
                    selection_id=str(selection["id"]),
                    handicap=str(selection.get("hc", "0.0")),
                )
                for order in selection.get("uo", []):
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
                            fill_qty=order['sm'],
                            cum_qty=order['s'] - order['sr'],
                            leaves_qty=order['sr'],
                            avg_px=order['avp'],
                            commission_amount=Decimal(0.0),
                            commission_currency=self.get_account_currency(),
                            liquidity_side=LiquiditySide.TAKER,  # TODO - Fix this?
                            timestamp=from_unix_time_ms(order['md']),
                        )
                    elif order["sm"] == 0 and any([order[x] != 0 for x in ("sc", "sl", "sv")]):
                        self._generate_order_cancelled(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            timestamp=from_unix_time_ms(order['cd']),
                        )
                    # This is a full order, none has traded yet (size_remaining = original placed size)
                    elif order['status'] == "E" and order["sr"] != 0 and order["sr"] == order["s"]:
                        self._generate_order_accepted(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            timestamp=from_unix_time_ms(order['pd']),
                        )
                    # A portion of this order has been filled, size_remaining < placed size, send a fill and an order accept
                    elif order['status'] == "E" and order["sr"] != 0 and order["sr"] < order["s"]:
                        self._generate_order_accepted(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            timestamp=from_unix_time_ms(order['pd']),
                        )
                        self._generate_order_filled(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            execution_id=execution_id,
                            instrument_id=instrument.id,
                            order_side=B2N_ORDER_STREAM_SIDE[order['side']],
                            fill_qty=order['sm'],
                            cum_qty=order['s'] - order['sr'],
                            leaves_qty=order['sr'],
                            avg_px=order['avp'],
                            commission_amount=Decimal(0.0),
                            commission_currency=self.get_account_currency(),  # TODO - look up on account
                            liquidity_side=LiquiditySide.NONE,
                            timestamp=from_unix_time_ms(order['md']),
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

    # -- RECONCILIATION -------------------------------------------------------------------------------

    async def generate_order_status_report(self) -> Optional[OrderStatusReport]:
        return await generate_order_status_report(self)

    async def generate_trades_list(self, order_id: OrderId, symbol: Symbol,  since: Optional[datetime]=None) -> List[ExecutionReport]:
        return await generate_trades_list(self)

    # -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    def _handle_event_py(self, event: Event):
        self._engine.process(event)

    # -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._engine.process(event)
