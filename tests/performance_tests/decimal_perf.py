#!/usr/bin/env python
# -*- coding: utf-8 -*-
# ----------------------------------------------------------------------------
# Name:        dec_perf
# Purpose:     Compare performance of different implementations of Decimal
#
# Author:      Michael Amrhein (michael@adrhinum.de)
#
# Copyright:   (c) 2014 ff. Michael Amrhein
# License:     This program is free software. You can redistribute it, use it
#              and/or modify it under the terms of the 2-clause BSD license.
#              For license details please read the file LICENSE.TXT provided
#              together with the source code.
# ----------------------------------------------------------------------------
# $Source$
# $Revision$


"""Compare performance of different implementations of Decimal."""


from __future__ import absolute_import, division, print_function

import math
import platform
from decimal import Decimal as StdLibDecimal
from timeit import Timer

from inv_trader.core.decimal import Decimal

PY_IMPL = platform.python_implementation()
PY_VERSION = platform.python_version()

dec_impls = ("StdLibDecimal", "Decimal")


def test_cdecimal():
    """Execute several computations for performance testing."""
    f = Decimal('23.25')
    g = Decimal('-23.2562398')
    h = f + g
    b = (--f == +f)
    b = (abs(g) == abs(-g))
    r = g - g
    r = f + g - h
    r = f - 23
    r = 23 - f
    b = (-(3 * f) == (-3) * f == 3 * (-f))
    b = ((2 * f) * f == f * (2 * f) == f * (f * 2))
    b = (3 * h == 3 * f + 3 * g)
    f2 = -2 * f
    b = ((-f2) / f == f2 / (-f) == -(f2 / f) == 2)
    b = (g / f == Decimal('-1.0002684'))
    b = (g // f == -2)
    b = (g // -f == 1)
    b = (g % -f == h)
    b = (divmod(24, f) == (Decimal(1), Decimal('.75')))
    b = (divmod(-g, f) == (1, -h))
    b = (f ** 2 == f * f)
    b = (g ** -2 == 1 / g ** 2)
    b = (2 ** f == 2.0 ** 23.25)
    b = (1 ** g == 1.0)
    b = (math.floor(f) == 23)
    b = (math.floor(g) == -24)
    b = (math.ceil(f) == 24)
    b = (math.ceil(g) == -23)
    b = (round(f) == 23)
    b = (round(g) == -23)
    if b:
        return r
    else:
        return -r


if __name__ == '__main__':
    print('---', PY_IMPL, PY_VERSION, '---')
    # for reference, run it with 'float'
    timer = Timer("testComputation(float)",
                  "from test_decimal_perf import testComputation")
    results = timer.repeat(10, 1000)
    print("float:", min(results))
    timer = Timer(test_cdecimal())
    results = timer.repeat(10, 1000)
    print("%s: %s" % (min(results)))

    print('---')
