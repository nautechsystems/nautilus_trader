# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import timedelta
from enum import Enum
from enum import unique

import pandas as pd
import pytz

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import is_datetime_utc
from nautilus_trader.model.objects import Currency


@unique
class ForexSession(Enum):
    SYDNEY = 1
    TOKYO = 2
    LONDON = 3
    NEW_YORK = 4


class ForexSessionFilter:
    """
    Provides methods to help filter trading strategy rules dependent on Forex session
    times.
    """

    def __init__(self):
        self._tz_sydney = pytz.timezone("Australia/Sydney")
        self._tz_tokyo = pytz.timezone("Asia/Tokyo")
        self._tz_london = pytz.timezone("Europe/London")
        self._tz_new_york = pytz.timezone("America/New_York")

    def local_from_utc(self, session: ForexSession, time_now: datetime) -> datetime:
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
        PyCondition.type(session, ForexSession, "session")
        PyCondition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        if session == ForexSession.SYDNEY:
            return time_now.astimezone(self._tz_sydney)

        if session == ForexSession.TOKYO:
            return time_now.astimezone(self._tz_tokyo)

        if session == ForexSession.LONDON:
            return time_now.astimezone(self._tz_london)

        if session == ForexSession.NEW_YORK:
            return time_now.astimezone(self._tz_new_york)

    def next_start(self, session: ForexSession, time_now: datetime) -> datetime:
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
        PyCondition.type(session, ForexSession, "session")
        PyCondition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        local_now: datetime = self.local_from_utc(session, time_now)
        next_start: datetime | None = None

        # Local days session start
        if session == ForexSession.SYDNEY:
            next_start = self._tz_sydney.localize(
                datetime(local_now.year, local_now.month, local_now.day, 7),
            )
        elif session == ForexSession.TOKYO:
            next_start = self._tz_tokyo.localize(
                datetime(local_now.year, local_now.month, local_now.day, 9),
            )
        elif session == ForexSession.LONDON:
            next_start = self._tz_london.localize(
                datetime(local_now.year, local_now.month, local_now.day, 8),
            )
        elif session == ForexSession.NEW_YORK:
            next_start = self._tz_new_york.localize(
                datetime(local_now.year, local_now.month, local_now.day, 8),
            )
        if next_start is None:
            raise ValueError("`next_start` was `None`, expected a value")

        # Already past this days session start
        if local_now > next_start:
            next_start += timedelta(days=1)

        # Weekend - next session start becomes next Mondays session start
        if next_start.weekday() > 4:
            diff = 7 - next_start.weekday()
            next_start += timedelta(days=diff)

        return next_start.astimezone(pytz.utc)

    def prev_start(self, session: ForexSession, time_now: datetime) -> datetime:
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
        PyCondition.type(session, ForexSession, "session")
        PyCondition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        local_now: datetime = self.local_from_utc(session, time_now)
        prev_start: datetime | None = None

        # Local days session start
        if session == ForexSession.SYDNEY:
            prev_start = self._tz_sydney.localize(
                datetime(local_now.year, local_now.month, local_now.day, 7),
            )
        elif session == ForexSession.TOKYO:
            prev_start = self._tz_tokyo.localize(
                datetime(local_now.year, local_now.month, local_now.day, 9),
            )
        elif session == ForexSession.LONDON:
            prev_start = self._tz_london.localize(
                datetime(local_now.year, local_now.month, local_now.day, 8),
            )
        elif session == ForexSession.NEW_YORK:
            prev_start = self._tz_new_york.localize(
                datetime(local_now.year, local_now.month, local_now.day, 8),
            )
        if prev_start is None:
            raise ValueError("`prev_start` was `None`, expected a value")

        # Prior to this days session start
        if local_now < prev_start:
            prev_start -= timedelta(days=1)

        # Weekend - previous session start becomes last Fridays session start
        if prev_start.weekday() > 4:
            diff = prev_start.weekday() - 4
            prev_start -= timedelta(days=diff)

        return prev_start.astimezone(pytz.utc)

    def next_end(self, session: ForexSession, time_now: datetime) -> datetime:
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
        PyCondition.type(session, ForexSession, "session")
        PyCondition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        local_now: datetime = self.local_from_utc(session, time_now)
        next_end: datetime | None = None

        # Local days session end
        if session == ForexSession.SYDNEY:
            next_end = self._tz_sydney.localize(
                datetime(local_now.year, local_now.month, local_now.day, 16),
            )
        elif session == ForexSession.TOKYO:
            next_end = self._tz_tokyo.localize(
                datetime(local_now.year, local_now.month, local_now.day, 18),
            )
        elif session == ForexSession.LONDON:
            next_end = self._tz_london.localize(
                datetime(local_now.year, local_now.month, local_now.day, 16),
            )
        elif session == ForexSession.NEW_YORK:
            next_end = self._tz_new_york.localize(
                datetime(local_now.year, local_now.month, local_now.day, 17),
            )
        if next_end is None:
            raise ValueError("`next_end` was `None`, expected a value")

        # Already past this days session end
        if local_now > next_end:
            next_end += timedelta(days=1)

        # Weekend - next session end becomes last Mondays session end
        if next_end.weekday() > 4:
            diff = 7 - next_end.weekday()
            next_end += timedelta(days=diff)

        return next_end.astimezone(pytz.utc)

    def prev_end(self, session: ForexSession, time_now: datetime) -> datetime:
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
        PyCondition.type(session, ForexSession, "session")
        PyCondition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        local_now: datetime = self.local_from_utc(session, time_now)
        prev_end: datetime | None = None

        # Local days session end
        if session == ForexSession.SYDNEY:
            prev_end = self._tz_sydney.localize(
                datetime(local_now.year, local_now.month, local_now.day, 16),
            )
        elif session == ForexSession.TOKYO:
            prev_end = self._tz_tokyo.localize(
                datetime(local_now.year, local_now.month, local_now.day, 18),
            )
        elif session == ForexSession.LONDON:
            prev_end = self._tz_london.localize(
                datetime(local_now.year, local_now.month, local_now.day, 16),
            )
        elif session == ForexSession.NEW_YORK:
            prev_end = self._tz_new_york.localize(
                datetime(local_now.year, local_now.month, local_now.day, 17),
            )
        if prev_end is None:
            raise ValueError("`prev_end` was `None`, expected a value")

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


class NewsEvent(Data):
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
    ts_event : int
        The UNIX timestamp (nanoseconds) when the news event occurred.
    ts_init : int
        The UNIX timestamp (nanoseconds) when the data object was initialized.

    """

    def __init__(
        self,
        impact: NewsImpact,
        name: str,
        currency: Currency,
        ts_event: int,
        ts_init: int,
    ):
        self._impact = impact
        self._name = name
        self._currency = currency
        self._ts_event = ts_event
        self._ts_init = ts_init

    @property
    def impact(self) -> NewsImpact:
        return self._impact

    @property
    def name(self) -> str:
        return self._name

    @property
    def currency(self) -> Currency:
        return self._currency

    @property
    def ts_event(self) -> int:
        return self._ts_event

    @property
    def ts_init(self) -> int:
        return self._ts_init


class EconomicNewsEventFilter:
    """
    Provides methods to help filter trading strategy rules based on economic news
    events.

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
        currencies: list[str],
        impacts: list[str],
        news_data: pd.DataFrame,
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

    def next_event(self, time_now: datetime) -> NewsEvent | None:
        """
        Return the next news event matching the filter conditions. Will return None if
        no news events match the filter conditions.

        Parameters
        ----------
        time_now : datetime
            The current time.

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
        PyCondition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        if time_now < self._unfiltered_data_start:
            raise ValueError(
                f"The given time_now at {time_now} was prior to the "
                f"available news data start at {self._unfiltered_data_start}",
            )

        if time_now > self._unfiltered_data_end:
            raise ValueError(
                f"The given time_now at {time_now} was after the "
                f"available news data end at {self._unfiltered_data_end}",
            )

        events = self._news_data[self._news_data.index >= time_now]

        if events.empty:
            return None

        index = 0
        row = events.iloc[index]
        ts_event = pd.Timestamp(events.index[index]).value
        return NewsEvent(
            NewsImpact[row["Impact"]],
            row["Name"],
            Currency.from_str(row["Currency"]),
            ts_event,
            ts_event,
        )

    def prev_event(self, time_now: datetime) -> NewsEvent | None:
        """
        Return the previous news event matching the initial filter conditions. Will
        return None if no news events match the filter conditions.

        Parameters
        ----------
        time_now : datetime
            The current time.

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
        PyCondition.true(is_datetime_utc(time_now), "time_now was not tz aware UTC")

        if time_now < self._unfiltered_data_start:
            raise ValueError(
                f"The given time_now at {time_now} was prior to the "
                f"available news data start at {self._unfiltered_data_start}",
            )

        if time_now > self._unfiltered_data_end:
            raise ValueError(
                f"The given time_now at {time_now} was after the "
                f"available news data end at {self._unfiltered_data_end}",
            )

        events = self._news_data[self._news_data.index <= time_now]
        if events.empty:
            return None

        index = -1
        row = events.iloc[index]
        ts_event = pd.Timestamp(events.index[index]).value
        return NewsEvent(
            NewsImpact[row["Impact"]],
            row["Name"],
            Currency.from_str(row["Currency"]),
            ts_event,
            ts_event,
        )
