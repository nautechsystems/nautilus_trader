# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock


cdef str RECV
cdef str SENT
cdef str CMD
cdef str EVT


cpdef enum LogLevel:
    VERBOSE = 0,
    DEBUG = 1,
    INFO = 2,
    WARNING = 3,
    ERROR = 4,
    CRITICAL = 5,
    FATAL = 6,

cdef inline str log_level_to_string(int value):
    if value == 0:
        return 'VRB'
    elif value == 1:
        return 'DBG'
    elif value == 2:
        return 'INF'
    elif value == 3:
        return 'WRN'
    elif value == 4:
        return 'ERR'
    elif value == 5:
        return 'CRT'
    elif value == 6:
        return 'FTL'

cdef inline LogLevel log_level_from_string(str value):
    if value == 'VRB':
        return LogLevel.VERBOSE
    elif value == 'DBG':
        return LogLevel.DEBUG
    elif value == 'INF':
        return LogLevel.INFO
    elif value == 'WRN':
        return LogLevel.WARNING
    elif value == 'ERR':
        return LogLevel.ERROR
    elif value == 'CRT':
        return LogLevel.CRITICAL
    elif value == 'FTL':
        return LogLevel.FATAL


cdef class LogMessage:
    cdef readonly datetime timestamp
    cdef readonly LogLevel level
    cdef readonly str text
    cdef readonly long thread_id
    cdef str level_string(self)
    cdef str as_string(self)


cdef class Logger:
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

    cpdef void change_log_file_name(self, str name) except *
    cpdef void log(self, LogMessage message) except *
    cpdef list get_log_store(self)
    cpdef void clear_log_store(self) except *
    cpdef void _log(self, LogMessage message) except *
    cdef str _format_output(self, LogMessage message)
    cdef void _in_memory_log_store(self, LogLevel level, str text) except *
    cdef void _print_to_console(self, LogLevel level, str text) except *


cdef class TestLogger(Logger):
    pass


cdef class LoggerAdapter:
    cdef Logger _logger

    cdef readonly bint bypassed
    cdef readonly str component_name

    cpdef Logger get_logger(self)
    cpdef void verbose(self, str message) except *
    cpdef void debug(self, str message) except *
    cpdef void info(self, str message) except *
    cpdef void warning(self, str message) except *
    cpdef void error(self, str message) except *
    cpdef void critical(self, str message) except *
    cpdef void exception(self, ex) except *
    cdef void _send_to_logger(self, LogLevel level, str message) except *
    cdef str _format_message(self, str message)


cpdef void nautilus_header(LoggerAdapter logger) except *
