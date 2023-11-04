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

from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price


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


def parse_option_kind(value: str) -> OptionKind:
    match value:
        case "C":
            return OptionKind.CALL
        case "P":
            return OptionKind.PUT
        case _:
            raise ValueError(f"Invalid `OptionKind`, was {value}")


def parse_min_price_increment(value: int, currency: Currency) -> Price:
    match value:
        case 0 | 9223372036854775807:  # 2**63-1 (TODO: Make limit constants)
            return Price(10 ** (-currency.precision), currency.precision)
        case _:
            return Price.from_raw(value, currency.precision)
