# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import pandas as pd
from cpython.datetime cimport datetime, timedelta
from enum import Enum

from nautilus_trader.core.functions cimport slice_dataframe
from nautilus_trader.core.datetime cimport ensure_utc_timestamp, ensure_utc_index


class ForexSession(Enum):
    UNDEFINED = 0,
    SYDNEY = 1,
    TOKYO = 2,
    EUROPE = 3,
    US = 4,


cdef class TimeRange:
    """
    Represents a time range with a start and end point.
    """

    cdef readonly datetime start
    cdef readonly datetime end

    def __init__(self, datetime start, datetime end):
        """
        Initializes a new instance of the TimeRange class.

        Parameters
        ----------
        start : datetime
            The start of the range.
        end : datetime
            The end of the range.

        """
        ensure_utc_timestamp(start)
        ensure_utc_timestamp(end)

        self.start = start
        self.end = end


class TimeSeriesFilter:

    @staticmethod
    def filter_forex_session(data, session):
        """
        Filter the given data by the given session. If the DatetimeIndex is not
        tz-aware UTC then it will be converted as such.

        Sydney Session  0700-1800 AEST   Monday to Friday
        Tokyo Session   0900-1800 Japan  Monday to Friday
        Europe Session  0800-1600 UTC    Monday to Friday
        US Session      0800-1700 EST    Monday to Friday

        Parameters
        ----------
        data : pd.Series or pd.DataFrame
            The time series data to filter.
        session : ForexSession
            The session to filter on.

        Returns
        -------
        pd.DataFrame
            The filtered data with tz-aware UTC index.

        """
        assert isinstance(session, ForexSession), "The session was not of type ForexSession."
        assert session != ForexSession.UNDEFINED, "The session was UNDEFINED."

        # DatetimeIndex to UTC
        data = ensure_utc_index(data)

        if session == ForexSession.SYDNEY:
            sydney_session = data.tz_convert('Australia/Sydney')
            sydney_session = sydney_session[sydney_session.index.dayofweek < 5]
            sydney_session = sydney_session[sydney_session.index.hour >= 7]
            sydney_session = sydney_session[sydney_session.index.hour <= 18]
            sydney_session = sydney_session.tz_convert('UTC')
            return sydney_session

        if session == ForexSession.TOKYO:
            tokyo_session = data.tz_convert('Japan')
            tokyo_session = tokyo_session[tokyo_session.index.dayofweek < 5]
            tokyo_session = tokyo_session[tokyo_session.index.hour >= 9]
            tokyo_session = tokyo_session[tokyo_session.index.hour <= 18]
            tokyo_session = tokyo_session.tz_convert('UTC')
            return tokyo_session

        if session == ForexSession.EUROPE:
            europe_session = data.tz_convert('GMT')
            europe_session = europe_session[europe_session.index.dayofweek < 5]
            europe_session = europe_session[europe_session.index.hour >= 8]
            europe_session = europe_session[europe_session.index.hour <= 16]
            europe_session.tz_convert('UTC')
            return europe_session

        if session == ForexSession.US:
            us_session = data.tz_convert('EST')
            us_session = us_session[us_session.index.dayofweek < 5]
            us_session = us_session[us_session.index.hour >= 8]
            us_session = us_session[us_session.index.hour <= 17]
            us_session = us_session.tz_convert('UTC')
            return us_session

        raise ValueError(f'Cannot filter given data (did not recognize given session \'{session.name}\').')

    @staticmethod
    def filter_forex_sessions(data, list sessions):
        """
        Filter the given data by the given sessions. If the DatetimeIndex is not
        tz-aware UTC then it will be converted as such.

        Sydney Session  0700-1800 AEST   Monday to Friday
        Tokyo Session   0900-1800 Japan  Monday to Friday
        Europe Session  0800-1600 UTC    Monday to Friday
        US Session      0800-1700 EST    Monday to Friday

        Parameters
        ----------
        data : pd.Series or pd.DataFrame
            The time series data to filter.
        sessions : list of ForexSession
            The session to filter on.

        Returns
        -------
        pd.DataFrame
            The filtered data with tz-aware UTC index.

        """
        # DatetimeIndex to UTC
        data = ensure_utc_index(data)

        cdef list columns = list(data.columns)
        cdef set unique_sessions = set(sessions)

        filtered_data = None

        for session in sessions:
            filtered_session = TimeSeriesFilter.filter_forex_session(data, session)
            if filtered_data is None:
                filtered_data = filtered_session
            else:
                filtered_data = filtered_data.merge(
                    filtered_session,
                    on=columns,
                    left_index=True,
                    right_index=True,
                    how='outer')

        assert filtered_data.index.is_monotonic_increasing, "data is not monotonically increasing"
        assert not filtered_data.isnull().values.any(), "some values are null"

        return filtered_data

    @staticmethod
    def filter_time_ranges(data, list time_ranges):
        """
        Filter the given data by the given time ranges. If the DatetimeIndex is
        not tz-aware UTC then it will be converted as such.

        Parameters
        ----------
        data : pd.Series or pd.DataFrame
            The time series data to filter.
        time_ranges : list of TimeRange
            The ranges to filter out.

        Returns
        -------
        pd.DataFrame
            The filtered data with tz-aware UTC index.

        """
        # DatetimeIndex to UTC
        data = ensure_utc_index(data)

        cdef list data_blocks = []
        cdef datetime previous_end = data.index[0]

        cdef TimeRange tr
        for tr in time_ranges:
            data_blocks.append(slice_dataframe(data, previous_end, tr.start))
            previous_end = tr.end

        return pd.concat(data_blocks)

    @staticmethod
    def filter_economic_events(
            data,
            events,
            list currencies,
            str impact,
            timedelta offset_before,
            timedelta offset_after):
        """
        Filter the given data by the given economic news events by currency and
        impact creating time ranges based on the specified before and after
        offsets.

        Parameters
        ----------
        data : pd.DataFrame or pd.Series
            The data to filter.
        events : pd.DataFrame or pd.Series
            The economic news events for the filter operation.
        currencies : list of str
            The currencies to include for the filter operation.
        impact : str
            The impact level for the filter operation.
        offset_before : timedelta
            The offset for the start of the filter time ranges.
        offset_after
            The offset for the end of the filter time ranges.

        Returns
        -------
        pd.DataFrame
            The filtered data with tz-aware UTC index.

        """
        # DatetimeIndex to UTC
        data = ensure_utc_index(data)
        events = ensure_utc_index(events)

        cdef list economic_events = list(events[(events.currency.isin(currencies))
                                              & (events.impact == impact)].index)

        time_ranges = []
        for datetime in economic_events:
            time_ranges.append(TimeRange(datetime - offset_before, datetime + offset_after))

        return TimeSeriesFilter.filter_time_ranges(data, time_ranges)
