
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class MoneyFlowIndex(Indicator):
    """
    The Money Flow Index (MFI) indicator.
    
    A momentum indicator that uses both price and volume to measure
    buying and selling pressure. It is also known as volume-weighted RSI.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).

    """
    
    def __cinit__(self):
        self.period = 0
        self.value = 0.0

    def __init__(self, int period):
        Condition.positive_int(period, "period")
        from nautilus_pyo3.nautilus_pyo3 import MoneyFlowIndex as _MFI
        self._inner = _MFI(period)
        super().__init__(params=[period])
        self._update_readonly_attributes()

    def __repr__(self):
        return f"{self.__class__.__name__}({self.period})"

    @property
    def name(self) -> str:
        """Return the indicator name."""
        return self.__class__.__name__

    @property
    def has_inputs(self) -> bool:
        """Return whether the indicator has received inputs."""
        return self._inner.has_inputs
    
    cpdef void _update_readonly_attributes(self):
        """Update the readonly attributes."""
        self.period = self._inner.period
        self.value = self._inner.value
        self._set_initialized(self._inner.initialized)
        self._set_has_inputs(self._inner.has_inputs)

    cpdef void update_raw(self, double typical_price, double volume):
        """Update the indicator with raw typical price and volume."""
        self._inner.update_raw(typical_price, volume)
        self._update_readonly_attributes()

    def update(self, double close, double high, double low, double volume):
        """Update the indicator with OHLC and volume."""
        result = self._inner.update(close, high, low, volume)
        self._update_readonly_attributes()
        return result

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.
        """
        cdef double typical = (bar.high.as_double() + bar.low.as_double() + bar.close.as_double()) / 3.0
        self.update_raw(typical, bar.volume.as_double())

    cpdef void reset(self):
        """Reset the indicator state."""
        self._inner.reset()
        self.value = 0.0
        self._set_initialized(False)
        self._set_has_inputs(False)


