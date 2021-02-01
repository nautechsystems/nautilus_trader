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

from decimal import Decimal

from cpython.datetime cimport datetime

import ccxt
from ccxt.base.errors import BaseError as CCXTError

from nautilus_trader.adapters.ccxt.execution cimport CCXTExecutionClient
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport from_posix_ms
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.order.base cimport PassiveOrder


cdef class BinanceExecutionClient(CCXTExecutionClient):
    """
    Provides an execution client for the Binance exchange.
    """

    def __init__(
        self,
        client not None: ccxt.Exchange,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `BinanceExecutionClient` class.

        Parameters
        ----------
        client : ccxt.Exchange
            The unified CCXT client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        Condition.true(client.name.upper() == "BINANCE", "client.name != BINANCE")

        super().__init__(
            client,
            account_id,
            engine,
            clock,
            logger,
        )

# -- COMMANDS --------------------------------------------------------------------------------------

    async def _submit_order(self, Order order):
        # Common arguments

        if order.time_in_force == TimeInForce.GTD:
            raise ValueError("TimeInForce.GTD not supported in this version.")

        if order.time_in_force == TimeInForce.DAY:
            raise ValueError("Binance does not support TimeInForce.DAY.")

        cdef dict params = {
            "newClientOrderId": order.cl_ord_id.value,
            "recvWindow": 10000  # TODO: Server time sync issue?
        }

        cdef str order_type
        if order.type == OrderType.MARKET:
            order_type = "MARKET"
        elif order.type == OrderType.LIMIT and order.is_post_only:
            # Cannot be hidden as post only is True
            order_type = "LIMIT_MAKER"
        elif order.type == OrderType.LIMIT:
            if order.is_hidden:
                raise ValueError("Binance does not support hidden orders.")
            order_type = "LIMIT"
            params["timeInForce"] = TimeInForceParser.to_str(order.time_in_force)
        elif order.type == OrderType.STOP_MARKET:
            if order.side == OrderSide.BUY:
                order_type = "STOP_LOSS"
            elif order.side == OrderSide.SELL:
                order_type = "TAKE_PROFIT"
            params["stopPrice"] = str(order.price)

        self._log.debug(f"Submitted {order}.")
        # Generate event here to ensure it is processed before OrderAccepted
        self._generate_order_submitted(
            cl_ord_id=order.cl_ord_id,
            timestamp=self._clock.utc_now_c(),
        )

        try:
            # Submit order and await response
            await self._client.create_order(
                symbol=order.symbol.code,
                type=order_type,
                side=OrderSideParser.to_str(order.side),
                amount=str(order.quantity),
                price=str(order.price) if isinstance(order, PassiveOrder) else None,
                params=params,
            )
        except CCXTError as ex:
            self._generate_order_rejected(
                cl_ord_id=order.cl_ord_id,
                reason=str(ex),
                timestamp=self._clock.utc_now_c(),
            )

# -- EVENTS ----------------------------------------------------------------------------------------

    cdef void _on_order_status(self, dict event) except *:
        event_info = event["info"]
        event_info["symbol"] = event["symbol"]
        event_info["timestamp"] = event["timestamp"]
        cdef OrderId order_id = OrderId(str(event_info["i"]))
        cdef datetime timestamp = from_posix_ms(event_info["E"])  # Event time (generic for now)
        cdef str exec_type = event_info["x"]
        if exec_type == "NEW":
            cl_ord_id = ClientOrderId(event_info["c"])  # ClientOrderId
            self._generate_order_accepted(cl_ord_id, order_id, timestamp)
        elif exec_type == "CANCELED":
            cl_ord_id = ClientOrderId(event_info["C"])  # Original ClientOrderId
            self._generate_order_cancelled(cl_ord_id, order_id, timestamp)
        elif exec_type == "EXPIRED":
            cl_ord_id = ClientOrderId(event_info["c"])  # ClientOrderId
            self._generate_order_expired(cl_ord_id, order_id, timestamp)

    cdef void _on_exec_report(self, dict event) except *:
        event_info = event["info"]
        event_info["symbol"] = event["symbol"]
        event_info["timestamp"] = event["timestamp"]
        cdef str exec_type = event_info["x"]
        if exec_type == "TRADE":
            fill_qty = Decimal(event_info["l"])
            cum_qty = Decimal(event_info["z"])
            leaves_qty = Decimal(event_info["q"]) - cum_qty
            self._generate_order_filled(
                cl_ord_id=ClientOrderId(event_info["c"]),
                order_id=OrderId(str(event_info["i"])),
                execution_id=ExecutionId(str(event_info["t"])),
                symbol=Symbol(event_info["symbol"], self.venue),
                order_side=OrderSideParser.from_str(event_info["S"]),
                fill_qty=fill_qty,
                cum_qty=cum_qty,
                leaves_qty=leaves_qty,
                avg_px=Decimal(str(event_info["L"])),
                commission_amount=Decimal(event_info["n"]),
                commission_currency=event_info["N"],
                liquidity_side=LiquiditySide.TAKER,
                timestamp=from_posix_ms(event_info["T"])
            )
