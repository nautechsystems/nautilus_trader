#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime


cpdef list pd_index_to_datetime_list(indexs)
cdef datetime pd_timestamp_to_datetime(index)
