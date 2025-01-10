# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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


# def default_auction_match(left: Ladder, right: Ladder) -> tuple[list, list]:
#     """Match bid/ask Ladders as default auction match function."""
#     if not (left.top() and right.top()):
#         return [], []
#     bid_volume = volume_traded_at_price(left, right.top().price, side=OrderSide.BUY)
#     ask_volume = volume_traded_at_price(right, left.top().price, side=OrderSide.SELL)
#     matched_volume = min(bid_volume, ask_volume)
#
#     if matched_volume == 0:
#         return [], []
#
#     traded_bids = find_trades_for_volume(left, matched_volume)
#     traded_asks = find_trades_for_volume(right, matched_volume)
#
#     return traded_bids, traded_asks
#
#
# def valid_trade_price(order_price, side, target_price):
#     if side == OrderSide.BUY:
#         return target_price <= order_price
#     if side == OrderSide.SELL:
#         return target_price >= order_price
#     else:
#         raise RuntimeError(side)
#
#
# def volume_traded_at_price(ladder: Ladder, price: float, side: OrderSide) -> float:
#     """Determine the total volume available to trade in `ladder` up to a certain `price`."""
#     total_volume = 0.0
#     for level in ladder.levels:
#         if not valid_trade_price(order_price=level.price, side=side, target_price=price):
#             break
#         else:
#             total_volume += level.volume()
#     return total_volume
#
#
# def find_trades_for_volume(ladder: Ladder, target_volume: float) -> list[BookOrder]:
#     """Assuming `target_volume` size has traded, find all trades up to that volume."""
#     remaining_size = target_volume
#     orders: list[BookOrder] = []
#     for level in ladder.levels:
#         if (remaining_size - level.volume()) > 0:
#             # Add the whole level
#             orders.extend(level.orders)
#             remaining_size -= level.volume()
#         elif remaining_size == 0.0:
#             break
#         else:
#             # We're going to be fully filled somewhere on this level
#             for order in level.orders:
#                 order_volume = remaining_size - order.size
#                 if order_volume > 0:
#                     # Size remaining, add this order and continue
#                     orders.append(order)
#                     remaining_size -= order.size
#                 elif order_volume == 0.0:
#                     # Exactly this order volume remaining, add and break
#                     orders.append(order)
#                     remaining_size -= order.size
#                     break
#                 elif order_volume < 0:
#                     # Less than this whole order volume remaining, add a partial fill
#                     fill_volume = remaining_size
#                     partial_order = BookOrder(order.price, fill_volume, order.side, order.order_id)
#                     orders.append(partial_order)
#                     remaining_size -= partial_order.size
#                     break
#     return orders
