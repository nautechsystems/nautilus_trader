#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, nonecheck=False

import pandas as pd
import pytz


cpdef str pad_string(str string, int length):
    """
    Return the given string padded to the given length.

    :param string: The string to pad.
    :param length: The length to pad to.
    :return: str.
    """
    return ((length - len(string)) * ' ') + string


cpdef str format_zulu_datetime(datetime dt):
    """
    Return the formatted string from the given datetime.
    
    :param dt: The datetime to format.
    :return: str.
    """
    cdef formatted_dt = ''
    try:
        formatted_dt = dt.isoformat(timespec='microseconds').partition('+')[0][:-3]
    except TypeError as ex:
        formatted_dt = dt.isoformat().partition('+')[0][:-3]
    if not formatted_dt.__contains__('.'):
        return formatted_dt + ':00.000Z'
    else:
        return formatted_dt + 'Z'


cpdef object with_utc_index(dataframe):
        """
        Return the given pandas dataframe with the index timestamps localized 
        or converted to UTC. If the dataframe is None then returns None.
        
        :param dataframe: The pd.Dataframe to localize.
        :return: pd.DataFrame or None.
        """
        if dataframe is not None:
            if not hasattr(dataframe.index, 'tz') or dataframe.index.tz is None:  # tz-naive
                return dataframe.tz_localize('UTC')
            elif dataframe.index.tz != pytz.UTC:
                return dataframe.tz_convert('UTC')
            else:
                return dataframe  # Already UTC

        return dataframe  # The input argument was None


cpdef object as_utc_timestamp(datetime timestamp):
    """
    Return the given timestamp converted to a pandas timestamp and UTC as required.
    
    :param timestamp: The timestamp to convert.
    :return: pd.Timestamp.
    """
    if not isinstance(timestamp, pd.Timestamp):
        timestamp = pd.Timestamp(timestamp)

    if timestamp.tz is None:  # tz-naive
        return timestamp.tz_localize('UTC')
    elif timestamp.tz != pytz.UTC:
        return timestamp.tz_convert('UTC')
    else:
        return timestamp  # Already UTC


cpdef float basis_points_as_percentage(float basis_points):
    """
    Return the given basis points expressed as a percentage where 100% = 1.0.
    
    :param basis_points: The basis points to convert to percentage.
    :return: float.
    """
    return basis_points * 0.0001
