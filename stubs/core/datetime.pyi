from datetime import datetime

import pandas as pd

def unix_nanos_to_dt(nanos: int):
    """
    Return the datetime (UTC) from the given UNIX timestamp (nanoseconds).

    Parameters
    ----------
    nanos : uint64_t
        The UNIX timestamp (nanoseconds) to convert.

    Returns
    -------
    pd.Timestamp

    """
def dt_to_unix_nanos(dt: pd.Timestamp):
    """
    Return the UNIX timestamp (nanoseconds) from the given datetime (UTC).

    Parameters
    ----------
    dt : pd.Timestamp | str | int
        The datetime to convert.

    Returns
    -------
    uint64_t

    Warnings
    --------
    This function expects a pandas `Timestamp` as standard Python `datetime`
    objects are only accurate to 1 microsecond (μs).

    """
def unix_nanos_to_iso8601(unix_nanos: int,  nanos_precision: bool = True) -> str:
    """
    Convert the given `unix_nanos` to an ISO 8601 (RFC 3339) format string.

    Parameters
    ----------
    unix_nanos : int
        The UNIX timestamp (nanoseconds) to be converted.
    nanos_precision : bool, default True
        If True, use nanosecond precision. If False, use millisecond precision.

    Returns
    -------
    str

    """
def format_iso8601(dt: datetime, nanos_precision: bool = True) -> str:
    """
    Format the given datetime as an ISO 8601 (RFC 3339) specification string.

    Parameters
    ----------
    dt : pd.Timestamp
        The datetime to format.
    nanos_precision : bool, default True
        If True, use nanosecond precision. If False, use millisecond precision.

    Returns
    -------
    str

    """
def  format_optional_iso8601(dt: datetime, nanos_precision: bool = True) -> str:
    """
    Format the given optional datetime as an ISO 8601 (RFC 3339) specification string.

    If value is `None` then will return the string "None".

    Parameters
    ----------
    dt : pd.Timestamp, optional
        The datetime to format.
    nanos_precision : bool, default True
        If True, use nanosecond precision. If False, use millisecond precision.

    Returns
    -------
    str

    """
def maybe_unix_nanos_to_dt(nanos):
    """
    Return the datetime (UTC) from the given UNIX timestamp (nanoseconds), or ``None``.

    If nanos is ``None``, then will return ``None``.

    Parameters
    ----------
    nanos : int, optional
        The UNIX timestamp (nanoseconds) to convert.

    Returns
    -------
    pd.Timestamp or ``None``

    """
def maybe_dt_to_unix_nanos(dt: pd.Timestamp):
    """
    Return the UNIX timestamp (nanoseconds) from the given datetime, or ``None``.

    If dt is ``None``, then will return ``None``.

    Parameters
    ----------
    dt : pd.Timestamp, optional
        The datetime to convert.

    Returns
    -------
    int64 or ``None``

    Warnings
    --------
    If the input is not ``None`` then this function expects a pandas `Timestamp`
    as standard Python `datetime` objects are only accurate to 1 microsecond (μs).

    """
def is_datetime_utc(dt: datetime) -> bool:
    """
    Return a value indicating whether the given timestamp is timezone aware UTC.

    Parameters
    ----------
    dt : datetime
        The datetime to check.

    Returns
    -------
    bool
        True if timezone aware UTC, else False.

    """
def is_tz_aware(time_object) -> bool:
    """
    Return a value indicating whether the given object is timezone aware.

    Parameters
    ----------
    time_object : datetime, pd.Timestamp, pd.Series, pd.DataFrame
        The time object to check.

    Returns
    -------
    bool
        True if timezone aware, else False.

    """
def is_tz_naive(time_object) -> bool:
    """
    Return a value indicating whether the given object is timezone naive.

    Parameters
    ----------
    time_object : datetime, pd.Timestamp, pd.DataFrame
        The time object to check.

    Returns
    -------
    bool
        True if object timezone naive, else False.

    """
def as_utc_timestamp(dt: datetime) -> datetime:
    """
    Ensure the given timestamp is tz-aware UTC.

    Parameters
    ----------
    dt : datetime
        The timestamp to check.

    Returns
    -------
    datetime

    """
def as_utc_index(data: pd.DataFrame) -> pd.DataFrame | None:
    """
    Ensure the given data has a DateTimeIndex which is tz-aware UTC.

    Parameters
    ----------
    data : pd.Series or pd.DataFrame.
        The object to ensure is UTC.

    Returns
    -------
    pd.Series, pd.DataFrame or ``None``

    """
def time_object_to_dt(time_object) -> datetime | None:
    """
    Return the datetime (UTC) from the given UNIX timestamp as integer (nanoseconds), string or pd.Timestamp.

    Parameters
    ----------
    time_object : pd.Timestamp | str | int | None
        The time object to convert.

    Returns
    -------
    pd.Timestamp or ``None``
        Returns None if the input is None.

    """
def max_date(date1: pd.Timestamp | str | int | None = None, date2: str | int | None = None) -> pd.Timestamp | None:
    """
    Return the maximum date as a datetime (UTC).

    Parameters
    ----------
    date1 : pd.Timestamp | str | int | None, optional
        The first date to compare. Can be a string, integer (timestamp), or None. Default is None.
    date2 : pd.Timestamp | str | int | None, optional
        The second date to compare. Can be a string, integer (timestamp), or None. Default is None.

    Returns
    -------
    pd.Timestamp | None
        The maximum date, or None if both input dates are None.

    """
def min_date(date1: pd.Timestamp | str | int | None = None, date2: str | int | None = None) -> pd.Timestamp | None:
    """
    Return the minimum date as a datetime (UTC).

    Parameters
    ----------
    date1 : pd.Timestamp | str | int | None, optional
        The first date to compare. Can be a string, integer (timestamp), or None. Default is None.
    date2 : pd.Timestamp | str | int | None, optional
        The second date to compare. Can be a string, integer (timestamp), or None. Default is None.

    Returns
    -------
    pd.Timestamp | None
        The minimum date, or None if both input dates are None.

    """

