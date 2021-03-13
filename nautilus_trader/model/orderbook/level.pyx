#
#
# import copy
# import operator
# from typing import Optional
#
# from model.order.base import Order
# import copy
# import operator
# from typing import Optional
#
# from model.order.base import Order
#
# #TODO - List of classes, could use structured arrays? https://docs.scipy.org/doc/numpy-1.13.0/user/basics.rec.html
#
# cpdef class Level:
#     cdef readonly list[:] orders
#
#     def insert(self, order: Order, priority=None):
#         """
#         Insert an order into this level of the Ladder/Orderbook
#         :param order: `Order` to insert
#         :param priority: Priority to insert into level queue
#         :return:
#         """
#         priority = priority or len(self.orders)
#         assert order.price == self.price
#         self.orders.insert(priority, order)
#
#     def update(self, volume: float = None, order_id: str = None):
#         """
#         Update a order or the volume on this level.
#
#         If `order` is None, use the order_id to update ONLY the volume of the order.
#         Only applicable for exchanges that send level updates only
#         :param volume: New volume
#         :param order_id: New order to update
#         :return:
#         """
#         if order_id is None:
#             self._update_level_volume(volume=volume)
#         else:
#             self._update_order_volume(order_id=order_id, volume=volume)
#
#     def _update_level_volume(self, volume: float):
#         assert len(self.orders) == 1
#         if volume != 0:
#             new_order = copy.copy(self.orders[0])
#             new_order.volume = volume
#             self.orders = [new_order]
#         else:
#             self.orders = []
#
#     def _update_order_volume(self, order_id: str, volume: float):
#         priority = [order.order_id for order in self.orders].index(order_id)
#         new_order = copy.copy(self.order_id_orders[order_id])
#         new_order.volume = volume
#         self.orders[priority] = new_order
#         self.order_id_orders[order_id] = new_order
#
#     def delete(self, order: Optional[Order] = None, order_id: str = None):
#         """
#         :param order: Order to search for deletion
#         :param order_id: Order Id to delete
#         :return:
#         """
#         assert_one([order, order_id], "Must pass `order` or `order_id`")
#         if order is not None:
#             return self._delete_order(order=order)
#         elif order_id is not None:
#             return self._delete_order_by_id(order_id=order_id)
#
#     def _delete_order_by_id(self, order_id: str):
#         order_idx = [order.order_id for order in self.orders].index(order_id)
#         del self.order_id_orders[order_id]
#         return self.orders.pop(order_idx)
#
#     def _delete_order(self, order):
#         """
#         Delete the volume from `order` from this level
#         :param order: Order to delete / fill
#         :return:
#         """
#         traded_orders = []
#         volume = order.volume
#         while volume > 0:
#             passive_order = self.orders.pop(0)
#             del self.order_id_orders[passive_order.order_id]
#             if volume < passive_order.volume:
#                 traded = passive_order.copy()
#                 passive_order.volume = passive_order.volume - volume
#                 self.insert(priority=0, order=passive_order)
#                 traded_orders.append(traded)
#                 break
#             elif volume == passive_order.volume:
#                 traded_orders.append(passive_order)
#                 break
#             else:
#                 traded_orders.append(passive_order)
#                 volume -= passive_order.volume
#         return traded_orders
#
#     @property
#     def volume(self):
#         return sum(map(operator.attrgetter("volume"), self.orders))
#
#     @property
#     def exposure(self):
#         return sum(map(operator.attrgetter("exposure"), self.orders))
#
#     def iter_orders(self):
#         return iter(self.orders)
#
#     def __eq__(self, other):
#         return (
#                 type(self) == type(other)
#                 and self.side == other.side
#                 and self.price == other.price
#                 and self.volume == other.volume
#         )
#
#     def __ge__(self, other):
#         return self.price > other.price
#
#     def dict(self, **kwargs):
#         kwargs.update(dict(exclude={"order_id_orders"}))
#         return super().dict(**kwargs)
