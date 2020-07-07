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

import os
import pytz
import pandas as pd
from cpython.datetime cimport datetime
from enum import Enum

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport ensure_utc_timestamp, ensure_utc_index
from nautilus_trader import PACKAGE_ROOT


class ForexSession(Enum):
    UNDEFINED = 0,
    SYDNEY = 1,
    TOKYO = 2,
    LONDON = 3,
    NEW_YORK = 4


cdef class ForexSessionFilter:
    """
    Provides methods to help filter trading strategy rules dependant on Forex session times.
    """

    def __init__(self):
        self.tz_sydney = pytz.timezone('Australia/Sydney')
        self.tz_tokyo = pytz.timezone('Japan')
        self.tz_london = pytz.timezone('Europe/London')
        self.tz_new_york = pytz.timezone('EST')

    cpdef bint is_sydney_session(self, datetime time_now):
        """
        Return a value indicating whether the given time_now is within the FX session.
        Sydney Session  0700-1600 'Australia/Sydney' Monday to Friday.
        
        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        bool
            True if time_now is in session, else False.
            
        """
        time_now = ensure_utc_timestamp(time_now)

        cdef datetime local = self.tz_sydney.fromutc(time_now)
        return local.day <= 4 and 7 <= local.hour <= 16

    cpdef bint is_tokyo_session(self, datetime time_now):
        """
        Return a value indicating whether the given time_now is within the FX session.
        Tokyo Session 0900-1800 'Japan' Monday to Friday.

        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        bool
            True if time_now is in session, else False.
            
        """
        time_now = ensure_utc_timestamp(time_now)

        cdef datetime local = self.tz_tokyo.fromutc(time_now)
        return local.day <= 4 and 9 <= local.hour <= 18

    cpdef bint is_london_session(self, datetime time_now):
        """
        Return a value indicating whether the given time_now is within the FX session.
        London Session 0800-1600 'Europe/London' Monday to Friday.

        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        bool
            True if time_now is in session, else False.
            
        """
        time_now = ensure_utc_timestamp(time_now)

        cdef datetime local = self.tz_london.fromutc(time_now)
        return local.day <= 4 and 8 <= local.hour <= 16

    cpdef bint is_new_york_session(self, datetime time_now):
        """
        Return a value indicating whether the given time_now is within the FX session.
        New York Session 0800-1700 'EST' Monday to Friday.

        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        bool
            True if time_now is in session, else False.

        """
        time_now = ensure_utc_timestamp(time_now)

        cdef datetime local = self.tz_new_york.fromutc(time_now)
        return local.day <= 4 and 8 <= local.hour <= 17

    cpdef datetime session_start(self, session: ForexSession, datetime datum):
        """
        Returns the local days session start. If datum is a local weekend then returns None.
        
        Parameters
        ----------
        session : ForexSession
            The session for the calculation.
        datum : datetime
            The datum datetime.

        Returns
        -------
        datetime
            The UTC time for the local days session start.
            
        """
        Condition.type(session, ForexSession, 'session')

        datum = ensure_utc_timestamp(datum)

        cdef datetime local
        if session == ForexSession.SYDNEY:
            local = self.tz_sydney.fromutc(datum)

            if local.weekday() <= 4:
                return datetime(local.year, local.month, local.day, 7).tz_convert('UTC')
            else:
                return None

        if session == ForexSession.TOKYO:
            local = self.tz_tokyo.fromutc(datum)

            if local.weekday() <= 4:
                return datetime(local.year, local.month, local.day, 9).tz_convert('UTC')
            else:
                return None

        if session == ForexSession.LONDON:
            local = self.tz_london.fromutc(datum)

            if local.weekday() <= 4:
                return datetime(local.year, local.month, local.day, 8).tz_convert('UTC')
            else:
                return None

        if session == ForexSession.NEW_YORK:
            local = self.tz_new_york.fromutc(datum)

            if local.weekday() <= 4:
                return datetime(local.year, local.month, local.day, 8).tz_convert('UTC')
            else:
                return None

    cpdef datetime session_end(self, session: ForexSession, datetime datum):
        """
        Returns the local days session end. If datum is a local weekend then returns None.
        
        Parameters
        ----------
        session : ForexSession
            The session for the calculation.
        datum : datetime
            The datum datetime.

        Returns
        -------
        datetime
            The UTC time for the local days session end.
            
        """
        Condition.type(session, ForexSession, 'session')

        datum = ensure_utc_timestamp(datum)

        cdef datetime local
        if session == ForexSession.SYDNEY:
            local = self.tz_sydney.fromutc(datum)

            if local.weekday() <= 4:
                return datetime(local.year, local.month, local.day, 16).tz_convert('UTC')
            else:
                return None

        if session == ForexSession.TOKYO:
            local = self.tz_tokyo.fromutc(datum)

            if local.weekday() <= 4:
                return datetime(local.year, local.month, local.day, 18).tz_convert('UTC')
            else:
                return None

        if session == ForexSession.LONDON:
            local = self.tz_london.fromutc(datum)

            if local.weekday() <= 4:
                return datetime(local.year, local.month, local.day, 16).tz_convert('UTC')
            else:
                return None

        if session == ForexSession.NEW_YORK:
            local = self.tz_new_york.fromutc(datum)

            if local.weekday() <= 4:
                return datetime(local.year, local.month, local.day, 17).tz_convert('UTC')
            else:
                return None


class NewsImpact(Enum):
    UNDEFINED = 0,
    NONE = 1,
    LOW = 2,
    MEDIUM = 3,
    HIGH = 4


cdef class NewsEvent:
    """
    Represents an economic news event.
    """

    def __init__(
            self,
            datetime timestamp,
            impact,
            name,
            currency):
        """

        Parameters
        ----------
        timestamp : datetime
            The timestamp for the start of the economic news event.
        impact : NewsImpact
            The expected impact for the economic news event.
        name : str
            The name of the economic news event.
        currency : str
            The currency the economic news event is expected to affect.
        """
        self.timestamp = timestamp
        self.impact = impact
        self.name = name
        self.currency = currency


cdef class EconomicNewsEventFilter:
    """
    Provides methods to help filter trading strategy rules based on economic news events.
    """

    def __init__(
            self,
            list currencies not None,
            list impacts not None,
            str news_csv_path not None='default'):
        """
        Initializes a new instance of the EconomicNewsEventFilter class.

        :param news_csv_path: The path to the short term interest rate data csv.
        """
        if news_csv_path == 'default':
            news_csv_path = os.path.join(PACKAGE_ROOT + '/_data/news/', 'news_events.zip')

        self.currencies = currencies
        self.impacts = impacts

        news_data = ensure_utc_index(pd.read_csv(news_csv_path, parse_dates=True, index_col=0))

        self._news_data = news_data[(news_data['Currency'].isin(currencies))
                                   & news_data['Impact'].isin(impacts)]

    cpdef NewsEvent next_event(self, datetime time_now):
        """
        Returns the next news event matching the initial filter conditions. 
        If there is no next event then returns None.
        
        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        datetime or None
            The datetime of the next news event in the filtered data or None.

        """
        events = self._news_data[self._news_data.index >= ensure_utc_timestamp(time_now)]

        if events.empty:
            return None

        cdef int index = 0
        row = events.iloc[index]
        return NewsEvent(events.index[index], row['Impact'], row['Name'], row['Currency'])

    cpdef NewsEvent prev_event(self, datetime time_now):
        """
        Returns the previous news event matching the initial filter conditions. 
        If there is no next event then returns None.
        
        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        datetime or None
            The datetime of the previous news event in the filtered data or None.

        """
        events = self._news_data[self._news_data.index <= ensure_utc_timestamp(time_now)]
        if events.empty:
            return None

        cdef int index = -1
        row = events.iloc[index]
        return NewsEvent(events.index[index], row['Impact'], row['Name'], row['Currency'])
