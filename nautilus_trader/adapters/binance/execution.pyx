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

import ccxt
from ccxt.base.errors import BaseError as CCXTError

from nautilus_trader.adapters.ccxt.execution cimport CCXTExecutionClient
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.identifiers cimport AccountId
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
                symbol=order.symbol.value,
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
