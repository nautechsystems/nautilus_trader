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

from typing import Any
from typing import Final

import msgspec


BYBIT_PONG: Final[str] = "pong"


class BybitListResult[T](msgspec.Struct):
    list: list[T]


class BybitListResultWithCursor[T](BybitListResult[T]):
    nextPageCursor: str | None = None


def bybit_coin_result(object_type: Any):
    return msgspec.defstruct("", [("coin", list[object_type])])


class LeverageFilter(msgspec.Struct):
    # Minimum leverage
    minLeverage: str
    # Maximum leverage
    maxLeverage: str
    # The step to increase/reduce leverage
    leverageStep: str


class LinearPriceFilter(msgspec.Struct):
    # Minimum order price
    minPrice: str
    # Maximum order price
    maxPrice: str
    # The step to increase/reduce order price
    tickSize: str


class SpotPriceFilter(msgspec.Struct):
    tickSize: str


class SpotLotSizeFilter(msgspec.Struct):
    basePrecision: str
    quotePrecision: str
    minOrderQty: str
    maxOrderQty: str
    minOrderAmt: str
    maxOrderAmt: str


class LinearLotSizeFilter(msgspec.Struct):
    # Maximum order quantity
    maxOrderQty: str
    # Minimum order quantity
    minOrderQty: str
    # The step to increase/reduce order quantity
    qtyStep: str
    # Maximum order qty for PostOnly order
    postOnlyMaxOrderQty: str
    maxMktOrderQty: str
    minNotionalValue: str


class OptionLotSizeFilter(msgspec.Struct):
    # Maximum order quantity
    maxOrderQty: str
    # Minimum order quantity
    minOrderQty: str
    # The step to increase/reduce order quantity
    qtyStep: str
