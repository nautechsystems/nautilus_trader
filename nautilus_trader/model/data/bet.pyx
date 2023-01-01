# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport order_side_from_str
from nautilus_trader.model.enums_c cimport order_side_to_str
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Bet:
    """
    Represents a bet "order" or "trade" in price space (not probability).

    Parameters
    ----------
    price : Price or decimal.Decimal
        The price of the bet.
    quantity : Quantity
        The size of the bet.
    side : OrderSide {``BUY``, ``SELL``}
        The side ( OrderSide.BUY = BACK, OrderSide.SELL = LAY ) of the bet.

    Raises
    ------
    ValueError
        If `price` is less than 1.0.
    """

    def __init__(
        self,
        object price,
        Quantity quantity,
        OrderSide side,
    ):
        Condition.in_range_int(price, 1, 1000, "price")

        self.price = price
        self.quantity = quantity
        self.side = side

    def __eq__(self, Bet other) -> bool:
        return Bet.to_dict_c(self) == Bet.to_dict_c(other)

    def __hash__(self) -> int:
        return hash(frozenset(Bet.to_dict_c(self)))

    def __str__(self) -> str:
        return f"{self.side},{self.price},{self.quantity}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cpdef stake(self):
        return self.quantity * (self.price - 1)

    cpdef cost(self):
        return self.quantity

    cpdef liability(self):
        if self.side == OrderSide.BUY:
            return self.cost()
        elif self.side == OrderSide.SELL:
            return self.stake()

    cpdef win_payoff(self):
        if self.side == OrderSide.BUY:
            return self.stake()
        elif self.side == OrderSide.SELL:
            return -self.stake()

    cpdef lose_payoff(self):
        if self.side == OrderSide.BUY:
            return -self.cost()
        elif self.side == OrderSide.SELL:
            return self.cost()

    cpdef exposure(self):
        return self.win_payoff() - self.lose_payoff()

    @staticmethod
    cdef Bet from_dict_c(dict values):
        Condition.not_none(values, "values")
        return Bet(
            price=Price.from_str_c(values["price"]),
            quantity=Quantity.from_str_c(values["quantity"]),
            side=order_side_from_str(values['side'])
        )

    @staticmethod
    cdef dict to_dict_c(Bet obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "price": str(obj.price),
            "quantity": str(obj.quantity),
            "side": order_side_to_str(obj.side),
        }

    @staticmethod
    def from_dict(dict values) -> Bet:
        """
        Return a Bet parsed from the given values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        Bet

        """
        return Bet.from_dict_c(values)

    @staticmethod
    def to_dict(Bet obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return Bet.to_dict_c(obj)


cpdef Bet nautilus_to_bet(Price price, Quantity quantity, OrderSide side):
    """
    Nautilus considers orders/trades in probability space; convert back to betting prices/quantities.
    """
    bet_price: Decimal = Decimal(1) / price
    return Bet(
        price=bet_price,
        quantity=quantity,
        side=side
    )
