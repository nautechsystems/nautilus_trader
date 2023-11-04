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

from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide


def parse_order_side(value: str) -> OrderSide:
    match value:
        case "A":
            return OrderSide.BUY
        case "B":
            return OrderSide.SELL
        case _:
            return OrderSide.NO_ORDER_SIDE


def parse_aggressor_side(value: str) -> AggressorSide:
    match value:
        case "A":
            return AggressorSide.BUYER
        case "B":
            return AggressorSide.SELLER
        case _:
            return AggressorSide.NO_AGGRESSOR


def parse_book_action(value: str) -> BookAction:
    match value:
        case "A":
            return BookAction.ADD
        case "C":
            return BookAction.DELETE
        case "M":
            return BookAction.UPDATE
        case "R":
            return BookAction.CLEAR
        case "T":
            return BookAction.UPDATE
        case "F":
            return BookAction.UPDATE
        case _:
            raise ValueError(f"Invalid `BookAction`, was {value}")
