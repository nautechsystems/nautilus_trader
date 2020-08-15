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
from cpython.datetime cimport datetime, timedelta
from enum import Enum, unique

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport as_utc_index, is_datetime_utc
from nautilus_trader import PACKAGE_ROOT


@unique
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
        self.tz_tokyo = pytz.timezone('Asia/Tokyo')
        self.tz_london = pytz.timezone('Europe/London')
        self.tz_new_york = pytz.timezone('America/New_York')

    cpdef datetime local_from_utc(self, session: ForexSession, datetime time_now):
        """
        Return the local datetime from the given session and time_now (UTC).

        Parameters
        ----------
        session : ForexSession
            The session for the local timezone conversion.
        time_now : datetime
            The time now (UTC).

        Returns
        -------
        datetime
            The converted local datetime.

        Raises
        ------
        ValueError
            If time_now is not tz aware UTC.

        """
        Condition.type(session, ForexSession, 'session')
        Condition.true(is_datetime_utc(time_now), 'time_now is tz aware UTC')

        if session == ForexSession.SYDNEY:
            return time_now.astimezone(self.tz_sydney)

        if session == ForexSession.TOKYO:
            return time_now.astimezone(self.tz_tokyo)

        if session == ForexSession.LONDON:
            return time_now.astimezone(self.tz_london)

        if session == ForexSession.NEW_YORK:
            return time_now.astimezone(self.tz_new_york)

    cpdef datetime next_start(self, session: ForexSession, datetime time_now):
        """
        Returns the next session start.

        All FX sessions run Monday to Friday local time.

        Sydney Session    0700-1600 'Australia/Sydney'

        Tokyo Session     0900-1800 'Asia/Tokyo'

        London Session    0800-1600 'Europe/London'

        New York Session  0800-1700 'America/New_York'

        Parameters
        ----------
        session : ForexSession
            The session for the start datetime.
        time_now : datetime
            The datetime now.

        Returns
        -------
        datetime

        Raises
        ------
        ValueError
            If time_now is not tz aware UTC.

        """
        Condition.type(session, ForexSession, 'session')
        Condition.true(is_datetime_utc(time_now), 'time_now is tz aware UTC')

        cdef datetime local_now = self.local_from_utc(session, time_now)
        cdef datetime next_start

        # Local days session start
        if session == ForexSession.SYDNEY:
            next_start = self.tz_sydney.localize(datetime(local_now.year, local_now.month, local_now.day, 7))
        elif session == ForexSession.TOKYO:
            next_start = self.tz_tokyo.localize(datetime(local_now.year, local_now.month, local_now.day, 9))
        elif session == ForexSession.LONDON:
            next_start = self.tz_london.localize(datetime(local_now.year, local_now.month, local_now.day, 8))
        elif session == ForexSession.NEW_YORK:
            next_start = self.tz_new_york.localize(datetime(local_now.year, local_now.month, local_now.day, 8))

        # Already past this days session start
        if local_now > next_start:
            next_start += timedelta(days=1)

        # Weekend - next session start becomes next Mondays session start
        if next_start.weekday() > 4:
            diff = 7 - next_start.weekday()
            next_start += timedelta(days=diff)

        return next_start.astimezone(pytz.utc)

    cpdef datetime prev_start(self, session: ForexSession, datetime time_now):
        """
        Returns the previous session start.

        All FX sessions run Monday to Friday local time.

        Sydney Session    0700-1600 'Australia/Sydney'

        Tokyo Session     0900-1800 'Asia/Tokyo'

        London Session    0800-1600 'Europe/London'

        New York Session  0800-1700 'America/New_York'

        Parameters
        ----------
        session : ForexSession
            The session for the start datetime.
        time_now : datetime
            The datetime now.

        Returns
        -------
        datetime

        Raises
        ------
        ValueError
            If time_now is not tz aware UTC.

        """
        Condition.type(session, ForexSession, 'session')
        Condition.true(is_datetime_utc(time_now), 'time_now is tz aware UTC')

        cdef datetime local_now = self.local_from_utc(session, time_now)
        cdef datetime prev_start

        # Local days session start
        if session == ForexSession.SYDNEY:
            prev_start = self.tz_sydney.localize(datetime(local_now.year, local_now.month, local_now.day, 7))
        elif session == ForexSession.TOKYO:
            prev_start = self.tz_tokyo.localize(datetime(local_now.year, local_now.month, local_now.day, 9))
        elif session == ForexSession.LONDON:
            prev_start = self.tz_london.localize(datetime(local_now.year, local_now.month, local_now.day, 8))
        elif session == ForexSession.NEW_YORK:
            prev_start = self.tz_new_york.localize(datetime(local_now.year, local_now.month, local_now.day, 8))

        # Prior to this days session start
        if local_now < prev_start:
            prev_start -= timedelta(days=1)

        # Weekend - previous session start becomes last Fridays session start
        if prev_start.weekday() > 4:
            diff = prev_start.weekday() - 4
            prev_start -= timedelta(days=diff)

        return prev_start.astimezone(pytz.utc)

    cpdef datetime next_end(self, session: ForexSession, datetime time_now):
        """
        Returns the next session end.

        All FX sessions run Monday to Friday local time.

        Sydney Session    0700-1600 'Australia/Sydney'

        Tokyo Session     0900-1800 'Asia/Tokyo'

        London Session    0800-1600 'Europe/London'

        New York Session  0800-1700 'America/New_York'

        Parameters
        ----------
        session : ForexSession
            The session for the end datetime.
        time_now : datetime
            The datetime now (UTC).

        Returns
        -------
        datetime

        Raises
        ------
        ValueError
            If time_now is not tz aware UTC.

        """
        Condition.type(session, ForexSession, 'session')
        Condition.true(is_datetime_utc(time_now), 'time_now is tz aware UTC')

        cdef datetime local_now = self.local_from_utc(session, time_now)
        cdef datetime next_end

        # Local days session end
        if session == ForexSession.SYDNEY:
            next_end = self.tz_sydney.localize(datetime(local_now.year, local_now.month, local_now.day, 16))
        elif session == ForexSession.TOKYO:
            next_end = self.tz_tokyo.localize(datetime(local_now.year, local_now.month, local_now.day, 18))
        elif session == ForexSession.LONDON:
            next_end = self.tz_london.localize(datetime(local_now.year, local_now.month, local_now.day, 16))
        elif session == ForexSession.NEW_YORK:
            next_end = self.tz_new_york.localize(datetime(local_now.year, local_now.month, local_now.day, 17))

        # Already past this days session end
        if local_now > next_end:
            next_end += timedelta(days=1)

        # Weekend - next session end becomes last Mondays session end
        if next_end.weekday() > 4:
            diff = 7 - next_end.weekday()
            next_end += timedelta(days=diff)

        return next_end.astimezone(pytz.utc)

    cpdef datetime prev_end(self, session: ForexSession, datetime time_now):
        """
        Returns the previous sessions end.

        All FX sessions run Monday to Friday local time.

        Sydney Session    0700-1600 'Australia/Sydney'

        Tokyo Session     0900-1800 'Asia/Tokyo'

        London Session    0800-1600 'Europe/London'

        New York Session  0800-1700 'America/New_York'

        Parameters
        ----------
        session : ForexSession
            The session for end datetime.
        time_now : datetime
            The datetime now.

        Returns
        -------
        datetime

        Raises
        ------
        ValueError
            If time_now is not tz aware UTC.

        """
        Condition.type(session, ForexSession, 'session')
        Condition.true(is_datetime_utc(time_now), 'time_now is tz aware UTC')

        cdef datetime local_now = self.local_from_utc(session, time_now)
        cdef datetime prev_end

        # Local days session end
        if session == ForexSession.SYDNEY:
            prev_end = self.tz_sydney.localize(datetime(local_now.year, local_now.month, local_now.day, 16))
        elif session == ForexSession.TOKYO:
            prev_end = self.tz_tokyo.localize(datetime(local_now.year, local_now.month, local_now.day, 18))
        elif session == ForexSession.LONDON:
            prev_end = self.tz_london.localize(datetime(local_now.year, local_now.month, local_now.day, 16))
        elif session == ForexSession.NEW_YORK:
            prev_end = self.tz_new_york.localize(datetime(local_now.year, local_now.month, local_now.day, 17))

        # Prior to this days session end
        if local_now < prev_end:
            prev_end -= timedelta(days=1)

        # Weekend - previous session end becomes Fridays session end
        if prev_end.weekday() > 4:
            diff = prev_end.weekday() - 4
            prev_end -= timedelta(days=diff)

        return prev_end.astimezone(pytz.utc)


@unique
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

        Parameters
        ----------
        currencies : list of str
            The list of three letter currency symbols to filter.
        impacts : list of str
            The list of impact levels to filter ('LOW', 'MEDIUM', 'HIGH').
        news_csv_path : str
            The path to the news data csv.

        """
        if news_csv_path == 'default':
            news_csv_path = os.path.join(PACKAGE_ROOT + '/_data/news/', 'news_events.csv')

        self.currencies = currencies
        self.impacts = impacts

        news_data = as_utc_index(pd.read_csv(news_csv_path, parse_dates=True, index_col=0))
        self.unfiltered_data_start = news_data.index[0]
        self.unfiltered_data_end = news_data.index[-1]

        self._news_data = news_data[(news_data['Currency'].isin(currencies))
                                   & news_data['Impact'].isin(impacts)]  # noqa (W504) easier to read

    cpdef NewsEvent next_event(self, datetime time_now):
        """
        Returns the next news event matching the filter conditions.
        Will return None if no news events match the filter conditions.

        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        NewsEvent or None
            The next news event in the filtered data if any.

        Raises
        ------
        ValueError
            The time_now < self.unfiltered_data_start
        ValueError
            The time_now > self.unfiltered_data_end
        ValueError
            If time_now is not tz aware UTC.

        """
        Condition.true(is_datetime_utc(time_now), 'time_now is tz aware UTC')

        if time_now < self.unfiltered_data_start:
            raise ValueError(f"The given time_now at {time_now} was prior to the "
                             f"available news data start at {self.unfiltered_data_start}.")

        if time_now > self.unfiltered_data_end:
            raise ValueError(f"The given time_now at {time_now} was after the "
                             f"available news data end at {self.unfiltered_data_end}.")

        events = self._news_data[self._news_data.index >= time_now]

        if events.empty:
            return None

        cdef int index = 0
        row = events.iloc[index]
        return NewsEvent(events.index[index], row['Impact'], row['Name'], row['Currency'])

    cpdef NewsEvent prev_event(self, datetime time_now):
        """
        Returns the previous news event matching the initial filter conditions.
        Will return None if no news events match the filter conditions.

        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        NewsEvent or None
            The previous news event in the filtered data if any.

        Raises
        ------
        ValueError
            The time_now < self.unfiltered_data_start
        ValueError
            The time_now > self.unfiltered_data_end
        ValueError
            If time_now is not tz aware UTC.

        """
        Condition.true(is_datetime_utc(time_now), 'time_now is tz aware UTC')

        if time_now < self.unfiltered_data_start:
            raise ValueError(f"The given time_now at {time_now} was prior to the "
                             f"available news data start at {self.unfiltered_data_start}.")

        if time_now > self.unfiltered_data_end:
            raise ValueError(f"The given time_now at {time_now} was after the "
                             f"available news data end at {self.unfiltered_data_end}.")

        events = self._news_data[self._news_data.index <= time_now]
        if events.empty:
            return None

        cdef int index = -1
        row = events.iloc[index]
        return NewsEvent(events.index[index], row['Impact'], row['Name'], row['Currency'])
