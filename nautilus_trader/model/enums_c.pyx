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

from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport AggregationSource
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport AssetClass
from nautilus_trader.core.rust.model cimport AssetType
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport CurrencyType
from nautilus_trader.core.rust.model cimport DepthType
from nautilus_trader.core.rust.model cimport InstrumentCloseType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport MarketStatus
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport TradingState
from nautilus_trader.core.rust.model cimport TrailingOffsetType
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.core.rust.model cimport account_type_from_cstr
from nautilus_trader.core.rust.model cimport account_type_to_cstr
from nautilus_trader.core.rust.model cimport aggregation_source_from_cstr
from nautilus_trader.core.rust.model cimport aggregation_source_to_cstr
from nautilus_trader.core.rust.model cimport aggressor_side_from_cstr
from nautilus_trader.core.rust.model cimport aggressor_side_to_cstr
from nautilus_trader.core.rust.model cimport asset_class_from_cstr
from nautilus_trader.core.rust.model cimport asset_class_to_cstr
from nautilus_trader.core.rust.model cimport asset_type_from_cstr
from nautilus_trader.core.rust.model cimport asset_type_to_cstr
from nautilus_trader.core.rust.model cimport bar_aggregation_from_cstr
from nautilus_trader.core.rust.model cimport bar_aggregation_to_cstr
from nautilus_trader.core.rust.model cimport book_action_from_cstr
from nautilus_trader.core.rust.model cimport book_action_to_cstr
from nautilus_trader.core.rust.model cimport book_type_from_cstr
from nautilus_trader.core.rust.model cimport book_type_to_cstr
from nautilus_trader.core.rust.model cimport contingency_type_from_cstr
from nautilus_trader.core.rust.model cimport contingency_type_to_cstr
from nautilus_trader.core.rust.model cimport currency_type_from_cstr
from nautilus_trader.core.rust.model cimport currency_type_to_cstr
from nautilus_trader.core.rust.model cimport depth_type_from_cstr
from nautilus_trader.core.rust.model cimport depth_type_to_cstr
from nautilus_trader.core.rust.model cimport instrument_close_type_from_cstr
from nautilus_trader.core.rust.model cimport instrument_close_type_to_cstr
from nautilus_trader.core.rust.model cimport liquidity_side_from_cstr
from nautilus_trader.core.rust.model cimport liquidity_side_to_cstr
from nautilus_trader.core.rust.model cimport market_status_from_cstr
from nautilus_trader.core.rust.model cimport market_status_to_cstr
from nautilus_trader.core.rust.model cimport oms_type_from_cstr
from nautilus_trader.core.rust.model cimport oms_type_to_cstr
from nautilus_trader.core.rust.model cimport option_kind_from_cstr
from nautilus_trader.core.rust.model cimport option_kind_to_cstr
from nautilus_trader.core.rust.model cimport order_side_from_cstr
from nautilus_trader.core.rust.model cimport order_side_to_cstr
from nautilus_trader.core.rust.model cimport order_status_from_cstr
from nautilus_trader.core.rust.model cimport order_status_to_cstr
from nautilus_trader.core.rust.model cimport order_type_from_cstr
from nautilus_trader.core.rust.model cimport order_type_to_cstr
from nautilus_trader.core.rust.model cimport position_side_from_cstr
from nautilus_trader.core.rust.model cimport position_side_to_cstr
from nautilus_trader.core.rust.model cimport price_type_from_cstr
from nautilus_trader.core.rust.model cimport price_type_to_cstr
from nautilus_trader.core.rust.model cimport time_in_force_from_cstr
from nautilus_trader.core.rust.model cimport time_in_force_to_cstr
from nautilus_trader.core.rust.model cimport trading_state_from_cstr
from nautilus_trader.core.rust.model cimport trading_state_to_cstr
from nautilus_trader.core.rust.model cimport trailing_offset_type_from_cstr
from nautilus_trader.core.rust.model cimport trailing_offset_type_to_cstr
from nautilus_trader.core.rust.model cimport trigger_type_from_cstr
from nautilus_trader.core.rust.model cimport trigger_type_to_cstr
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.model.data.bar_aggregation cimport BarAggregation


cpdef AccountType account_type_from_str(str value) except *:
    return account_type_from_cstr(pystr_to_cstr(value))


cpdef str account_type_to_str(AccountType value):
    return cstr_to_pystr(account_type_to_cstr(value))


cpdef AggregationSource aggregation_source_from_str(str value) except *:
    return aggregation_source_from_cstr(pystr_to_cstr(value))


cpdef str aggregation_source_to_str(AggregationSource value):
    return cstr_to_pystr(aggregation_source_to_cstr(value))


cpdef AggressorSide aggressor_side_from_str(str value) except *:
    return aggressor_side_from_cstr(pystr_to_cstr(value))


cpdef str aggressor_side_to_str(AggressorSide value):
    return cstr_to_pystr(aggressor_side_to_cstr(value))


cpdef AssetClass asset_class_from_str(str value) except *:
    return asset_class_from_cstr(pystr_to_cstr(value))


cpdef str asset_class_to_str(AssetClass value):
    return cstr_to_pystr(asset_class_to_cstr(value))


cpdef AssetType asset_type_from_str(str value) except *:
    return asset_type_from_cstr(pystr_to_cstr(value))


cpdef str asset_type_to_str(AssetType value):
    return cstr_to_pystr(asset_type_to_cstr(value))


cpdef BarAggregation bar_aggregation_from_str(str value) except *:
    return <BarAggregation>bar_aggregation_from_cstr(pystr_to_cstr(value))


cpdef str bar_aggregation_to_str(BarAggregation value):
    return cstr_to_pystr(bar_aggregation_to_cstr(<uint8_t>value))


cpdef BookAction book_action_from_str(str value) except *:
    return book_action_from_cstr(pystr_to_cstr(value))


cpdef str book_action_to_str(BookAction value):
    return cstr_to_pystr(book_action_to_cstr(value))


cpdef BookType book_type_from_str(str value) except *:
    return book_type_from_cstr(pystr_to_cstr(value))


cpdef str book_type_to_str(BookType value):
    return cstr_to_pystr(book_type_to_cstr(value))


cpdef ContingencyType contingency_type_from_str(str value) except *:
    return contingency_type_from_cstr(pystr_to_cstr(value))


cpdef str contingency_type_to_str(ContingencyType value):
    return cstr_to_pystr(contingency_type_to_cstr(value))


cpdef CurrencyType currency_type_from_str(str value) except *:
    return currency_type_from_cstr(pystr_to_cstr(value))


cpdef str currency_type_to_str(CurrencyType value):
    return cstr_to_pystr(currency_type_to_cstr(value))


cpdef DepthType depth_type_from_str(str value) except *:
    return depth_type_from_cstr(pystr_to_cstr(value))


cpdef str depth_type_to_str(DepthType value):
    return cstr_to_pystr(depth_type_to_cstr(value))


cpdef InstrumentCloseType instrument_close_type_from_str(str value) except *:
    return instrument_close_type_from_cstr(pystr_to_cstr(value))


cpdef str instrument_close_type_to_str(InstrumentCloseType value):
    return cstr_to_pystr(instrument_close_type_to_cstr(value))


cpdef LiquiditySide liquidity_side_from_str(str value) except *:
    return liquidity_side_from_cstr(pystr_to_cstr(value))


cpdef str liquidity_side_to_str(LiquiditySide value):
    return cstr_to_pystr(liquidity_side_to_cstr(value))


cpdef MarketStatus market_status_from_str(str value) except *:
    return market_status_from_cstr(pystr_to_cstr(value))


cpdef str market_status_to_str(MarketStatus value):
    return cstr_to_pystr(market_status_to_cstr(value))


cpdef OmsType oms_type_from_str(str value) except *:
    return oms_type_from_cstr(pystr_to_cstr(value))


cpdef str oms_type_to_str(OmsType value):
    return cstr_to_pystr(oms_type_to_cstr(value))


cpdef OptionKind option_kind_from_str(str value) except *:
    return option_kind_from_cstr(pystr_to_cstr(value))


cpdef str option_kind_to_str(OptionKind value):
    return cstr_to_pystr(option_kind_to_cstr(value))


cpdef OrderSide order_side_from_str(str value) except *:
    return order_side_from_cstr(pystr_to_cstr(value))


cpdef str order_side_to_str(OrderSide value):
    return cstr_to_pystr(order_side_to_cstr(value))


cpdef OrderStatus order_status_from_str(str value) except *:
    return order_status_from_cstr(pystr_to_cstr(value))


cpdef str order_status_to_str(OrderStatus value):
    return cstr_to_pystr(order_status_to_cstr(value))


cpdef OrderType order_type_from_str(str value) except *:
    return order_type_from_cstr(pystr_to_cstr(value))


cpdef str order_type_to_str(OrderType value):
    return cstr_to_pystr(order_type_to_cstr(value))


cpdef PositionSide position_side_from_str(str value) except *:
    return position_side_from_cstr(pystr_to_cstr(value))


cpdef str position_side_to_str(PositionSide value):
    return cstr_to_pystr(position_side_to_cstr(value))


cpdef PriceType price_type_from_str(str value) except *:
    return price_type_from_cstr(pystr_to_cstr(value))


cpdef str price_type_to_str(PriceType value):
    return cstr_to_pystr(price_type_to_cstr(value))


cpdef TimeInForce time_in_force_from_str(str value) except *:
    return time_in_force_from_cstr(pystr_to_cstr(value))


cpdef str time_in_force_to_str(TimeInForce value):
    return cstr_to_pystr(time_in_force_to_cstr(value))


cpdef TradingState trading_state_from_str(str value) except *:
    return trading_state_from_cstr(pystr_to_cstr(value))


cpdef str trading_state_to_str(TradingState value):
    return cstr_to_pystr(trading_state_to_cstr(value))


cpdef TrailingOffsetType trailing_offset_type_from_str(str value) except *:
    return trailing_offset_type_from_cstr(pystr_to_cstr(value))


cpdef str trailing_offset_type_to_str(TrailingOffsetType value):
    return cstr_to_pystr(trailing_offset_type_to_cstr(value))


cpdef TriggerType trigger_type_from_str(str value) except *:
    return trigger_type_from_cstr(pystr_to_cstr(value))


cpdef str trigger_type_to_str(TriggerType value):
    return cstr_to_pystr(trigger_type_to_cstr(value))
