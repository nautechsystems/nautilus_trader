from cpython.datetime cimport datetime

from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.averages cimport MovingAverage
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport TradeTick


cdef class OnBalanceVolume(Indicator):
    cdef object _obv

    cdef readonly int period
    cdef readonly double value

    cpdef void update_raw(self, double open, double close, double volume)


cdef class VolumeWeightedAveragePrice(Indicator):
    cdef int _day
    cdef double _price_volume
    cdef double _volume_total

    cdef readonly double value

    cpdef void update_raw(self, double price, double volume, datetime timestamp)


cdef class KlingerVolumeOscillator(Indicator):
    cdef MovingAverage _fast_ma
    cdef MovingAverage _slow_ma
    cdef MovingAverage _signal_ma
    cdef double _hlc3
    cdef double _previous_hlc3

    cdef readonly int fast_period
    cdef readonly int slow_period
    cdef readonly int signal_period
    cdef readonly double value

    cpdef void update_raw(self, double high, double low, double close, double volume)


cdef class Pressure(Indicator):
    cdef object _atr
    cdef MovingAverage _average_volume

    cdef readonly int period
    cdef readonly double value
    cdef readonly double value_cumulative

    cpdef void update_raw(self, double high, double low, double close, double volume)
