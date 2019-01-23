#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="guid.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from uuid import uuid4

from inv_trader.model.identifiers cimport GUID


cdef class GuidFactory:
    """
    The abstract base class for all GUID factories.
    """

    cpdef GUID generate(self):
        """
        :return: A GUID.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented.")


cdef class TestGuidFactory(GuidFactory):
    """
    Provides a fake GUID factory for testing purposes.
    """

    def __init__(self):
        """
        Initializes a new instance of the TestGuidFactory class.
        """
        super().__init__()
        self._guid = GUID(uuid4())

    cpdef GUID generate(self):
        """
        :return: The single test GUID instance.
        """
        return self._guid


cdef class LiveGuidFactory(GuidFactory):
    """
    Provides a GUID factory for live trading. Generates actual GUIDs based on
    Pythons UUID4.
    """

    def __init__(self):
        """
        Initializes a new instance of the LiveGuidFactory class.
        """
        super().__init__()

    cpdef GUID generate(self):
        """
        :return: A new GUID.
        """
        return GUID(uuid4())
