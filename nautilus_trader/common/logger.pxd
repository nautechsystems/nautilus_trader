# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock


cdef class Logger:
    """
    Provides a logger for the trader client which wraps the Python logging module.
    """
    cdef int _log_level_console
    cdef int _log_level_file
    cdef int _log_level_store
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
    cpdef void log(self, int level, str message)
    cpdef list get_log_store(self)
    cpdef void clear_log_store(self)
    cpdef void _debug(self, datetime timestamp, str message)
    cpdef void _info(self, datetime timestamp, str message)
    cpdef void _warning(self, datetime timestamp, str message)
    cpdef void _error(self, datetime timestamp, str message)
    cpdef void _critical(self, datetime timestamp, str message)
    cdef str _format_message(self, datetime timestamp, str log_level, str message)
    cdef void _log_store_handler(self, int level, str message)
    cdef void _console_print_handler(self, int level, str message)


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
