# -------------------------------------------------------------------------------------------------
# <copyright file="generators.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.identifiers cimport IdTag, OrderId, PositionId
from nautilus_trader.common.clock cimport Clock


cdef class IdentifierGenerator:
    """
    Provides a generator for unique order identifiers.
    """
    cdef Clock _clock

    cdef readonly str prefix
    cdef readonly IdTag id_tag_trader
    cdef readonly IdTag id_tag_strategy
    cdef readonly int counter

    cpdef void reset(self)

    cdef str _generate(self)
    cdef str _get_datetime_tag(self)


cdef class OrderIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique OrderIds.
    """
    cpdef OrderId generate(self)


cdef class PositionIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique PositionIds.
    """
    cpdef PositionId generate(self)
