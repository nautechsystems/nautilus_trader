# -*- coding: utf-8 -*-
# ----------------------------------------------------------------------------
# Name:        _cdecimalfp
# Purpose:     Decimal fixed-point arithmetic (Cython implementation)
#
# Author:      Michael Amrhein (michael@adrhinum.de)
#
# Copyright:   (c) 2014 ff. Michael Amrhein
#              Portions adopted from FixedPoint.py written by Tim Peters
# License:     This program is free software. You can redistribute it, use it
# License:     This program is part of a larger application. For license
#              details please read the file LICENSE.TXT provided together
#              with the application.
# ----------------------------------------------------------------------------
# $Source$
# $Revision$

# cython: language_level=3, boundscheck=False, wraparound=False

"""Decimal fixed-point arithmetic."""


from __future__ import absolute_import, division

# standard lib imports
import locale
from decimal import Decimal as _StdLibDecimal
from fractions import Fraction
from functools import reduce
from math import floor, log10
from numbers import Complex, Integral, Rational, Real
try:
    from math import gcd
except ImportError:
    from fractions import gcd

# cython cimports
from cpython.long cimport *
from cpython.number cimport *
from cpython.object cimport Py_EQ, Py_NE, PyObject_RichCompare
from libc.limits cimport LLONG_MAX
from libc.stdlib cimport atoi


# Compatible testing for strings
py_str = type(u'')
py_bytes = type(b'')
str_types = (py_bytes, py_str)


# constants used for power to 10
cdef PYLONG_10 = PyLong_FromLong(10)
cdef LLONG_MAX_LOG10 = int(log10(PyLong_FromLongLong(LLONG_MAX)))


# Extension type Decimal

cdef class Decimal:

    """Decimal number with a given number of fractional digits.

    Args:
        value (see below): numerical value (default: None)
        precision (numbers.Integral): number of fractional digits (default:
            None)

    If `value` is given, it must either be a string (type `str` or `unicode`
    in Python 2.x, `bytes` or `str` in Python 3.x), an instance of
    `numbers.Integral` (for example `int` or `long` in Python 2.x, `int` in
    Python 3.x), `number.Rational` (for example `fractions.Fraction`),
    `decimal.Decimal`, a finite instance of `numbers.Real` (for example
    `float`) or be convertable to a `float` or an `int`.

    If a string is given as value, it must be a string in one of two formats:

    * [+|-]<int>[.<frac>][<e|E>[+|-]<exp>] or
    * [+|-].<frac>[<e|E>[+|-]<exp>].

    If given value is `None`, Decimal(0) is returned.

    Returns:
        :class:`Decimal` instance derived from `value` according
            to `precision`

    The value is always adjusted to the given precision or the precision is
    calculated from the given value, if no precision is given. For performance
    reasons, in the latter case the conversion of a `numbers.Rational` (like
    `fractions.Fraction`) or a `float` tries to give an exact result as a
    :class:`Decimal` only up to a fixed limit of fractional digits
    (`decimalfp.LIMIT_PREC`).

    Raises:
        TypeError: `precision` is given, but not of type `Integral`.
        TypeError: `value` is not an instance of the types listed above and
            not convertable to `float` or `int`.
        ValueError: `precision` is given, but not >= 0.
        ValueError: `value` can not be converted to a `Decimal` (with a number
            of fractional digits <= `LIMIT_PREC` if no `precision` is given).

    :class:`Decimal` instances are immutable.

    """

    def __init__(self, object value=None, object precision=None):

        cdef object num
        cdef object den
        cdef int prec
        cdef Decimal dec

        if precision is None:
            if value is None:
                self._value = 0
                self._precision = 0
                return
        else:
            if not isinstance(precision, int):
                raise TypeError("Precision must be of <type 'int'>.")
            if precision < 0:
                raise ValueError("Precision must be >= 0.")
            if value is None:
                self._value = 0
                self._precision = precision
                return

        # Decimal
        if isinstance(value, Decimal):
            self._precision = prec = (<Decimal>value)._precision
            self._value = (<Decimal>value)._value
            if precision is not None and precision != prec:
                _adjust(self, precision)
            return

        # String
        if isinstance(value, str_types):
            prec = -1 if precision is None else precision
            try:
                s = value.encode()
            except AttributeError:
                s = value
            try:
                _dec_from_str(self, s, prec)
            except ValueError:
                raise ValueError("Can't convert %s to Decimal." % repr(value))
            return

        # Integral
        if isinstance(value, Integral):
            lValue = PyNumber_Long(value)
            if precision is None:
                self._precision = 0
                self._value = lValue
            else:
                self._precision = precision
                self._value = lValue * base10pow(precision)
            return

        # Decimal (from standard library)
        if isinstance(value, _StdLibDecimal):
            if value.is_finite():
                sign, digits, exp = value.as_tuple()
                coeff = (-1) ** sign * reduce(lambda x, y: x * 10 + y, digits)
                prec = -1 if precision is None else precision
                _dec_from_coeff_exp(self, coeff, exp, prec)
                return
            else:
                raise ValueError("Can't convert %s to Decimal." % repr(value))

        # Rational
        if isinstance(value, Rational):
            prec = -1 if precision is None else precision
            num, den = value.numerator, value.denominator
            try:
                _dec_from_rational(self, num, den, prec)
            except ValueError:
                    raise ValueError("Can't convert %s exactly to Decimal."
                                     % repr(value))
            return

        # Float
        if isinstance(value, float):
            try:
                num, den = value.as_integer_ratio()
            except (ValueError, OverflowError):
                raise ValueError("Can't convert %s to Decimal." % repr(value))
            prec = -1 if precision is None else precision
            try:
                _dec_from_rational(self, num, den, prec)
            except ValueError:
                    raise ValueError("Can't convert %s exactly to Decimal."
                                     % repr(value))
            return

        # Others
        # If there's a float or int equivalent to value, use it
        ev = None
        try:
            ev = PyNumber_Float(value)
        except:
            try:
                ev = PyNumber_Int(value)
            except:
                pass
        if ev == value:     # do we really have the same value?
            dec = Decimal(ev, precision)
            self._value = dec._value
            self._precision = dec._precision
            return

        # unable to create Decimal
        raise TypeError("Can't convert %s to Decimal." % repr(value))

    # to be compatible to fractions.Fraction
    @classmethod
    def from_float(cls, f):
        """Convert a finite float (or int) to a :class:`Decimal`.

        Args:
            f (float or int): number to be converted to a `Decimal`

        Returns:
            :class:`Decimal` instance derived from `f`

        Raises:
            TypeError: `f` is neither a `float` nor an `int`.
            ValueError: `f` can not be converted to a :class:`Decimal` with
                a precision <= `LIMIT_PREC`.

        Beware that Decimal.from_float(0.3) != Decimal('0.3').
        """
        if not isinstance(f, (float, Integral)):
            raise TypeError("%s is not a float." % repr(f))
        return cls(f)

    # to be compatible to fractions.Fraction
    @classmethod
    def from_decimal(cls, d):
        """Convert a finite decimal number to a :class:`Decimal`.

        Args:
            d (see below): decimal number to be converted to a
                :class:`Decimal`

        `d` can be of type :class:`Decimal`, `numbers.Integral` or
        `decimal.Decimal`.

        Returns:
            :class:`Decimal` instance derived from `d`

        Raises:
            TypeError: `d` is not an instance of the types listed above.
            ValueError: `d` can not be converted to a :class:`Decimal`.
        """
        if not isinstance(d, (Decimal, Integral, _StdLibDecimal)):
            raise TypeError("%s is not a Decimal." % repr(d))
        return cls(d)

    @classmethod
    def from_real(cls, r, exact=True):
        """Convert a Real number to a :class:`Decimal`.

        Args:
            r (`numbers.Real`): number to be converted to a :class:`Decimal`
            exact (`bool`): `True` if `r` shall exactly be represented by
                the resulting :class:`Decimal`

        Returns:
            :class:`Decimal` instance derived from `r`

        Raises:
            TypeError: `r` is not an instance of `numbers.Real`.
            ValueError: `exact` is `True` and `r` can not exactly be converted
                to a :class:`Decimal` with a precision <= `LIMIT_PREC`.

        If `exact` is `False` and `r` can not exactly be represented by a
        `Decimal` with a precision <= `LIMIT_PREC`, the result is rounded to a
        precision = `LIMIT_PREC`.
        """
        if not isinstance(r, Real):
            raise TypeError("%s is not a Real." % repr(r))
        try:
            return cls(r)
        except ValueError:
            if exact:
                raise
            else:
                return cls(r, LIMIT_PREC)

    @property
    def precision(self):
        """Return precision of `self`."""
        return self._precision

    @property
    def magnitude(self):
        """Return magnitude of `self` in terms of power to 10, i.e. the
        largest integer exp so that 10 ** exp <= self."""
        return int(floor(log10(abs(self._value)))) - self._precision

    @property
    def numerator(self):
        """Return the numerator from the pair of integers with the smallest
        positive denominator, whose ratio is equal to `self`."""
        n, d = self.as_integer_ratio()
        return n

    @property
    def denominator(self):
        """Return the smallest positive denominator from the pairs of
        integers, whose ratio is equal to `self`."""
        n, d = self.as_integer_ratio()
        return d

    @property
    def real(self):
        """The real part of `self`.

        Returns `self` (Real numbers are their real component)."""
        return self

    @property
    def imag(self):
        """The imaginary part of `self`.

        Returns 0 (Real numbers have no imaginary component)."""
        return 0

    def adjusted(self, precision=None, rounding=None):
        """Return copy of `self`, adjusted to the given `precision`, using the
        given `rounding` mode.

        Args:
            precision (int): number of fractional digits (default: None)
            rounding (str): rounding mode (default: None)

        Returns:
            :class:`Decimal` instance derived from `self`, adjusted
                to the given `precision`, using the given `rounding` mode

        If no `precision` is given, the result is adjusted to the minimum
        precision preserving x == x.adjusted().

        If no `rounding` mode is given, the default mode from the current
        context (from module `decimal`) is used.

        If the given `precision` is less than the precision of `self`, the
        result is rounded and thus information may be lost.
        """
        cdef Decimal result
        if precision is None:
            v, p = _reduce(self)
            result = Decimal()
            result._value = v
            result._precision = p
        else:
            if not isinstance(precision, int):
                raise TypeError("Precision must be of <type 'int'>.")
            result = Decimal(self)
            _adjust(result, precision, rounding)
        return result

    def quantize(self, quant, rounding=None):
        """Return integer multiple of `quant` closest to `self`.

        Args:
            quant (Rational): quantum to get a multiple from
            rounding (str): rounding mode (default: None)

        A string can be given for `quant` as long as it is convertable to a
        :class:`Decimal`.

        If no `rounding` mode is given, the default mode from the current
        context (from module `decimal`) is used.

        Returns:
            :class:`Decimal` instance that is the integer multiple of `quant`
                closest to `self` (according to `rounding` mode); if result
                can not be represented as :class:`Decimal`, an instance of
                `Fraction` is returned

        Raises:
            TypeError: `quant` is not a Rational number or can not be
                converted to a :class:`Decimal`
        """
        try:
            num, den = quant.numerator, quant.denominator
        except AttributeError:
            try:
                num, den = quant.as_integer_ratio()
            except AttributeError:
                try:
                    quant = Decimal(quant)
                except (TypeError, ValueError):
                    raise TypeError("Can't quantize to a '%s': %s."
                                    % (quant.__class__.__name__, quant))
                num, den = quant.as_integer_ratio()
        mult = _div_rounded(self._value * den,
                            base10pow(self._precision) * num,
                            rounding)
        return Decimal(mult) * quant

    def as_tuple(self):
        """Return a tuple (sign, coeff, exp) so that
        self == (-1) ** sign * coeff * 10 ** exp."""
        v = self._value
        sign = int(v < 0)
        coeff = abs(v)
        exp = - self._precision
        return sign, coeff, exp

    # return lowest fraction equal to self
    def as_integer_ratio(self):
        """Return the pair of numerator and denominator with the smallest
        positive denominator, whose ratio is equal to self."""
        n, d = self._value, base10pow(self._precision)
        g = gcd(n, d)
        return n // g, d // g

    def __copy__(self):
        """Return self (Decimal instances are immutable)."""
        return self

    def __deepcopy__(self, memo):
        return self.__copy__()

    def __reduce__(self):
        return (Decimal, (), (self._value, self._precision))

    def __setstate__(self, state):
        self._value, self._precision = state

    # string representation
    def __repr__(self):
        """repr(self)"""
        cdef int sp, rp, n
        sp = self._precision
        rv, rp = _reduce(self)
        if rp == 0:
            s = str(rv)
        else:
            s = str(abs(rv))
            n = len(s)
            if n > rp:
                s = "'%s%s.%s'" % ((rv < 0) * '-', s[0:-rp], s[-rp:])
            else:
                s = "'%s0.%s%s'" % ((rv < 0) * '-', (rp-n) * '0', s)
        if sp == rp:
            return "Decimal(%s)" % (s)
        else:
            return "Decimal(%s, %s)" % (s, sp)

    def __str__(self):
        """str(self)"""
        sp = self._precision
        if sp == 0:
            return "%i" % self._value
        else:
            sv = self._value
            i = _int(sv, sp)
            f = sv - i * 10 ** sp
            s = (i == 0 and f < 0) * '-'  # -1 < self < 0 => i = 0 and f < 0 !
            return '%s%i.%0*i' % (s, i, sp, abs(f))

    def __format__(self, fmtSpec):
        """Return `self` converted to a string according to given format
        specifier.

        Args:
            fmtSpec (str): a standard format specifier for a number

        Returns:
            str: `self` converted to a string according to `fmtSpec`
        """
        cdef int nToFill, prec, xtraShift
        (fmtFill, fmtAlign, fmtSign, fmtMinWidth, fmtThousandsSep,
            fmtGrouping, fmtDecimalPoint, fmtPrecision,
            fmtType) = _getFormatParams(fmtSpec)
        nToFill = fmtMinWidth
        prec = self._precision
        if fmtPrecision is None:
            fmtPrecision = prec
        if fmtType == '%':
            percentSign = '%'
            nToFill -= 1
            xtraShift = 2
        else:
            percentSign = ''
            xtraShift = 0
        val = _get_adjusted_value(self, fmtPrecision + xtraShift)
        if val < 0:
            sign = '-'
            nToFill -= 1
            val = abs(val)
        elif fmtSign == '-':
            sign = ''
        else:
            sign = fmtSign
            nToFill -= 1
        rawDigits = format(val, '>0%i' % (fmtPrecision + 1))
        if fmtPrecision:
            decimalPoint = fmtDecimalPoint
            rawDigits, fracPart = (rawDigits[:-fmtPrecision],
                                   rawDigits[-fmtPrecision:])
            nToFill -= fmtPrecision + 1
        else:
            decimalPoint = ''
            fracPart = ''
        if fmtAlign == '=':
            intPart = _padDigits(rawDigits, max(0, nToFill), fmtFill,
                                 fmtThousandsSep, fmtGrouping)
            return sign + intPart + decimalPoint + fracPart + percentSign
        else:
            intPart = _padDigits(rawDigits, 0, fmtFill,
                                 fmtThousandsSep, fmtGrouping)
            raw = sign + intPart + decimalPoint + fracPart + percentSign
            if nToFill > len(intPart):
                fmt = "%s%s%i" % (fmtFill, fmtAlign, fmtMinWidth)
                return format(raw, fmt)
            else:
                return raw

    # compare to Decimal or any type that can be converted to a Decimal
    def _make_comparable(self, other):
        cdef int selfPrec, otherPrec
        cdef Decimal dec
        if isinstance(other, Decimal):
            selfPrec, otherPrec = self._precision, (<Decimal>other)._precision
            if selfPrec == otherPrec:
                return self._value, (<Decimal>other)._value
            elif selfPrec < otherPrec:
                return (_get_adjusted_value(self, otherPrec),
                        (<Decimal>other)._value)
            else:
                return (self._value,
                        _get_adjusted_value(<Decimal>other, selfPrec))
        if isinstance(other, Integral):
            return self._value, other * base10pow(self._precision)
        if isinstance(other, Rational):
            return (self.numerator * other.denominator,
                    other.numerator * self.denominator)
        if isinstance(other, Real):
            try:
                num, den = other.as_integer_ratio()
            except AttributeError:
                raise NotImplementedError
            except (ValueError, OverflowError):
                # 'nan' and 'inf'
                return self._value, other
            return (self.numerator * den, num * self.denominator)
        if isinstance(other, _StdLibDecimal):
            return (self, Decimal(other))
        if isinstance(other, Complex) and other.imag == 0:
            return self._make_comparable(other.real)
        else:
            raise NotImplementedError

    def __richcmp__(self, other, int cmp):
        """Compare `self` and `other` using operator `cmp`."""
        sv = self._value
        sp = self._precision
        if isinstance(other, Decimal):
            ov = (<Decimal>other)._value
            op = (<Decimal>other)._precision
            # if sp == op, we are done, otherwise we adjust the value with the
            # lesser precision
            if sp < op:
                sv *= 10 ** (op - sp)
            elif sp > op:
                ov *= 10 ** (sp - op)
            return PyObject_RichCompare(sv, ov, cmp)
        elif isinstance(other, Integral):
            ov = int(other) * 10 ** sp
            return PyObject_RichCompare(sv, ov, cmp)
        elif isinstance(other, Rational):
            # cross-wise product of numerator and denominator
            sv *= other.denominator
            ov = other.numerator * 10 ** sp
            return PyObject_RichCompare(sv, ov, cmp)
        elif isinstance(other, Real):
            try:
                num, den = other.as_integer_ratio()
            except AttributeError:
                return NotImplemented
            except (ValueError, OverflowError):
                # 'nan' and 'inf'
                return PyObject_RichCompare(sv, other, cmp)
            # cross-wise product of numerator and denominator
            sv *= den
            ov = num * 10 ** sp
            return PyObject_RichCompare(sv, ov, cmp)
        elif isinstance(other, _StdLibDecimal):
            if other.is_finite():
                sign, digits, exp = other.as_tuple()
                ov = (-1) ** sign * reduce(lambda x, y: x * 10 + y, digits)
                op = abs(exp)
                # if sp == op, we are done, otherwise we adjust the value with
                # the lesser precision
                if sp < op:
                    sv *= 10 ** (op - sp)
                elif sp > op:
                    ov *= 10 ** (sp - op)
                return PyObject_RichCompare(sv, ov, cmp)
            else:
                # 'nan' and 'inf'
                return PyObject_RichCompare(sv, other, cmp)
        elif isinstance(other, Complex):
            if cmp in (Py_EQ, Py_NE):
                if other.imag == 0:
                    return PyObject_RichCompare(self, other.real, cmp)
                else:
                    return False if cmp == Py_EQ else True
        # don't know how to compare
        return NotImplemented

    def __hash__(self):
        """hash(self)"""
        cdef int sp
        sv, sp = self._value, self._precision
        if sp == 0:                         # if self == int(self),
            return hash(sv)                           # same hash as int
        else:                               # otherwise same hash as
            return hash(Fraction(sv, base10pow(sp)))  # equivalent fraction

    # return 0 or 1 for truth-value testing
    def __nonzero__(self):
        """bool(self)"""
        return self._value != 0
    #__bool__ = __nonzero__

    # return integer portion as int
    def __int__(self):
        """math.trunc(self)"""
        return _int(self._value, self._precision)
    __trunc__ = __int__

    # convert to float (may loose precision!)
    def __float__(self):
        """float(self)"""
        return self._value / base10pow(self._precision)

    def __pos__(self):
        """+self"""
        return self

    def __neg__(self):
        """-self"""
        cdef Decimal result
        result = Decimal(self)
        result._value = -result._value
        return result

    def __abs__(self):
        """abs(self)"""
        cdef Decimal result
        result = Decimal(self)
        result._value = abs(result._value)
        return result

    def __add__(x, y):
        """x + y"""
        if isinstance(x, Decimal):
            return add(x, y)
        if isinstance(y, Decimal):
            return add(y, x)
        return NotImplemented

    def __sub__(x, y):
        """x - y"""
        if isinstance(x, Decimal):
            return sub(x, y)
        if isinstance(y, Decimal):
            return add(-y, x)
        return NotImplemented

    def __mul__(x, y):
        """x * y"""
        if isinstance(x, Decimal):
            return mul(x, y)
        if isinstance(y, Decimal):
            return mul(y, x)
        return NotImplemented

    def __div__(x, y):
        """x / y"""
        if isinstance(x, Decimal):
            return div1(x, y)
        if isinstance(y, Decimal):
            return div2(x, y)
        return NotImplemented

    # Decimal division is true division
    def __truediv__(x, y):
        """x / y"""
        if isinstance(x, Decimal):
            return div1(x, y)
        if isinstance(y, Decimal):
            return div2(x, y)
        return NotImplemented

    def __divmod__(x, y):
        """x // y, x % y"""
        if isinstance(x, Decimal):
            return divmod1(x, y)
        if isinstance(y, Decimal):
            return divmod2(x, y)
        return NotImplemented

    def __floordiv__(x, y):
        """x // y"""
        if isinstance(x, Decimal):
            return floordiv1(x, y)
        if isinstance(y, Decimal):
            return floordiv2(x, y)
        return NotImplemented

    def __mod__(x, y):
        """x % y"""
        if isinstance(x, Decimal):
            return mod1(x, y)
        if isinstance(y, Decimal):
            return mod2(x, y)
        return NotImplemented

    def __pow__(x, y, mod):
        """x ** y

        If y is an integer (or a Rational with denominator = 1), the
        result will be a Decimal. Otherwise, the result will be a float or
        complex since roots are generally irrational.

        `mod` must always be None (otherwise a `TypeError` is raised).
        """
        if mod is not None:
            raise TypeError("3rd argument not allowed unless all arguments "
                            "are integers")
        if isinstance(x, Decimal):
            return pow1(x, y)
        if isinstance(y, Decimal):
            return pow2(x, y)
        return NotImplemented

    def __floor__(self):
        """math.floor(self)"""
        n, d = self._value, base10pow(self._precision)
        return n // d

    def __ceil__(self):
        """math.ceil(self)"""
        n, d = self._value, base10pow(self._precision)
        return -(-n // d)

    def __round__(self, precision=None):
        """round(self [, ndigits])

        Round `self` to a given precision in decimal digits (default 0).
        `ndigits` may be negative.

        Note: This method is called by the built-in `round` function only in
        Python 3.x! It returns an `int` when called with one argument,
        otherwise a :class:`Decimal`.
        """
        if precision is None:
            # return integer
            return int(self.adjusted(0, ROUNDING.default))
        # otherwise return Decimal
        return self.adjusted(precision, ROUNDING.default)


# register Decimal as Rational
Rational.register(Decimal)


# helper functions:


# 10 ** exp as PyLong
cdef object base10pow(int exp):
    if 0 <= exp < LLONG_MAX_LOG10:
        return PyLong_FromLongLong(10 ** exp)
    return PyNumber_Power(PYLONG_10, exp, None)

# parse string
import re
_pattern = r"""
            \s*
            (?P<sign>[+|-])?
            (
                (?P<int>\d+)(\.(?P<frac>\d*))?
                |
                \.(?P<onlyfrac>\d+)
            )
            ([eE](?P<exp>[+|-]?\d+))?
            \s*$
            """.encode()
_parseString = re.compile(_pattern, re.VERBOSE).match

# parse a format specifier
# [[fill]align][sign][0][minimumwidth][,][.precision][type]

_pattern = r"""
            \A
            (?:
                (?P<fill>.)?
                (?P<align>[<>=^])
            )?
            (?P<sign>[-+ ])?
            (?P<zeropad>0)?
            (?P<minimumwidth>(?!0)\d+)?
            (?P<thousands_sep>,)?
            (?:\.(?P<precision>0|(?!0)\d+))?
            (?P<type>[fFn%])?
            \Z
            """
_parseFormatSpec = re.compile(_pattern, re.VERBOSE).match
del re, _pattern


cdef void _dec_from_str(Decimal dec, bytes s, int prec) except *:
    cdef bytes sExp, sInt, sFrac
    cdef object nInt, nFrac, coeff, parsed
    cdef int exp, shift10
    cdef char *pend
    parsed = _parseString(s)
    if parsed is None:
        raise ValueError
    sExp = parsed.group('exp')
    if sExp:
        exp = atoi(sExp)
    else:
        exp = 0
    sInt = parsed.group('int')
    if sInt:
        nInt = PyLong_FromString(sInt, &pend, 10)
        sFrac = parsed.group('frac')
    else:
        nInt = PyLong_FromLong(0)
        sFrac = parsed.group('onlyfrac')
    if sFrac:
        nFrac = PyLong_FromString(sFrac, &pend, 10)
        shift10 = len(sFrac)
    else:
        nFrac = PyLong_FromLong(0)
        shift10 = 0
    coeff = nInt * base10pow(shift10) + nFrac
    exp -= shift10
    if parsed.group('sign') == b'-':
        coeff = -coeff
    _dec_from_coeff_exp(dec, coeff, exp, prec)


cdef void _dec_from_coeff_exp(Decimal dec, object coeff, int exp,
                              int prec) except *:
    """Set `dec` so that it equals coeff * 10 ** exp, rounded to precision
    `prec`."""
    cdef int shift10
    if prec == -1:
        if exp > 0:
            dec._precision = 0
            dec._value = coeff * base10pow(exp)
        else:
            dec._precision = abs(exp)
            dec._value = coeff
    else:
        dec._precision = prec
        shift10 = exp + prec
        if shift10 == 0:
            dec._value = coeff
        if shift10 > 0:
            dec._value = coeff * base10pow(shift10)
        else:
            dec._value = _div_rounded(coeff, base10pow(-shift10))


cdef void _dec_from_rational(Decimal dec, object num, object den,
                             int prec) except *:
    if prec >= 0:
        dec._value = _div_rounded(num * base10pow(prec), den)
        dec._precision = prec
    else:
        rem = _approx_rational(dec, num, den)
        if rem:
            raise ValueError


cdef bint _approx_rational(Decimal dec, object num, object den,
                           int minPrec=0) except *:
    """Approximate num / den as Decimal.

    Computes q, p, r, so that
        q * 10 ** -p + r = num / den
    and p <= max(minPrec, LIMIT_PREC) and r -> 0.
    Sets `dec` to q * 10 ** -p. Returns True if r != 0, False otherwise.
    """
    cdef int maxPrec, prec
    maxPrec = max(minPrec, LIMIT_PREC)
    while True:
        prec = (minPrec + maxPrec) // 2
        quot, rem = divmod(num * base10pow(prec), den)
        if prec == maxPrec:
            break
        if rem == 0:
            maxPrec = prec
        elif minPrec >= maxPrec - 2:
            minPrec = maxPrec
        else:
            minPrec = prec
    dec._value = quot
    dec._precision = prec
    return (rem != 0)


_dfltFormatParams = {'fill': ' ',
                     'align': '<',
                     'sign': '-',
                     #'zeropad': '',
                     'minimumwidth': 0,
                     'thousands_sep': '',
                     'grouping': [3, 0],
                     'decimal_point': '.',
                     'precision': None,
                     'type': 'f'}


def _getFormatParams(formatSpec):
    m = _parseFormatSpec(formatSpec)
    if m is None:
        raise ValueError("Invalid format specifier: " + formatSpec)
    fill = m.group('fill')
    zeropad = m.group('zeropad')
    if fill:                            # fill overrules zeropad
        fmtFill = fill
        fmtAlign = m.group('align')
    elif zeropad:                       # zeropad overrules align
        fmtFill = '0'
        fmtAlign = "="
    else:
        fmtFill = _dfltFormatParams['fill']
        fmtAlign = m.group('align') or _dfltFormatParams['align']
    fmtSign = m.group('sign') or _dfltFormatParams['sign']
    minimumwidth = m.group('minimumwidth')
    if minimumwidth:
        fmtMinWidth = int(minimumwidth)
    else:
        fmtMinWidth = _dfltFormatParams['minimumwidth']
    fmtType = m.group('type') or _dfltFormatParams['type']
    if fmtType == 'n':
        lconv = locale.localeconv()
        fmtThousandsSep = m.group('thousands_sep') and lconv['thousands_sep']
        fmtGrouping = lconv['grouping']
        fmtDecimalPoint = lconv['decimal_point']
    else:
        fmtThousandsSep = (m.group('thousands_sep') or
                           _dfltFormatParams['thousands_sep'])
        fmtGrouping = _dfltFormatParams['grouping']
        fmtDecimalPoint = _dfltFormatParams['decimal_point']
    precision = m.group('precision')
    if precision:
        fmtPrecision = int(precision)
    else:
        fmtPrecision = None
    return (fmtFill, fmtAlign, fmtSign, fmtMinWidth, fmtThousandsSep,
            fmtGrouping, fmtDecimalPoint, fmtPrecision, fmtType)

def _padDigits(digits, minWidth, fill, sep=None, grouping=None):
    nDigits = len(digits)
    if sep and grouping:
        slices = []
        i = j = 0
        limit = max(minWidth, nDigits) if fill == '0' else nDigits
        for l in _iterGrouping(grouping):
            j = min(i + l, limit)
            slices.append((i, j))
            if j >= limit:
                break
            i = j
            limit = max(limit - 1, nDigits, i + 1)
        if j < limit:
            slices.append((j, limit))
        digits = (limit - nDigits) * fill + digits
        raw = sep.join([digits[limit - j: limit - i]
                       for i, j in reversed(slices)])
        return (minWidth - len(raw)) * fill + raw
    else:
        return (minWidth - nDigits) * fill + digits

def _iterGrouping(grouping):
    l = None
    for i in grouping[:len(grouping) - 1]:
        yield i
        l = i
    i = grouping[len(grouping) - 1]
    if i == 0:
        while l:
            yield l
    elif i != locale.CHAR_MAX:
        yield i


# helper functions for decimal arithmetic


cdef void _adjust(Decimal dec, int prec, object rounding=None) except *:
    """Adjust Decimal `dec` to precision `prec` using given rounding mode
    (or default mode if none is given)."""
    cdef int dp
    dp = prec - dec._precision
    if dp == 0:
        return
    elif dp > 0:
        dec._value *= base10pow(dp)
    elif prec >= 0:
        dec._value =  _div_rounded(dec._value, base10pow(-dp), rounding)
    else:
        dec._value = (_div_rounded(dec._value, base10pow(-dp), rounding)
                      * base10pow(-prec))
    dec._precision = max(prec, 0)


cdef object _get_adjusted_value(Decimal dec, int prec, object rounding=None):
    """Return rv so that rv // 10 ** rp = v // 10 ** p,
    rounded to precision rp using given rounding mode (or default mode if none
    is given)."""
    cdef int dp
    dp = prec - dec._precision
    if dp == 0:
        return dec._value
    elif dp > 0:
        return dec._value * base10pow(dp)
    elif prec >= 0:
        return _div_rounded(dec._value, base10pow(-dp), rounding)
    else:
        return (_div_rounded(dec._value, base10pow(-dp), rounding)
                * base10pow(-prec))


cdef tuple _reduce(Decimal dec):
    """Return rv, rp so that rv // 10 ** rp = dec and rv % 10 != 0
    """
    v, p = dec._value, dec._precision
    if v == 0:
        return 0, 0
    while p > 0 and v % 10 == 0:
        p -= 1
        v = v // 10
    return v, p


# divide x by y, return rounded result
cdef object _div_rounded(object x, object y, object rounding=None):
    """Return x // y, rounded using given rounding mode (or default mode
    if none is given)."""
    quot, rem = divmod(x, y)
    if rem == 0:              # no need for rounding
        return quot
    return quot + _round(quot, rem, y, rounding)


cdef object _int(object v, int p):
    """Return integral part of shifted decimal"""
    if p == 0:
        return v
    if v == 0:
        return v
    if p > 0:
        if v > 0:
            return PyNumber_Long(v // base10pow(p))
        else:
            return -PyNumber_Long(-v // base10pow(p))
    else:
        return PyNumber_Long(v * base10pow(-p))


cdef object _div(object num, object den, int minPrec):
    """Return num / den as Decimal, if possible with precision <=
    max(minPrec, LIMIT_PREC), otherwise as Fraction"""
    cdef Decimal dec
    dec = Decimal()
    rem = _approx_rational(dec, num, den, minPrec)
    if rem == 0:
        return dec
    else:
        return Fraction(num, den)


cdef object add(Decimal x, object y):
    """x + y"""
    cdef int p
    cdef Decimal result
    if isinstance(y, Decimal):
        p = x._precision - (<Decimal>y)._precision
        if p == 0:
            result = Decimal(x)
            result._value += (<Decimal>y)._value
        elif p > 0:
            result = Decimal(x)
            result._value += (<Decimal>y)._value * base10pow(p)
        else:
            result = Decimal(y)
            result._value += x._value * base10pow(-p)
        return result
    elif isinstance(y, Integral):
        p = x._precision
        result = Decimal(x)
        result._value += y * base10pow(p)
        return result
    elif isinstance(y, Rational):
        y_numerator, y_denominator = (y.numerator, y.denominator)
    elif isinstance(y, Real):
        try:
            y_numerator, y_denominator = y.as_integer_ratio()
        except (ValueError, OverflowError, AttributeError):
            raise ValueError("Unsupported operand: %s" % repr(y))
    elif isinstance(y, _StdLibDecimal):
        return add(x, Decimal(y))
    else:
        return NotImplemented
    # handle Rational and Real
    x_denominator = base10pow(x._precision)
    num = x._value * y_denominator + x_denominator * y_numerator
    den = y_denominator * x_denominator
    minPrec = x._precision
    # return num / den as Decimal or as Fraction
    return _div(num, den, minPrec)


cdef object sub(Decimal x, object y):
    """x - y"""
    cdef int p
    cdef Decimal result
    if isinstance(y, Decimal):
        p = x._precision - (<Decimal>y)._precision
        if p == 0:
            result = Decimal(x)
            result._value -= (<Decimal>y)._value
        elif p > 0:
            result = Decimal(x)
            result._value -= (<Decimal>y)._value * base10pow(p)
        else:
            result = Decimal(y)
            result._value = x._value * base10pow(-p) - (<Decimal>y)._value
        return result
    elif isinstance(y, Integral):
        p = x._precision
        result = Decimal(x)
        result._value -= y * base10pow(p)
        return result
    elif isinstance(y, Rational):
        y_numerator, y_denominator = (y.numerator, y.denominator)
    elif isinstance(y, Real):
        try:
            y_numerator, y_denominator = y.as_integer_ratio()
        except (ValueError, OverflowError, AttributeError):
            raise ValueError("Unsupported operand: %s" % repr(y))
    elif isinstance(y, _StdLibDecimal):
        return sub(x, Decimal(y))
    else:
        return NotImplemented
    # handle Rational and Real
    x_denominator = base10pow(x._precision)
    num = x._value * y_denominator - x_denominator * y_numerator
    den = y_denominator * x_denominator
    minPrec = x._precision
    # return num / den as Decimal or as Fraction
    return _div(num, den, minPrec)


cdef object mul(Decimal x, object y):
    """x * y"""
    if isinstance(y, Decimal):
        result = Decimal(x)
        (<Decimal>result)._value *= (<Decimal>y)._value
        (<Decimal>result)._precision += (<Decimal>y)._precision
        return result
    elif isinstance(y, Integral):
        result = Decimal(x)
        (<Decimal>result)._value *= y
        return result
    elif isinstance(y, Rational):
        y_numerator, y_denominator = (y.numerator, y.denominator)
    elif isinstance(y, Real):
        try:
            y_numerator, y_denominator = y.as_integer_ratio()
        except (ValueError, OverflowError, AttributeError):
            raise ValueError("Unsupported operand: %s" % repr(y))
    elif isinstance(y, _StdLibDecimal):
        return x.__mul__(Decimal(y))
    else:
        return NotImplemented
    # handle Rational and Real
    num = x._value * y_numerator
    den = y_denominator * base10pow(x._precision)
    minPrec = x._precision
    # return num / den as Decimal or as Fraction
    return _div(num, den, minPrec)


cdef object div1(Decimal x, object y):
    """x / y"""
    cdef int xp, yp
    if isinstance(y, Decimal):
        xp, yp = x._precision, (<Decimal>y)._precision
        num = x._value * base10pow(yp)
        den = (<Decimal>y)._value * base10pow(xp)
        minPrec = max(0, xp - yp)
        # return num / den as Decimal or as Fraction
        return _div(num, den, minPrec)
    elif isinstance(y, Rational):       # includes Integral
        y_numerator, y_denominator = (y.numerator, y.denominator)
    elif isinstance(y, Real):
        try:
            y_numerator, y_denominator = y.as_integer_ratio()
        except (ValueError, OverflowError, AttributeError):
            raise ValueError("Unsupported operand: %s" % repr(y))
    elif isinstance(y, _StdLibDecimal):
        return div1(x, Decimal(y))
    else:
        return NotImplemented
    # handle Rational and Real
    num = x._value * y_denominator
    den = y_numerator * base10pow(x._precision)
    minPrec = x._precision
    # return num / den as Decimal or as Fraction
    return _div(num, den, minPrec)


cdef object div2(object x, Decimal y):
    """x / y"""
    cdef int xp, yp
    if isinstance(x, Decimal):
        xp, yp = (<Decimal>x)._precision, y._precision
        num = (<Decimal>x)._value * base10pow(yp)
        den = y._value * base10pow(xp)
        minPrec = max(0, xp - yp)
        # return num / den as Decimal or as Fraction
        return _div(num, den, minPrec)
    if isinstance(x, Rational):
        x_numerator, x_denominator = (x.numerator, x.denominator)
    elif isinstance(x, Real):
        try:
            x_numerator, x_denominator = x.as_integer_ratio()
        except (ValueError, OverflowError, AttributeError):
            raise ValueError("Unsupported operand: %s" % repr(x))
    elif isinstance(x, _StdLibDecimal):
        return div1(Decimal(x), y)
    else:
        return NotImplemented
    # handle Rational and float
    num = x_numerator * base10pow(y._precision)
    den = y._value * x_denominator
    minPrec = y._precision
    # return num / den as Decimal or as Fraction
    return _div(num, den, minPrec)


cdef tuple divmod1(Decimal x, object y):
    """x // y, x % y"""
    cdef int xp, yp
    cdef Decimal r
    if isinstance(y, Decimal):
        xp, yp = x._precision, (<Decimal>y)._precision
        if xp >= yp:
            r = Decimal(x)
            xv = x._value
            yv = (<Decimal>y)._value * base10pow(xp - yp)
        else:
            r = Decimal(y)
            xv = x._value * base10pow(yp - xp)
            yv = (<Decimal>y)._value
        q = xv // yv
        r._value = xv - q * yv
        return Decimal(q, r._precision), r
    elif isinstance(y, Integral):
        r = Decimal(x)
        xv = x._value
        xp = x._precision
        yv = y * base10pow(xp)
        q = xv // yv
        r._value = xv - q * yv
        return Decimal(q, xp), r
    elif isinstance(y, _StdLibDecimal):
        return x.__divmod__(Decimal(y))
    else:
        return x // y, x % y


cdef tuple divmod2(object x, Decimal y):
    """x // y, x % y"""
    cdef int xp, yp
    cdef Decimal r
    if isinstance(x, Decimal):
        xp, yp = (<Decimal>x)._precision, y._precision
        if xp >= yp:
            r = Decimal(x)
            xv = (<Decimal>x)._value
            yv = y._value * base10pow(xp - yp)
        else:
            r = Decimal(y)
            xv = (<Decimal>x)._value * base10pow(yp - xp)
            yv = y._value
        q = xv // yv
        r._value = xv - q * yv
        return Decimal(q, r._precision), r
    elif isinstance(x, Integral):
        r = Decimal(y)
        yv = y._value
        yp = y._precision
        xv = x * base10pow(yp)
        q = xv // yv
        r._value = xv - q * yv
        return Decimal(q, yp), r
    elif isinstance(x, _StdLibDecimal):
        return Decimal(x).__divmod__(y)
    else:
        return x // y, x % y


cdef object floordiv1(Decimal x, object y):
    """x // y"""
    if isinstance(y, (Decimal, Integral, _StdLibDecimal)):
        return divmod1(x, y)[0]
    else:
        return Decimal(floor(x / y), x._precision)


cdef object floordiv2(object x, Decimal y):
    """x // y"""
    if isinstance(x, (Decimal, Integral, _StdLibDecimal)):
        return divmod2(x, y)[0]
    else:
        return Decimal(floor(x / y), y._precision)


cdef object mod1(Decimal x, object y):
    """x % y"""
    if isinstance(y, (Decimal, Integral, _StdLibDecimal)):
        return divmod1(x, y)[1]
    else:
        return x - y * (x // y)


cdef object mod2(object x, Decimal y):
    """x % y"""
    if isinstance(x, (Decimal, Integral, _StdLibDecimal)):
        return divmod2(x, y)[1]
    else:
        return x - y * (x // y)


cdef object pow1(Decimal x, object y):
    """x ** y"""
    cdef int exp
    cdef Decimal result
    if isinstance(y, Integral):
        exp = int(y)
        if exp >= 0:
            result = Decimal()
            result._value = x._value ** exp
            result._precision = x._precision * exp
            return result
        else:
            return 1 / pow1(x, -y)
    elif isinstance(y, Rational):
        if y.denominator == 1:
            return x ** y.numerator
        else:
            return float(x) ** float(y)
    else:
        return float(x) ** y


cdef object pow2(object x, Decimal y):
    """x ** y"""
    if y.denominator == 1:
        return x ** y.numerator
    return x ** float(y)


# helper for different rounding modes

cdef int _round(q, r, y, rounding=None):
    if rounding is None:
        rounding = get_rounding()
    if rounding == ROUNDING.ROUND_HALF_UP:
        # Round 5 up (away from 0)
        # |remainder| > |divisor|/2 or
        # |remainder| = |divisor|/2 and quotient >= 0
        # => add 1
        ar, ay = abs(2 * r), abs(y)
        if ar > ay or (ar == ay and q >= 0):
            return 1
        else:
            return 0
    if rounding == ROUNDING.ROUND_HALF_EVEN:
        # Round 5 to even, rest to nearest
        # |remainder| > |divisor|/2 or
        # |remainder| = |divisor|/2 and quotient not even
        # => add 1
        ar, ay = abs(2 * r), abs(y)
        if ar > ay or (ar == ay and q % 2 != 0):
            return 1
        else:
            return 0
    if rounding == ROUNDING.ROUND_HALF_DOWN:
        # Round 5 down
        # |remainder| > |divisor|/2 or
        # |remainder| = |divisor|/2 and quotient < 0
        # => add 1
        ar, ay = abs(2 * r), abs(y)
        if ar > ay or (ar == ay and q < 0):
            return 1
        else:
            return 0
    if rounding == ROUNDING.ROUND_DOWN:
        # Round towards 0 (aka truncate)
        # quotient negativ => add 1
        if q < 0:
            return 1
        else:
            return 0
    if rounding == ROUNDING.ROUND_UP:
        # Round away from 0
        # quotient not negativ => add 1
        if q >= 0:
            return 1
        else:
            return 0
    if rounding == ROUNDING.ROUND_CEILING:
        # Round up (not away from 0 if negative)
        # => always add 1
        return 1
    if rounding == ROUNDING.ROUND_FLOOR:
        # Round down (not towards 0 if negative)
        # => never add 1
        return 0
    if rounding == ROUNDING.ROUND_05UP:
        # Round down unless last digit is 0 or 5
        # quotient not negativ and quotient divisible by 5 without remainder
        # or quotient negativ and (quotient + 1) not divisible by 5 without
        # remainder => add 1
        if q >= 0 and q % 5 == 0 or q < 0 and (q + 1) % 5 != 0:
            return 1
        else:
            return 0


"""Rounding parameters for decimal fixed-point arithmetic."""


# standard library imports
from decimal import getcontext as _getcontext
from enum import Enum

# third-party imports


# local imports


# precision limit for division or conversion without explicitly given
# precision
LIMIT_PREC = 32


# rounding modes equivalent to those defined in standard lib module 'decimal'
class ROUNDING(Enum):

    """Enumeration of rounding modes."""

    # Implementation of __index__ depends on values in ROUNDING being ints
    # starting with 1 !!!
    __next_value__ = 1

    def __new__(cls, doc):
        """Return new member of the Enum."""
        member = object.__new__(cls)
        member._value_ = cls.__next_value__
        cls.__next_value__ += 1
        member.__doc__ = doc
        return member

    def __index__(self):
        """Return `self` converted to an `int`."""
        return self.value - 1

    __int__ = __index__

    #: Round away from zero if last digit after rounding towards
    #: zero would have been 0 or 5; otherwise round towards zero.
    ROUND_05UP = 'Round away from zero if last digit after rounding towards '\
        'zero would have been 0 or 5; otherwise round towards zero.'
    #: Round towards Infinity.
    ROUND_CEILING = 'Round towards Infinity.'
    #: Round towards zero.
    ROUND_DOWN = 'Round towards zero.'
    #: Round towards -Infinity.
    ROUND_FLOOR = 'Round towards -Infinity.'
    #: Round to nearest with ties going towards zero.
    ROUND_HALF_DOWN = 'Round to nearest with ties going towards zero.'
    #: Round to nearest with ties going to nearest even integer.
    ROUND_HALF_EVEN = \
        'Round to nearest with ties going to nearest even integer.'
    #: Round to nearest with ties going away from zero.
    ROUND_HALF_UP = 'Round to nearest with ties going away from zero.'
    #: Round away from zero.
    ROUND_UP = 'Round away from zero.'


# Python 2 / Python 3
import sys  # noqa: I100, I202
if sys.version_info[0] < 3:
    # rounding mode of builtin round function
    ROUNDING.default = ROUNDING.ROUND_HALF_UP
else:
    # In 3.0 round changed from half-up to half-even !
    ROUNDING.default = ROUNDING.ROUND_HALF_EVEN
del sys


# functions to get / set rounding mode
def get_rounding():
    """Return rounding mode from current context."""
    ctx = _getcontext()
    return ROUNDING[ctx.rounding]


def set_rounding(rounding):
    """Set rounding mode in current context.

    Args:
        rounding (ROUNDING): rounding mode to be set

    """
    ctx = _getcontext()
    ctx.rounding = rounding.name


__all__ = [
    'LIMIT_PREC',
    'ROUNDING',
    'get_rounding',
    'set_rounding',
]
