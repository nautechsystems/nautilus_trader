from collections.abc import Callable
from datetime import datetime
from datetime import timedelta
from datetime import tzinfo
from typing import Any, ClassVar

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.enums import ComponentTrigger
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import UUID4, ComponentState
from nautilus_trader.core.nautilus_pyo3 import ComponentId
from nautilus_trader.core.nautilus_pyo3 import Identifier
from nautilus_trader.core.nautilus_pyo3 import LogColor
from nautilus_trader.core.nautilus_pyo3 import LogLevel
from nautilus_trader.core.nautilus_pyo3 import MessageBusListener
from nautilus_trader.core.nautilus_pyo3 import TraderId
from stubs.serialization.base import Serializer
from nautilus_trader.core.message import Event


_COMPONENT_CLOCKS: dict[UUID4, list[TestClock]]
_FORCE_STOP: bool
LOGGING_PYO3: bool


class Clock:
    """
    The base class for all clocks.

    Notes
    -----
    An *active* timer is one which has not expired.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    @property
    def timer_names(self) -> list[str]:
        """
        Return the names of *active* timers running in the clock.

        Returns
        -------
        list[str]

        """
        ...

    @property
    def timer_count(self) -> int:
        """
        Return the count of *active* timers running in the clock.

        Returns
        -------
        int

        """
        ...

    def timestamp(self) -> float:
        """
        Return the current UNIX timestamp in seconds.

        Returns
        -------
        double

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        ...

    def timestamp_ms(self) -> int:
        """
        Return the current UNIX timestamp in milliseconds (ms).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        ...

    def timestamp_us(self) -> int:
        """
        Return the current UNIX timestamp in microseconds (μs).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        ...

    def timestamp_ns(self) -> int:
        """
        Return the current UNIX timestamp in nanoseconds (ns).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        ...

    def utc_now(self) -> datetime:
        """
        Return the current time (UTC).

        Returns
        -------
        datetime
            The current tz-aware UTC time of the clock.

        """
        ...

    def local_now(self, tz: tzinfo | None = None) -> datetime:
        """
        Return the current datetime of the clock in the given local timezone.

        Parameters
        ----------
        tz : tzinfo, optional
            The local timezone (if None the system local timezone is assumed for
            the target timezone).

        Returns
        -------
        datetime
            tz-aware in local timezone.

        """
        ...

    def register_default_handler(self, handler: Callable[TimeEvent, None]) -> None:
        """
        Register the given handler as the clocks default handler.

        Parameters
        ----------
        handler : Callable[[TimeEvent], None]
            The handler to register.

        Raises
        ------
        TypeError
            If `handler` is not of type `Callable`.

        """
        ...

    def next_time_ns(self, name: str) -> int:
        """
        Find a particular timer.

        Parameters
        ----------
        name : str
            The name of the timer.

        Returns
        -------
        uint64_t

        Raises
        ------
        ValueError
            If `name` is not a valid string.

        """
        ...

    def set_time_alert(
        self,
        name: str,
        alert_time: datetime,
        callback: Callable[TimeEvent, None] | None = None,
        override: bool = False,
        allow_past: bool = True,
    ) -> None:
        """
        Set a time alert for the given time.

        When the time is reached the handler will be passed the `TimeEvent`
        containing the timers unique name. If no handler is passed then the
        default handler (if registered) will receive the `TimeEvent`.

        Parameters
        ----------
        name : str
            The name for the alert (must be unique for this clock).
        alert_time : datetime
            The time for the alert.
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.
        override: bool, default False
            If override is set to True an alert with a given name can be overwritten if it exists already.
        allow_past : bool, default True
            If True, allows an `alert_time` in the past and adjusts it to the current time
            for immediate firing. If False, raises an error when the `alert_time` is in the
            past, requiring it to be in the future.

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        KeyError
            If `name` is not unique for this clock.
        TypeError
            If `handler` is not of type `Callable` or ``None``.
        ValueError
            If `handler` is ``None`` and no default handler is registered.

        Warnings
        --------
        If `alert_time` is in the past or at current time, then an immediate
        time event will be generated (rather than being invalid and failing a condition check).

        """
        ...

    def set_time_alert_ns(
        self,
        name: str,
        alert_time_ns: int,
        callback: Callable[TimeEvent, None] | None = None,
        allow_past: bool = True,
    ) -> None:
        """
        Set a time alert for the given time.

        When the time is reached the handler will be passed the `TimeEvent`
        containing the timers unique name. If no callback is passed then the
        default handler (if registered) will receive the `TimeEvent`.

        Parameters
        ----------
        name : str
            The name for the alert (must be unique for this clock).
        alert_time_ns : uint64_t
            The UNIX timestamp (nanoseconds) for the alert.
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.
        allow_past : bool, default True
            If True, allows an `alert_time_ns` in the past and adjusts it to the current time
            for immediate firing. If False, panics when the `alert_time_ns` is in the
            past, requiring it to be in the future.

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        ValueError
            If `name` is not unique for this clock.
        TypeError
            If `callback` is not of type `Callable` or ``None``.
        ValueError
            If `callback` is ``None`` and no default handler is registered.

        Warnings
        --------
        If `alert_time_ns` is in the past or at current time, then an immediate
        time event will be generated (rather than being invalid and failing a condition check).

        """
        ...

    def set_timer(
        self,
        name: str,
        interval: timedelta,
        start_time: datetime | None = None,
        stop_time: datetime | None = None,
        callback: Callable[TimeEvent, None] | None = None,
        allow_past: bool = True,
        fire_immediately: bool = False,
    ) -> None:
        """
        Set a timer to run.

        The timer will run from the start time (optionally until the stop time).
        When the intervals are reached the handlers will be passed the
        `TimeEvent` containing the timers unique name. If no handler is passed
        then the default handler (if registered) will receive the `TimeEvent`.

        Parameters
        ----------
        name : str
            The name for the timer (must be unique for this clock).
        interval : timedelta
            The time interval for the timer.
        start_time : datetime, optional
            The start time for the timer (if None then starts immediately).
        stop_time : datetime, optional
            The stop time for the timer (if None then repeats indefinitely).
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.
        allow_past : bool, default True
            If True, allows timers where the next event time may be in the past.
            If False, raises an error when the next event time would be in the past.
        fire_immediately : bool, default False
            If True, the timer will fire immediately at the start time,
            then fire again after each interval. If False, the timer will
            fire after the first interval has elapsed (default behavior).

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        KeyError
            If `name` is not unique for this clock.
        ValueError
            If `interval` is not positive (> 0).
        ValueError
            If `stop_time` is not ``None`` and `stop_time` < time now.
        ValueError
            If `stop_time` is not ``None`` and `start_time` + `interval` > `stop_time`.
        TypeError
            If `handler` is not of type `Callable` or ``None``.
        ValueError
            If `handler` is ``None`` and no default handler is registered.

        """
        ...

    def set_timer_ns(
        self,
        name: str,
        interval_ns: int,
        start_time_ns: int,
        stop_time_ns: int,
        callback: Callable[TimeEvent, None] | None = None,
        allow_past: bool = True,
        fire_immediately: bool = False,
    ) -> None:
        """
        Set a timer to run.

        The timer will run from the start time until the stop time.
        When the intervals are reached the handlers will be passed the
        `TimeEvent` containing the timers unique name. If no handler is passed
        then the default handler (if registered) will receive the `TimeEvent`.

        Parameters
        ----------
        name : str
            The name for the timer (must be unique for this clock).
        interval_ns : uint64_t
            The time interval (nanoseconds) for the timer.
        start_time_ns : uint64_t
            The start UNIX timestamp (nanoseconds) for the timer.
        stop_time_ns : uint64_t
            The stop UNIX timestamp (nanoseconds) for the timer.
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.
        allow_past : bool, default True
            If True, allows timers where the next event time may be in the past.
            If False, raises an error when the next event time would be in the past.
        fire_immediately : bool, default False
            If True, the timer will fire immediately at the start time,
            then fire again after each interval. If False, the timer will
            fire after the first interval has elapsed (default behavior).

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        KeyError
            If `name` is not unique for this clock.
        ValueError
            If `interval` is not positive (> 0).
        ValueError
            If `stop_time` is not ``None`` and `stop_time` < time now.
        ValueError
            If `stop_time` is not ``None`` and `start_time` + interval > `stop_time`.
        TypeError
            If `callback` is not of type `Callable` or ``None``.
        ValueError
            If `callback` is ``None`` and no default handler is registered.

        """
        ...

    def cancel_timer(self, name: str) -> None:
        """
        Cancel the timer corresponding to the given label.

        Parameters
        ----------
        name : str
            The name for the timer to cancel.

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        KeyError
            If `name` is not an active timer name for this clock.

        """
        ...

    def cancel_timers(self) -> None:
        """
        Cancel all timers.
        """
        ...


def get_component_clocks(instance_id: UUID4) -> listTestClock: ...
def register_component_clock(instance_id: UUID4, clock: Clock) -> None: ...
def deregister_component_clock(instance_id: UUID4, clock: Clock) -> None: ...
def remove_instance_component_clocks(instance_id: UUID4) -> None: ...
def set_backtest_force_stop(value: bool) -> None: ...
def is_backtest_force_stop() -> bool: ...


class TestClock(Clock):
    """
    Provides a monotonic clock for backtesting and unit testing.

    """

    __test__: bool = ...

    def __init__(self) -> None: ...
    def __del__(self) -> None: ...

    @property
    def timer_names(self) -> list[str]: ...

    @property
    def timer_count(self) -> int: ...

    def timestamp(self) -> float: ...
    def timestamp_ms(self) -> int: ...
    def timestamp_us(self) -> int: ...
    def timestamp_ns(self) -> int: ...
    def register_default_handler(self, callback: Callable[TimeEvent, None]) -> None: ...

    def set_time_alert_ns(
        self,
        name: str,
        alert_time_ns: int,
        callback: Callable[TimeEvent, None] | None = None,
        allow_past: bool = True,
    ) -> None: ...

    def set_timer_ns(
        self,
        name: str,
        interval_ns: int,
        start_time_ns: int,
        stop_time_ns: int,
        callback: Callable[TimeEvent, None] | None = None,
        allow_past: bool = True,
        fire_immediately: bool = False,
    ) -> None: ...

    def next_time_ns(self, name: str) -> int: ...
    def cancel_timer(self, name: str) -> None: ...
    def cancel_timers(self) -> None: ...

    def set_time(self, to_time_ns: int) -> None:
        """
        Set the clocks datetime to the given time (UTC).

        Parameters
        ----------
        to_time_ns : uint64_t
            The UNIX timestamp (nanoseconds) to set.

        """
        ...

    def advance_time(self, to_time_ns: int, set_time: bool = True) -> list[TimeEventHandler]:
        """
        Advance the clocks time to the given `to_time_ns`.

        Parameters
        ----------
        to_time_ns : uint64_t
            The UNIX timestamp (nanoseconds) to advance the clock to.
        set_time : bool
            If the clock should also be set to the given `to_time_ns`.

        Returns
        -------
        list[TimeEventHandler]
            Sorted chronologically.

        Raises
        ------
        ValueError
            If `to_time_ns` is < the clocks current time.

        """
        ...


class LiveClock(Clock):
    """
    Provides a monotonic clock for live trading.

    All times are tz-aware UTC.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the clocks timers.
    """

    def __init__(self) -> None: ...
    def __del__(self) -> None: ...

    @property
    def timer_names(self) -> list[str]: ...

    @property
    def timer_count(self) -> int: ...

    def timestamp(self) -> float: ...
    def timestamp_ms(self) -> int: ...
    def timestamp_us(self) -> int: ...
    def timestamp_ns(self) -> int: ...
    def register_default_handler(self, callback: Callable[TimeEvent, None]) -> None: ...

    def set_time_alert_ns(
        self,
        name: str,
        alert_time_ns: int,
        callback: Callable[TimeEvent, None] | None = None,
        allow_past: bool = True,
    ) -> None: ...

    def set_timer_ns(
        self,
        name: str,
        interval_ns: int,
        start_time_ns: int,
        stop_time_ns: int,
        callback: Callable[TimeEvent, None] | None = None,
        allow_past: bool = True,
        fire_immediately: bool = False,
    ) -> None: ...

    def next_time_ns(self, name: str) -> int: ...
    def cancel_timer(self, name: str) -> None: ...
    def cancel_timers(self) -> None: ...


def create_pyo3_conversion_wrapper(callback: Any) -> Callable[[Any], Any]: ...


class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.

    Parameters
    ----------
    name : str
        The event name.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the time event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        name: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...

    def __getstate__(self) -> Any: ...
    def __setstate__(self, state: Any) -> None: ...

    def __eq__(self, other: TimeEvent) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

    @property
    def name(self) -> str:
        """
        Return the name of the time event.

        Returns
        -------
        str

        """
        ...

    @property
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        ...

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        int

        """
        ...

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        ...


class TimeEventHandler:
    """
    Represents a time event with its associated handler.
    """

    event: TimeEvent

    def __init__(
        self,
        event: TimeEvent,
        handler: Callable[[TimeEvent], None],
    ) -> None: ...

    def handle(self) -> None:
        """Call the handler with the contained time event."""
        ...

    def __eq__(self, other: TimeEventHandler) -> bool: ...
    def __lt__(self, other: TimeEventHandler) -> bool: ...
    def __le__(self, other: TimeEventHandler) -> bool: ...
    def __gt__(self, other: TimeEventHandler) -> bool: ...
    def __ge__(self, other: TimeEventHandler) -> bool: ...
    def __repr__(self) -> str: ...


RECV: str = ...
SENT: str = ...
CMD: str = ...
EVT: str = ...
DOC: str = ...
RPT: str = ...
REQ: str = ...
RES: str = ...


def set_logging_clock_realtime_mode() -> None: ...
def set_logging_clock_static_mode() -> None: ...
def set_logging_clock_static_time(time_ns: int) -> None: ...
def log_color_from_str(value: str) -> LogColor: ...
def log_color_to_str(value: LogColor) -> str: ...
def log_level_from_str(value: str) -> LogLevel: ...
def log_level_to_str(value: LogLevel) -> str: ...


class LogGuard:
    """
    Provides a `LogGuard` which serves as a token to signal the initialization
    of the logging subsystem. It also ensures that the global logger is flushed
    of any buffered records when the instance is destroyed.
    """

    def __del__(self) -> None: ...


def init_logging(
    trader_id: TraderId | None = None,
    machine_id: str | None = None,
    instance_id: UUID4 | None = None,
    level_stdout: LogLevel = ...,
    level_file: LogLevel = ...,
    directory: str | None = None,
    file_name: str | None = None,
    file_format: str | None = None,
    component_levels: dict[ComponentId, LogLevel] | None = None,
    colors: bool = True,
    bypass: bool = False,
    print_config: bool = False,
    max_file_size: int = 0,
    max_backup_count: int = 5,
) -> LogGuard:
    """
    Initialize the logging subsystem.

    Provides an interface into the logging subsystem implemented in Rust.

    This function should only be called once per process, at the beginning of the application
    run. Subsequent calls will raise a `RuntimeError`, as there can only be one `LogGuard`
    per initialized system.

    Parameters
    ----------
    trader_id : TraderId, optional
        The trader ID for the logger.
    machine_id : str, optional
        The machine ID.
    instance_id : UUID4, optional
        The instance ID.
    level_stdout : LogLevel, default ``INFO``
        The minimum log level to write to stdout.
    level_file : LogLevel, default ``OFF``
        The minimum log level to write to a file.
    directory : str, optional
        The path to the log file directory.
        If ``None`` then will write to the current working directory.
    file_name : str, optional
        The custom log file name (will use a '.log' suffix for plain text or '.json' for JSON).
        If ``None`` will not log to a file (unless `file_auto` is True).
    file_format : str { 'JSON' }, optional
        The log file format. If ``None`` (default) then will log in plain text.
        If set to 'JSON' then logs will be in JSON format.
    component_levels : dict[ComponentId, LogLevel]
        The additional per component log level filters, where keys are component
        IDs (e.g. actor/strategy IDs) and values are log levels.
    colors : bool, default True
        If ANSI codes should be used to produce colored log lines.
    bypass : bool, default False
        If the output for the core logging subsystem is bypassed (useful for logging tests).
    print_config : bool, default False
        If the core logging configuration should be printed to stdout on initialization.
    max_file_size : uint64_t, default 0
        The maximum size of log files in bytes before rotation occurs.
        If set to 0, file rotation is disabled.
    max_backup_count : uint32_t, default 5
        The maximum number of backup log files to keep when rotating.

    Returns
    -------
    LogGuard

    Raises
    ------
    RuntimeError
        If the logging subsystem has already been initialized.

    """
    ...


def is_logging_initialized() -> bool: ...
def is_logging_pyo3() -> bool: ...
def set_logging_pyo3(value: bool) -> None: ...
def flush_logger() -> None: ...


class Logger:
    """
    Provides a logger adapter into the logging subsystem.

    Parameters
    ----------
    name : str
        The name of the logger. This will appear within each log line.

    """

    def __init__(self, name: str) -> None: ...

    @property
    def name(self) -> str:
        """
        Return the name of the logger.

        Returns
        -------
        str

        """
        ...

    def debug(
        self,
        message: str,
        color: LogColor = ...,
    ) -> None:
        """
        Log the given DEBUG level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        ...

    def info(
        self, message: str,
        color: LogColor = ...,
    ) -> None:
        """
        Log the given INFO level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        ...

    def warning(
        self,
        message: str,
        color: LogColor = ...,
    ) -> None:
        """
        Log the given WARNING level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        ...

    def error(
        self,
        message: str,
        color: LogColor = ...,
    ) -> None:
        """
        Log the given ERROR level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        ...

    def exception(
        self,
        message: str,
        ex,
    ) -> None:
        """
        Log the given exception including stack trace information.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        ex : Exception
            The exception to log.

        """
        ...


def log_header(
    trader_id: TraderId,
    machine_id: str,
    instance_id: UUID4,
    component: str,
) -> None: ...


def log_sysinfo(component: str) -> None: ...


def component_state_from_str(value: str) -> ComponentState: ...
def component_state_to_str(value: ComponentState) -> str: ...
def component_trigger_from_str(value: str) -> ComponentTrigger: ...
def component_trigger_to_str(value: ComponentTrigger) -> str: ...


class ComponentFSMFactory:
    """
    Provides a generic component Finite-State Machine.
    """

    @staticmethod
    def get_state_transition_table() -> dict:
        """
        The default state transition table.

        Returns
        -------
        dict[int, int]
            C Enums.

        """
        ...

    @staticmethod
    def create() -> nautilus_trader.core.fsm.FiniteStateMachine: ...


class Component:
    """
    The base class for all system components.

    A component is not considered initialized until a message bus is registered
    (this either happens when one is passed to the constructor, or when
    registered with a trader).

    Thus, if the component does not receive a message bus through the constructor,
    then it will be in a ``PRE_INITIALIZED`` state, otherwise if one is passed
    then it will be in an ``INITIALIZED`` state.

    Parameters
    ----------
    clock : Clock
        The clock for the component.
    trader_id : TraderId, optional
        The trader ID associated with the component.
    component_id : Identifier, optional
        The component ID. If ``None`` is passed then the identifier will be
        taken from `type(self).__name__`.
    component_name : str, optional
        The custom component name.
    msgbus : MessageBus, optional
        The message bus for the component (required before initialized).
    config : NautilusConfig, optional
        The configuration for the component.

    Raises
    ------
    ValueError
        If `component_name` is not a valid string.
    TypeError
        If `config` is not of type `NautilusConfig`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    trader_id: TraderId | None
    id: Identifier
    type: type

    def __init__(
        self,
        clock: Clock,
        trader_id: TraderId | None = None,
        component_id: Identifier | None = None,
        component_name: str | None = None,
        msgbus: MessageBus | None = None,
        config: NautilusConfig | None = None,
    ) -> None: ...

    def __eq__(self, other: Component) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

    @classmethod
    def fully_qualified_name(cls) -> str:
        """
        Return the fully qualified name for the components class.

        Returns
        -------
        str

        References
        ----------
        https://www.python.org/dev/peps/pep-3155/

        """
        ...

    @property
    def state(self) -> ComponentState:
        """
        Return the components current state.

        Returns
        -------
        ComponentState

        """
        ...

    @property
    def is_initialized(self) -> bool:
        """
        Return whether the component has been initialized (component.state >= ``INITIALIZED``).

        Returns
        -------
        bool

        """
        ...

    @property
    def is_running(self) -> bool:
        """
        Return whether the current component state is ``RUNNING``.

        Returns
        -------
        bool

        """
        ...

    @property
    def is_stopped(self) -> bool:
        """
        Return whether the current component state is ``STOPPED``.

        Returns
        -------
        bool

        """
        ...

    @property
    def is_disposed(self) -> bool:
        """
        Return whether the current component state is ``DISPOSED``.

        Returns
        -------
        bool

        """
        ...

    @property
    def is_degraded(self) -> bool:
        """
        Return whether the current component state is ``DEGRADED``.

        Returns
        -------
        bool

        """
        ...

    @property
    def is_faulted(self) -> bool:
        """
        Return whether the current component state is ``FAULTED``.

        Returns
        -------
        bool

        """
        ...

    def _start(self) -> None: ...
    def _stop(self) -> None: ...
    def _resume(self) -> None: ...
    def _reset(self) -> None: ...
    def _dispose(self) -> None: ...
    def _degrade(self) -> None: ...
    def _fault(self) -> None: ...

    def start(self) -> None:
        """
        Start the component.

        While executing `on_start()` any exception will be logged and reraised, then the component
        will remain in a ``STARTING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        ...

    def stop(self) -> None:
        """
        Stop the component.

        While executing `on_stop()` any exception will be logged and reraised, then the component
        will remain in a ``STOPPING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        ...

    def resume(self) -> None:
        """
        Resume the component.

        While executing `on_resume()` any exception will be logged and reraised, then the component
        will remain in a ``RESUMING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        ...

    def reset(self) -> None:
        """
        Reset the component.

        All stateful fields are reset to their initial value.

        While executing `on_reset()` any exception will be logged and reraised, then the component
        will remain in a ``RESETTING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        ...

    def dispose(self) -> None:
        """
        Dispose of the component.

        While executing `on_dispose()` any exception will be logged and reraised, then the component
        will remain in a ``DISPOSING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        ...

    def degrade(self) -> None:
        """
        Degrade the component.

        While executing `on_degrade()` any exception will be logged and reraised, then the component
        will remain in a ``DEGRADING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        ...

    def fault(self) -> None:
        """
        Fault the component.

        Calling this method multiple times has the same effect as calling it once (it is idempotent).
        Once called, it cannot be reversed, and no other methods should be called on this instance.

        While executing `on_fault()` any exception will be logged and reraised, then the component
        will remain in a ``FAULTING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        ...

    def shutdown_system(self, reason: str | None = None) -> None:
        """
        Initiate a system-wide shutdown by generating and publishing a `ShutdownSystem` command.

        The command is handled by the system's `NautilusKernel`, which will invoke either `stop` (synchronously)
        or `stop_async` (asynchronously) depending on the execution context and the presence of an active event loop.

        Parameters
        ----------
        reason : str, optional
            The reason for issuing the shutdown command.

        """
        ...


class MessageBus:
    """
    Provides a generic message bus to facilitate various messaging patterns.

    The bus provides both a producer and consumer API for Pub/Sub, Req/Rep, as
    well as direct point-to-point messaging to registered endpoints.

    Pub/Sub wildcard patterns for hierarchical topics are possible:
     - `*` asterisk represents one or more characters in a pattern.
     - `?` question mark represents a single character in a pattern.

    Given a topic and pattern potentially containing wildcard characters, i.e.
    `*` and `?`, where `?` can match any single character in the topic, and `*`
    can match any number of characters including zero characters.

    The asterisk in a wildcard matches any character zero or more times. For
    example, `comp*` matches anything beginning with `comp` which means `comp`,
    `complete`, and `computer` are all matched.

    A question mark matches a single character once. For example, `c?mp` matches
    `camp` and `comp`. The question mark can also be used more than once.
    For example, `c??p` would match both of the above examples and `coop`.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the message bus.
    clock : Clock
        The clock for the message bus.
    name : str, optional
        The custom name for the message bus.
    serializer : Serializer, optional
        The serializer for database operations.
    database : nautilus_pyo3.RedisMessageBusDatabase, optional
        The backing database for the message bus.
    config : MessageBusConfig, optional
        The configuration for the message bus.

    Raises
    ------
    ValueError
        If `name` is not ``None`` and not a valid string.

    Warnings
    --------
    This message bus is not thread-safe and must be called from the same thread
    as the event loop.
    """

    trader_id: TraderId
    serializer: Serializer
    has_backing: bool
    sent_count: int
    req_count: int
    res_count: int
    pub_count: int

    _clock: Clock
    _log: Logger
    _database: RedisMessageBusDatabase | None
    _listeners: list[MessageBusListener]
    _endpoints: dict[str, Callable[[Any], None]]
    _patterns: dict[str, Any] # Changed from Subscription[:] due to type hint complexities
    _subscriptions: dict[Subscription, list[str]]
    _correlation_index: dict[UUID4, Callable[[Any], None]]
    _publishable_types: tuple[type, ...]
    _streaming_types: set[type]
    _resolved: bool

    def __init__(
        self,
        trader_id: TraderId,
        clock: Clock,
        instance_id: UUID4 | None = None,
        name: str | None = None,
        serializer: Serializer | None = None,
        database: nautilus_pyo3.RedisMessageBusDatabase | None = None,
        config: Any | None = None,
    ) -> None: ...

    def endpoints(self) -> list[str]:
        """
        Return all endpoint addresses registered with the message bus.

        Returns
        -------
        list[str]

        """
        ...

    def topics(self) -> list[str]:
        """
        Return all topics with active subscribers.

        Returns
        -------
        list[str]

        """
        ...

    def subscriptions(self, pattern: str | None = None) -> list[Subscription]:
        """
        Return all subscriptions matching the given topic `pattern`.

        Parameters
        ----------
        pattern : str, optional
            The topic pattern filter. May include wildcard characters `*` and `?`.
            If ``None`` then query is for **all** topics.

        Returns
        -------
        list[Subscription]

        """
        ...

    def streaming_types(self) -> set[type]:
        """
        Return all types registered for external streaming -> internal publishing.

        Returns
        -------
        set[type]

        """
        ...

    def has_subscribers(self, pattern: str | None = None) -> bool:
        """
        If the message bus has subscribers for the give topic `pattern`.

        Parameters
        ----------
        pattern : str, optional
            The topic filter. May include wildcard characters `*` and `?`.
            If ``None`` then query is for **all** topics.

        Returns
        -------
        bool

        """
        ...

    def is_subscribed(self, topic: str, handler: Callable[[Any], None]) -> bool:
        """
        Return if topic and handler is subscribed to the message bus.

        Does not consider any previous `priority`.

        Parameters
        ----------
        topic : str
            The topic of the subscription.
        handler : Callable[[Any], None]
            The handler of the subscription.

        Returns
        -------
        bool

        """
        ...

    def is_pending_request(self, request_id: UUID4) -> bool:
        """
        Return if the given `request_id` is still pending a response.

        Parameters
        ----------
        request_id : UUID4
            The request ID to check (to match the correlation_id).

        Returns
        -------
        bool

        """
        ...

    def is_streaming_type(self, cls: type) -> bool:
        """
        Return whether the given type has been registered for external message streaming.

        Returns
        -------
        bool
            True if registered, else False.

        """
        ...

    def dispose(self) -> None:
        """
        Dispose of the message bus which will close the internal channel and thread.

        """
        ...

    def register(self, endpoint: str, handler: Callable[[Any], None]) -> None:
        """
        Register the given `handler` to receive messages at the `endpoint` address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to register.
        handler : Callable[[Any], None]
            The handler for the registration.

        Raises
        ------
        ValueError
            If `endpoint` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.
        KeyError
            If `endpoint` already registered.

        """
        ...

    def deregister(self, endpoint: str, handler: Callable[[Any], None]) -> None:
        """
        Deregister the given `handler` from the `endpoint` address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to deregister.
        handler : Callable[[Any], None]
            The handler to deregister.

        Raises
        ------
        ValueError
            If `endpoint` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.
        KeyError
            If `endpoint` is not registered.
        ValueError
            If `handler` is not registered at the endpoint.

        """
        ...

    def add_streaming_type(self, cls: type) -> None:
        """
        Register the given type for external->internal message bus streaming.

        Parameters
        ----------
        type : cls
            The type to add for streaming.

        """
        ...

    def add_listener(self, listener: nautilus_pyo3.MessageBusListener) -> None:
        """
        Adds the given listener to the message bus.

        Parameters
        ----------
        listener : MessageBusListener
            The listener to add.

        """
        ...

    def send(self, endpoint: str, msg: Any) -> None:
        """
        Send the given message to the given `endpoint` address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to send the message to.
        msg : object
            The message to send.

        """
        ...

    def request(self, endpoint: str, request: Request) -> None: # Cannot import Request without circular dependency
        """
        Handle the given `request`.

        Will log an error if the correlation ID already exists.

        Parameters
        ----------
        endpoint : str
            The endpoint address to send the request to.
        request : Request
            The request to handle.

        """
        ...

    def response(self, response: Response) -> None: # Cannot import Response without circular dependency
        """
        Handle the given `response`.

        Will log an error if the correlation ID is not found.

        Parameters
        ----------
        response : Response
            The response to handle

        """
        ...

    def subscribe(
        self,
        topic: str,
        handler: Callable[[Any], None],
        priority: int = 0,
    ) -> None:
        """
        Subscribe to the given message `topic` with the given callback `handler`.

        Parameters
        ----------
        topic : str
            The topic for the subscription. May include wildcard characters
            `*` and `?`.
        handler : Callable[[Any], None]
            The handler for the subscription.
        priority : int, optional
            The priority for the subscription. Determines the ordering of
            handlers receiving messages being processed, higher priority
            handlers will receive messages prior to lower priority handlers.

        Raises
        ------
        ValueError
            If `topic` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.

        Warnings
        --------
        Assigning priority handling is an advanced feature which *shouldn't
        normally be needed by most users*. **Only assign a higher priority to the
        subscription if you are certain of what you're doing**. If an inappropriate
        priority is assigned then the handler may receive messages before core
        system components have been able to process necessary calculations and
        produce potential side effects for logically sound behavior.

        """
        ...

    def unsubscribe(self, topic: str, handler: Callable[[Any], None]) -> None:
        """
        Unsubscribe the given callback `handler` from the given message `topic`.

        Parameters
        ----------
        topic : str, optional
            The topic to unsubscribe from. May include wildcard characters `*`
            and `?`.
        handler : Callable[[Any], None]
            The handler for the subscription.

        Raises
        ------
        ValueError
            If `topic` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.

        """
        ...

    def publish(self, topic: str, msg: Any, external_pub: bool = True) -> None:
        """
        Publish the given message for the given `topic`.

        Subscription handlers will receive the message in priority order
        (highest first).

        Parameters
        ----------
        topic : str
            The topic to publish on.
        msg : object
            The message to publish.
        external_pub : bool, default True
            If the message should also be published externally.

        """
        ...


def is_matching_py(topic: str, pattern: str) -> bool: ...


class Subscription:
    """
    Represents a subscription to a particular topic.

    This is an internal class intended to be used by the message bus to organize
    topics and their subscribers.

    Parameters
    ----------
    topic : str
        The topic for the subscription. May include wildcard characters `*` and `?`.
    handler : Callable[[Message], None]
        The handler for the subscription.
    priority : int
        The priority for the subscription.

    Raises
    ------
    ValueError
        If `topic` is not a valid string.
    ValueError
        If `handler` is not of type `Callable`.
    ValueError
        If `priority` is negative (< 0).

    Notes
    -----
    The subscription equality is determined by the topic and handler,
    priority is not considered (and could change).
    """

    topic: str
    handler: Callable[[Any], None]
    priority: int

    def __init__(
        self,
        topic: str,
        handler: Callable[[Any], None],
        priority: int = 0,
    ) -> None: ...

    def __eq__(self, other: Subscription) -> bool: ...
    def __lt__(self, other: Subscription) -> bool: ...
    def __le__(self, other: Subscription) -> bool: ...
    def __gt__(self, other: Subscription) -> bool: ...
    def __ge__(self, other: Subscription) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...


class Throttler:
    """
    Provides a generic throttler which can either buffer or drop messages.

    Will throttle messages to the given maximum limit-interval rate.
    If an `output_drop` handler is provided, then will drop messages which
    would exceed the rate limit. Otherwise will buffer messages until within
    the rate limit, then send.

    Parameters
    ----------
    name : str
        The unique name of the throttler.
    limit : int
        The limit setting for the throttling.
    interval : timedelta
        The interval setting for the throttling.
    clock : Clock
        The clock for the throttler.
    output_send : Callable[[Any], None]
        The output handler to send messages from the throttler.
    output_drop : Callable[[Any], None], optional
        The output handler to drop messages from the throttler.
        If ``None`` then messages will be buffered.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    ValueError
        If `limit` is not positive (> 0).
    ValueError
        If `interval` is not positive (> 0).
    ValueError
        If `output_send` is not of type `Callable`.
    ValueError
        If `output_drop` is not of type `Callable` or ``None``.

    Warnings
    --------
    This throttler is not thread-safe and must be called from the same thread as
    the event loop.

    The internal buffer queue is unbounded and so a bounded queue should be
    upstream.

    """

    name: str
    limit: int
    interval: timedelta
    is_limiting: bool
    recv_count: int
    sent_count: int

    def __init__(
        self,
        name: str,
        limit: int,
        interval: timedelta,
        clock: Clock,
        output_send: Callable[[Any], None],
        output_drop: Callable[[Any], None] | None = None,
    ) -> None: ...

    @property
    def qsize(self) -> int:
        """
        Return the qsize of the internal buffer.

        Returns
        -------
        int

        """
        ...

    def reset(self) -> None:
        """
        Reset the state of the throttler.

        """
        ...

    def used(self) -> float:
        """
        Return the percentage of maximum rate currently used.

        Returns
        -------
        double
            [0, 1.0].

        """
        ...

    def send(self, msg) -> None:
        """
        Send the given message through the throttler.

        Parameters
        ----------
        msg : object
            The message to send.

        """
        ...

    def _process(self, event: TimeEvent) -> None: ...
    def _resume(self, event: TimeEvent) -> None: ...

