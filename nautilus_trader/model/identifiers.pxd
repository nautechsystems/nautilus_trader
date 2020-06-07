# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.types cimport Identifier
from nautilus_trader.model.c_enums.account_type cimport AccountType


cdef class Symbol(Identifier):
    cdef readonly str code
    cdef readonly Venue venue
    @staticmethod
    cdef Symbol from_string(str value)


cdef class Venue(Identifier):
    pass


cdef class Exchange(Venue):
    pass


cdef class Brokerage(Identifier):
    pass


cdef class IdTag(Identifier):
    pass


cdef class TraderId(Identifier):
    cdef readonly str name
    cdef readonly IdTag order_id_tag
    @staticmethod
    cdef TraderId from_string(str value)


cdef class StrategyId(Identifier):
    cdef readonly str name
    cdef readonly IdTag order_id_tag
    @staticmethod
    cdef StrategyId from_string(str value)


cdef class AccountId(Identifier):
    cdef readonly Brokerage broker
    cdef readonly AccountNumber account_number
    cdef readonly AccountType account_type
    @staticmethod
    cdef AccountId from_string(str value)


cdef class AccountNumber(Identifier):
    pass


cdef class AtomicOrderId(Identifier):
    pass


cdef class OrderId(Identifier):
    pass


cdef class OrderIdBroker(Identifier):
    pass


cdef class PositionId(Identifier):
    pass


cdef class PositionIdBroker(Identifier):
    pass


cdef class ExecutionId(Identifier):
    pass


cdef class InstrumentId(Identifier):
    pass
