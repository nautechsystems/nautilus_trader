from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.calculators cimport RolloverInterestCalculator
from nautilus_trader.backtest.engine cimport SimulatedExchange
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.data cimport Data


cdef class SimulationModule(Actor):
    cdef readonly SimulatedExchange exchange

    cpdef void register_venue(self, SimulatedExchange exchange)
    cpdef void pre_process(self, Data data)
    cpdef void process(self, uint64_t ts_now)
    cpdef void log_diagnostics(self, Logger logger)
    cpdef void reset(self)


cdef class FXRolloverInterestModule(SimulationModule):
    cdef RolloverInterestCalculator _calculator
    cdef object _rollover_spread
    cdef datetime _rollover_time
    cdef bint _rollover_applied
    cdef dict _rollover_totals
    cdef int _day_number

    cdef void _apply_rollover_interest(self, datetime timestamp, int iso_week_day)
