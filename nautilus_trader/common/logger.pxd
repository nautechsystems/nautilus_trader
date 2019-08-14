# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock


cpdef enum LogLevel:
    DEBUG = 0
    INFO = 1,
    WARNING = 2,
    ERROR = 3,
    CRITICAL = 4,
    FATAL = 5,

cdef inline str level_str(int value):
    if value == 0:
        return 'DBG'
    elif value == 1:
        return 'INF'
    elif value == 2:
        return 'WRN'
    elif value == 3:
        return 'ERR'
    elif value == 4:
        return 'CRT'
    elif value == 5:
        return 'FTL'


cdef class LogMessage:
    """
    Represents a log message including timestamp and log level.
    """
    cdef readonly datetime timestamp
    cdef readonly LogLevel level
    cdef readonly str text
    cdef readonly int thread_id
    cdef str level_string(self)
    cdef str as_string(self)


cdef class Logger:
    """
    The abstract base class for all Loggers.
    """
    cdef LogLevel _log_level_console
    cdef LogLevel _log_level_file
    cdef LogLevel _log_level_store
    cdef bint _console_prints
    cdef bint _log_thread
    cdef bint _log_to_file
    cdef str _log_file_path
    cdef str _log_file
    cdef list _log_store
    cdef object _log_file_handler
    cdef object _logger

    cdef readonly str name
    cdef readonly bint bypass_logging
    cdef readonly Clock clock

    cpdef void change_log_file_name(self, str name)
    cpdef void log(self, LogMessage message)
    cpdef list get_log_store(self)
    cpdef void clear_log_store(self)
    cpdef void _log(self, LogMessage message)
    cdef str _format_console_output(self, LogMessage message)
    cdef void _log_store_handler(self, LogLevel level, str text)
    cdef void _console_print_handler(self, LogLevel level, str text)


cdef class TestLogger(Logger):
    """
    Provides a single threaded logger for testing.
    """
    pass


cdef class LoggerAdapter:
    """
    Provides an adapter for a components logger.
    """
    cdef Logger _logger

    cdef readonly bint bypassed
    cdef readonly str component_name

    cpdef Logger get_logger(self)
    cpdef void debug(self, str message)
    cpdef void info(self, str message)
    cpdef void warning(self, str message)
    cpdef void error(self, str message)
    cpdef void critical(self, str message)
    cpdef void exception(self, ex)
    cdef str _format_message(self, str message)


cpdef void nautilus_header(LoggerAdapter logger)
