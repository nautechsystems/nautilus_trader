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

from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.order cimport Order


cdef class BinanceSubmitOrderBuilder:

    @staticmethod
    cdef tuple build(Order order):
        cdef str order_side = OrderSideParser.to_str(order.side).capitalize()
        cdef str order_qty = str(order.quantity)

        # Build args and custom params
        cdef list args = [order.symbol.code]
        cdef dict custom_params = {
            "newClientOrderId": order.cl_ord_id.value,
            "recvWindow": 10000  # TODO: Server time sync issue?
        }

        if order.type == OrderType.MARKET:
            args.append("MARKET")
            args.append(order_side)
            args.append(order_qty)
        elif order.type == OrderType.LIMIT and order.is_post_only:
            args.append("LIMIT_MAKER")
            args.append(order_side)
            args.append(order_qty)
            args.append(str(order.price))
        elif order.type == OrderType.LIMIT:
            args.append("LIMIT")
            args.append(order_side)
            args.append(order_qty)
            args.append(str(order.price))
            # TimeInForce
            # -----------
            if order.time_in_force == TimeInForce.DAY:
                pass
            elif order.time_in_force == TimeInForce.GTD:
                pass
            else:
                custom_params["timeInForce"] = TimeInForceParser.to_str(order.time_in_force)
        elif order.type == OrderType.STOP_MARKET:
            if order.side == OrderSide.BUY:
                args.append("STOP_LOSS")
            elif order.side == OrderSide.SELL:
                args.append("TAKE_PROFIT")
            args.append(order_side)
            args.append(order_qty)
            args.append(str(order.price))
            # TimeInForce
            # -----------
            if order.time_in_force == TimeInForce.DAY:
                pass
            elif order.time_in_force == TimeInForce.GTD:
                pass

        return args, custom_params
