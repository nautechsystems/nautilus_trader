# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

cdef extern from "deque.h":

    ctypedef struct deque_type:
        pass

    ctypedef int deque_val_type

    deque_type* deque_alloc()
    bint deque_is_empty(deque_type *d)
    void deque_free(deque_type *d)
    void deque_push_front(deque_type *d, deque_val_type v)
    void deque_push_back(deque_type *d, deque_val_type v)
    deque_val_type deque_pop_front(deque_type *d)
    deque_val_type deque_pop_back(deque_type *d)
    deque_val_type deque_peek_front(deque_type *d)
    deque_val_type deque_peek_back(deque_type *d)


cdef class Deque:
    cdef deque_type* _deque

    cdef readonly unsigned int maxlen
