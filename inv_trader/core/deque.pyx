#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="deque.pyx" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython cimport list
from inv_trader.core.precondition cimport Precondition


cdef class Deque(list):
    """
    Represents a double ended bounded queue strongly typed to handle double
    precision floating point numbers.
    """

    def __init__(self, int maxlen):
        """
        Initializes a new instance of the DequeDouble class.

        :param maxlen: The maximum length of the queue (> 0).
        """
        Precondition.positive(maxlen, 'maxlen')

        super().__init__()
        self.maxlen = maxlen

    cpdef void appendright(self, x):
        """
        Append the given value to the end of the deque.
        
        :param x: The value to append.
        """
        if self.__len__() == self.maxlen:
            self.pop(0)
        self.append(x)

    cpdef void appendleft(self, x):
        """
        Append the given value to the start of the deque.
        
        :param x: The value to append.
        """
        if self.__len__() == self.maxlen:
            self.pop(self.__len__() - 1)
        self.insert(0, x)

    cpdef bint is_empty(self):
        """
        :return: A value indicating whether the deque is empty.
        """
        return self == []


cdef class DequeDouble(list):
    """
    Represents a double ended bounded queue strongly typed to handle double
    precision floating point numbers.
    """

    def __init__(self, int maxlen):
        """
        Initializes a new instance of the DequeDouble class.

        :param maxlen: The maximum length of the queue (> 0).
        """
        Precondition.positive(maxlen, 'maxlen')

        super().__init__()
        self.maxlen = maxlen

    cpdef void appendright(self, double x):
        """
        Append the given value to the end of the deque.
        
        :param x: The value to append.
        """
        if self.__len__() == self.maxlen:
            self.pop(0)
        self.append(x)

    cpdef void appendleft(self, double x):
        """
        Append the given value to the start of the deque.
        
        :param x: The value to append.
        """
        if self.__len__() == self.maxlen:
            self.pop(self.__len__() - 1)
        self.insert(0, x)

    cpdef bint is_empty(self):
        """
        :return: A value indicating whether the deque is empty.
        """
        return self == []
