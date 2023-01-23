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

from libc.stdint cimport uint8_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.enums_c cimport DepthType
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.data cimport BookOrder
from nautilus_trader.model.orderbook.level cimport Level


cdef class Ladder:
    """
    Represents a ladder of price levels in a book.

    A ladder is on one side of the book, either bid or ask/offer.

    Parameters
    ----------
    reverse : bool
        If the ladder should be represented in reverse order of price (bids).
    price_precision : uint8
        The price precision of the books orders.
    size_precision : uint8
        The size precision of the books orders.

    Raises
    ------
    OverflowError
        If `price_precision` is negative (< 0).
    OverflowError
        If `size_precision` is negative (< 0).
    """

    def __init__(
        self,
        bint reverse,
        uint8_t price_precision,
        uint8_t size_precision,
    ):
        Condition.not_negative_int(price_precision, "price_precision")
        Condition.not_negative_int(size_precision, "size_precision")

        self._order_id_level_index: dict[str, Level] = {}

        self.levels: list[Level] = []
        self.is_reversed = reverse
        self.price_precision = price_precision
        self.size_precision = size_precision

    def __repr__(self) -> str:
        return f"{Ladder.__name__}({self.levels})"

    cpdef void add(self, BookOrder order) except *:
        """
        Add the given order to the ladder.

        Parameters
        ----------
        order : BookOrder
            The order to add.

        """
        Condition.not_none(order, "order")

        cdef list existing_prices = self.prices()

        cdef int price_idx
        cdef Level level
        if order.price in existing_prices:
            # Level exists, add new order
            price_idx = existing_prices.index(order.price)
            level = self.levels[price_idx]
            level.add(order=order)
        else:
            # New price, create Level
            level = Level(price=order.price)
            level.add(order)

            if self.is_reversed:
                # TODO(cs): Temporary sorting strategy to fix #894
                self.levels.append(level)
                self.levels = list(reversed(sorted(self.levels)))
            else:
                price_idx = bisect_right(existing_prices, level.price)
                self.levels.insert(price_idx, level)

        self._order_id_level_index[order.order_id] = level

    cpdef void update(self, BookOrder order) except *:
        """
        Update the given order in the ladder.

        Parameters
        ----------
        order : BookOrder
            The order to add.

        """
        Condition.not_none(order, "order")

        if order.order_id not in self._order_id_level_index:
            self.add(order=order)
            return

        # Find the existing order
        cdef Level level = self._order_id_level_index[order.order_id]
        if order.price == level.price:
            # This update contains a volume update
            level.update(order=order)
            if not level.orders:
                self.levels.remove(level)
        else:
            # New price for this order, delete and insert
            self.delete(order=order)
            self.add(order=order)

    cpdef void delete(self, BookOrder order) except *:
        """
        Delete the given order in the ladder.

        Parameters
        ----------
        order : BookOrder

        Raises
        ------
        KeyError
            If `order.order_id` is not contained in the order ID level index.

        """
        Condition.not_none(order, "order")

        cdef Level level = self._order_id_level_index.get(order.order_id)
        if level is None:
            return
            # TODO: raise KeyError("Cannot delete order: not found at level.")
        cdef int price_idx = self.prices().index(level.price)
        level.delete(order=order)
        self._order_id_level_index.pop(order.order_id)
        if not level.orders:
            del self.levels[price_idx]

    cpdef list depth(self, int n=1):
        """
        Return the levels in the ladder to the given depth.

        Parameters
        ----------
        n : int
            The maximum level to query.

        Returns
        -------
        list[Level]

        """
        if not self.levels:
            return []
        n = n or len(self.levels)
        return self.levels[:n]

    cpdef list prices(self):
        """
        The prices in the ladder.

        Returns
        -------
        list[double]

        """
        return [level.price for level in self.levels]

    cpdef list volumes(self):
        """
        The volumes in the ladder.

        Returns
        -------
        list[double]

        """
        return [level.volume() for level in self.levels]

    cpdef list exposures(self):
        """
        The exposures in the ladder.

        Returns
        -------
        list[double]

        """
        return [level.exposure() for level in self.levels]

    cpdef Level top(self):
        """
        The top `Level` in the ladder.

        Returns
        -------
        Level or ``None``

        """
        cdef list top = self.depth(1)
        if top:
            return top[0]
        else:
            return None

    cpdef list simulate_order_fills(self, BookOrder order, DepthType depth_type=DepthType.VOLUME):
        """
        Return a simulation of where this order would be filled in the ladder.

        Parameters
        ----------
        order : BookOrder
            The order to simulate.
        depth_type : DepthType
            The depth type to simulate.

        Returns
        -------
        list[(Price, Quantity)]

        """
        Condition.not_none(order, "order")

        cdef list fills = []
        cdef double cumulative_denominator = 0.0
        cdef double current = 0.0
        cdef double target = order.size if depth_type == DepthType.VOLUME else order.price * order.size
        cdef double remainder = 0.0

        cdef Level level
        cdef BookOrder book_order
        for level in self.levels:
            if self.is_reversed and level.price < order.price:
                break
            elif not self.is_reversed and level.price > order.price:
                break
            for book_order in level.orders:
                current = book_order.size if depth_type == DepthType.VOLUME else book_order.exposure()
                if (cumulative_denominator + current) >= target:
                    # This order has filled us, calc and return
                    remainder = target - cumulative_denominator
                    fills.append((
                        Price(book_order.price, precision=self.price_precision),
                        Quantity(remainder, precision=self.size_precision),
                    ))
                    cumulative_denominator += remainder
                    return fills
                else:
                    # Add this order and continue
                    fills.append((
                        Price(book_order.price, precision=self.price_precision),
                        Quantity(current, precision=self.size_precision),
                    ))
                    cumulative_denominator += current

        return fills
