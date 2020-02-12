# -------------------------------------------------------------------------------------------------
# <copyright file="deque.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from libc.stdlib cimport malloc, free


cdef class Deque:

    def __init__(self, unsigned int maxlen):
        self.maxlen = maxlen
        self._deque = <deque_type*>malloc(maxlen)
        self._deque = deque_alloc()
        if not self._deque:
            raise MemoryError()

    def __dealloc__(self):
        free(self._deque)
