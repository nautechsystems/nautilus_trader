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

from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.model.c_enums.order_side cimport OrderSide


uuid_factory = UUIDFactory()


cdef class Order:
    def __init__(self, double price, double volume, OrderSide side, str id=None):
        self.price = price
        self.volume = volume
        self.side = side
        self.id = id or str(uuid_factory.generate())

    cpdef void update_price(self, double price) except *:
        self.price = price

    cpdef void update_volume(self, double volume) except *:
        self.volume = volume

    def __eq__(self, other):
        return self.id == other.id

    def __repr__(self):
        return f"Order({self.price}, {self.volume}, {self.side}, {self.id})"
