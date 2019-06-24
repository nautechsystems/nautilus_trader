#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False


cdef class ObjectStorer:
    """"
    A test class which stores the given objects.
    """

    def __init__(self):
        """
        Initializes a new instance of the ObjectStorer class.
        """
        self._store = []

    cpdef list get_store(self):
        """"
        Return the list or stored objects.
        
        return: List[Object].
        """
        return self._store

    cpdef void store(self, object obj):
        """"
        Store the given object.
        """
        self.count += 1
        self._store.append(obj)

    cpdef void store_2(self, object obj1, object obj2):
        """"
        Store the given objects as a tuple.
        """
        self.store((obj1, obj2))
