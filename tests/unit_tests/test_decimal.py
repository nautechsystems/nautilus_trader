#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_decimal.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from __future__ import absolute_import, division

import copy
import math
import operator
import sys
import unittest
from decimal import Decimal as _StdLibDecimal, InvalidOperation
from fractions import Fraction
from pickle import dumps, loads

from inv_trader.core.decimal import (
    Decimal,
    LIMIT_PREC,
    ROUNDING,
    set_rounding,
)

__metaclass__ = type


# set default rounding to ROUND_HALF_UP
set_rounding(ROUNDING.ROUND_HALF_UP)


class IntWrapper:

    def __init__(self, i):
        self.i = i

    def __int__(self):
        """int(self)"""
        return self.i

    def __eq__(self, i):
        """self == i"""
        return self.i == i


class DecimalTest(unittest.TestCase):

    """Mix-in defining the tests."""

    def test_constructor(self):
        self.assertEqual(Decimal(), 0)
        self.assertEqual(Decimal(precision=3), 0)
        self.assertTrue(Decimal(Decimal(1)))
        self.assertEqual(Decimal(Decimal(1)), Decimal(1))
        self.assertEqual(Decimal(Decimal('1.248'), 2), Decimal('1.25'))
        self.assertTrue(Decimal(-1, 2))
        self.assertEqual(Decimal(-1, 2), -1)
        self.assertTrue(Decimal(-37, 100))
        self.assertEqual(Decimal(-37, 100), -37)
        self.assertTrue(Decimal(sys.maxsize ** 100))
        self.assertTrue(Decimal(1.111e12))
        self.assertEqual(Decimal(1.111e12), 1.111e12)
        self.assertTrue(Decimal(sys.float_info.max))
        self.assertEqual(Decimal(sys.float_info.max), sys.float_info.max)
        self.assertTrue(Decimal(sys.float_info.max, 27))
        self.assertRaises(ValueError, Decimal, sys.float_info.min)
        self.assertEqual(Decimal(sys.float_info.min, 32), 0)
        self.assertEqual(Decimal(sys.float_info.min, 320), Decimal('2225073858507', 320) / 10 ** 320)
        self.assertTrue(Decimal(u'+21.4'))
        self.assertTrue(Decimal(b'+21.4'))
        self.assertNotEqual(Decimal('+21.4'), 21.4)
        self.assertTrue(Decimal('+1e2'))
        self.assertEqual(Decimal('+1e2'), 100)
        self.assertTrue(Decimal('12345678901234567890e-234'))
        self.assertTrue(Decimal('-1E2'))
        self.assertEqual(Decimal('-1E2'), -100)
        self.assertTrue(Decimal('   .23e+2 '))
        self.assertRaises(ValueError, Decimal, ' -  1.23e ')
        self.assertTrue(Decimal('+1e-2000'))
        self.assertTrue(Decimal(_StdLibDecimal('-123.4567')))
        self.assertTrue(Decimal(_StdLibDecimal('-123.4567'), 2))
        self.assertEqual(Decimal('-123.4567'), Decimal(_StdLibDecimal('-123.4567')))
        self.assertEqual(Decimal('1.234e12'), Decimal(_StdLibDecimal('1.234e12')))
        self.assertEqual(Decimal('1.234e-12'), Decimal(_StdLibDecimal('1.234e-12')))
        self.assertRaises(TypeError, Decimal, 1, '')
        self.assertRaises(ValueError, Decimal, 1, -1)
        self.assertRaises(TypeError, Decimal, Decimal)
        self.assertRaises(ValueError, Decimal, 123.4567)
        self.assertRaises(ValueError, Decimal, float('nan'))
        self.assertRaises(ValueError, Decimal, float('inf'))
        self.assertRaises(ValueError, Decimal, _StdLibDecimal('nan'))
        self.assertRaises(ValueError, Decimal, _StdLibDecimal('inf'))
        self.assertRaises(ValueError, Decimal, Fraction(1, 3))
        self.assertEqual(Decimal(Fraction(1, 4)), Decimal('0.25'))
        self.assertEqual(Decimal('0.33'), Fraction('0.33'))
        self.assertEqual(Decimal(IntWrapper(7)), Decimal('7'))

    def test_properties(self):
        d = Decimal('18.123')
        self.assertEqual(d.precision, 3)
        d = Decimal('18.123', 5)
        self.assertEqual(d.precision, 5)
        self.assertIs(d.real, d)
        self.assertEqual(d.imag, 0)

    def test_alternate_constructors(self):
        self.assertEqual(Decimal.from_float(1.111e12), Decimal(1.111e12))
        self.assertEqual(Decimal.from_float(12), Decimal(12))
        self.assertRaises(TypeError, Decimal.from_float, '1.111e12')
        self.assertEqual(Decimal.from_decimal(_StdLibDecimal('1.23e12')), Decimal('1.23e12'))
        self.assertEqual(Decimal.from_decimal(_StdLibDecimal(12)), Decimal(12))
        self.assertRaises(TypeError, Decimal.from_decimal, '1.23e12')
        self.assertEqual(Decimal.from_real(0.25), Decimal(0.25))
        self.assertEqual(Decimal.from_real(Fraction(1, 4)), Decimal(0.25))
        self.assertRaises(ValueError, Decimal.from_real, Fraction(1, 3))
        self.assertEqual(Decimal.from_real(Fraction(1, 3), exact=False), Decimal(Fraction(1, 3), LIMIT_PREC))
        self.assertRaises(ValueError, Decimal.from_real, float('nan'))
        self.assertRaises(ValueError, Decimal.from_real, float('inf'))
        self.assertRaises(TypeError, Decimal.from_real, 'a')

    def test_hash(self):
        d = Decimal('7.5')
        self.assertEqual(hash(d), hash(d))
        self.assertEqual(hash(d), hash(Decimal(d)))
        self.assertEqual(hash(Decimal('1.5')), hash(Decimal('1.5000')))
        self.assertNotEqual(hash(Decimal('1.5')), hash(Decimal('1.5001')))
        self.assertEqual(hash(Decimal('25')), hash(25))
        self.assertEqual(hash(Decimal('0.25')), hash(0.25))
        self.assertNotEqual(hash(Decimal('0.33')), hash(0.33))
        self.assertEqual(hash(Decimal('0.25')), hash(Fraction(1, 4)))
        self.assertEqual(hash(Decimal('0.33')), hash(Fraction('0.33')))

    def test_magnitude(self):
        self.assertEqual(Decimal('12.345').magnitude, 1)
        self.assertEqual(Decimal('10').magnitude, 1)
        self.assertEqual(Decimal('-286718.338524635465625').magnitude, 5)
        self.assertEqual(Decimal('286718338524635465627.500').magnitude, 20)
        self.assertEqual(Decimal('0.12345').magnitude, -1)
        self.assertEqual(Decimal('-0.0012345').magnitude, -3)
        self.assertEqual(Decimal('-0.01').magnitude, -2)

    def test_coercions(self):
        f = Decimal('23.4560')
        g = Decimal('57.99999999999999999999999999999999999')
        h = Decimal('.07')
        i = Decimal('-59.')
        self.assertEqual(int(f), 23)
        self.assertEqual(int(-f), -23)
        self.assertEqual(int(g), 57)
        self.assertEqual(int(-g), -57)
        self.assertEqual(int(h), 0)
        self.assertEqual(int(-h), 0)
        self.assertEqual(int(i), -59)
        self.assertEqual(int(-i), 59)
        self.assertEqual(float(f), 23.456)
        self.assertEqual(float(g), 58.0)
        self.assertEqual(float(h), 0.07)
        self.assertEqual(float(i), -59.)
        self.assertEqual(str(f), '23.4560')
        self.assertEqual(str(g), '57.99999999999999999999999999999999999')
        self.assertEqual(str(h), '0.07')
        self.assertEqual(str(i), '-59')
        self.assertTrue(float(Decimal(sys.float_info.max)))
        self.assertEqual(str(Decimal(-20.7e-3, 5)), '-0.02070')
        self.assertEqual(str(Decimal(-20.7e-12, 13)), '-0.0000000000207')
        self.assertEqual(str(Decimal(-20.5e-12, 13)), '-0.0000000000205')
        self.assertEqual(str(Decimal()), '0')
        self.assertEqual(str(Decimal(0, 2)), '0.00')
        self.assertEqual(repr(Decimal('-23.400007', 3)), "Decimal('-23.4', 3)")
        self.assertEqual(repr(f), "Decimal('23.456', 4)")
        self.assertEqual(repr(f), repr(copy.copy(f)))
        self.assertEqual(repr(f), repr(copy.deepcopy(f)))
        self.assertEqual(repr(h), "Decimal('0.07')")
        self.assertEqual(repr(i), "Decimal(-59)")
        self.assertEqual(repr(Decimal()), 'Decimal(0)')
        self.assertEqual(repr(Decimal(0, 2)), 'Decimal(0, 2)')

    def test_as_tuple(self):
        d = Decimal('23')
        self.assertEqual(d.as_tuple(), (0, 23, 0))
        d = Decimal('123.4567890')
        self.assertEqual(d.as_tuple(), (0, 1234567890, -7))
        d = Decimal('-12.3000')
        self.assertEqual(d.as_tuple(), (1, 123000, -4))

    def test_comparision(self):
        f = Decimal('23.456')
        g = Decimal('23.4562398')
        h = Decimal('-12.3')
        self.assertTrue(f != g)
        self.assertTrue(f < g)
        self.assertTrue(g >= f)
        self.assertTrue(min(f, g) == min(g, f) == f)
        self.assertTrue(max(f, g) == max(g, f) == g)
        self.assertTrue(g != h)
        self.assertTrue(h < g)
        self.assertTrue(g >= h)
        self.assertTrue(min(f, h) == min(h, f) == h)
        self.assertTrue(max(f, h) == max(h, f) == f)

    def test_mixed_type_comparision(self):
        d = Decimal('23.456')
        f = _StdLibDecimal('-23.456')
        g = Fraction('23.4562398')
        h = -12.5 + 0j
        self.assertTrue(d == Fraction('23.456'))
        self.assertTrue(Decimal('-12.5') == h)
        self.assertTrue(Decimal('-12.3') != h)
        self.assertEqual(-d, f)
        self.assertTrue(d > f)
        self.assertTrue(f <= d)
        self.assertTrue(d != g)
        self.assertTrue(d < g)
        self.assertTrue(g >= d)
        self.assertTrue(min(d, g) == min(g, d) == d)
        self.assertTrue(max(d, g) == max(g, d) == g)
        self.assertNotEqual(d, 'abc')
        if sys.version_info.major < 3:
            self.assertTrue(d < 'abc')
            self.assertTrue(d <= 'abc')
            self.assertTrue('abc' > d)
            self.assertTrue('abc' >= d)
        else:
            for op in (operator.lt, operator.le, operator.gt, operator.ge):
                self.assertRaises(TypeError, op, d, 'abc')
                self.assertRaises(TypeError, op, 'abc', d)
        # corner cases
        f_nan = float('nan')
        f_inf = float('inf')
        d_nan = _StdLibDecimal('nan')
        d_inf = _StdLibDecimal('inf')
        cmplx = 7 + 3j
        for num in (f_nan, f_inf, d_nan, d_inf, cmplx):
            self.assertFalse(d == num)
            self.assertFalse(num == d)
            self.assertTrue(d != num)
            self.assertTrue(num != d)
        for op in (operator.lt, operator.le):
            self.assertTrue(op(d, f_inf))
            self.assertFalse(op(f_inf, d))
            self.assertTrue(op(d, d_inf))
            self.assertFalse(op(d_inf, d))
        for op in (operator.gt, operator.ge):
            self.assertFalse(op(d, f_inf))
            self.assertTrue(op(d_inf, d))
            self.assertFalse(op(d, d_inf))
            self.assertTrue(op(f_inf, d))
        for op in (operator.lt, operator.le, operator.gt, operator.ge):
            self.assertFalse(op(d, f_nan))
            self.assertFalse(op(f_nan, d))
            # Decimal from standard lib is not compatible with float here:
            # ordering comparisons with 'nan' raises an exception.
            self.assertRaises(InvalidOperation, op, d, d_nan)
            self.assertRaises(InvalidOperation, op, d_nan, d)
        if sys.version_info.major < 3:
            self.assertTrue(d < cmplx)
            self.assertTrue(d <= cmplx)
            self.assertTrue(cmplx > d)
            self.assertTrue(cmplx >= d)
        else:
            for op in (operator.lt, operator.le, operator.gt, operator.ge):
                self.assertRaises(TypeError, op, d, cmplx)
                self.assertRaises(TypeError, op, cmplx, d)
                self.assertRaises(TypeError, op, d, h)
                self.assertRaises(TypeError, op, h, d)

    def test_adjustment(self):
        f = Decimal('23.456')
        g = Decimal('23.4562398').adjusted(3)
        h = Decimal('23.4565').adjusted(3)
        self.assertEqual(f.precision, g.precision)
        self.assertEqual(f, g)
        self.assertNotEqual(f, h)
        self.assertEqual(f.adjusted(0), 23)
        self.assertEqual(f.adjusted(-1), 20)
        self.assertEqual(f.adjusted(-5), 0)
        self.assertRaises(TypeError, f.adjusted, 3.7)
        f = Decimal('23.45600')
        self.assertEqual(f.precision, 5)
        g = f.adjusted()
        self.assertEqual(f, g)
        self.assertEqual(g.precision, 3)

    def test_quantization(self):
        f = Decimal('23.456')
        g = Decimal('23.4562398').quantize(Decimal('0.001'))
        h = Decimal('23.4565').quantize(Decimal('0.001'))
        self.assertEqual(f.precision, g.precision)
        self.assertEqual(f, g)
        self.assertNotEqual(f, h)
        self.assertEqual(f.quantize(1), 23)
        self.assertEqual(f.quantize('10'), 20)
        self.assertEqual(f.quantize(_StdLibDecimal(100)), 0)
        f = Decimal(1.4, 28)
        for quant in [Decimal('0.02'), Decimal(3), Fraction(1, 3)]:
            q = f.quantize(quant)
            d = abs(f - q)
            self.assertTrue(d < quant)
            r = q / quant
            self.assertEqual(r.denominator, 1)
        self.assertRaises(TypeError, f.quantize, complex(5))
        self.assertRaises(TypeError, f.quantize, 'a')

    def test_rounding(self):
        self.assertEqual(round(Decimal('23.456')), 23)
        self.assertEqual(round(Decimal('23.456'), 1), Decimal('23.5'))
        self.assertEqual(round(Decimal('2345.6'), -2), Decimal('2300'))
        if sys.version_info.major < 3:
            # In versions before 3.0 round always returns a float
            self.assertEqual(type(round(Decimal('23.456'))), float)
            self.assertEqual(type(round(Decimal('23.456'), 1)), float)
        else:
            # Beginning with version 3.0 round returns an int, if called
            # with one arg, otherwise the type of the first arg
            self.assertEqual(type(round(Decimal('23.456'))), int)
            self.assertEqual(type(round(Decimal('23.456'), 1)), Decimal)
        # test whether rounding is compatible with decimal
        for i in range(-34, 39, 11):
            d1 = _StdLibDecimal(i) / 4
            d2 = Decimal(d1, 2)
            self.assertEqual(round(d1), round(d2))
        for rounding in ROUNDING:
            for i in range(-34, 39, 11):
                e = _StdLibDecimal(i)
                d1 = e / 4
                d2 = Decimal(d1, 2)
                i1 = int(d1.quantize(e, rounding=rounding.name))
                i2 = int(d2.adjusted(0, rounding=rounding))
                self.assertEqual(i1, i2)

    def test_computation(self):
        f = Decimal('23.25')
        g = Decimal('-23.2562398')
        h = f + g
        self.assertEqual(--f, +f)
        self.assertEqual(f + f, 2 * f)
        self.assertEqual(f - f, 0)
        self.assertEqual(f + g, g + f)
        self.assertNotEqual(f - g, g - f)
        self.assertEqual(abs(g), abs(-g))
        self.assertEqual(g - g, 0)
        self.assertEqual(f + g - h, 0)
        self.assertEqual(f - 23.25, 0)
        self.assertEqual(23.25 - f, 0)
        self.assertEqual(f * g, g * f)
        self.assertNotEqual(f / g, g / f)
        self.assertTrue(-(3 * f) == (-3) * f == 3 * (-f))
        self.assertTrue((2 * f) * f == f * (2 * f) == f * (f * 2))
        self.assertEqual(3 * h, 3 * f + 3 * g)
        f2 = -2 * f
        self.assertTrue((-f2) / f == f2 / (-f) == -(f2 / f) == 2)
        self.assertEqual(g / f, Fraction(-116281199, 116250000))
        self.assertEqual(2 / Decimal(5), Decimal(2) / 5)
        self.assertRaises(ZeroDivisionError, f.__truediv__, 0)
        self.assertEqual(g // f, -2)
        self.assertEqual(g // -f, 1)
        self.assertEqual(g % -f, h)
        self.assertEqual(divmod(24, f), (Decimal(1, 2), Decimal('.75')))
        self.assertEqual(divmod(f, g), (-1, h))
        self.assertEqual(divmod(-f, g), (0, -f))
        self.assertEqual(divmod(f, -g), (0, f))
        self.assertEqual(divmod(-f, -g), (-1, -h))
        self.assertEqual(divmod(g, f), (-2, 2 * f + g))
        self.assertEqual(divmod(-g, f), (1, -h))
        self.assertEqual(divmod(g, -f), (1, h))
        self.assertEqual(divmod(-g, -f), (-2, -2 * f - g))
        self.assertNotEqual(divmod(g, f), divmod(f, g))
        self.assertEqual(divmod(f, g), (f // g, f % g))
        self.assertEqual(divmod(-f, g), (-f // g, -f % g))
        self.assertEqual(divmod(f, -g), (f // -g, f % -g))
        self.assertEqual(divmod(-f, -g), (-f // -g, -f % -g))
        self.assertEqual(divmod(g, f), (g // f, g % f))
        self.assertEqual(divmod(-g, f), (-g // f, -g % f))
        self.assertEqual(divmod(g, -f), (g // -f, g % -f))
        self.assertEqual(divmod(-g, -f), (-g // -f, -g % -f))
        self.assertEqual(f ** 2, f * f)
        self.assertEqual(f ** Decimal(3), f ** 3)
        self.assertEqual(g ** -2, 1 / g ** 2)
        self.assertEqual(2 ** f, 2.0 ** 23.25)
        self.assertEqual(Decimal(2) ** f, 2.0 ** 23.25)
        self.assertEqual(f ** 2.5, 23.25 ** 2.5)
        self.assertEqual(1 ** g, 1.0)
        self.assertEqual(math.trunc(f), 23)
        self.assertEqual(math.trunc(g), -23)
        self.assertEqual(math.floor(f), 23)
        self.assertEqual(math.floor(g), -24)
        self.assertEqual(math.ceil(f), 24)
        self.assertEqual(math.ceil(g), -23)
        self.assertEqual(round(f), 23)
        self.assertEqual(round(g), -23)
        self.assertRaises(TypeError, pow, f, 2, 5)

    def test_mixed_type_computation(self):
        f = Decimal('21.456')
        g = Fraction(1, 3)
        h = -12.5
        i = 9
        self.assertEqual(f + g, Fraction(8171, 375))
        self.assertEqual(g + f, Fraction(8171, 375))
        self.assertEqual(f + h, Decimal('8.956'))
        self.assertEqual(h + f, Decimal('8.956'))
        self.assertEqual(f + i, Decimal('30.456'))
        self.assertEqual(i + f, Decimal('30.456'))
        self.assertEqual(f - g, Fraction(7921, 375))
        self.assertEqual(g - f, Fraction(-7921, 375))
        self.assertEqual(f - h, Decimal('33.956'))
        self.assertEqual(h - f, Decimal('-33.956'))
        self.assertEqual(f - i, Decimal('12.456'))
        self.assertEqual(i - f, Decimal('-12.456'))
        self.assertEqual(f * g, Fraction(894, 125))
        self.assertEqual(g * f, Fraction(894, 125))
        self.assertEqual(f * h, Decimal('-268.2', 4))
        self.assertEqual(h * f, Decimal('-268.2', 4))
        self.assertEqual(f * i, Decimal('193.104'))
        self.assertEqual(i * f, Decimal('193.104'))
        self.assertEqual(f / g, Fraction(8046, 125))
        self.assertEqual(g / f, Fraction(125, 8046))
        self.assertEqual(f / h, Decimal('-1.71648'))
        self.assertEqual(h / f, Fraction(-3125, 5364))
        self.assertEqual(f / i, Decimal('2.384'))
        self.assertEqual(i / f, Fraction(125, 298))
        self.assertEqual(f // g, 64)
        self.assertEqual(g // f, 0)
        self.assertEqual(f // h, Decimal(-2, 3))
        self.assertEqual(h // f, Decimal(-1, 3))
        self.assertEqual(f // i, Decimal(2, 3))
        self.assertEqual(i // f, Decimal(0, 3))
        self.assertEqual(f % g, Fraction(46, 375))
        self.assertEqual(g % f, Fraction(1, 3))
        self.assertEqual(f % h, Decimal('-3.544'))
        self.assertEqual(h % f, Decimal('8.956'))
        self.assertEqual(f % i, Decimal('3.456'))
        self.assertEqual(i % f, Decimal(9, 3))
        self.assertEqual(Decimal('0.5') + 0.3, Fraction(14411518807585587, 18014398509481984))
        self.assertEqual(0.3 + Decimal('0.5'), Fraction(14411518807585587, 18014398509481984))
        self.assertEqual(Decimal('0.5') - 0.3, Fraction(3602879701896397, 18014398509481984))
        self.assertEqual(0.3 - Decimal('0.5'), Fraction(-3602879701896397, 18014398509481984))
        self.assertEqual(Decimal(2 ** 32) * 0.3, Decimal('1288490188.7999999523162841796875'))
        self.assertEqual(0.3 * Decimal(2 ** 32), Decimal('1288490188.7999999523162841796875'))
        self.assertEqual(Decimal('0.5') * 0.3, Fraction(5404319552844595, 36028797018963968))
        self.assertEqual(0.3 * Decimal('0.5'), Fraction(5404319552844595, 36028797018963968))
        self.assertEqual(0.3 / Decimal(2), Fraction(5404319552844595, 36028797018963968))
        self.assertEqual(Decimal(2) / 0.3, Fraction(36028797018963968, 5404319552844595))
        # corner cases
        nan = float('nan')
        inf = float('inf')
        cmplx = 7 + 3j
        for op in (operator.add, operator.sub, operator.mul, operator.truediv,
                   operator.floordiv, operator.mod):
            self.assertRaises(ValueError, op, f, nan)
            self.assertRaises(ValueError, op, nan, f)
            self.assertRaises(ValueError, op, f, inf)
            self.assertRaises(ValueError, op, inf, f)
            self.assertRaises(TypeError, op, f, cmplx)
            self.assertRaises(TypeError, op, cmplx, f)

    def test_pickle(self):
        d = Decimal(Decimal(1))
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal(-1, 2)
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal(-37, 100)
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal(sys.maxsize ** 100)
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal(1.111e12)
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal('+21.4')
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal('+1e2')
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal('12345678901234567890e-234')
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal('-1E2')
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal('.23e+2')
        self.assertEqual(loads(dumps(d)), d)
        d = Decimal('+1e-2000')
        self.assertEqual(loads(dumps(d)), d)

    def test_format(self):
        d = Decimal('0.0038')
        self.assertEqual(format(d), str(d))
        self.assertEqual(format(d, '<.3'), str(d.adjusted(3)))
        self.assertEqual(format(d, '<.4'), str(d.adjusted(4)))
        self.assertEqual(format(d, '<.7'), str(d.adjusted(7)))
        d = Decimal('-59.')
        self.assertEqual(format(d), str(d))
        self.assertEqual(format(d, '<.3'), str(d.adjusted(3)))
        self.assertEqual(format(d, '>10.3'), '   %s' % d.adjusted(3))
        self.assertRaises(ValueError, format, d, ' +012.5F')
        self.assertRaises(ValueError, format, d, '_+012.5F')
        self.assertRaises(ValueError, format, d, '+012.5e')
        self.assertRaises(ValueError, format, d, '+012.5E')
        self.assertRaises(ValueError, format, d, '+012.5g')
        self.assertRaises(ValueError, format, d, '+012.5G')
