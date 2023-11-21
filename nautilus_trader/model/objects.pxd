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

from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport AggregationSource
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport AssetClass
from nautilus_trader.core.rust.model cimport AssetType
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport Currency_t
from nautilus_trader.core.rust.model cimport CurrencyType
from nautilus_trader.core.rust.model cimport HaltReason
from nautilus_trader.core.rust.model cimport InstrumentCloseType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport MarketStatus
from nautilus_trader.core.rust.model cimport Money_t
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.rust.model cimport Quantity_t
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport TradingState
from nautilus_trader.core.rust.model cimport TrailingOffsetType
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Currency


cpdef AccountType account_type_from_str(str value)
cpdef str account_type_to_str(AccountType value)

cpdef AggregationSource aggregation_source_from_str(str value)
cpdef str aggregation_source_to_str(AggregationSource value)

cpdef AggressorSide aggressor_side_from_str(str value)
cpdef str aggressor_side_to_str(AggressorSide value)

cpdef AssetClass asset_class_from_str(str value)
cpdef str asset_class_to_str(AssetClass value)

cpdef AssetType asset_type_from_str(str value)
cpdef str asset_type_to_str(AssetType value)

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

cpdef HaltReason halt_reason_from_str(str value)
cpdef str halt_reason_to_str(HaltReason value)

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


cdef class Quantity:
    cdef Quantity_t _mem

    cdef bint eq(self, Quantity other)
    cdef bint ne(self, Quantity other)
    cdef bint lt(self, Quantity other)
    cdef bint le(self, Quantity other)
    cdef bint gt(self, Quantity other)
    cdef bint ge(self, Quantity other)
    cdef bint is_zero(self)
    cdef bint is_negative(self)
    cdef bint is_positive(self)
    cdef uint64_t raw_uint64_c(self)
    cdef double as_f64_c(self)

    cdef Quantity add(self, Quantity other)
    cdef Quantity sub(self, Quantity other)
    cdef void add_assign(self, Quantity other)
    cdef void sub_assign(self, Quantity other)

    @staticmethod
    cdef object _extract_decimal(object obj)

    @staticmethod
    cdef bint _compare(a, b, int op)

    @staticmethod
    cdef double raw_to_f64_c(uint64_t raw)

    @staticmethod
    cdef Quantity from_mem_c(Quantity_t mem)

    @staticmethod
    cdef Quantity from_raw_c(uint64_t raw, uint8_t precision)

    @staticmethod
    cdef Quantity zero_c(uint8_t precision)

    @staticmethod
    cdef Quantity from_str_c(str value)

    @staticmethod
    cdef Quantity from_int_c(int value)

    cpdef str to_str(self)
    cpdef object as_decimal(self)
    cpdef double as_double(self)


cdef class Price:
    cdef Price_t _mem

    cdef bint eq(self, Price other)
    cdef bint ne(self, Price other)
    cdef bint lt(self, Price other)
    cdef bint le(self, Price other)
    cdef bint gt(self, Price other)
    cdef bint ge(self, Price other)
    cdef bint is_zero(self)
    cdef bint is_negative(self)
    cdef bint is_positive(self)
    cdef int64_t raw_int64_c(self)
    cdef double as_f64_c(self)

    cdef Price add(self, Price other)
    cdef Price sub(self, Price other)
    cdef void add_assign(self, Price other)
    cdef void sub_assign(self, Price other)

    @staticmethod
    cdef object _extract_decimal(object obj)

    @staticmethod
    cdef bint _compare(a, b, int op)

    @staticmethod
    cdef double raw_to_f64_c(uint64_t raw)

    @staticmethod
    cdef Price from_mem_c(Price_t mem)

    @staticmethod
    cdef Price from_raw_c(int64_t raw, uint8_t precision)

    @staticmethod
    cdef Price from_str_c(str value)

    @staticmethod
    cdef Price from_int_c(int value)

    cpdef object as_decimal(self)
    cpdef double as_double(self)


cdef class Money:
    cdef Money_t _mem

    cdef str currency_code_c(self)
    cdef bint is_zero(self)
    cdef bint is_negative(self)
    cdef bint is_positive(self)
    cdef int64_t raw_int64_c(self)
    cdef double as_f64_c(self)

    @staticmethod
    cdef double raw_to_f64_c(uint64_t raw)

    @staticmethod
    cdef Money from_raw_c(uint64_t raw, Currency currency)

    @staticmethod
    cdef Money from_str_c(str value)

    @staticmethod
    cdef object _extract_decimal(object obj)

    cdef Money add(self, Money other)
    cdef Money sub(self, Money other)
    cdef void add_assign(self, Money other)
    cdef void sub_assign(self, Money other)

    cpdef str to_str(self)
    cpdef object as_decimal(self)
    cpdef double as_double(self)


cdef class Currency:
    cdef Currency_t _mem

    cdef uint8_t get_precision(self)

    @staticmethod
    cdef void register_c(Currency currency, bint overwrite=*)

    @staticmethod
    cdef Currency from_internal_map_c(str code)

    @staticmethod
    cdef Currency from_str_c(str code, bint strict=*)

    @staticmethod
    cdef bint is_fiat_c(str code)

    @staticmethod
    cdef bint is_crypto_c(str code)


cdef class AccountBalance:
    cdef readonly Money total
    """The total account balance.\n\n:returns: `Money`"""
    cdef readonly Money locked
    """The account balance locked (assigned to pending orders).\n\n:returns: `Money`"""
    cdef readonly Money free
    """The account balance free for trading.\n\n:returns: `Money`"""
    cdef readonly Currency currency
    """The currency of the account.\n\n:returns: `Currency`"""

    @staticmethod
    cdef AccountBalance from_dict_c(dict values)
    cpdef dict to_dict(self)


cdef class MarginBalance:
    cdef readonly Money initial
    """The initial margin requirement.\n\n:returns: `Money`"""
    cdef readonly Money maintenance
    """The maintenance margin requirement.\n\n:returns: `Money`"""
    cdef readonly Currency currency
    """The currency of the margin.\n\n:returns: `Currency`"""
    cdef readonly InstrumentId instrument_id
    """The instrument ID associated with the margin.\n\n:returns: `InstrumentId` or ``None``"""

    @staticmethod
    cdef MarginBalance from_dict_c(dict values)
    cpdef dict to_dict(self)
