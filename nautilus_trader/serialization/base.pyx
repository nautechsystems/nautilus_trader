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
from typing import Callable

from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.adapters.binance.common.types import BinanceTicker

from nautilus_trader.common.messages cimport ComponentStateChanged
from nautilus_trader.common.messages cimport ShutdownSystem
from nautilus_trader.common.messages cimport TradingStateChanged
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderEmulated
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderPendingCancel
from nautilus_trader.model.events.order cimport OrderPendingUpdate
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderReleased
from nautilus_trader.model.events.order cimport OrderSubmitted
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.events.position cimport PositionChanged
from nautilus_trader.model.events.position cimport PositionClosed
from nautilus_trader.model.events.position cimport PositionOpened
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.betting cimport BettingInstrument
from nautilus_trader.model.instruments.binary_option cimport BinaryOption
from nautilus_trader.model.instruments.cfd cimport Cfd
from nautilus_trader.model.instruments.crypto_future cimport CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual cimport CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair cimport CurrencyPair
from nautilus_trader.model.instruments.equity cimport Equity
from nautilus_trader.model.instruments.futures_contract cimport FuturesContract
from nautilus_trader.model.instruments.futures_spread cimport FuturesSpread
from nautilus_trader.model.instruments.option_contract cimport OptionContract
from nautilus_trader.model.instruments.option_spread cimport OptionSpread
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument


# Default mappings for Nautilus objects
_OBJECT_TO_DICT_MAP: dict[str, Callable[[None], dict]] = {
    CancelOrder.__name__: CancelOrder.to_dict_c,
    SubmitOrder.__name__: SubmitOrder.to_dict_c,
    SubmitOrderList.__name__: SubmitOrderList.to_dict_c,
    ModifyOrder.__name__: ModifyOrder.to_dict_c,
    ShutdownSystem.__name__: ShutdownSystem.to_dict_c,
    ComponentStateChanged.__name__: ComponentStateChanged.to_dict_c,
    TradingStateChanged.__name__: TradingStateChanged.to_dict_c,
    AccountState.__name__: AccountState.to_dict_c,
    OrderAccepted.__name__: OrderAccepted.to_dict_c,
    OrderCancelRejected.__name__: OrderCancelRejected.to_dict_c,
    OrderCanceled.__name__: OrderCanceled.to_dict_c,
    OrderDenied.__name__: OrderDenied.to_dict_c,
    OrderEmulated.__name__: OrderEmulated.to_dict_c,
    OrderExpired.__name__: OrderExpired.to_dict_c,
    OrderFilled.__name__: OrderFilled.to_dict_c,
    OrderInitialized.__name__: OrderInitialized.to_dict_c,
    OrderPendingCancel.__name__: OrderPendingCancel.to_dict_c,
    OrderPendingUpdate.__name__: OrderPendingUpdate.to_dict_c,
    OrderRejected.__name__: OrderRejected.to_dict_c,
    OrderReleased.__name__: OrderReleased.to_dict_c,
    OrderSubmitted.__name__: OrderSubmitted.to_dict_c,
    OrderTriggered.__name__: OrderTriggered.to_dict_c,
    OrderModifyRejected.__name__: OrderModifyRejected.to_dict_c,
    OrderUpdated.__name__: OrderUpdated.to_dict_c,
    PositionOpened.__name__: PositionOpened.to_dict_c,
    PositionChanged.__name__: PositionChanged.to_dict_c,
    PositionClosed.__name__: PositionClosed.to_dict_c,
    Instrument.__name__: Instrument.base_to_dict_c,
    SyntheticInstrument.__name__: SyntheticInstrument.to_dict_c,
    BettingInstrument.__name__: BettingInstrument.to_dict_c,
    BinaryOption.__name__: BinaryOption.to_dict_c,
    Equity.__name__: Equity.to_dict_c,
    FuturesContract.__name__: FuturesContract.to_dict_c,
    FuturesSpread.__name__: FuturesSpread.to_dict_c,
    OptionContract.__name__: OptionContract.to_dict_c,
    OptionSpread.__name__: OptionSpread.to_dict_c,
    Cfd.__name__: Cfd.to_dict_c,
    CurrencyPair.__name__: CurrencyPair.to_dict_c,
    CryptoPerpetual.__name__: CryptoPerpetual.to_dict_c,
    CryptoFuture.__name__: CryptoFuture.to_dict_c,
    OrderBookDelta.__name__: OrderBookDelta.to_dict_c,
    OrderBookDeltas.__name__: OrderBookDeltas.to_dict_c,
    TradeTick.__name__: TradeTick.to_dict_c,
    QuoteTick.__name__: QuoteTick.to_dict_c,
    Bar.__name__: Bar.to_dict_c,
    InstrumentStatus.__name__: InstrumentStatus.to_dict_c,
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
    ShutdownSystem.__name__: ShutdownSystem.from_dict_c,
    ComponentStateChanged.__name__: ComponentStateChanged.from_dict_c,
    TradingStateChanged.__name__: TradingStateChanged.from_dict_c,
    AccountState.__name__: AccountState.from_dict_c,
    OrderAccepted.__name__: OrderAccepted.from_dict_c,
    OrderCancelRejected.__name__: OrderCancelRejected.from_dict_c,
    OrderCanceled.__name__: OrderCanceled.from_dict_c,
    OrderDenied.__name__: OrderDenied.from_dict_c,
    OrderEmulated.__name__: OrderEmulated.from_dict_c,
    OrderExpired.__name__: OrderExpired.from_dict_c,
    OrderFilled.__name__: OrderFilled.from_dict_c,
    OrderInitialized.__name__: OrderInitialized.from_dict_c,
    OrderPendingCancel.__name__: OrderPendingCancel.from_dict_c,
    OrderPendingUpdate.__name__: OrderPendingUpdate.from_dict_c,
    OrderReleased.__name__: OrderReleased.from_dict_c,
    OrderRejected.__name__: OrderRejected.from_dict_c,
    OrderSubmitted.__name__: OrderSubmitted.from_dict_c,
    OrderTriggered.__name__: OrderTriggered.from_dict_c,
    OrderModifyRejected.__name__: OrderModifyRejected.from_dict_c,
    OrderUpdated.__name__: OrderUpdated.from_dict_c,
    PositionOpened.__name__: PositionOpened.from_dict_c,
    PositionChanged.__name__: PositionChanged.from_dict_c,
    PositionClosed.__name__: PositionClosed.from_dict_c,
    Instrument.__name__: Instrument.base_from_dict_c,
    SyntheticInstrument.__name__: SyntheticInstrument.from_dict_c,
    BettingInstrument.__name__: BettingInstrument.from_dict_c,
    BinaryOption.__name__: BinaryOption.from_dict_c,
    Equity.__name__: Equity.from_dict_c,
    FuturesContract.__name__: FuturesContract.from_dict_c,
    FuturesSpread.__name__: FuturesSpread.from_dict_c,
    OptionContract.__name__: OptionContract.from_dict_c,
    OptionSpread.__name__: OptionSpread.from_dict_c,
    Cfd.__name__: Cfd.from_dict_c,
    CurrencyPair.__name__: CurrencyPair.from_dict_c,
    CryptoPerpetual.__name__: CryptoPerpetual.from_dict_c,
    CryptoFuture.__name__: CryptoFuture.from_dict_c,
    OrderBookDelta.__name__: OrderBookDelta.from_dict_c,
    OrderBookDeltas.__name__: OrderBookDeltas.from_dict_c,
    TradeTick.__name__: TradeTick.from_dict_c,
    QuoteTick.__name__: QuoteTick.from_dict_c,
    Bar.__name__: Bar.from_dict_c,
    InstrumentStatus.__name__: InstrumentStatus.from_dict_c,
    InstrumentClose.__name__: InstrumentClose.from_dict_c,
    BinanceBar.__name__: BinanceBar.from_dict,
    BinanceTicker.__name__: BinanceTicker.from_dict,
}


_EXTERNAL_PUBLISHABLE_TYPES = {
    str,
    int,
    float,
    bytes,
    SubmitOrder,
    SubmitOrderList,
    ModifyOrder,
    CancelOrder,
    ShutdownSystem,
    ComponentStateChanged,
    TradingStateChanged,
    AccountState,
    OrderAccepted,
    OrderCancelRejected,
    OrderCanceled,
    OrderDenied,
    OrderEmulated,
    OrderExpired,
    OrderFilled,
    OrderInitialized,
    OrderPendingCancel,
    OrderPendingUpdate,
    OrderReleased,
    OrderRejected,
    OrderSubmitted,
    OrderTriggered,
    OrderModifyRejected,
    OrderUpdated,
    PositionOpened,
    PositionChanged,
    PositionClosed,
    Instrument,
    SyntheticInstrument,
    BettingInstrument,
    BinaryOption,
    Equity,
    FuturesContract,
    OptionContract,
    CurrencyPair,
    CryptoPerpetual,
    CryptoFuture,
    OrderBookDelta,
    OrderBookDeltas,
    TradeTick,
    QuoteTick,
    Bar,
    InstrumentStatus,
    InstrumentClose,
    BinanceBar,
    BinanceTicker,
}


cpdef void register_serializable_type(
    cls: type,
    to_dict: Callable[[Any], dict[str, Any]],
    from_dict: Callable[[dict[str, Any]], Any],
):
    """
    Register the given type with the global serialization type maps.

    The `type` will also be registered as an external publishable type and
    will be published externally on the message bus unless also added to
    the `MessageBusConfig.types_filter`.

    Parameters
    ----------
    cls : type
        The type to register.
    to_dict : Callable[[Any], dict[str, Any]]
        The delegate to instantiate a dict of primitive types from an object.
    from_dict : Callable[[dict[str, Any]], Any]
        The delegate to instantiate an object from a dict of primitive types.

    Raises
    ------
    TypeError
        If `to_dict` or `from_dict` are not of type `Callable`.
    KeyError
        If `type` already registered with the global type maps.

    """
    Condition.callable(to_dict, "to_dict")
    Condition.callable(from_dict, "from_dict")
    Condition.not_in(cls.__name__, _OBJECT_TO_DICT_MAP, "cls.__name__", "_OBJECT_TO_DICT_MAP")
    Condition.not_in(cls.__name__, _OBJECT_FROM_DICT_MAP, "cls.__name__", "_OBJECT_FROM_DICT_MAP")

    _OBJECT_TO_DICT_MAP[cls.__name__] = to_dict
    _OBJECT_FROM_DICT_MAP[cls.__name__] = from_dict
    _EXTERNAL_PUBLISHABLE_TYPES.add(cls)


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
        raise NotImplementedError("method `serialize` must be implemented in the subclass")  # pragma: no cover

    cpdef object deserialize(self, bytes obj_bytes):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `deserialize` must be implemented in the subclass")  # pragma: no cover
