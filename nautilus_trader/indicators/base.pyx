from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick


cdef class Indicator:
    """
    The base class for all indicators.

    Parameters
    ----------
    params : list
        The initialization parameters for the indicator.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, list params not None):
        self._params = params.copy()

        self.name = type(self).__name__
        self.has_inputs = False
        self.initialized = False

    def __repr__(self) -> str:
        return f"{self.name}({self._params_str()})"

    cdef str _params_str(self):
        return str(self._params)[1:-1].replace("'", '') if self._params else ''

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError(f"Cannot handle {repr(tick)}: method `handle_quote_tick` not implemented in subclass")  # pragma: no cover

    cpdef void handle_trade_tick(self, TradeTick tick):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError(f"Cannot handle {repr(tick)}: method `handle_trade_tick` not implemented in subclass")  # pragma: no cover

    cpdef void handle_bar(self, Bar bar):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError(f"Cannot handle {repr(bar)}: method `handle_bar` not implemented in subclass")  # pragma: no cover

    cpdef void reset(self):
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        self._reset()
        self.has_inputs = False
        self.initialized = False

    cpdef void _set_has_inputs(self, bint setting):
        self.has_inputs = setting

    cpdef void _set_initialized(self, bint setting):
        self.initialized = setting

    cpdef void _reset(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `_reset` must be implemented in the subclass")  # pragma: no cover
