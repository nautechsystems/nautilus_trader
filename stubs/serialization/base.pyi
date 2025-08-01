from typing import Any
from typing import Callable
from typing import Dict
from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import ShutdownSystem
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderCancelRejected
from nautilus_trader.model.events.order import OrderDenied
from nautilus_trader.model.events.order import OrderEmulated
from nautilus_trader.model.events.order import OrderExpired
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.events.order import OrderModifyRejected
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderReleased
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderTriggered
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.events.position import PositionChanged
from nautilus_trader.model.events.position import PositionClosed
from nautilus_trader.model.events.position import PositionOpened
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.instruments.binary_option import BinaryOption
from nautilus_trader.model.instruments.cfd import Cfd
from nautilus_trader.model.instruments.commodity import Commodity
from nautilus_trader.model.instruments.crypto_future import CryptoFuture
from nautilus_trader.model.instruments.crypto_option import CryptoOption
from nautilus_trader.model.instruments.crypto_perpetual import CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.instruments.futures_contract import FuturesContract
from nautilus_trader.model.instruments.futures_spread import FuturesSpread
from nautilus_trader.model.instruments.index import IndexInstrument
from nautilus_trader.model.instruments.option_contract import OptionContract
from nautilus_trader.model.instruments.option_spread import OptionSpread
from nautilus_trader.model.instruments.synthetic import SyntheticInstrument


_OBJECT_TO_DICT_MAP: Dict[str, Callable[[Any], dict]] = ...
_OBJECT_FROM_DICT_MAP: Dict[str, Callable[[dict], Any]] = ...
_EXTERNAL_PUBLISHABLE_TYPES: set = ...


def register_serializable_type(
    cls: type,
    to_dict: Callable[[Any], dict[str, Any]],
    from_dict: Callable[[dict[str, Any]], Any],
) -> None:
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
    ...


class Serializer:
    """
    The base class for all serializers.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self) -> None:
        ...

    def serialize(self, obj: object) -> bytes:
        """Abstract method (implement in subclass)."""
        ...

    def deserialize(self, obj_bytes: bytes) -> object:
        """Abstract method (implement in subclass)."""
        ...

