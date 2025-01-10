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

from libc.stdint cimport uint64_t

from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


cdef class PositionEvent(Event):
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    cdef readonly TraderId trader_id
    """The trader ID associated with the event.\n\n:returns: `TraderId`"""
    cdef readonly StrategyId strategy_id
    """The strategy ID associated with the event.\n\n:returns: `StrategyId`"""
    cdef readonly InstrumentId instrument_id
    """The instrument ID associated with the event.\n\n:returns: `InstrumentId`"""
    cdef readonly PositionId position_id
    """The position ID associated with the event.\n\n:returns: `PositionId`"""
    cdef readonly AccountId account_id
    """The account ID associated with the position.\n\n:returns: `AccountId`"""
    cdef readonly ClientOrderId opening_order_id
    """The client order ID for the order which opened the position.\n\n:returns: `ClientOrderId`"""
    cdef readonly ClientOrderId closing_order_id
    """The client order ID for the order which closed the position.\n\n:returns: `ClientOrderId` or ``None``"""
    cdef readonly OrderSide entry
    """The entry direction from open.\n\n:returns: `OrderSide`"""
    cdef readonly PositionSide side
    """The position side.\n\n:returns: `PositionSide`"""
    cdef readonly double signed_qty
    """The position signed quantity (positive for ``LONG``, negative for ``SHORT``).\n\n:returns: `double`"""
    cdef readonly Quantity quantity
    """The position open quantity.\n\n:returns: `Quantity`"""
    cdef readonly Quantity peak_qty
    """The peak directional quantity reached by the position.\n\n:returns: `Quantity`"""
    cdef readonly Quantity last_qty
    """The last fill quantity for the position.\n\n:returns: `Quantity`"""
    cdef readonly Price last_px
    """The last fill price for the position.\n\n:returns: `Price`"""
    cdef readonly Currency currency
    """The position quote currency.\n\n:returns: `Currency`"""
    cdef readonly double avg_px_open
    """The average open price.\n\n:returns: `double`"""
    cdef readonly double avg_px_close
    """The average closing price.\n\n:returns: `double`"""
    cdef readonly double realized_return
    """The realized return for the position.\n\n:returns: `double`"""
    cdef readonly Money realized_pnl
    """The realized PnL for the position (including commissions).\n\n:returns: `Money`"""
    cdef readonly Money unrealized_pnl
    """The unrealized PnL for the position (including commissions).\n\n:returns: `Money`"""
    cdef readonly uint64_t ts_opened
    """UNIX timestamp (nanoseconds) when the position was opened.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_closed
    """UNIX timestamp (nanoseconds) when the position was closed.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t duration_ns
    """The total open duration (nanoseconds).\n\n:returns: `uint64_t`"""


cdef class PositionOpened(PositionEvent):

    @staticmethod
    cdef PositionOpened create_c(
        Position position,
        OrderFilled fill,
        UUID4 event_id,
        uint64_t ts_init,
    )

    @staticmethod
    cdef PositionOpened from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(PositionOpened obj)


cdef class PositionChanged(PositionEvent):

    @staticmethod
    cdef PositionChanged create_c(
        Position position,
        OrderFilled fill,
        UUID4 event_id,
        uint64_t ts_init,
    )

    @staticmethod
    cdef PositionChanged from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(PositionChanged obj)


cdef class PositionClosed(PositionEvent):

    @staticmethod
    cdef PositionClosed create_c(
        Position position,
        OrderFilled fill,
        UUID4 event_id,
        uint64_t ts_init,
    )

    @staticmethod
    cdef PositionClosed from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(PositionClosed obj)
