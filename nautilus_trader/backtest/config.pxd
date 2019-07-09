#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="config.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.objects cimport Money

cdef class BacktestConfig:
    """
    Provides a configuration for a BacktestEngine.
    """
    cdef readonly bint frozen_account
    cdef readonly Money starting_capital
    cdef readonly Currency account_currency
    cdef readonly float commission_rate_bp
    cdef readonly bint bypass_logging
    cdef readonly int level_console
    cdef readonly int level_file
    cdef readonly int level_store
    cdef readonly bint console_prints
    cdef readonly bint log_thread
    cdef readonly bint log_to_file
    cdef readonly str log_file_path
