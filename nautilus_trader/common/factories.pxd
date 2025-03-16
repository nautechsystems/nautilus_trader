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

from decimal import Decimal

from cpython.datetime cimport datetime

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.generators cimport ClientOrderIdGenerator
from nautilus_trader.common.generators cimport OrderListIdGenerator
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport TrailingOffsetType
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.limit_if_touched cimport LimitIfTouchedOrder
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_if_touched cimport MarketIfTouchedOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.model.orders.stop_limit cimport StopLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_limit cimport TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market cimport TrailingStopMarketOrder


cdef class OrderFactory:
    cdef Clock _clock
    cdef CacheFacade _cache
    cdef ClientOrderIdGenerator _order_id_generator
    cdef OrderListIdGenerator _order_list_id_generator

    cdef readonly TraderId trader_id
    """The order factories trader ID.\n\n:returns: `TraderId`"""
    cdef readonly StrategyId strategy_id
    """The order factories trading strategy ID.\n\n:returns: `StrategyId`"""
    cdef readonly bint use_uuid_client_order_ids
    """If UUID4's should be used for client order ID values.\n\n:returns: `bool`"""

    cpdef void set_client_order_id_count(self, int count)
    cpdef void set_order_list_id_count(self, int count)
    cpdef ClientOrderId generate_client_order_id(self)
    cpdef OrderListId generate_order_list_id(self)
    cpdef void reset(self)

    cpdef OrderList create_list(self, list orders)

    cpdef MarketOrder market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        TimeInForce time_in_force=*,
        bint reduce_only=*,
        bint quote_quantity=*,
        ExecAlgorithmId exec_algorithm_id=*,
        dict exec_algorithm_params=*,
        list[str] tags=*,
        ClientOrderId client_order_id=*,
    )

    cpdef LimitOrder limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        TimeInForce time_in_force=*,
        datetime expire_time=*,
        bint post_only=*,
        bint reduce_only=*,
        bint quote_quantity=*,
        Quantity display_qty=*,
        TriggerType emulation_trigger=*,
        InstrumentId trigger_instrument_id=*,
        ExecAlgorithmId exec_algorithm_id=*,
        dict exec_algorithm_params=*,
        list[str] tags=*,
        ClientOrderId client_order_id=*,
    )

    cpdef StopMarketOrder stop_market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price trigger_price,
        TriggerType trigger_type=*,
        TimeInForce time_in_force=*,
        datetime expire_time=*,
        bint reduce_only=*,
        bint quote_quantity=*,
        TriggerType emulation_trigger=*,
        InstrumentId trigger_instrument_id=*,
        ExecAlgorithmId exec_algorithm_id=*,
        dict exec_algorithm_params=*,
        list[str] tags=*,
        ClientOrderId client_order_id=*,
    )

    cpdef StopLimitOrder stop_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        Price trigger_price,
        TriggerType trigger_type=*,
        TimeInForce time_in_force=*,
        datetime expire_time=*,
        bint post_only=*,
        bint reduce_only=*,
        bint quote_quantity=*,
        Quantity display_qty=*,
        TriggerType emulation_trigger=*,
        InstrumentId trigger_instrument_id=*,
        ExecAlgorithmId exec_algorithm_id=*,
        dict exec_algorithm_params=*,
        list[str] tags=*,
        ClientOrderId client_order_id=*,
    )

    cpdef MarketToLimitOrder market_to_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        TimeInForce time_in_force=*,
        datetime expire_time=*,
        bint reduce_only=*,
        bint quote_quantity=*,
        Quantity display_qty=*,
        ExecAlgorithmId exec_algorithm_id=*,
        dict exec_algorithm_params=*,
        list[str] tags=*,
        ClientOrderId client_order_id=*,
    )

    cpdef MarketIfTouchedOrder market_if_touched(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price trigger_price,
        TriggerType trigger_type=*,
        TimeInForce time_in_force=*,
        datetime expire_time=*,
        bint reduce_only=*,
        bint quote_quantity=*,
        TriggerType emulation_trigger=*,
        InstrumentId trigger_instrument_id=*,
        ExecAlgorithmId exec_algorithm_id=*,
        dict exec_algorithm_params=*,
        list[str] tags=*,
        ClientOrderId client_order_id=*,
    )

    cpdef LimitIfTouchedOrder limit_if_touched(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        Price trigger_price,
        TriggerType trigger_type=*,
        TimeInForce time_in_force=*,
        datetime expire_time=*,
        bint post_only=*,
        bint reduce_only=*,
        bint quote_quantity=*,
        Quantity display_qty=*,
        TriggerType emulation_trigger=*,
        InstrumentId trigger_instrument_id=*,
        ExecAlgorithmId exec_algorithm_id=*,
        dict exec_algorithm_params=*,
        list[str] tags=*,
        ClientOrderId client_order_id=*,
    )

    cpdef TrailingStopMarketOrder trailing_stop_market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        trailing_offset: Decimal,
        Price trigger_price=*,
        TriggerType trigger_type=*,
        TrailingOffsetType trailing_offset_type=*,
        TimeInForce time_in_force=*,
        datetime expire_time=*,
        bint reduce_only=*,
        bint quote_quantity=*,
        TriggerType emulation_trigger=*,
        InstrumentId trigger_instrument_id=*,
        ExecAlgorithmId exec_algorithm_id=*,
        dict exec_algorithm_params=*,
        list[str] tags=*,
        ClientOrderId client_order_id=*,
    )

    cpdef TrailingStopLimitOrder trailing_stop_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        limit_offset: Decimal,
        trailing_offset: Decimal,
        Price price=*,
        Price trigger_price=*,
        TriggerType trigger_type=*,
        TrailingOffsetType trailing_offset_type=*,
        TimeInForce time_in_force=*,
        datetime expire_time=*,
        bint post_only=*,
        bint reduce_only=*,
        bint quote_quantity=*,
        Quantity display_qty=*,
        TriggerType emulation_trigger=*,
        InstrumentId trigger_instrument_id=*,
        ExecAlgorithmId exec_algorithm_id=*,
        dict exec_algorithm_params=*,
        list[str] tags=*,
        ClientOrderId client_order_id=*,
    )

    cpdef OrderList bracket(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price entry_trigger_price=*,
        Price entry_price=*,
        Price sl_trigger_price=*,
        Price tp_trigger_price=*,
        Price tp_price=*,
        OrderType entry_order_type=*,
        OrderType tp_order_type=*,
        TimeInForce time_in_force=*,
        TimeInForce sl_time_in_force=*,
        TimeInForce tp_time_in_force=*,
        datetime expire_time=*,
        bint entry_post_only=*,
        bint tp_post_only=*,
        bint quote_quantity=*,
        TriggerType emulation_trigger=*,
        InstrumentId trigger_instrument_id=*,
        ContingencyType contingency_type=*,
        ExecAlgorithmId entry_exec_algorithm_id=*,
        ExecAlgorithmId sl_exec_algorithm_id=*,
        ExecAlgorithmId tp_exec_algorithm_id=*,
        dict entry_exec_algorithm_params=*,
        dict sl_exec_algorithm_params=*,
        dict tp_exec_algorithm_params=*,
        list[str] entry_tags=*,
        list[str] sl_tags=*,
        list[str] tp_tags=*,
        ClientOrderId entry_client_order_id=*,
        ClientOrderId sl_client_order_id=*,
        ClientOrderId tp_client_order_id=*,
    )
