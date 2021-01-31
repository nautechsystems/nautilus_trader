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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.order.base cimport Order


cdef class BitmexOrderRequestBuilder:

    @staticmethod
    cdef dict build(Order order):
        """
        Build the CCXT arguments and custom parameters to create the given order.

        Parameters
        ----------
        order : Order
            The order for the request.

        Returns
        -------
        dict[str, object]
            The arguments for the create order request.

        """
        Condition.not_none(order, "order")

        if order.time_in_force == TimeInForce.GTD:
            raise ValueError("GTD not supported in this version.")

        cdef dict params = {
            "clOrdID": order.cl_ord_id.value,
        }

        cdef list exec_instructions = []
        if order.type == OrderType.MARKET:
            params["type"] = "Market"
        elif order.type == OrderType.LIMIT:
            params["type"] = "Limit"
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
            params["type"] = "StopMarket"
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

        return params

    @staticmethod
    def build_py(Order order):
        """
        Build the CCXT arguments and custom parameters to create the given order.

        Wraps the `build` method for testing and access from pure Python. For
        best performance use the C access `build` method.

        Parameters
        ----------
        order : Order
            The order to build.

        Returns
        -------
        list[object], dict[str, object]
            The arguments and custom parameters.

        """
        return BitmexOrderRequestBuilder.build(order)
