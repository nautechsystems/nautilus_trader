#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="msgpack.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from bson import BSON
from bson.raw_bson import RawBSONDocument

from nautilus_trader.model.objects cimport Symbol, Price, Tick, Bar

cdef str UTF8 = 'utf-8'


cdef class DataSerializer:
    """
    Provides a serializer for data objects.
    """

    cpdef object serialize_ticks(self, list ticks):
        """
        Serialize the given tick.
        
        :param ticks: The ticks to serialize.
        :return: RawBSONDocument.
        """
        return RawBSONDocument(BSON.encode({
            "Symbol": ticks[0].symbol.value,
            "Values": [tick.values_str().encode(UTF8) for tick in ticks]}))
