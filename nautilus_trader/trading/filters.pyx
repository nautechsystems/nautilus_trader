# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import datetime

from libc.stdint cimport uint64_t

from enum import Enum
from enum import unique

import pandas as pd
import pytz

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport is_datetime_utc


@unique
class ForexSession(Enum):
    SYDNEY = 1
    TOKYO = 2
    LONDON = 3
    NEW_YORK = 4


cdef class ForexSessionFilter:
    """
    Provides methods to help filter trading strategy rules dependent on Forex session times.
    """

    def __init__(self):
        self._tz_sydney = pytz.timezone("Australia/Sydney")
        self._tz_tokyo = pytz.timezone("Asia/Tokyo")
        self._tz_london = pytz.timezone("Europe/London")
        self._tz_new_york = pytz.timezone("America/New_York")

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
            If `time_now` is not tz aware UTC.

        """
        Condition.type(session, ForexSession, "session")
        Condition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        if session == ForexSession.SYDNEY:
            return time_now.astimezone(self._tz_sydney)

        if session == ForexSession.TOKYO:
            return time_now.astimezone(self._tz_tokyo)

        if session == ForexSession.LONDON:
            return time_now.astimezone(self._tz_london)

        if session == ForexSession.NEW_YORK:
            return time_now.astimezone(self._tz_new_york)

    cpdef datetime next_start(self, session: ForexSession, datetime time_now):
        """
        Return the next session start.

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
            If `time_now` is not tz aware UTC.

        """
        Condition.type(session, ForexSession, "session")
        Condition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        cdef datetime local_now = self.local_from_utc(session, time_now)
        cdef datetime next_start = None

        # Local days session start
        if session == ForexSession.SYDNEY:
            next_start = self._tz_sydney.localize(datetime(local_now.year, local_now.month, local_now.day, 7))
        elif session == ForexSession.TOKYO:
            next_start = self._tz_tokyo.localize(datetime(local_now.year, local_now.month, local_now.day, 9))
        elif session == ForexSession.LONDON:
            next_start = self._tz_london.localize(datetime(local_now.year, local_now.month, local_now.day, 8))
        elif session == ForexSession.NEW_YORK:
            next_start = self._tz_new_york.localize(datetime(local_now.year, local_now.month, local_now.day, 8))

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
        Return the previous session start.

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
            If `time_now` is not tz aware UTC.

        """
        Condition.type(session, ForexSession, "session")
        Condition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        cdef datetime local_now = self.local_from_utc(session, time_now)
        cdef datetime prev_start = None

        # Local days session start
        if session == ForexSession.SYDNEY:
            prev_start = self._tz_sydney.localize(datetime(local_now.year, local_now.month, local_now.day, 7))
        elif session == ForexSession.TOKYO:
            prev_start = self._tz_tokyo.localize(datetime(local_now.year, local_now.month, local_now.day, 9))
        elif session == ForexSession.LONDON:
            prev_start = self._tz_london.localize(datetime(local_now.year, local_now.month, local_now.day, 8))
        elif session == ForexSession.NEW_YORK:
            prev_start = self._tz_new_york.localize(datetime(local_now.year, local_now.month, local_now.day, 8))

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
        Return the next session end.

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
            If `time_now` is not tz aware UTC.

        """
        Condition.type(session, ForexSession, "session")
        Condition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        cdef datetime local_now = self.local_from_utc(session, time_now)
        cdef datetime next_end = None

        # Local days session end
        if session == ForexSession.SYDNEY:
            next_end = self._tz_sydney.localize(datetime(local_now.year, local_now.month, local_now.day, 16))
        elif session == ForexSession.TOKYO:
            next_end = self._tz_tokyo.localize(datetime(local_now.year, local_now.month, local_now.day, 18))
        elif session == ForexSession.LONDON:
            next_end = self._tz_london.localize(datetime(local_now.year, local_now.month, local_now.day, 16))
        elif session == ForexSession.NEW_YORK:
            next_end = self._tz_new_york.localize(datetime(local_now.year, local_now.month, local_now.day, 17))

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
        Return the previous sessions end.

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
            If `time_now` is not tz aware UTC.

        """
        Condition.type(session, ForexSession, "session")
        Condition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        cdef datetime local_now = self.local_from_utc(session, time_now)
        cdef datetime prev_end = None

        # Local days session end
        if session == ForexSession.SYDNEY:
            prev_end = self._tz_sydney.localize(datetime(local_now.year, local_now.month, local_now.day, 16))
        elif session == ForexSession.TOKYO:
            prev_end = self._tz_tokyo.localize(datetime(local_now.year, local_now.month, local_now.day, 18))
        elif session == ForexSession.LONDON:
            prev_end = self._tz_london.localize(datetime(local_now.year, local_now.month, local_now.day, 16))
        elif session == ForexSession.NEW_YORK:
            prev_end = self._tz_new_york.localize(datetime(local_now.year, local_now.month, local_now.day, 17))

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
    NONE = 1
    LOW = 2
    MEDIUM = 3
    HIGH = 4


cdef class NewsEvent(Data):
    """
    Represents an economic news event.

    Parameters
    ----------
    impact : NewsImpact
        The expected impact for the economic news event.
    name : str
        The name of the economic news event.
    currency : Currency
        The currency the economic news event is expected to affect.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the news event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    """

    def __init__(
        self,
        impact: NewsImpact,
        str name,
        Currency currency,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        super().__init__(ts_event, ts_init)

        self.impact = impact
        self.name = name
        self.currency = currency


cdef class EconomicNewsEventFilter:
    """
    Provides methods to help filter trading strategy rules based on economic news events.

    Parameters
    ----------
    currencies : list[str]
        The list of three letter currency codes to filter.
    impacts : list[str]
        The list of impact levels to filter ('LOW', 'MEDIUM', 'HIGH').
    news_data : pd.DataFrame
        The economic news data.
    """

    def __init__(
        self,
        list currencies not None,
        list impacts not None,
        news_data not None: pd.DataFrame,
    ):
        self._currencies = currencies
        self._impacts = impacts

        self._unfiltered_data_start = news_data.index[0]
        self._unfiltered_data_end = news_data.index[-1]

        self._news_data = news_data[
            news_data["Currency"].isin(currencies) & news_data["Impact"].isin(impacts)
        ]

    @property
    def unfiltered_data_start(self):
        """
        Return the start of the raw data.

        Returns
        -------
        datetime

        """
        return self._unfiltered_data_start

    @property
    def unfiltered_data_end(self):
        """
        Return the end of the raw data.

        Returns
        -------
        datetime

        """
        return self._unfiltered_data_end

    @property
    def currencies(self):
        """
        Return the currencies the data is filtered on.

        Returns
        -------
        list[str]

        """
        return self._currencies

    @property
    def impacts(self):
        """
        Return the news impacts the data is filtered on.

        Returns
        -------
        list[str]

        """
        return self._impacts

    cpdef NewsEvent next_event(self, datetime time_now):
        """
        Return the next news event matching the filter conditions.
        Will return None if no news events match the filter conditions.

        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        NewsEvent or ``None``
            The next news event in the filtered data if any.

        Raises
        ------
        ValueError
            The `time_now` < `self.unfiltered_data_start`.
        ValueError
            The `time_now` > `self.unfiltered_data_end`.
        ValueError
            If `time_now` is not tz aware UTC.

        """
        Condition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        if time_now < self._unfiltered_data_start:
            raise ValueError(f"The given time_now at {time_now} was prior to the "
                             f"available news data start at {self._unfiltered_data_start}")

        if time_now > self._unfiltered_data_end:
            raise ValueError(f"The given time_now at {time_now} was after the "
                             f"available news data end at {self._unfiltered_data_end}")

        events = self._news_data[self._news_data.index >= time_now]

        if events.empty:
            return None

        cdef int index = 0
        row = events.iloc[index]
        cdef uint64_t ts_event = int(pd.Timestamp(events.index[index]).to_datetime64())
        return NewsEvent(
            NewsImpact[row["Impact"]],
            row["Name"],
            Currency.from_str_c(row["Currency"]),
            ts_event,
            ts_event,
        )

    cpdef NewsEvent prev_event(self, datetime time_now):
        """
        Return the previous news event matching the initial filter conditions.
        Will return None if no news events match the filter conditions.

        Parameters
        ----------
        time_now : datetime

        Returns
        -------
        NewsEvent or ``None``
            The previous news event in the filtered data if any.

        Raises
        ------
        ValueError
            The `time_now` < `self.unfiltered_data_start`.
        ValueError
            The `time_now` > `self.unfiltered_data_end`.
        ValueError
            If `time_now` is not tz aware UTC.

        """
        Condition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        if time_now < self._unfiltered_data_start:
            raise ValueError(f"The given time_now at {time_now} was prior to the "
                             f"available news data start at {self._unfiltered_data_start}")

        if time_now > self._unfiltered_data_end:
            raise ValueError(f"The given time_now at {time_now} was after the "
                             f"available news data end at {self._unfiltered_data_end}")

        events = self._news_data[self._news_data.index <= time_now]
        if events.empty:
            return None

        cdef int index = -1
        row = events.iloc[index]
        cdef uint64_t ts_event = int(pd.Timestamp(events.index[index]).to_datetime64())
        return NewsEvent(
            NewsImpact[row["Impact"]],
            row["Name"],
            Currency.from_str_c(row["Currency"]),
            ts_event,
            ts_event,
        )
