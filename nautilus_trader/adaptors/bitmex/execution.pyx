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

from nautilus_trader.adaptors.ccxt.execution cimport CCXTExecutionClient
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.order.base cimport PassiveOrder


cdef class BitmexExecutionClient(CCXTExecutionClient):
    """
    Provides an execution client for the Bitmex exchange.
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
        Initialize a new instance of the `BitmexExecutionClient` class.

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
        Condition.true(client.name.upper() == "BITMEX", "client.name != BITMEX")

        super().__init__(
            client,
            account_id,
            engine,
            clock,
            logger,
        )

# -- COMMANDS --------------------------------------------------------------------------------------

    async def _submit_order(self, Order order):
        if order.time_in_force == TimeInForce.GTD:
            raise ValueError("GTD not supported in this version.")

        cdef dict params = {
            "clOrdID": order.cl_ord_id.value,
        }

        cdef str order_type
        cdef list exec_instructions = []
        if order.type == OrderType.MARKET:
            order_type = "Market"
        elif order.type == OrderType.LIMIT:
            order_type = "Limit"
            if order.is_hidden:
                params["displayQty"] = 0
            # Execution instructions
            if order.is_post_only:
                exec_instructions.append("ParticipateDoNotInitiate")
            if order.is_reduce_only:
                exec_instructions.append("ReduceOnly")
            if exec_instructions:
                params["execInst"] = ','.join(exec_instructions)
        elif order.type == OrderType.STOP_MARKET:
            order_type = "StopMarket"
            params["stopPx"] = str(order.price)
            if order.is_reduce_only:
                params["execInst"] = "ReduceOnly"

        if order.time_in_force == TimeInForce.DAY:
            params["timeInForce"] = "Day"
        elif order.time_in_force == TimeInForce.GTC:
            params["timeInForce"] = "GoodTillCancel"
        elif order.time_in_force == TimeInForce.IOC:
            params["timeInForce"] = "ImmediateOrCancel"
        elif order.time_in_force == TimeInForce.FOK:
            params["timeInForce"] = "FillOrKill"

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
                side=OrderSideParser.to_str(order.side).capitalize(),
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
