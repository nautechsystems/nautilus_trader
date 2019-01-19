#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="money.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from inv_trader.core.decimal cimport Decimal


cpdef Decimal money_zero()
cpdef Decimal money(float amount)
