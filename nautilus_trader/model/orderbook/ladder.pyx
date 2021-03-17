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

from bisect import bisect
import logging

from nautilus_trader.model.orderbook.level import Level
from nautilus_trader.model.orderbook.order cimport Order


logger = logging.getLogger(__name__)


cdef class Ladder:
    def __init__(self, reverse):
        self.levels = []  # type: list[Level]
        self.reverse = reverse
        self.order_id_levels = {}

    cpdef void add(self, Order order) except *:
        # Level exists, add new order
        if order.price in self.prices:
            idx = tuple(self.prices).index(order.price)
            level = self.levels[idx]
            level.add(order=order)
        # New price, create Level
        else:
            level = Level(orders=[order])
            self.levels.insert(bisect(self.levels, level), level)
        self.order_id_levels[order.id] = level

    cpdef void update(self, Order order) except *:
        if order.id not in self.order_id_levels:
            self.add(order=order)
            return
        # Find the existing order
        level = self.order_id_levels[order.id]
        if order.price == level.price:
            # This update contains a volume update
            level.update(order=order)
        else:
            # New price for this order, delete and insert
            self.delete(order=order)
            self.add(order=order)

    cpdef void delete(self, Order order) except *:
        level = self.order_id_levels[order.id]
        price_idx = self.levels.index(level)
        level.delete(order=order)
        del self.order_id_levels[order.id]
        if not level.orders:
            del self.levels[price_idx]

    cpdef list depth(self, int n=1):
        if not self.levels:
            return []
        n = n or len(self.levels)
        return list(reversed(self.levels[-n:])) if self.reverse else self.levels[:n]

    def _get_level(self, price):
        return self.levels[self.levels.index(Level(price))]

    @property
    def prices(self):
        return [level.price for level in self.levels]

    @property
    def volumes(self):
        return [level.volume for level in self.levels]

    @property
    def exposures(self):
        return [level.exposure for level in self.levels]

    @property
    def top(self):
        top = self.depth(1)
        if top:
            return top[0]
