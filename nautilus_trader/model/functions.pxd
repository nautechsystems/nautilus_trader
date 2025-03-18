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

from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport AggregationSource
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport AssetClass
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport CurrencyType
from nautilus_trader.core.rust.model cimport InstrumentClass
from nautilus_trader.core.rust.model cimport InstrumentCloseType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport MarketStatus
from nautilus_trader.core.rust.model cimport MarketStatusAction
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.rust.model cimport RecordFlag
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport TradingState
from nautilus_trader.core.rust.model cimport TrailingOffsetType
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.objects cimport Currency


cpdef AccountType account_type_from_str(str value)
cpdef str account_type_to_str(AccountType value)

cpdef AggregationSource aggregation_source_from_str(str value)
cpdef str aggregation_source_to_str(AggregationSource value)

cpdef AggressorSide aggressor_side_from_str(str value)
cpdef str aggressor_side_to_str(AggressorSide value)

cpdef AssetClass asset_class_from_str(str value)
cpdef str asset_class_to_str(AssetClass value)

cpdef InstrumentClass instrument_class_from_str(str value)
cpdef str instrument_class_to_str(InstrumentClass value)

cpdef BarAggregation bar_aggregation_from_str(str value)
cpdef str bar_aggregation_to_str(BarAggregation value)

cpdef BookAction book_action_from_str(str value)
cpdef str book_action_to_str(BookAction value)

cpdef BookType book_type_from_str(str value)
cpdef str book_type_to_str(BookType value)

cpdef ContingencyType contingency_type_from_str(str value)
cpdef str contingency_type_to_str(ContingencyType value)

cpdef CurrencyType currency_type_from_str(str value)
cpdef str currency_type_to_str(CurrencyType value)

cpdef InstrumentCloseType instrument_close_type_from_str(str value)
cpdef str instrument_close_type_to_str(InstrumentCloseType value)

cpdef LiquiditySide liquidity_side_from_str(str value)
cpdef str liquidity_side_to_str(LiquiditySide value)

cpdef MarketStatus market_status_from_str(str value)
cpdef str market_status_to_str(MarketStatus value)

cpdef MarketStatusAction market_status_action_from_str(str value)
cpdef str market_status_action_to_str(MarketStatusAction value)

cpdef OmsType oms_type_from_str(str value)
cpdef str oms_type_to_str(OmsType value)

cpdef OptionKind option_kind_from_str(str value)
cpdef str option_kind_to_str(OptionKind value)

cpdef OrderSide order_side_from_str(str value)
cpdef str order_side_to_str(OrderSide value)

cpdef OrderStatus order_status_from_str(str value)
cpdef str order_status_to_str(OrderStatus value)

cpdef OrderType order_type_from_str(str value)
cpdef str order_type_to_str(OrderType value)

cpdef RecordFlag record_flag_from_str(str value)
cpdef str record_flag_to_str(RecordFlag value)

cpdef PositionSide position_side_from_str(str value)
cpdef str position_side_to_str(PositionSide value)

cpdef PriceType price_type_from_str(str value)
cpdef str price_type_to_str(PriceType value)

cpdef TimeInForce time_in_force_from_str(str value)
cpdef str time_in_force_to_str(TimeInForce value)

cpdef TradingState trading_state_from_str(str value)
cpdef str trading_state_to_str(TradingState value)

cpdef TrailingOffsetType trailing_offset_type_from_str(str value)
cpdef str trailing_offset_type_to_str(TrailingOffsetType value)

cpdef TriggerType trigger_type_from_str(str value)
cpdef str trigger_type_to_str(TriggerType value)

cpdef order_side_to_pyo3(OrderSide value)
cpdef order_type_to_pyo3(OrderType value)
cpdef order_status_to_pyo3(OrderStatus value)
cpdef time_in_force_to_pyo3(TimeInForce value)

cpdef OrderSide order_side_from_pyo3(value)
cpdef OrderType order_type_from_pyo3(value)
cpdef OrderStatus order_status_from_pyo3(value)
cpdef TimeInForce time_in_force_from_pyo3(value)
cpdef LiquiditySide liquidity_side_from_pyo3(value)
cpdef ContingencyType contingency_type_from_pyo3(value)
cpdef PositionSide position_side_from_pyo3(value)
