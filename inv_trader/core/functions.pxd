#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime


cdef inline str format_zulu_datetime(datetime dt, str timespec=None):
    """
    Return the formatted string from the given datetime.
    
    :param dt: The datetime to format.
    :param timespec: The timespec for formatting.
    :return: str.
    """
    if timespec:
        try:
            return dt.isoformat(timespec=timespec).partition('+')[0] + 'Z'
        except TypeError as ex:
            return dt.isoformat().partition('+')[0] + '.000Z'
    else:
        return dt.isoformat().partition('+')[0] + 'Z'
