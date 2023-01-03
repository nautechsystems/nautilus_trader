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

from typing import Any, Callable

from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.adapters.binance.common.types import BinanceTicker

from nautilus_trader.common.events.risk cimport TradingStateChanged
from nautilus_trader.common.events.system cimport ComponentStateChanged
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.ticker cimport Ticker
from nautilus_trader.model.data.venue cimport InstrumentClose
from nautilus_trader.model.data.venue cimport InstrumentStatusUpdate
from nautilus_trader.model.data.venue cimport VenueStatusUpdate
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderPendingCancel
from nautilus_trader.model.events.order cimport OrderPendingUpdate
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderSubmitted
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.events.position cimport PositionChanged
from nautilus_trader.model.events.position cimport PositionClosed
from nautilus_trader.model.events.position cimport PositionOpened
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.betting cimport BettingInstrument
from nautilus_trader.model.instruments.crypto_future cimport CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual cimport CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair cimport CurrencyPair
from nautilus_trader.model.instruments.equity cimport Equity
from nautilus_trader.model.instruments.future cimport Future
from nautilus_trader.model.instruments.option cimport Option


# Default mappings for Nautilus objects
_OBJECT_TO_DICT_MAP: dict[str, Callable[[None], dict]] = {
    CancelOrder.__name__: CancelOrder.to_dict_c,
    SubmitOrder.__name__: SubmitOrder.to_dict_c,
    SubmitOrderList.__name__: SubmitOrderList.to_dict_c,
    ModifyOrder.__name__: ModifyOrder.to_dict_c,
    ComponentStateChanged.__name__: ComponentStateChanged.to_dict_c,
    TradingStateChanged.__name__: TradingStateChanged.to_dict_c,
    AccountState.__name__: AccountState.to_dict_c,
    OrderAccepted.__name__: OrderAccepted.to_dict_c,
    OrderCancelRejected.__name__: OrderCancelRejected.to_dict_c,
    OrderCanceled.__name__: OrderCanceled.to_dict_c,
    OrderDenied.__name__: OrderDenied.to_dict_c,
    OrderExpired.__name__: OrderExpired.to_dict_c,
    OrderFilled.__name__: OrderFilled.to_dict_c,
    OrderInitialized.__name__: OrderInitialized.to_dict_c,
    OrderPendingCancel.__name__: OrderPendingCancel.to_dict_c,
    OrderPendingUpdate.__name__: OrderPendingUpdate.to_dict_c,
    OrderRejected.__name__: OrderRejected.to_dict_c,
    OrderSubmitted.__name__: OrderSubmitted.to_dict_c,
    OrderTriggered.__name__: OrderTriggered.to_dict_c,
    OrderModifyRejected.__name__: OrderModifyRejected.to_dict_c,
    OrderUpdated.__name__: OrderUpdated.to_dict_c,
    PositionOpened.__name__: PositionOpened.to_dict_c,
    PositionChanged.__name__: PositionChanged.to_dict_c,
    PositionClosed.__name__: PositionClosed.to_dict_c,
    Instrument.__name__: Instrument.base_to_dict_c,
    BettingInstrument.__name__: BettingInstrument.to_dict_c,
    Equity.__name__: Equity.to_dict_c,
    Future.__name__: Future.to_dict_c,
    Option.__name__: Option.to_dict_c,
    CurrencyPair.__name__: CurrencyPair.to_dict_c,
    CryptoPerpetual.__name__: CryptoPerpetual.to_dict_c,
    CryptoFuture.__name__: CryptoFuture.to_dict_c,
    TradeTick.__name__: TradeTick.to_dict_c,
    Ticker.__name__: Ticker.to_dict_c,
    QuoteTick.__name__: QuoteTick.to_dict_c,
    Bar.__name__: Bar.to_dict_c,
    InstrumentStatusUpdate.__name__: InstrumentStatusUpdate.to_dict_c,
    VenueStatusUpdate.__name__: VenueStatusUpdate.to_dict_c,
    InstrumentClose.__name__: InstrumentClose.to_dict_c,
    BinanceBar.__name__: BinanceBar.to_dict,
    BinanceTicker.__name__: BinanceTicker.to_dict,
}


# Default mappings for Nautilus objects
_OBJECT_FROM_DICT_MAP: dict[str, Callable[[dict], Any]] = {
    CancelOrder.__name__: CancelOrder.from_dict_c,
    SubmitOrder.__name__: SubmitOrder.from_dict_c,
    SubmitOrderList.__name__: SubmitOrderList.from_dict_c,
    ModifyOrder.__name__: ModifyOrder.from_dict_c,
    ComponentStateChanged.__name__: ComponentStateChanged.from_dict_c,
    TradingStateChanged.__name__: TradingStateChanged.from_dict_c,
    AccountState.__name__: AccountState.from_dict_c,
    OrderAccepted.__name__: OrderAccepted.from_dict_c,
    OrderCancelRejected.__name__: OrderCancelRejected.from_dict_c,
    OrderCanceled.__name__: OrderCanceled.from_dict_c,
    OrderDenied.__name__: OrderDenied.from_dict_c,
    OrderExpired.__name__: OrderExpired.from_dict_c,
    OrderFilled.__name__: OrderFilled.from_dict_c,
    OrderInitialized.__name__: OrderInitialized.from_dict_c,
    OrderPendingCancel.__name__: OrderPendingCancel.from_dict_c,
    OrderPendingUpdate.__name__: OrderPendingUpdate.from_dict_c,
    OrderRejected.__name__: OrderRejected.from_dict_c,
    OrderSubmitted.__name__: OrderSubmitted.from_dict_c,
    OrderTriggered.__name__: OrderTriggered.from_dict_c,
    OrderModifyRejected.__name__: OrderModifyRejected.from_dict_c,
    OrderUpdated.__name__: OrderUpdated.from_dict_c,
    PositionOpened.__name__: PositionOpened.from_dict_c,
    PositionChanged.__name__: PositionChanged.from_dict_c,
    PositionClosed.__name__: PositionClosed.from_dict_c,
    Instrument.__name__: Instrument.base_from_dict_c,
    BettingInstrument.__name__: BettingInstrument.from_dict_c,
    Equity.__name__: Equity.from_dict_c,
    Future.__name__: Future.from_dict_c,
    Option.__name__: Option.from_dict_c,
    CurrencyPair.__name__: CurrencyPair.from_dict_c,
    CryptoPerpetual.__name__: CryptoPerpetual.from_dict_c,
    CryptoFuture.__name__: CryptoFuture.from_dict_c,
    TradeTick.__name__: TradeTick.from_dict_c,
    Ticker.__name__: Ticker.from_dict_c,
    QuoteTick.__name__: QuoteTick.from_dict_c,
    Bar.__name__: Bar.from_dict_c,
    InstrumentStatusUpdate.__name__: InstrumentStatusUpdate.from_dict_c,
    VenueStatusUpdate.__name__: VenueStatusUpdate.from_dict_c,
    InstrumentClose.__name__: InstrumentClose.from_dict_c,
    BinanceBar.__name__: BinanceBar.from_dict,
    BinanceTicker.__name__: BinanceTicker.from_dict,
}


cpdef void register_serializable_object(
    obj,
    to_dict: Callable[[Any], dict[str, Any]],
    from_dict: Callable[[dict[str, Any]], Any],
) except *:
    """
    Register the given object with the global serialization object maps.

    Parameters
    ----------
    obj : object
        The object to register.
    to_dict : Callable[[Any], dict[str, Any]]
        The delegate to instantiate a dict of primitive types from the object.
    from_dict : Callable[[dict[str, Any]], Any]
        The delegate to instantiate the object from a dict of primitive types.

    Raises
    ------
    TypeError
        If `to_dict` or `from_dict` are not of type `Callable`.
    KeyError
        If obj already registered with the global object maps.

    """
    Condition.callable(to_dict, "to_dict")
    Condition.callable(from_dict, "from_dict")
    Condition.not_in(obj.__name__, _OBJECT_TO_DICT_MAP, "obj.__name__", "_OBJECT_TO_DICT_MAP")
    Condition.not_in(obj.__name__, _OBJECT_FROM_DICT_MAP, "obj.__name__", "_OBJECT_FROM_DICT_MAP")

    _OBJECT_TO_DICT_MAP[obj.__name__] = to_dict
    _OBJECT_FROM_DICT_MAP[obj.__name__] = from_dict


cdef class Serializer:
    """
    The base class for all serializers.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self):
        super().__init__()

    cpdef bytes serialize(self, object obj):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef object deserialize(self, bytes obj_bytes):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover
