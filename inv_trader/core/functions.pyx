#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime
import pandas as pd


cpdef list pd_index_to_datetime_list(indexs):
    """
    TBA
    :param indexs: 
    :return: 
    """
    return list(map(pd_timestamp_to_datetime, pd.to_datetime(indexs, utc=True)))

cdef datetime pd_timestamp_to_datetime(index):
    """
    Return a datetime from the given pandas index.
    :param index: The index to convert.
    :return: The cpython datetime.
    """
    return datetime(index.year,
                    index.month,
                    index.day,
                    index.hour,
                    index.minute,
                    index.second,
                    tzinfo=index.tzinfo)
