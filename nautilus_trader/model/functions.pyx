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

from nautilus_trader.core import nautilus_pyo3

from libc.stdint cimport uint8_t

from nautilus_trader.core.rust.model cimport account_type_from_cstr
from nautilus_trader.core.rust.model cimport account_type_to_cstr
from nautilus_trader.core.rust.model cimport aggregation_source_from_cstr
from nautilus_trader.core.rust.model cimport aggregation_source_to_cstr
from nautilus_trader.core.rust.model cimport aggressor_side_from_cstr
from nautilus_trader.core.rust.model cimport aggressor_side_to_cstr
from nautilus_trader.core.rust.model cimport asset_class_from_cstr
from nautilus_trader.core.rust.model cimport asset_class_to_cstr
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
from nautilus_trader.core.rust.model cimport instrument_class_from_cstr
from nautilus_trader.core.rust.model cimport instrument_class_to_cstr
from nautilus_trader.core.rust.model cimport instrument_close_type_from_cstr
from nautilus_trader.core.rust.model cimport instrument_close_type_to_cstr
from nautilus_trader.core.rust.model cimport liquidity_side_from_cstr
from nautilus_trader.core.rust.model cimport liquidity_side_to_cstr
from nautilus_trader.core.rust.model cimport market_status_action_from_cstr
from nautilus_trader.core.rust.model cimport market_status_action_to_cstr
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
from nautilus_trader.core.rust.model cimport record_flag_from_cstr
from nautilus_trader.core.rust.model cimport record_flag_to_cstr
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


cpdef AccountType account_type_from_str(str value):
    return account_type_from_cstr(pystr_to_cstr(value))


cpdef str account_type_to_str(AccountType value):
    return cstr_to_pystr(account_type_to_cstr(value))


cpdef AggregationSource aggregation_source_from_str(str value):
    return aggregation_source_from_cstr(pystr_to_cstr(value))


cpdef str aggregation_source_to_str(AggregationSource value):
    return cstr_to_pystr(aggregation_source_to_cstr(value))


cpdef AggressorSide aggressor_side_from_str(str value):
    return aggressor_side_from_cstr(pystr_to_cstr(value))


cpdef str aggressor_side_to_str(AggressorSide value):
    return cstr_to_pystr(aggressor_side_to_cstr(value))


cpdef AssetClass asset_class_from_str(str value):
    return asset_class_from_cstr(pystr_to_cstr(value))


cpdef str asset_class_to_str(AssetClass value):
    return cstr_to_pystr(asset_class_to_cstr(value))


cpdef InstrumentClass instrument_class_from_str(str value):
    return instrument_class_from_cstr(pystr_to_cstr(value))


cpdef str instrument_class_to_str(InstrumentClass value):
    return cstr_to_pystr(instrument_class_to_cstr(value))


cpdef BarAggregation bar_aggregation_from_str(str value):
    return <BarAggregation>bar_aggregation_from_cstr(pystr_to_cstr(value))


cpdef str bar_aggregation_to_str(BarAggregation value):
    return cstr_to_pystr(bar_aggregation_to_cstr(<uint8_t>value))


cpdef BookAction book_action_from_str(str value):
    return book_action_from_cstr(pystr_to_cstr(value))


cpdef str book_action_to_str(BookAction value):
    return cstr_to_pystr(book_action_to_cstr(value))


cpdef BookType book_type_from_str(str value):
    return book_type_from_cstr(pystr_to_cstr(value))


cpdef str book_type_to_str(BookType value):
    return cstr_to_pystr(book_type_to_cstr(value))


cpdef ContingencyType contingency_type_from_str(str value):
    return contingency_type_from_cstr(pystr_to_cstr(value))


cpdef str contingency_type_to_str(ContingencyType value):
    return cstr_to_pystr(contingency_type_to_cstr(value))


cpdef CurrencyType currency_type_from_str(str value):
    return currency_type_from_cstr(pystr_to_cstr(value))


cpdef str currency_type_to_str(CurrencyType value):
    return cstr_to_pystr(currency_type_to_cstr(value))


cpdef InstrumentCloseType instrument_close_type_from_str(str value):
    return instrument_close_type_from_cstr(pystr_to_cstr(value))


cpdef str instrument_close_type_to_str(InstrumentCloseType value):
    return cstr_to_pystr(instrument_close_type_to_cstr(value))


cpdef LiquiditySide liquidity_side_from_str(str value):
    return liquidity_side_from_cstr(pystr_to_cstr(value))


cpdef str liquidity_side_to_str(LiquiditySide value):
    return cstr_to_pystr(liquidity_side_to_cstr(value))


cpdef MarketStatus market_status_from_str(str value):
    return market_status_from_cstr(pystr_to_cstr(value))


cpdef str market_status_to_str(MarketStatus value):
    return cstr_to_pystr(market_status_to_cstr(value))


cpdef MarketStatusAction market_status_action_from_str(str value):
    return market_status_action_from_cstr(pystr_to_cstr(value))


cpdef str market_status_action_to_str(MarketStatusAction value):
    return cstr_to_pystr(market_status_action_to_cstr(value))


cpdef OmsType oms_type_from_str(str value):
    return oms_type_from_cstr(pystr_to_cstr(value))


cpdef str oms_type_to_str(OmsType value):
    return cstr_to_pystr(oms_type_to_cstr(value))


cpdef OptionKind option_kind_from_str(str value):
    return option_kind_from_cstr(pystr_to_cstr(value))


cpdef str option_kind_to_str(OptionKind value):
    return cstr_to_pystr(option_kind_to_cstr(value))


cpdef OrderSide order_side_from_str(str value):
    return order_side_from_cstr(pystr_to_cstr(value))


cpdef str order_side_to_str(OrderSide value):
    return cstr_to_pystr(order_side_to_cstr(value))


cpdef OrderStatus order_status_from_str(str value):
    return order_status_from_cstr(pystr_to_cstr(value))


cpdef str order_status_to_str(OrderStatus value):
    return cstr_to_pystr(order_status_to_cstr(value))


cpdef OrderType order_type_from_str(str value):
    return order_type_from_cstr(pystr_to_cstr(value))


cpdef str order_type_to_str(OrderType value):
    return cstr_to_pystr(order_type_to_cstr(value))


cpdef RecordFlag record_flag_from_str(str value):
    return record_flag_from_cstr(pystr_to_cstr(value))


cpdef str record_flag_to_str(RecordFlag value):
    return cstr_to_pystr(record_flag_to_cstr(value))


cpdef PositionSide position_side_from_str(str value):
    return position_side_from_cstr(pystr_to_cstr(value))


cpdef str position_side_to_str(PositionSide value):
    return cstr_to_pystr(position_side_to_cstr(value))


cpdef PriceType price_type_from_str(str value):
    return price_type_from_cstr(pystr_to_cstr(value))


cpdef str price_type_to_str(PriceType value):
    return cstr_to_pystr(price_type_to_cstr(value))


cpdef TimeInForce time_in_force_from_str(str value):
    return time_in_force_from_cstr(pystr_to_cstr(value))


cpdef str time_in_force_to_str(TimeInForce value):
    return cstr_to_pystr(time_in_force_to_cstr(value))


cpdef TradingState trading_state_from_str(str value):
    return trading_state_from_cstr(pystr_to_cstr(value))


cpdef str trading_state_to_str(TradingState value):
    return cstr_to_pystr(trading_state_to_cstr(value))


cpdef TrailingOffsetType trailing_offset_type_from_str(str value):
    return trailing_offset_type_from_cstr(pystr_to_cstr(value))


cpdef str trailing_offset_type_to_str(TrailingOffsetType value):
    return cstr_to_pystr(trailing_offset_type_to_cstr(value))


cpdef TriggerType trigger_type_from_str(str value):
    return trigger_type_from_cstr(pystr_to_cstr(value))


cpdef str trigger_type_to_str(TriggerType value):
    return cstr_to_pystr(trigger_type_to_cstr(value))


cpdef order_side_to_pyo3(OrderSide value):
    if value == OrderSide.BUY:
        return nautilus_pyo3.OrderSide.BUY
    if value == OrderSide.SELL:
        return nautilus_pyo3.OrderSide.SELL
    if value == OrderSide.NO_ORDER_SIDE:
        return nautilus_pyo3.OrderSide.NO_ORDER_SIDE

    raise ValueError(f"Unsupported `OrderSide`, was '{order_side_to_str(value)}'")


cpdef order_type_to_pyo3(OrderType value):
    if value == OrderType.MARKET:
        return nautilus_pyo3.OrderType.MARKET
    if value == OrderType.LIMIT:
        return nautilus_pyo3.OrderType.LIMIT
    if value == OrderType.STOP_MARKET:
        return nautilus_pyo3.OrderType.STOP_MARKET
    if value == OrderType.STOP_LIMIT:
        return nautilus_pyo3.OrderType.STOP_LIMIT
    if value == OrderType.MARKET_TO_LIMIT:
        return nautilus_pyo3.OrderType.MARKET_TO_LIMIT
    if value == OrderType.MARKET_IF_TOUCHED:
        return nautilus_pyo3.OrderType.MARKET_IF_TOUCHED
    if value == OrderType.LIMIT_IF_TOUCHED:
        return nautilus_pyo3.OrderType.LIMIT_IF_TOUCHED
    if value == OrderType.TRAILING_STOP_MARKET:
        return nautilus_pyo3.OrderType.TRAILING_STOP_MARKET
    if value == OrderType.TRAILING_STOP_LIMIT:
        return nautilus_pyo3.OrderType.TRAILING_STOP_LIMIT

    raise ValueError(f"Unsupported `OrderType`, was '{order_type_to_str(value)}'")


cpdef order_status_to_pyo3(OrderStatus value):
    if value == OrderStatus.INITIALIZED:
        return nautilus_pyo3.OrderStatus.INITIALIZED
    if value == OrderStatus.DENIED:
        return nautilus_pyo3.OrderStatus.DENIED
    if value == OrderStatus.EMULATED:
        return nautilus_pyo3.OrderStatus.EMULATED
    if value == OrderStatus.RELEASED:
        return nautilus_pyo3.OrderStatus.RELEASED
    if value == OrderStatus.SUBMITTED:
        return nautilus_pyo3.OrderStatus.SUBMITTED
    if value == OrderStatus.ACCEPTED:
        return nautilus_pyo3.OrderStatus.ACCEPTED
    if value == OrderStatus.REJECTED:
        return nautilus_pyo3.OrderStatus.REJECTED
    if value == OrderStatus.CANCELED:
        return nautilus_pyo3.OrderStatus.CANCELED
    if value == OrderStatus.EXPIRED:
        return nautilus_pyo3.OrderStatus.EXPIRED
    if value == OrderStatus.TRIGGERED:
        return nautilus_pyo3.OrderStatus.TRIGGERED
    if value == OrderStatus.PENDING_UPDATE:
        return nautilus_pyo3.OrderStatus.PENDING_UPDATE
    if value == OrderStatus.PENDING_CANCEL:
        return nautilus_pyo3.OrderStatus.PENDING_CANCEL
    if value == OrderStatus.PARTIALLY_FILLED:
        return nautilus_pyo3.OrderStatus.PARTIALLY_FILLED
    if value == OrderStatus.FILLED:
        return nautilus_pyo3.OrderStatus.FILLED

    raise ValueError(f"Unsupported `OrderStatus`, was '{order_status_to_str(value)}'")


cpdef time_in_force_to_pyo3(TimeInForce value):
    if value == TimeInForce.GTC:
        return nautilus_pyo3.TimeInForce.GTC
    if value == TimeInForce.IOC:
        return nautilus_pyo3.TimeInForce.IOC
    if value == TimeInForce.FOK:
        return nautilus_pyo3.TimeInForce.FOK
    if value == TimeInForce.GTD:
        return nautilus_pyo3.TimeInForce.GTD
    if value == TimeInForce.DAY:
        return nautilus_pyo3.TimeInForce.DAY
    if value == TimeInForce.AT_THE_OPEN:
        return nautilus_pyo3.TimeInForce.AT_THE_OPEN
    if value == TimeInForce.AT_THE_CLOSE:
        return nautilus_pyo3.TimeInForce.AT_THE_CLOSE

    raise ValueError(f"Unsupported `TimeInForce`, was '{time_in_force_to_str(value)}'")


cpdef OrderSide order_side_from_pyo3(value: nautilus_pyo3.OrderSide):
    if value == nautilus_pyo3.OrderSide.BUY:
        return OrderSide.BUY
    if value == nautilus_pyo3.OrderSide.SELL:
        return OrderSide.SELL
    if value == nautilus_pyo3.OrderSide.NO_ORDER_SIDE:
        return OrderSide.NO_ORDER_SIDE

    raise ValueError(f"Unsupported `OrderSide`, was '{value}'")


cpdef OrderType order_type_from_pyo3(value: nautilus_pyo3.OrderType):
    if value == nautilus_pyo3.OrderType.MARKET:
        return OrderType.MARKET
    if value == nautilus_pyo3.OrderType.LIMIT:
        return OrderType.LIMIT
    if value == nautilus_pyo3.OrderType.STOP_MARKET:
        return OrderType.STOP_MARKET
    if value == nautilus_pyo3.OrderType.STOP_LIMIT:
        return OrderType.STOP_LIMIT
    if value == nautilus_pyo3.OrderType.MARKET_TO_LIMIT:
        return OrderType.MARKET_TO_LIMIT
    if value == nautilus_pyo3.OrderType.MARKET_IF_TOUCHED:
        return OrderType.MARKET_IF_TOUCHED
    if value == nautilus_pyo3.OrderType.LIMIT_IF_TOUCHED:
        return OrderType.LIMIT_IF_TOUCHED
    if value == nautilus_pyo3.OrderType.TRAILING_STOP_MARKET:
        return OrderType.TRAILING_STOP_MARKET
    if value == nautilus_pyo3.OrderType.TRAILING_STOP_LIMIT:
        return OrderType.TRAILING_STOP_LIMIT

    raise ValueError(f"Unsupported `OrderType`, was '{value}'")


cpdef OrderStatus order_status_from_pyo3(value: nautilus_pyo3.OrderStatus):
    if value == nautilus_pyo3.OrderStatus.INITIALIZED:
        return OrderStatus.INITIALIZED
    if value == nautilus_pyo3.OrderStatus.DENIED:
        return OrderStatus.DENIED
    if value == nautilus_pyo3.OrderStatus.EMULATED:
        return OrderStatus.EMULATED
    if value == nautilus_pyo3.OrderStatus.RELEASED:
        return OrderStatus.RELEASED
    if value == nautilus_pyo3.OrderStatus.SUBMITTED:
        return OrderStatus.SUBMITTED
    if value == nautilus_pyo3.OrderStatus.ACCEPTED:
        return OrderStatus.ACCEPTED
    if value == nautilus_pyo3.OrderStatus.REJECTED:
        return OrderStatus.REJECTED
    if value == nautilus_pyo3.OrderStatus.CANCELED:
        return OrderStatus.CANCELED
    if value == nautilus_pyo3.OrderStatus.EXPIRED:
        return OrderStatus.EXPIRED
    if value == nautilus_pyo3.OrderStatus.TRIGGERED:
        return OrderStatus.TRIGGERED
    if value == nautilus_pyo3.OrderStatus.PENDING_UPDATE:
        return OrderStatus.PENDING_UPDATE
    if value == nautilus_pyo3.OrderStatus.PENDING_CANCEL:
        return OrderStatus.PENDING_CANCEL
    if value == nautilus_pyo3.OrderStatus.PARTIALLY_FILLED:
        return OrderStatus.PARTIALLY_FILLED
    if value == nautilus_pyo3.OrderStatus.FILLED:
        return OrderStatus.FILLED

    raise ValueError(f"Unsupported `OrderStatus`, was '{value}'")


cpdef TimeInForce time_in_force_from_pyo3(value: nautilus_pyo3.TimeInForce):
    if value == nautilus_pyo3.TimeInForce.GTC:
        return TimeInForce.GTC
    if value == nautilus_pyo3.TimeInForce.IOC:
        return TimeInForce.IOC
    if value == nautilus_pyo3.TimeInForce.FOK:
        return TimeInForce.FOK
    if value == nautilus_pyo3.TimeInForce.GTD:
        return TimeInForce.GTD
    if value == nautilus_pyo3.TimeInForce.DAY:
        return TimeInForce.DAY
    if value == nautilus_pyo3.TimeInForce.AT_THE_OPEN:
        return TimeInForce.AT_THE_OPEN
    if value == nautilus_pyo3.TimeInForce.AT_THE_CLOSE:
        return TimeInForce.AT_THE_CLOSE

    raise ValueError(f"Unsupported `TimeInForce`, was '{value}'")


cpdef LiquiditySide liquidity_side_from_pyo3(value: nautilus_pyo3.LiquiditySide):
    if value == nautilus_pyo3.LiquiditySide.NO_LIQUIDITY_SIDE:
        return LiquiditySide.NO_LIQUIDITY_SIDE
    if value == nautilus_pyo3.LiquiditySide.MAKER:
        return LiquiditySide.MAKER
    if value == nautilus_pyo3.LiquiditySide.TAKER:
        return LiquiditySide.TAKER

    raise ValueError(f"Unsupported `LiquiditySide`, was '{value}'")


cpdef ContingencyType contingency_type_from_pyo3(value: nautilus_pyo3.ContingencyType):
    if value == nautilus_pyo3.ContingencyType.NO_CONTINGENCY:
        return ContingencyType.NO_CONTINGENCY
    if value == nautilus_pyo3.ContingencyType.OCO:
        return ContingencyType.OCO
    if value == nautilus_pyo3.ContingencyType.OTO:
        return ContingencyType.OTO
    if value == nautilus_pyo3.ContingencyType.OUO:
        return ContingencyType.OUO

    raise ValueError(f"Unsupported `ContingencyType`, was '{value}'")


cpdef PositionSide position_side_from_pyo3(value: nautilus_pyo3.PositionSide):
    if value == nautilus_pyo3.PositionSide.FLAT:
        return PositionSide.FLAT
    if value == nautilus_pyo3.PositionSide.LONG:
        return PositionSide.LONG
    if value == nautilus_pyo3.PositionSide.SHORT:
        return PositionSide.SHORT

    raise ValueError(f"Unsupported `PositionSide`, was '{value}'")
