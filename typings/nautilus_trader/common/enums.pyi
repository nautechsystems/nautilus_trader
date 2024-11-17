from enum import IntEnum
from typing import Any, Callable, List, Optional
from uuid import UUID

class ComponentState(IntEnum):
    PRE_INITIALIZED = 0
    READY = 1
    STARTING = 2
    RUNNING = 3
    STOPPING = 4
    STOPPED = 5
    RESUMING = 6
    RESETTING = 7
    DISPOSING = 8
    DISPOSED = 9
    DEGRADING = 10
    DEGRADED = 11
    FAULTING = 12
    FAULTED = 13

class ComponentTrigger(IntEnum):
    INITIALIZE = 1
    START = 2
    START_COMPLETED = 3
    STOP = 4
    STOP_COMPLETED = 5
    RESUME = 6
    RESUME_COMPLETED = 7
    RESET = 8
    RESET_COMPLETED = 9
    DISPOSE = 10
    DISPOSE_COMPLETED = 11
    DEGRADE = 12
    DEGRADE_COMPLETED = 13
    FAULT = 14
    FAULT_COMPLETED = 15

class LogColor(IntEnum):
    NORMAL = 0
    GREEN = 1
    BLUE = 2
    MAGENTA = 3
    CYAN = 4
    YELLOW = 5
    RED = 6

class LogLevel(IntEnum):
    OFF = 0
    TRACE = 1
    DEBUG = 2
    INFO = 3
    WARNING = 4
    ERROR = 5

class TestClock:
    def register_default_handler(self, callback: Callable) -> None: ...
    def set_time(self, to_time_ns: int) -> None: ...
    def timestamp(self) -> float: ...
    def timestamp_ms(self) -> int: ...
    def timestamp_us(self) -> int: ...
    def timestamp_ns(self) -> int: ...
    def timer_names(self) -> List[str]: ...
    def timer_count(self) -> int: ...
    def set_time_alert(
        self, name: str, alert_time_ns: int, callback: Callable
    ) -> None: ...
    def set_timer(
        self,
        name: str,
        interval_ns: int,
        start_time_ns: int,
        stop_time_ns: int,
        callback: Callable,
    ) -> None: ...
    def advance_time(self, to_time_ns: int, set_time: bool) -> List[Any]: ...
    def next_time(self, name: str) -> int: ...
    def cancel_timer(self, name: str) -> None: ...
    def cancel_timers(self) -> None: ...

class LiveClock:
    def register_default_handler(self, callback: Callable) -> None: ...
    def timestamp(self) -> float: ...
    def timestamp_ms(self) -> int: ...
    def timestamp_us(self) -> int: ...
    def timestamp_ns(self) -> int: ...
    def timer_names(self) -> List[str]: ...
    def timer_count(self) -> int: ...
    def set_time_alert(
        self, name: str, alert_time_ns: int, callback: Callable
    ) -> None: ...
    def set_timer(
        self,
        name: str,
        interval_ns: int,
        start_time_ns: int,
        stop_time_ns: int,
        callback: Callable,
    ) -> None: ...
    def next_time(self, name: str) -> int: ...
    def cancel_timer(self, name: str) -> None: ...
    def cancel_timers(self) -> None: ...

def logging_is_initialized() -> bool: ...
def logging_set_bypass() -> None: ...
def logging_shutdown() -> None: ...
def logging_is_colored() -> bool: ...
def logging_clock_set_realtime_mode() -> None: ...
def logging_clock_set_static_mode() -> None: ...
def logging_clock_set_static_time(time_ns: int) -> None: ...
def logging_init(
    trader_id: Any,
    instance_id: UUID,
    level_stdout: LogLevel,
    level_file: LogLevel,
    directory: Optional[str] = None,
    file_name: Optional[str] = None,
    file_format: Optional[str] = None,
    component_levels: Optional[str] = None,
    is_colored: bool = True,
    is_bypassed: bool = False,
    print_config: bool = True,
) -> Any: ...
def logger_log(
    level: LogLevel,
    color: LogColor,
    component: str,
    message: str,
) -> None: ...
def logging_log_header(
    trader_id: Any,
    machine_id: str,
    instance_id: UUID,
    component: str,
) -> None: ...
def logging_log_sysinfo(component: str) -> None: ...
