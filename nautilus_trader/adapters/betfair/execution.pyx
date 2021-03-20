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

import betfairlightweight

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger

from nautilus_trader.core.message cimport Event
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitOrder

from nautilus_trader.model.identifiers cimport AccountId

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider

from adapters.betfair.common import order_submit_to_betfair, order_amend_to_betfair, order_cancel_to_betfair, \
    BETFAIR_VENUE

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
        self.is_connected = False

    cpdef void connect(self) except *:
        self._log.info("Connecting...")
        self._client.login()
        self._log.info("APIClient login successful.", LogColor.GREEN)

        self._log.info("Loading Instruments.")
        self._instrument_provider.load_all()
        self._log.info(f"Loaded {len(self._instrument_provider._instruments)} Instruments.")

        self.is_connected = True
        self._log.info("Connected.")

    def _connect_order_stream(self):
        """

        :return:
        """
        Condition.not_none(active_orders, "active_orders")

        cdef dict order_states = {}
        cdef dict order_filled = {}
        cdef dict position_states = {}

        if not active_orders:
            # Nothing to resolve
            return ExecutionStateReport(
                client=self.name,
                account_id=self.account_id,
                order_states=order_states,
                order_filled=order_filled,
                position_states=position_states,
            )

        cdef int count = len(active_orders)
        self._log.info(
            f"Resolving state: {count} active order{'s' if count > 1 else ''}...",
            LogColor.BLUE,
        )

        cdef Instrument instrument
        cdef Order order
        cdef str status
        cdef dict response
        cdef list trades
        cdef list order_trades
        for order in active_orders:
            if order.id.is_null():
                self._log.error(f"Cannot resolve state for {repr(order.cl_ord_id)}, "
                                f"OrderId was 'NULL'.")
                continue  # Cannot resolve order
            instrument = self._instrument_provider.find_c(order.symbol)
            if instrument is None:
                self._log.error(f"Cannot resolve state for {repr(order.cl_ord_id)}, "
                                f"instrument for {order.instrument_id} not found.")
                continue  # Cannot resolve order

            try:
                response = await self._client.fetch_order(
                    id=order.id.value,
                    symbol=order.symbol.value,
                )
                trades = await self._client.fetch_my_trades(
                    symbol=order.symbol.value,
                    since=to_unix_time_ms(order.timestamp),
                )
                order_trades = [trade for trade in trades if trade["order"] == order.id.value]

            except betfairlightweight.BetfairError as ex:
                self._log_ccxt_error(ex, self._update_balances.__name__)
                continue
            if response is None:
                self._log.error(f"No order found for {order.id.value}.")
                continue
            # self._log.info(str(response), LogColor.BLUE)  # TODO: Development

            cum_qty = order.filled_qty.as_decimal()
            for trade in order_trades:
                execution_id = ExecutionId(str(response["id"]))
                if execution_id in order.execution_ids_c():
                    continue  # Trade already applied
                self._generate_order_filled(
                    cl_ord_id=order.cl_ord_id,
                    order_id=order.id,
                    execution_id=ExecutionId(str(response["id"])),
                    instrument_id=order.instrument_id,
                    order_side=order.side,
                    fill_qty=Decimal(f"{trade['amount']:.{instrument.size_precision}}"),
                    cum_qty=cum_qty,
                    leaves_qty=order.quantity - cum_qty,
                    avg_px=Decimal(trade["price"]),
                    commission_amount=trade["fee"]["cost"],
                    commission_currency=trade["fee"]["currency"],
                    liquidity_side=LiquiditySide.TAKER if trade["takerOrMaker"] == "taker" else LiquiditySide.MAKER,
                    timestamp=from_unix_time_ms(trade["timestamp"]),
                )

            status = response["status"]
            if status == "open":
                if cum_qty > 0:
                    order_states[order.id] = OrderState.PARTIALLY_FILLED
                    order_filled[order.id] = cum_qty
            elif status == "closed":
                order_states[order.id] = OrderState.FILLED
                order_filled[order.id] = cum_qty
            elif status == "canceled":
                order_states[order.id] = OrderState.CANCELLED
                timestamp = from_unix_time_ms(<long>response["timestamp"])
                self._generate_order_cancelled(order.cl_ord_id, order.id, timestamp)
            elif status == "expired":
                order_states[order.id] = OrderState.EXPIRED
                self._generate_order_expired(order.cl_ord_id, order.id, timestamp)

        return ExecutionStateReport(
            client=self.name,
            account_id=self.account_id,
            order_states=order_states,
            order_filled=order_filled,
            position_states=position_states,
        )

    cpdef void disconnect(self) except *:
        self._client.client_logout()
        self._log.info("Disconnected.")

    # -- COMMAND HANDLERS ------------------------------------------------------------------------------
    # TODO - Add support for bulk updates - betfair allows up to 200 inserts / 60 updates / 60 cancels per request

    cpdef void submit_order(self, SubmitOrder command) except *:
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_submit_to_betfair(command=command, instrument=instrument)
        self._client.betting.place_orders(**kw)

    cpdef void amend_order(self, AmendOrder command) except *:
        # TODO - Need to know instrument_id
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_amend_to_betfair(command=command)
        self._client.betting.replace_orders(**kw)

    cpdef void cancel_order(self, CancelOrder command) except *:
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_cancel_to_betfair(command=command)
        self._client.betting.cancel_orders(**kw)

    # -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    def _handle_event_py(self, event: Event):
        self._engine.process(event)

    # -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._engine.process(event)
