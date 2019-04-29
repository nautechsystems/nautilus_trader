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


cpdef object with_utc_index(dataframe)

cpdef object as_utc_timestamp(datetime timestamp)

cdef inline str format_zulu_datetime(datetime dt, str timespec=None):
    """
    Return the formatted string from the given datetime.
    
    :param dt: The datetime to format.
    :param timespec: The timespec for formatting.
    :return: str.
    """
    cdef formatted_dt = ''
    if timespec is not None:
        try:
            formatted_dt = dt.isoformat(timespec=timespec).partition('+')[0][:-3]
        except TypeError as ex:
            formatted_dt = dt.isoformat().partition('+')[0][:-3]
        if not formatted_dt.__contains__('.'):
            return formatted_dt + ':00.000Z'
        else:
            return formatted_dt + 'Z'
    else:
        return dt.isoformat().partition('+')[0] + 'Z'


cdef inline float basis_points_as_percentage(float basis_points):
    """
    Return the given basis points expressed as a percentage where 100% = 1.0.
    
    :param basis_points: The basis points to convert to percentage.
    :return: float.
    """
    return basis_points * 0.0001
