#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, nonecheck=False

from cpython.datetime cimport datetime


cpdef str pad_string(str string, int length)
cpdef str format_zulu_datetime(datetime dt)
cpdef object with_utc_index(dataframe)
cpdef object as_utc_timestamp(datetime timestamp)
cpdef float basis_points_as_percentage(float basis_points)
