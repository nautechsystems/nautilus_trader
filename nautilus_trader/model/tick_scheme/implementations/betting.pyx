# # -------------------------------------------------------------------------------------------------
# #  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
# #  https://nautechsystems.io
# #
# #  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
# #  You may not use this file except in compliance with the License.
# #  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# #
# #  Unless required by applicable law or agreed to in writing, software
# #  distributed under the License is distributed on an "AS IS" BASIS,
# #  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# #  See the License for the specific language governing permissions and
# #  limitations under the License.
# # -------------------------------------------------------------------------------------------------
# from nautilus_trader.model.c_enums.order_side import OrderSide
# from numpy cimport ndarray
#
# from nautilus_trader.model.objects cimport Price
#
# import numpy as np
#
# from nautilus_trader.core.correctness import Condition
#
# cdef class TickScheme:
#     """
#     Represents a instrument tick scheme, mapping the prices available for an instrument
#     """
#
#     def __init__(
#             self,
#             list mappings not None,
#             int price_precision,
#     ):
#         """
#         Initialize a new instance of the `Instrument` class.
#
#         Parameters
#         ----------
#         mappings : list
#             The instrument identifier for the instrument.
#         price_precision: int
#             The instrument price precision
#         """
#
#         self.mappings = self._validate_mapping(mappings)
#         self.price_precision = price_precision
#         self.ticks = self.build_ticks(mappings)
#         self.min_tick = self.ticks[0]
#         self.max_tick = self.ticks[-1]
#
#     cdef list _validate_mapping(self, list mappings):
#         for x in mappings:
#             assert len(x) == 3, "Mappings should be list of tuples like [(start, stop, increment), ...]"
#             start, stop, incr = x
#             assert start < stop, f"Start should be less than stop (start={start}, stop={stop})"
#             assert incr <= start and incr <= stop, f"Increment should be less than start and stop ({start}, {stop}, {incr})"
#         return mappings
#
#     @staticmethod
#     cdef ndarray build_ticks(self, list mappings):
#         """ Expand mappings in the full tick values """
#         cdef list ticks = []
#         for start, end, step in mappings:
#             ticks.extend([
#                 Price(value=x, precision=self.price_precision)
#                 for x in np.arange(start, end, step)
#             ])
#         return np.asarray(ticks)
#
#     cpdef Price next_ask_tick(self, Price price):
#         """
#         For a given price, return the next ask (higher) price on the ladder
#
#         :param price: The relative price
#         :return: Price
#         """
#         cdef int idx
#         if price >= self.max_tick:
#             return None
#         idx = self.ticks.searchsorted(price)
#         if price in self.ticks:
#             return self.ticks[idx + 1]
#         else:
#             return self.ticks[idx]
#
#     cpdef Price next_bid_tick(self, Price price):
#         """
#         For a given price, return the next bid (lower)price on the ladder
#
#         :param price: The relative price
#         :return: Price
#         """
#         cdef int idx
#         if price <= self.min_tick:
#             return None
#         idx = self.ticks.searchsorted(price)
#         if price in self.ticks:
#             return self.ticks[idx - 1]
#         else:
#             return self.ticks[idx - 1]
#
#     # cdef int nearest_tick(self, Price price, OrderSide side) except *:
#     #     cdef int idx
#     #     if not self.min_tick <= price <= self.max_tick:
#     #         return -1
#     #     if price in self.ticks:
#     #         return price
#     #     idx = self.ticks.searchsorted(price)
#     #
#     #
#     # cpdef Price nearest_ask_tick(self, Price price):
#     #
#     #     idx =
#     #         return self.ticks[idx - 1]
#     #
#     # cpdef Price nearest_bid_tick(self, Price price)
#
#
# TICK_SCHEMES = {}
#
# cpdef void register_tick_scheme(str name, tick_scheme: TickScheme):
#     Condition.not_in(name, TICK_SCHEMES, "name", "TICK_SCHEMES")
#     TICK_SCHEMES[name] = tick_scheme
