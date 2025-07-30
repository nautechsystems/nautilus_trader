from typing import Any
from typing import Callable
from nautilus_trader.core.nautilus_pyo3 import AccountState
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import BettingInstrument
from nautilus_trader.core.nautilus_pyo3 import BinaryOption
from nautilus_trader.core.nautilus_pyo3 import CryptoFuture
from nautilus_trader.core.nautilus_pyo3 import CryptoOption
from nautilus_trader.core.nautilus_pyo3 import CryptoPerpetual
from nautilus_trader.core.nautilus_pyo3 import CurrencyPair
from nautilus_trader.core.nautilus_pyo3 import Equity
from nautilus_trader.core.nautilus_pyo3 import FuturesContract
from nautilus_trader.core.nautilus_pyo3 import FuturesSpread
from nautilus_trader.core.nautilus_pyo3 import InstrumentClose
from nautilus_trader.core.nautilus_pyo3 import InstrumentStatus
from nautilus_trader.core.nautilus_pyo3 import OrderAccepted
from nautilus_trader.core.nautilus_pyo3 import OrderBookDelta
from nautilus_trader.core.nautilus_pyo3 import OrderBookDeltas
from nautilus_trader.core.nautilus_pyo3 import OrderCanceled
from nautilus_trader.core.nautilus_pyo3 import OrderCancelRejected
from nautilus_trader.core.nautilus_pyo3 import OrderDenied
from nautilus_trader.core.nautilus_pyo3 import OrderEmulated
from nautilus_trader.core.nautilus_pyo3 import OrderExpired
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import OrderInitialized
from nautilus_trader.core.nautilus_pyo3 import OrderModifyRejected
from nautilus_trader.core.nautilus_pyo3 import OrderPendingCancel
from nautilus_trader.core.nautilus_pyo3 import OrderPendingUpdate
from nautilus_trader.core.nautilus_pyo3 import OrderRejected
from nautilus_trader.core.nautilus_pyo3 import OrderReleased
from nautilus_trader.core.nautilus_pyo3 import OrderSubmitted
from nautilus_trader.core.nautilus_pyo3 import OrderTriggered
from nautilus_trader.core.nautilus_pyo3 import OrderUpdated
from nautilus_trader.core.nautilus_pyo3 import OptionContract
from nautilus_trader.core.nautilus_pyo3 import OptionSpread
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import SyntheticInstrument
from nautilus_trader.core.nautilus_pyo3 import TradeTick


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