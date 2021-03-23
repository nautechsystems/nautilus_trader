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
from typing import Dict, Optional

import betfairlightweight

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.message cimport Event
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.liquidity_side import LiquiditySide
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.model.identifiers cimport ClientOrderId, OrderId
from nautilus_trader.model.identifiers import ExecutionId, Symbol
from nautilus_trader.execution.messages import OrderStatusReport, ExecutionReport
from nautilus_trader.model.order.base import Order
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE, B2N_ORDER_STREAM_SIDE, order_cancel_to_betfair, \
    order_amend_to_betfair

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
        self._client.login()
        self._log.info("APIClient login successful.", LogColor.GREEN)

        self._log.info("Loading Instruments.")
        self._instrument_provider.load_all()
        self._log.info(f"Loaded {len(self._instrument_provider._instruments)} Instruments.")

        await self._stream.connect()

        self.is_connected = True
        self._log.info("Connected.")

    def _connect_order_stream(self):
        """

        :return:
        """
        pass

    # async def state_report(self, list active_orders):
    #     """
    #     Return an execution state report based on the given list of active
    #     orders.
    #     Parameters
    #     ----------
    #     active_orders : list[Order]
    #         The orders which currently have an 'active' status.
    #     Returns
    #     -------
    #     ExecutionStateReport
    #     """
    #     Condition.not_none(active_orders, "active_orders")
    #
    #     cdef dict order_states = {}
    #     cdef dict order_filled = {}
    #     cdef dict position_states = {}
    #
    #     if not active_orders:
    #         # Nothing to resolve
    #         return ExecutionStateReport(
    #             client=self.name,
    #             account_id=self.account_id,
    #             order_states=order_states,
    #             order_filled=order_filled,
    #             position_states=position_states,
    #         )
    #
    #     cdef int count = len(active_orders)
    #     self._log.info(
    #         f"Resolving state: {count} active order{'s' if count > 1 else ''}...",
    #         LogColor.BLUE,
    #     )
    #
    #     cdef Instrument instrument
    #     cdef Order order
    #     cdef str status
    #     cdef dict response
    #     cdef list trades
    #     cdef list order_trades
    #     for order in active_orders:
    #         if order.id.is_null():
    #             self._log.error(f"Cannot resolve state for {repr(order.cl_ord_id)}, "
    #                             f"OrderId was 'NULL'.")
    #             continue  # Cannot resolve order
    #         instrument = self._instrument_provider.find_c(order.symbol)
    #         if instrument is None:
    #             self._log.error(f"Cannot resolve state for {repr(order.cl_ord_id)}, "
    #                             f"instrument for {order.instrument_id} not found.")
    #             continue  # Cannot resolve order
    #
    #         try:
    #             response = await self._client.fetch_order(
    #                 id=order.id.value,
    #                 symbol=order.symbol.value,
    #             )
    #             trades = await self._client.fetch_my_trades(
    #                 symbol=order.symbol.value,
    #                 since=to_unix_time_ms(order.timestamp),
    #             )
    #             order_trades = [trade for trade in trades if trade["order"] == order.id.value]
    #
    #         except betfairlightweight.BetfairError as ex:
    #             self._log_ccxt_error(ex, self._update_balances.__name__)
    #             continue
    #         if response is None:
    #             self._log.error(f"No order found for {order.id.value}.")
    #             continue
    #         # self._log.info(str(response), LogColor.BLUE)  # TODO: Development
    #
    #         cum_qty = order.filled_qty.as_decimal()
    #         for trade in order_trades:
    #             from model.identifiers import ExecutionId
    #             execution_id = ExecutionId(str(response["id"]))
    #             if execution_id in order.execution_ids_c():
    #                 continue  # Trade already applied
    #             self._generate_order_filled(
    #                 cl_ord_id=order.cl_ord_id,
    #                 order_id=order.id,
    #                 execution_id=ExecutionId(str(response["id"])),
    #                 instrument_id=order.instrument_id,
    #                 order_side=order.side,
    #                 fill_qty=Decimal(f"{trade['amount']:.{instrument.size_precision}}"),
    #                 cum_qty=cum_qty,
    #                 leaves_qty=order.quantity - cum_qty,
    #                 avg_px=Decimal(trade["price"]),
    #                 commission_amount=trade["fee"]["cost"],
    #                 commission_currency=trade["fee"]["currency"],
    #                 liquidity_side=LiquiditySide.TAKER if trade["takerOrMaker"] == "taker" else LiquiditySide.MAKER,
    #                 timestamp=from_unix_time_ms(trade["timestamp"]),
    #             )
    #
    #         status = response["status"]
    #         if status == "open":
    #             if cum_qty > 0:
    #                 order_states[order.id] = OrderState.PARTIALLY_FILLED
    #                 order_filled[order.id] = cum_qty
    #         elif status == "closed":
    #             order_states[order.id] = OrderState.FILLED
    #             order_filled[order.id] = cum_qty
    #         elif status == "canceled":
    #             order_states[order.id] = OrderState.CANCELLED
    #             timestamp = from_unix_time_ms(<long>response["timestamp"])
    #             self._generate_order_cancelled(order.cl_ord_id, order.id, timestamp)
    #         elif status == "expired":
    #             order_states[order.id] = OrderState.EXPIRED
    #             self._generate_order_expired(order.cl_ord_id, order.id, timestamp)
    #
    #     return ExecutionStateReport(
    #         client=self.name,
    #         account_id=self.account_id,
    #         order_states=order_states,
    #         order_filled=order_filled,
    #         position_states=position_states,
    #     )

    cpdef void disconnect(self) except *:
        self._client.client_logout()
        self._log.info("Disconnected.")

    # -- COMMAND HANDLERS ------------------------------------------------------------------------------
    # TODO - Add support for bulk updates - betfair allows up to 200 inserts / 60 updates / 60 cancels per request,
    #  we might want to throttle updates if they're coming faster than x / sec? Maybe this is for risk engine? We could
    #  use some heuristics about the avg network latency and add an optional flag for bulk inserts etc.

    cpdef void submit_order(self, SubmitOrder command) except *:
        pass

    # TODO How to mix async (awaiting on place_orders) with submit order?
    # cpdef void submit_order(self, SubmitOrder command) except *:
    #     instrument = self._instrument_provider._instruments[command.instrument_id]
    #     kw = order_submit_to_betfair(command=command, instrument=instrument)
    #     self._generate_order_submitted(
    #         cl_ord_id=command.order.cl_ord_id, order_id=command.order.id, timestamp=self._clock.utc_now_c(),
    #     )
    #     resp = await self._loop.run_in_executor(self._client.betting.place_orders, **kw)
    #     assert len(resp['result']['instructionReports']) == 1, "Should only be a single order"
    #     bet_id = resp['result']['instructionReports'][0]['betId']
    #     self.order_id_to_cl_ord_id[bet_id] = command.order.cl_ord_id
    #     self._generate_order_accepted(
    #         cl_ord_id=command.order.cl_ord_id,
    #         order_id=bet_id,
    #         timestamp=self._clock.utc_now_c()
    #     )

    cpdef void amend_order(self, AmendOrder command) except *:
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_amend_to_betfair(command=command)
        self._client.betting.replace_orders(**kw)

    cpdef void cancel_order(self, CancelOrder command) except *:
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_cancel_to_betfair(command=command)
        self._client.betting.cancel_orders(**kw)

    # -- Instrument helpers ---------------------------------------------------------

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
                    print("order:", order)
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
                            commission_currency="AUD",  # TODO - look up on account
                            liquidity_side=LiquiditySide.NONE,
                            timestamp=order['md'],
                        )
                    elif order["sm"] == 0 and any([order[x] != 0 for x in ("sc", "sl", "sv")]):
                        self._generate_order_cancelled(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            timestamp=order['cd'],
                        )
                    # This is a full order, none has traded yet (size_remaining = original placed size)
                    elif order['status'] == "E" and order["sr"] != 0 and order["sr"] == order["s"]:
                        self._generate_order_accepted(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            timestamp=order['pd'],
                        )
                    # A portion of this order has been filled, size_remaining < placed size, send a fill and an order accept
                    elif order['status'] == "E" and order["sr"] != 0 and order["sr"] < order["s"]:
                        self._generate_order_accepted(
                            cl_ord_id=cl_ord_id,
                            order_id=order_id,
                            timestamp=order['pd'],
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
                            commission_currency="AUD",  # TODO - look up on account
                            liquidity_side=LiquiditySide.NONE,
                            timestamp=order['md'],
                        )
                    else:
                        self._log.error("Unknown order state: {order}")
                        # raise KeyError("Unknown order type", order, None)

                # TODO - These should be covered by filled orders above, but potentially we should add a checksum against
                # these values?
                for trade in selection.get("mb", []):
                    pass
                for trade in selection.get("ml", []):
                    pass

                # TODO - Should be no difference for fullImage at this stage. We just send all updates individually
                if selection.get("fullImage", False):
                    pass

    # -- RECONCILIATION -------------------------------------------------------------------------------

    async def generate_order_status_report(self, order: Order) -> Optional[OrderStatusReport]:
        # return self._client.betting.list_current_orders()
        raise NotADirectoryError

    async def generate_trades_list(self, order_id: OrderId, symbol: Symbol,  since: Optional[datetime]=None) -> List[ExecutionReport]:
        # return self._client.betting.list_cleared_orders()
        raise NotImplementedError

    # -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    def _handle_event_py(self, event: Event):
        self._engine.process(event)

    # -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._engine.process(event)
