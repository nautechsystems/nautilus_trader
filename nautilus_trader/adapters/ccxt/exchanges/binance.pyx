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
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.order cimport Order


cdef class BinanceOrderRequestBuilder:

    @staticmethod
    cdef dict build(Order order):
        """
        Build the CCXT request to create the given order for Binance.

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
            raise ValueError("TimeInForce.GTD not supported in this version.")

        if order.time_in_force == TimeInForce.DAY:
            raise ValueError("Binance does not support TimeInForce.DAY.")

        cdef dict params = {
            "newClientOrderId": order.cl_ord_id.value,
            "recvWindow": 10000  # TODO: Server time sync issue?
        }

        if order.type == OrderType.MARKET:
            params["type"] = "MARKET"
        elif order.type == OrderType.LIMIT and order.is_post_only:
            # Cannot be hidden as post only is True
            params["type"] = "LIMIT_MAKER"
        elif order.type == OrderType.LIMIT:
            if order.is_hidden:
                raise ValueError("Binance does not support hidden orders.")
            params["type"] = "LIMIT"
            params["timeInForce"] = TimeInForceParser.to_str(order.time_in_force)
        elif order.type == OrderType.STOP_MARKET:
            if order.side == OrderSide.BUY:
                params["type"] = "STOP_LOSS"
            elif order.side == OrderSide.SELL:
                params["type"] = "TAKE_PROFIT"
            params["stopPrice"] = str(order.price)

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
        return BinanceOrderRequestBuilder.build(order)


cdef class BinanceOrderFillParser:

    @staticmethod
    cdef dict parse(dict report):
        """
        Parse the information needed to generate an `OrderFilled` event from the
        given parameters.

        Parameters
        ----------
        report : dict[str, object]
            The execution report.

        Returns
        -------
        dict[str, object]
            The parsed information.

        """
        Condition.not_none(report, "report")

        return {
            "exec_id": str(report["t"]),         # Execution id
            "symbol": report["symbol"],
            "fill_qty": report["l"],             # Last executed quantity
            "cum_qty": report["z"],              # Cumulative filled quantity
            "avg_px": report["L"],               # Last executed price
            "liquidity_side": LiquiditySide.TAKER,  # TODO: Implement
            "commission": report["n"],           # Commission amount
            "commission_currency": report["N"],  # Commission asset
            "timestamp": report["T"],            # Transaction time
        }

    @staticmethod
    def parse_py(dict report):
        """
        Parse the information needed to generate an order filled event from the
        given parameters.

        Parameters
        ----------
        report : dict[str, object]
            The execution report.

        Returns
        -------
        OrderFilled

        """
        return BinanceOrderFillParser.parse(report)
