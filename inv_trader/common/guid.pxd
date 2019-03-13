#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="guid.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.model.identifiers cimport GUID


cdef class GuidFactory:
    """
    The abstract base class for all GUID factories.
    """

    cpdef GUID generate(self)


cdef class TestGuidFactory(GuidFactory):
    """
    Provides a fake GUID factory for testing purposes.
    """
    cdef GUID _guid



cdef class LiveGuidFactory(GuidFactory):
    """
    Provides a GUID factory for live trading. Generates actual GUIDs based on
    Pythons UUID4.
    """
    pass
