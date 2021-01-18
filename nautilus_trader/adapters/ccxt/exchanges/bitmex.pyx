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
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.order cimport Order


cdef class BitmexOrderBuilder:

    @staticmethod
    cdef tuple build(Order order):
        """
        Build the CCXT arguments and custom parameters to create the given order.

        Parameters
        ----------
        order : Order
            The order to build.

        Returns
        -------
        list[object], dict[str, object]
            The arguments and custom parameters.

        """
        Condition.not_none(order, "order")

        if order.time_in_force == TimeInForce.GTD:
            raise ValueError("GTD not supported in this version.")

        cdef str order_side = OrderSideParser.to_str(order.side).capitalize()
        cdef str order_qty = str(order.quantity)

        # Build args and custom params
        cdef list args = [order.symbol.code]
        cdef dict custom_params = {
            "clOrdID": order.cl_ord_id.value,
        }

        cdef str exec_inst = None
        if order.type == OrderType.MARKET:
            args.append("Market")
            args.append(order_side)
            args.append(order_qty)
        elif order.type == OrderType.LIMIT:
            args.append("Limit")
            args.append(order_side)
            args.append(order_qty)
            args.append(str(order.price))

            # Execution instructions
            if order.is_post_only:
                exec_inst = "ParticipateDoNotInitiate"
            elif order.is_hidden:
                custom_params["displayQty"] = 0
            if order.is_reduce_only:
                if exec_inst:
                    exec_inst += ",ReduceOnly"
                else:
                    exec_inst = "ReduceOnly"

            if exec_inst:
                custom_params["execInst"] = exec_inst

        elif order.type == OrderType.STOP_MARKET:
            args.append("StopMarket")
            args.append(order_side)
            args.append(order_qty)
            custom_params["stopPx"] = str(order.price)
            if order.is_reduce_only:
                custom_params["execInst"] = "ReduceOnly"

        if order.time_in_force == TimeInForce.DAY:
            custom_params["timeInForce"] = "Day"
        elif order.time_in_force == TimeInForce.GTC:
            custom_params["timeInForce"] = "GoodTillCancel"
        elif order.time_in_force == TimeInForce.IOC:
            custom_params["timeInForce"] = "ImmediateOrCancel"
        elif order.time_in_force == TimeInForce.FOK:
            custom_params["timeInForce"] = "FillOrKill"

        return args, custom_params

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
        return BitmexOrderBuilder.build(order)
