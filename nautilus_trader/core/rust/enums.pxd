# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.object cimport PyObject
from libc.stdint cimport uint8_t

from nautilus_trader.core.rust.c_enums cimport BarAggregation
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport AggregationSource
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport AssetClass
from nautilus_trader.core.rust.model cimport AssetType
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport CurrencyType
from nautilus_trader.core.rust.model cimport DepthType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport account_type_from_pystr
from nautilus_trader.core.rust.model cimport account_type_to_pystr
from nautilus_trader.core.rust.model cimport aggregation_source_from_pystr
from nautilus_trader.core.rust.model cimport aggregation_source_to_pystr
from nautilus_trader.core.rust.model cimport aggressor_side_from_pystr
from nautilus_trader.core.rust.model cimport aggressor_side_to_pystr
from nautilus_trader.core.rust.model cimport asset_class_from_pystr
from nautilus_trader.core.rust.model cimport asset_class_to_pystr
from nautilus_trader.core.rust.model cimport asset_type_from_pystr
from nautilus_trader.core.rust.model cimport asset_type_to_pystr
from nautilus_trader.core.rust.model cimport bar_aggregation_from_pystr
from nautilus_trader.core.rust.model cimport bar_aggregation_to_pystr
from nautilus_trader.core.rust.model cimport book_action_from_pystr
from nautilus_trader.core.rust.model cimport book_action_to_pystr
from nautilus_trader.core.rust.model cimport book_type_from_pystr
from nautilus_trader.core.rust.model cimport book_type_to_pystr
from nautilus_trader.core.rust.model cimport contingency_type_from_pystr
from nautilus_trader.core.rust.model cimport contingency_type_to_pystr
from nautilus_trader.core.rust.model cimport currency_type_from_pystr
from nautilus_trader.core.rust.model cimport currency_type_to_pystr
from nautilus_trader.core.rust.model cimport depth_type_from_pystr
from nautilus_trader.core.rust.model cimport depth_type_to_pystr
from nautilus_trader.core.rust.model cimport liquidity_side_from_pystr
from nautilus_trader.core.rust.model cimport liquidity_side_to_pystr
from nautilus_trader.core.rust.model cimport oms_type_from_pystr
from nautilus_trader.core.rust.model cimport oms_type_to_pystr
from nautilus_trader.core.rust.model cimport option_kind_to_pystr
from nautilus_trader.core.rust.model cimport option_kind_from_pystr
from nautilus_trader.core.rust.model cimport order_side_from_pystr
from nautilus_trader.core.rust.model cimport order_side_to_pystr
from nautilus_trader.core.rust.model cimport order_status_from_pystr
from nautilus_trader.core.rust.model cimport order_status_to_pystr
from nautilus_trader.core.rust.model cimport order_type_from_pystr
from nautilus_trader.core.rust.model cimport order_type_to_pystr
from nautilus_trader.core.string cimport pyobj_to_str


cpdef inline AccountType account_type_from_str(str value) except *:
    return account_type_from_pystr(<PyObject *>value)


cpdef inline str account_type_to_str(AccountType value):
    return pyobj_to_str(account_type_to_pystr(value))


cpdef inline AggregationSource aggregation_source_from_str(str value) except *:
    return aggregation_source_from_pystr(<PyObject *>value)


cpdef inline str aggregation_source_to_str(AggregationSource value):
    return pyobj_to_str(aggregation_source_to_pystr(value))


cpdef inline AggressorSide aggressor_side_from_str(str value) except *:
    return aggressor_side_from_pystr(<PyObject *>value)


cpdef inline str aggressor_side_to_str(AggressorSide value):
    return pyobj_to_str(aggressor_side_to_pystr(value))


cpdef inline AssetClass asset_class_from_str(str value) except *:
    return asset_class_from_pystr(<PyObject *>value)


cpdef inline str asset_class_to_str(AssetClass value):
    return pyobj_to_str(asset_class_to_pystr(value))


cpdef inline AssetType asset_type_from_str(str value) except *:
    return asset_type_from_pystr(<PyObject *>value)


cpdef inline str asset_type_to_str(AssetType value):
    return pyobj_to_str(asset_type_to_pystr(value))


cpdef inline BarAggregation bar_aggregation_from_str(str value) except *:
    return <BarAggregation>bar_aggregation_from_pystr(<PyObject *>value)


cpdef inline str bar_aggregation_to_str(uint8_t value):
    return pyobj_to_str(bar_aggregation_to_pystr(value))


cpdef inline BookAction book_action_from_str(str value) except *:
    return book_action_from_pystr(<PyObject *>value)


cpdef inline str book_action_to_str(BookAction value):
    return pyobj_to_str(book_action_to_pystr(value))


cpdef inline BookType book_type_from_str(str value) except *:
    return book_type_from_pystr(<PyObject *>value)


cpdef inline str book_type_to_str(BookType value):
    return pyobj_to_str(book_type_to_pystr(value))


cpdef inline ContingencyType contingency_type_from_str(str value) except *:
    return contingency_type_from_pystr(<PyObject *>value)


cpdef inline str contingency_type_to_str(ContingencyType value):
    return pyobj_to_str(contingency_type_to_pystr(value))


cpdef inline CurrencyType currency_type_from_str(str value) except *:
    return currency_type_from_pystr(<PyObject *>value)


cpdef inline str currency_type_to_str(CurrencyType value):
    return pyobj_to_str(currency_type_to_pystr(value))


cpdef inline DepthType depth_type_from_str(str value) except *:
    return depth_type_from_pystr(<PyObject *>value)


cpdef inline str depth_type_to_str(DepthType value):
    return pyobj_to_str(depth_type_to_pystr(value))


cpdef inline LiquiditySide liquidity_side_from_str(str value) except *:
    return liquidity_side_from_pystr(<PyObject *>value)


cpdef inline str liquidity_side_to_str(LiquiditySide value):
    return pyobj_to_str(liquidity_side_to_pystr(value))


cpdef inline OmsType oms_type_from_str(str value) except *:
    return oms_type_from_pystr(<PyObject *>value)


cpdef inline str oms_type_to_str(OmsType value):
    return pyobj_to_str(oms_type_to_pystr(value))


cpdef inline OptionKind option_kind_from_str(str value) except *:
    return option_kind_from_pystr(<PyObject *>value)


cpdef inline str option_kind_to_str(OptionKind value):
    return pyobj_to_str(option_kind_to_pystr(value))


cpdef inline OrderSide order_side_from_str(str value) except *:
    return order_side_from_pystr(<PyObject *>value)


cpdef inline str order_side_to_str(OrderSide value):
    return pyobj_to_str(order_side_to_pystr(value))


cpdef inline OrderStatus order_status_from_str(str value) except *:
    return order_status_from_pystr(<PyObject *>value)


cpdef inline str order_status_to_str(OrderStatus value):
    return pyobj_to_str(order_status_to_pystr(value))


cpdef inline OrderType order_type_from_str(str value) except *:
    return order_type_from_pystr(<PyObject *>value)


cpdef inline str order_type_to_str(OrderType value):
    return pyobj_to_str(order_type_to_pystr(value))
