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
from nautilus_trader.model.data.bar_aggregation cimport BarAggregation


cpdef AccountType account_type_from_str(str value) except *
cpdef str account_type_to_str(AccountType value)

cpdef AggregationSource aggregation_source_from_str(str value) except *
cpdef str aggregation_source_to_str(AggregationSource value)

cpdef AggressorSide aggressor_side_from_str(str value) except *
cpdef str aggressor_side_to_str(AggressorSide value)

cpdef AssetClass asset_class_from_str(str value) except *
cpdef str asset_class_to_str(AssetClass value)

cpdef AssetType asset_type_from_str(str value) except *
cpdef str asset_type_to_str(AssetType value)

cpdef BarAggregation bar_aggregation_from_str(str value) except *
cpdef str bar_aggregation_to_str(BarAggregation value)

cpdef BookAction book_action_from_str(str value) except *
cpdef str book_action_to_str(BookAction value)

cpdef BookType book_type_from_str(str value) except *
cpdef str book_type_to_str(BookType value)

cpdef ContingencyType contingency_type_from_str(str value) except *
cpdef str contingency_type_to_str(ContingencyType value)

cpdef CurrencyType currency_type_from_str(str value) except *
cpdef str currency_type_to_str(CurrencyType value)

cpdef DepthType depth_type_from_str(str value) except *
cpdef str depth_type_to_str(DepthType value)

cpdef InstrumentCloseType instrument_close_type_from_str(str value) except *
cpdef str instrument_close_type_to_str(InstrumentCloseType value)

cpdef LiquiditySide liquidity_side_from_str(str value) except *
cpdef str liquidity_side_to_str(LiquiditySide value)

cpdef MarketStatus market_status_from_str(str value) except *
cpdef str market_status_to_str(MarketStatus value)

cpdef OmsType oms_type_from_str(str value) except *
cpdef str oms_type_to_str(OmsType value)

cpdef OptionKind option_kind_from_str(str value) except *
cpdef str option_kind_to_str(OptionKind value)

cpdef OrderSide order_side_from_str(str value) except *
cpdef str order_side_to_str(OrderSide value)

cpdef OrderStatus order_status_from_str(str value) except *
cpdef str order_status_to_str(OrderStatus value)

cpdef OrderType order_type_from_str(str value) except *
cpdef str order_type_to_str(OrderType value)

cpdef PositionSide position_side_from_str(str value) except *
cpdef str position_side_to_str(PositionSide value)

cpdef PriceType price_type_from_str(str value) except *
cpdef str price_type_to_str(PriceType value)

cpdef TimeInForce time_in_force_from_str(str value) except *
cpdef str time_in_force_to_str(TimeInForce value)

cpdef TradingState trading_state_from_str(str value) except *
cpdef str trading_state_to_str(TradingState value)

cpdef TrailingOffsetType trailing_offset_type_from_str(str value) except *
cpdef str trailing_offset_type_to_str(TrailingOffsetType value)

cpdef TriggerType trigger_type_from_str(str value) except *
cpdef str trigger_type_to_str(TriggerType value)
