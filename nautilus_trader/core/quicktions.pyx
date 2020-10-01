# flake8: noqa
# cython: language_level=3str
## cython: profile=True

# Copyright (c) 2001, 2002, 2003, 2004, 2005, 2006, 2007, 2008, 2009, 2010,
# 2011, 2012, 2013, 2014 Python Software Foundation; All Rights Reserved
#
# Based on the "fractions" module in CPython 3.4+.
# https://hg.python.org/cpython/file/b18288f24501/Lib/fractions.py
#
# Adapted for efficient Cython compilation by Stefan Behnel.
#

# Full credits to the author scoder gh@behnel.de
# This source code originally found at https://github.com/scoder/quicktions/blob/master/src/quicktions.pyx
# The following modifications have been made;
# - flake8: noqa at top of file
# - Move cdef _numerator to quicktions.pxd
# - Move cdef _denominator to quicktions.pxd
# - Move cdef cdef Py_hash_t _hash to quicktions.pxd

"""
Fast fractions data type for rational numbers.
This is an almost-drop-in replacement for the standard library's
"fractions.Fraction".
"""

from __future__ import division, absolute_import, print_function


__all__ = ['Fraction']

__version__ = '1.11'

cimport cython
from cpython.unicode cimport Py_UNICODE_TODECIMAL
from cpython.object cimport Py_LT, Py_LE, Py_EQ, Py_NE, Py_GT, Py_GE
from cpython.version cimport PY_MAJOR_VERSION

cdef extern from *:
    cdef long LONG_MAX, INT_MAX
    cdef long long PY_LLONG_MIN, PY_LLONG_MAX
    cdef long long MAX_SMALL_NUMBER "(PY_LLONG_MAX / 100)"

cdef object Rational, Integral, Real, Complex, Decimal, math, operator, sys
cdef object PY_MAX_LONG_LONG = PY_LLONG_MAX

from numbers import Rational, Integral, Real, Complex
from decimal import Decimal
import math
import operator
import sys

cdef bint _decimal_supports_integer_ratio = hasattr(Decimal, "as_integer_ratio")  # Py3.6+


# Cache widely used 10**x int objects.
DEF CACHED_POW10 = 58  # sys.getsizeof(tuple[58]) == 512 bytes  in Py3.7

cdef tuple _cache_pow10():
    cdef int i
    l = []
    x = 1
    for i in range(CACHED_POW10):
        l.append(x)
        x *= 10
    return tuple(l)

cdef tuple POW_10 = _cache_pow10()


cdef pow10(Py_ssize_t i):
    if 0 <= i < CACHED_POW10:
        return POW_10[i]
    else:
        return 10 ** (<object> i)


# Half-private GCD implementation.

cdef extern from *:
    """
    #if PY_VERSION_HEX < 0x030500F0 || !CYTHON_COMPILING_IN_CPYTHON
        #define _PyLong_GCD(a, b) (NULL)
    #endif
    """
    # CPython 3.5+ has a fast PyLong GCD implementation that we can use.
    int PY_VERSION_HEX
    int IS_CPYTHON "CYTHON_COMPILING_IN_CPYTHON"
    _PyLong_GCD(a, b)


cpdef _gcd(a, b):
    """Calculate the Greatest Common Divisor of a and b as a non-negative number.
    """
    if PY_VERSION_HEX < 0x030500F0 or not IS_CPYTHON:
        return _gcd_fallback(a, b)
    if not isinstance(a, int):
        a = int(a)
    if not isinstance(b, int):
        b = int(b)
    return _PyLong_GCD(a, b)


ctypedef unsigned long long ullong
ctypedef unsigned long ulong
ctypedef unsigned int uint

ctypedef fused cunumber:
    ullong
    ulong
    uint


cdef ullong _abs(long long x):
    if x == PY_LLONG_MIN:
        return (<ullong>PY_LLONG_MAX) + 1
    return abs(x)


cdef cunumber _igcd(cunumber a, cunumber b):
    """Euclid's GCD algorithm"""
    while b:
        a, b = b, a%b
    return a


cdef cunumber _ibgcd(cunumber a, cunumber b):
    """Binary GCD algorithm.
    See https://en.wikipedia.org/wiki/Binary_GCD_algorithm
    """
    cdef uint shift = 0
    if not a:
        return b
    if not b:
        return a

    # Find common pow2 factors.
    while not (a|b) & 1:
        a >>= 1
        b >>= 1
        shift += 1

    # Exclude factor 2.
    while not a & 1:
        a >>= 1

    # a is always odd from here on.
    while b:
        while not b & 1:
            b >>= 1
        if a > b:
            a, b = b, a
        b -= a

    # Restore original pow2 factor.
    return a << shift


cdef _py_gcd(ullong a, ullong b):
    if a <= <ullong>INT_MAX and b <= <ullong>INT_MAX:
        return <int> _igcd[uint](a, b)
    elif a <= <ullong>LONG_MAX and b <= <ullong>LONG_MAX:
        return <long> _igcd[ulong](a, b)
    elif b:
        a = _igcd[ullong](a, b)
    # try PyInt downcast in Py2
    if PY_MAJOR_VERSION < 3 and a <= <ullong>LONG_MAX:
        return <long>a
    return a


cdef _gcd_fallback(a, b):
    """Fallback GCD implementation if _PyLong_GCD() is not available.
    """
    # Try doing the computation in C space.  If the numbers are too
    # large at the beginning, do object calculations until they are small enough.
    cdef ullong au, bu
    cdef long long ai, bi

    # Optimistically try to switch to C space.
    try:
        ai, bi = a, b
    except OverflowError:
        pass
    else:
        au = _abs(ai)
        bu = _abs(bi)
        return _py_gcd(au, bu)

    # Do object calculation until we reach the C space limit.
    a = abs(a)
    b = abs(b)
    while b > PY_MAX_LONG_LONG:
        a, b = b, a%b
    while b and a > PY_MAX_LONG_LONG:
        a, b = b, a%b
    if not b:
        return a
    return _py_gcd(a, b)


# Constants related to the hash implementation;  hash(x) is based
# on the reduction of x modulo the prime _PyHASH_MODULUS.

cdef Py_hash_t _PyHASH_MODULUS
try:
    _PyHASH_MODULUS = sys.hash_info.modulus
except AttributeError:  # pre Py3.2
    # adapted from pyhash.h in Py3.4
    _PyHASH_MODULUS = (<Py_hash_t>1) << (61 if sizeof(Py_hash_t) >= 8 else 31) - 1


# Value to be used for rationals that reduce to infinity modulo
# _PyHASH_MODULUS.
cdef Py_hash_t _PyHASH_INF
try:
    _PyHASH_INF = sys.hash_info.inf
except AttributeError:  # pre Py3.2
    _PyHASH_INF = hash(float('+inf'))


cdef class Fraction:
    """A Rational number.
    Takes a string like '3/2' or '1.5', another Rational instance, a
    numerator/denominator pair, or a float.
    Examples
    --------
    >>> Fraction(10, -8)
    Fraction(-5, 4)
    >>> Fraction(Fraction(1, 7), 5)
    Fraction(1, 35)
    >>> Fraction(Fraction(1, 7), Fraction(2, 3))
    Fraction(3, 14)
    >>> Fraction('314')
    Fraction(314, 1)
    >>> Fraction('-35/4')
    Fraction(-35, 4)
    >>> Fraction('3.1415') # conversion from numeric string
    Fraction(6283, 2000)
    >>> Fraction('-47e-2') # string may include a decimal exponent
    Fraction(-47, 100)
    >>> Fraction(1.47)  # direct construction from float (exact conversion)
    Fraction(6620291452234629, 4503599627370496)
    >>> Fraction(2.25)
    Fraction(9, 4)
    >>> from decimal import Decimal
    >>> Fraction(Decimal('1.47'))
    Fraction(147, 100)
    """

    def __cinit__(self, numerator=0, denominator=None, *, bint _normalize=True):
        cdef Fraction value
        self._hash = -1
        if denominator is None:
            if type(numerator) is int or type(numerator) is long:
                self._numerator = numerator
                self._denominator = 1
                return

            elif type(numerator) is float:
                # Exact conversion
                self._numerator, self._denominator = numerator.as_integer_ratio()
                return

            elif type(numerator) is Fraction:
                self._numerator = (<Fraction>numerator)._numerator
                self._denominator = (<Fraction>numerator)._denominator
                return

            elif isinstance(numerator, unicode):
                numerator, denominator, is_normalised = _parse_fraction(<unicode>numerator)
                if is_normalised:
                    _normalize = False
                # fall through to normalisation below

            elif PY_MAJOR_VERSION < 3 and isinstance(numerator, bytes):
                numerator, denominator, is_normalised = _parse_fraction(<bytes>numerator)
                if is_normalised:
                    _normalize = False
                # fall through to normalisation below

            elif isinstance(numerator, float):
                # Exact conversion
                self._numerator, self._denominator = numerator.as_integer_ratio()
                return

            elif isinstance(numerator, (Fraction, Rational)):
                self._numerator = numerator.numerator
                self._denominator = numerator.denominator
                return

            elif isinstance(numerator, Decimal):
                if _decimal_supports_integer_ratio:
                    # Exact conversion
                    self._numerator, self._denominator = numerator.as_integer_ratio()
                else:
                    value = Fraction.from_decimal(numerator)
                    self._numerator = (<Fraction>value)._numerator
                    self._denominator = (<Fraction>value)._denominator
                return

            else:
                raise TypeError("argument should be a string "
                                "or a Rational instance")

        elif type(numerator) is int is type(denominator):
            pass  # *very* normal case

        elif PY_MAJOR_VERSION < 3 and type(numerator) is long is type(denominator):
            pass  # *very* normal case

        elif type(numerator) is Fraction is type(denominator):
            numerator, denominator = (
                (<Fraction>numerator)._numerator * (<Fraction>denominator)._denominator,
                (<Fraction>denominator)._numerator * (<Fraction>numerator)._denominator
            )

        elif (isinstance(numerator, (Fraction, Rational)) and
              isinstance(denominator, (Fraction, Rational))):
            numerator, denominator = (
                numerator.numerator * denominator.denominator,
                denominator.numerator * numerator.denominator
            )

        else:
            raise TypeError("both arguments should be "
                            "Rational instances")

        if denominator == 0:
            raise ZeroDivisionError(f'Fraction({numerator}, 0)')
        if _normalize:
            g = _gcd(numerator, denominator)
            # NOTE: 'is' tests on integers are generally a bad idea, but
            # they are fast and if they fail here, it'll still be correct
            if denominator < 0:
                if g is 1:
                    numerator = -numerator
                    denominator = -denominator
                else:
                    g = -g
            if g is not 1:
                numerator //= g
                denominator //= g
        self._numerator = numerator
        self._denominator = denominator

    @classmethod
    def from_float(cls, f):
        """Converts a finite float to a rational number, exactly.
        Beware that Fraction.from_float(0.3) != Fraction(3, 10).
        """
        try:
            ratio = f.as_integer_ratio()
        except (ValueError, OverflowError, AttributeError):
            pass  # not something we can convert, raise concrete exceptions below
        else:
            return cls(*ratio)

        if isinstance(f, Integral):
            return cls(f)
        elif not isinstance(f, float):
            raise TypeError(f"{cls.__name__}.from_float() only takes floats, not {f!r} ({type(f).__name__})")
        if math.isinf(f):
            raise OverflowError(f"Cannot convert {f!r} to {cls.__name__}.")
        raise ValueError(f"Cannot convert {f!r} to {cls.__name__}.")

    @classmethod
    def from_decimal(cls, dec):
        """Converts a finite Decimal instance to a rational number, exactly."""
        cdef Py_ssize_t exp
        if isinstance(dec, Integral):
            dec = Decimal(int(dec))
        elif not isinstance(dec, Decimal):
            raise TypeError(
                f"{cls.__name__}.from_decimal() only takes Decimals, not {dec!r} ({type(dec).__name__})")
        if dec.is_infinite():
            raise OverflowError(f"Cannot convert {dec} to {cls.__name__}.")
        if dec.is_nan():
            raise ValueError(f"Cannot convert {dec} to {cls.__name__}.")
        sign, digits, exp = dec.as_tuple()
        digits = int(''.join(map(str, digits)))
        if sign:
            digits = -digits
        if exp >= 0:
            return cls(digits * pow10(exp))
        else:
            return cls(digits, pow10(-exp))

    cdef as_integer_ratio(self):
        """Return the integer ratio as a tuple.
        Return a tuple of two integers, whose ratio is equal to the
        Fraction and with a positive denominator.
        """
        return (self._numerator, self._denominator)

    cdef limit_denominator(self, max_denominator=1000000):
        """Closest Fraction to self with denominator at most max_denominator.
        >>> Fraction('3.141592653589793').limit_denominator(10)
        Fraction(22, 7)
        >>> Fraction('3.141592653589793').limit_denominator(100)
        Fraction(311, 99)
        >>> Fraction(4321, 8765).limit_denominator(10000)
        Fraction(4321, 8765)
        """
        # Algorithm notes: For any real number x, define a *best upper
        # approximation* to x to be a rational number p/q such that:
        #
        #   (1) p/q >= x, and
        #   (2) if p/q > r/s >= x then s > q, for any rational r/s.
        #
        # Define *best lower approximation* similarly.  Then it can be
        # proved that a rational number is a best upper or lower
        # approximation to x if, and only if, it is a convergent or
        # semiconvergent of the (unique shortest) continued fraction
        # associated to x.
        #
        # To find a best rational approximation with denominator <= M,
        # we find the best upper and lower approximations with
        # denominator <= M and take whichever of these is closer to x.
        # In the event of a tie, the bound with smaller denominator is
        # chosen.  If both denominators are equal (which can happen
        # only when max_denominator == 1 and self is midway between
        # two integers) the lower bound---i.e., the floor of self, is
        # taken.

        if max_denominator < 1:
            raise ValueError("max_denominator should be at least 1")
        if self._denominator <= max_denominator:
            return Fraction(self)

        p0, q0, p1, q1 = 0, 1, 1, 0
        n, d = self._numerator, self._denominator
        while True:
            a = n//d
            q2 = q0+a*q1
            if q2 > max_denominator:
                break
            p0, q0, p1, q1 = p1, q1, p0+a*p1, q2
            n, d = d, n-a*d

        k = (max_denominator-q0)//q1
        bound1 = Fraction(p0+k*p1, q0+k*q1)
        bound2 = Fraction(p1, q1)
        if abs(bound2 - self) <= abs(bound1-self):
            return bound2
        else:
            return bound1

    @property
    def numerator(self):
        return self._numerator

    @property
    def denominator(self):
        return self._denominator

    def __repr__(self):
        """repr(self)"""
        return '%s(%s, %s)' % (self.__class__.__name__,
                               self._numerator, self._denominator)

    def __str__(self):
        """str(self)"""
        if self._denominator == 1:
            return str(self._numerator)
        else:
            return '%s/%s' % (self._numerator, self._denominator)

    def __add__(a, b):
        """a + b"""
        return _math_op(a, b, _add, _math_op_add)

    def __sub__(a, b):
        """a - b"""
        return _math_op(a, b, _sub, _math_op_sub)

    def __mul__(a, b):
        """a * b"""
        return _math_op(a, b, _mul, _math_op_mul)

    def __div__(a, b):
        """a / b"""
        return _math_op(a, b, _div, _math_op_div)

    def __truediv__(a, b):
        """a / b"""
        return _math_op(a, b, _div, _math_op_truediv)

    def __floordiv__(a, b):
        """a // b"""
        return _math_op(a, b, _floordiv, _math_op_floordiv)

    def __mod__(a, b):
        """a % b"""
        return _math_op(a, b, _mod, _math_op_mod)

    def __divmod__(a, b):
        """divmod(self, other): The pair (self // other, self % other).
        Sometimes this can be computed faster than the pair of
        operations.
        """
        return _math_op(a, b, _divmod, _math_op_divmod)

    def __pow__(a, b, x):
        """a ** b
        If b is not an integer, the result will be a float or complex
        since roots are generally irrational. If b is an integer, the
        result will be rational.
        """
        if x is not None:
            return NotImplemented
        if isinstance(a, Fraction):
            # normal call
            if isinstance(b, (int, long, Fraction, Rational)):
                return _pow(a.numerator, a.denominator, b.numerator, b.denominator)
            else:
                return (a.numerator / a.denominator) ** b
        else:
            # reversed call
            bn, bd = b.numerator, b.denominator
            if bd == 1 and bn >= 0:
                # If a is an int, keep it that way if possible.
                return a ** bn

            if isinstance(a, (int, long, Rational)):
                return _pow(a.numerator, a.denominator, bn, bd)

            if bd == 1:
                return a ** bn

            return a ** (bn / bd)

    def __pos__(a):
        """+a: Coerces a subclass instance to Fraction"""
        if type(a) is Fraction:
            return a
        return Fraction(a._numerator, a._denominator, _normalize=False)

    def __neg__(a):
        """-a"""
        return Fraction(-a._numerator, a._denominator, _normalize=False)

    def __abs__(a):
        """abs(a)"""
        return Fraction(abs(a._numerator), a._denominator, _normalize=False)

    def __trunc__(a):
        """trunc(a)"""
        if a._numerator < 0:
            return -(-a._numerator // a._denominator)
        else:
            return a._numerator // a._denominator

    def __floor__(a):
        """math.floor(a)"""
        return a.numerator // a.denominator

    def __ceil__(a):
        """math.ceil(a)"""
        # The negations cleverly convince floordiv to return the ceiling.
        return -(-a.numerator // a.denominator)

    def __round__(self, ndigits=None):
        """round(self, ndigits)
        Rounds half toward even.
        """
        if ndigits is None:
            floor, remainder = divmod(self.numerator, self.denominator)
            if remainder * 2 < self.denominator:
                return floor
            elif remainder * 2 > self.denominator:
                return floor + 1
            # Deal with the half case:
            elif floor % 2 == 0:
                return floor
            else:
                return floor + 1
        shift = pow10(abs(<Py_ssize_t>ndigits))
        # See _operator_fallbacks.forward to check that the results of
        # these operations will always be Fraction and therefore have
        # round().
        if ndigits > 0:
            return Fraction(round(self * shift), shift)
        else:
            return Fraction(round(self / shift) * shift)

    def __float__(self):
        """float(self) = self.numerator / self.denominator
        It's important that this conversion use the integer's "true"
        division rather than casting one side to float before dividing
        so that ratios of huge integers convert without overflowing.
        """
        return _as_float(self.numerator, self.denominator)

    # Concrete implementations of Complex abstract methods.
    def __complex__(self):
        """complex(self) == complex(float(self), 0)"""
        return complex(float(self))

    # == +self
    real = property(__pos__, doc="Real numbers are their real component.")

    # == 0
    @property
    def imag(self):
        "Real numbers have no imaginary component."
        return 0

    def conjugate(self):
        """Conjugate is a no-op for Reals."""
        return +self

    def __hash__(self):
        """hash(self)"""
        if self._hash != -1:
            return self._hash

        cdef Py_hash_t result

        # Py2 and Py3 use completely different hash functions, we provide both
        if PY_MAJOR_VERSION == 2:
            if self._denominator == 1:
                # Get integers right.
                result = hash(self._numerator)
            else:
                # Expensive check, but definitely correct.
                float_val = _as_float(self._numerator, self._denominator)
                if self == float_val:
                    result = hash(float_val)
                else:
                    # Use tuple's hash to avoid a high collision rate on
                    # simple fractions.
                    result = hash((self._numerator, self._denominator))
            self._hash = result
            return result

        # In order to make sure that the hash of a Fraction agrees
        # with the hash of a numerically equal integer, float or
        # Decimal instance, we follow the rules for numeric hashes
        # outlined in the documentation.  (See library docs, 'Built-in
        # Types').

        if PY_VERSION_HEX < 0x030800B1:
            # dinv is the inverse of self._denominator modulo the prime
            # _PyHASH_MODULUS, or 0 if self._denominator is divisible by
            # _PyHASH_MODULUS.
            dinv = pow(self._denominator, _PyHASH_MODULUS - 2, _PyHASH_MODULUS)
            if not dinv:
                result = _PyHASH_INF
            else:
                result = abs(self._numerator) * dinv % _PyHASH_MODULUS
        else:
            # Py3.8+
            try:
                dinv = pow(self._denominator, -1, _PyHASH_MODULUS)
            except ValueError:
                # ValueError means there is no modular inverse.
                result = _PyHASH_INF
            else:
                # The general algorithm now specifies that the absolute value of
                # the hash is
                #    (|N| * dinv) % P
                # where N is self._numerator and P is _PyHASH_MODULUS.  That's
                # optimized here in two ways:  first, for a non-negative int i,
                # hash(i) == i % P, but the int hash implementation doesn't need
                # to divide, and is faster than doing % P explicitly.  So we do
                #    hash(|N| * dinv)
                # instead.  Second, N is unbounded, so its product with dinv may
                # be arbitrarily expensive to compute.  The final answer is the
                # same if we use the bounded |N| % P instead, which can again
                # be done with an int hash() call.  If 0 <= i < P, hash(i) == i,
                # so this nested hash() call wastes a bit of time making a
                # redundant copy when |N| < P, but can save an arbitrarily large
                # amount of computation for large |N|.
                result = hash(hash(abs(self._numerator)) * dinv)

        if self._numerator < 0:
            result = -result
            if result == -1:
                result = -2
        self._hash = result
        return result

    def __richcmp__(a, b, int op):
        if isinstance(a, Fraction):
            if op == Py_EQ:
                return (<Fraction>a)._eq(b)
            elif op == Py_NE:
                result = (<Fraction>a)._eq(b)
                return NotImplemented if result is NotImplemented else not result
        else:
            a, b = b, a
            if op == Py_EQ:
                return (<Fraction>a)._eq(b)
            elif op == Py_NE:
                result = (<Fraction>a)._eq(b)
                return NotImplemented if result is NotImplemented else not result
            elif op == Py_LT:
                op = Py_GE
            elif op == Py_GT:
                op = Py_LE
            elif op == Py_LE:
                op = Py_GT
            elif op == Py_GE:
                op = Py_LT
            else:
                return NotImplemented
        return (<Fraction>a)._richcmp(b, op)

    @cython.final
    cdef _eq(a, b):
        if type(b) is int or type(b) is long:
            return a._numerator == b and a._denominator == 1
        if type(b) is Fraction:
            return (a._numerator == (<Fraction>b)._numerator and
                    a._denominator == (<Fraction>b)._denominator)
        if isinstance(b, Rational):
            return (a._numerator == b.numerator and
                    a._denominator == b.denominator)
        if isinstance(b, Complex) and b.imag == 0:
            b = b.real
        if isinstance(b, float):
            if math.isnan(b) or math.isinf(b):
                # comparisons with an infinity or nan should behave in
                # the same way for any finite a, so treat a as zero.
                return 0.0 == b
            else:
                return a == a.from_float(b)
        return NotImplemented

    @cython.final
    cdef _richcmp(self, other, int op):
        """Helper for comparison operators, for internal use only.
        Implement comparison between a Rational instance `self`, and
        either another Rational instance or a float `other`.  If
        `other` is not a Rational instance or a float, return
        NotImplemented. `op` should be one of the six standard
        comparison operators.
        """
        # convert other to a Rational instance where reasonable.
        if isinstance(other, (int, long)):
            a = self._numerator
            b = self._denominator * other
        elif type(other) is Fraction:
            a = self._numerator * (<Fraction>other)._denominator
            b = self._denominator * (<Fraction>other)._numerator
        elif isinstance(other, float):
            if math.isnan(other) or math.isinf(other):
                a, b = 0.0, other  # Comparison to 0.0 is just as good as any.
            else:
                return self._richcmp(self.from_float(other), op)
        elif isinstance(other, (Fraction, Rational)):
            a = self._numerator * other.denominator
            b = self._denominator * other.numerator
        else:
            # comparisons with complex should raise a TypeError, for consistency
            # with int<->complex, float<->complex, and complex<->complex comparisons.
            if PY_MAJOR_VERSION < 3 and isinstance(other, complex):
                raise TypeError("no ordering relation is defined for complex numbers")
            return NotImplemented

        if op == Py_LT:
            return a < b
        elif op == Py_GT:
            return a > b
        elif op == Py_LE:
            return a <= b
        elif op == Py_GE:
            return a >= b
        else:
            return NotImplemented

    def __bool__(self):
        """a != 0"""
        return self._numerator != 0

    # support for pickling, copy, and deepcopy

    def __reduce__(self):
        return (type(self), (str(self),))

    def __copy__(self):
        if type(self) is Fraction:
            return self     # I'm immutable; therefore I am my own clone
        return type(self)(self._numerator, self._denominator)

    def __deepcopy__(self, memo):
        if type(self) is Fraction:
            return self     # My components are also immutable
        return type(self)(self._numerator, self._denominator)


# Register with Python's numerical tower.
Rational.register(Fraction)


cdef _pow(an, ad, bn, bd):
    if bd == 1:
        if bn >= 0:
            return Fraction(an ** bn,
                            ad ** bn,
                            _normalize=False)
        elif an >= 0:
            return Fraction(ad ** -bn,
                            an ** -bn,
                            _normalize=False)
        else:
            return Fraction((-ad) ** -bn,
                            (-an) ** -bn,
                            _normalize=False)
    else:
        # A fractional power will generally produce an
        # irrational number.
        return (an / ad) ** (bn / bd)


cdef _as_float(numerator, denominator):
    return numerator / denominator


"""
In general, we want to implement the arithmetic operations so
that mixed-mode operations either call an implementation whose
author knew about the types of both arguments, or convert both
to the nearest built in type and do the operation there. In
Fraction, that means that we define __add__ and __radd__ as:
    def __add__(self, other):
        # Both types have numerators/denominator attributes,
        # so do the operation directly
        if isinstance(other, (int, Fraction)):
            return Fraction(self.numerator * other.denominator +
                            other.numerator * self.denominator,
                            self.denominator * other.denominator)
        # float and complex don't have those operations, but we
        # know about those types, so special case them.
        elif isinstance(other, float):
            return float(self) + other
        elif isinstance(other, complex):
            return complex(self) + other
        # Let the other type take over.
        return NotImplemented
    def __radd__(self, other):
        # radd handles more types than add because there's
        # nothing left to fall back to.
        if isinstance(other, Rational):
            return Fraction(self.numerator * other.denominator +
                            other.numerator * self.denominator,
                            self.denominator * other.denominator)
        elif isinstance(other, Real):
            return float(other) + float(self)
        elif isinstance(other, Complex):
            return complex(other) + complex(self)
        return NotImplemented
There are 5 different cases for a mixed-type addition on
Fraction. I'll refer to all of the above code that doesn't
refer to Fraction, float, or complex as "boilerplate". 'r'
will be an instance of Fraction, which is a subtype of
Rational (r : Fraction <: Rational), and b : B <:
Complex. The first three involve 'r + b':
    1. If B <: Fraction, int, float, or complex, we handle
       that specially, and all is well.
    2. If Fraction falls back to the boilerplate code, and it
       were to return a value from __add__, we'd miss the
       possibility that B defines a more intelligent __radd__,
       so the boilerplate should return NotImplemented from
       __add__. In particular, we don't handle Rational
       here, even though we could get an exact answer, in case
       the other type wants to do something special.
    3. If B <: Fraction, Python tries B.__radd__ before
       Fraction.__add__. This is ok, because it was
       implemented with knowledge of Fraction, so it can
       handle those instances before delegating to Real or
       Complex.
The next two situations describe 'b + r'. We assume that b
didn't know about Fraction in its implementation, and that it
uses similar boilerplate code:
    4. If B <: Rational, then __radd_ converts both to the
       builtin rational type (hey look, that's us) and
       proceeds.
    5. Otherwise, __radd__ tries to find the nearest common
       base ABC, and fall back to its builtin type. Since this
       class doesn't subclass a concrete type, there's no
       implementation to fall back to, so we need to try as
       hard as possible to return an actual value, or the user
       will get a TypeError.
"""


cdef _add(an, ad, bn, bd):
    """a + b"""
    return Fraction(an * bd + bn * ad, ad * bd)

cdef _sub(an, ad, bn, bd):
    """a - b"""
    return Fraction(an * bd - bn * ad, ad * bd)

cdef _mul(an, ad, bn, bd):
    """a * b"""
    return Fraction(an * bn, ad * bd)

cdef _div(an, ad, bn, bd):
    """a / b"""
    return Fraction(an * bd, ad * bn)

cdef _floordiv(an, ad, bn, bd):
    """a // b -> int"""
    return (an * bd) // (bn * ad)

cdef _divmod(an, ad, bn, bd):
    div, n_mod = divmod(an * bd, ad * bn)
    return div, Fraction(n_mod, ad * bd)

cdef _mod(an, ad, bn, bd):
    return Fraction((an * bd) % (bn * ad), ad * bd)


cdef:
    _math_op_add = operator.add
    _math_op_sub = operator.sub
    _math_op_mul = operator.mul
    _math_op_div = getattr(operator, 'div', operator.truediv)  # Py2/3
    _math_op_truediv = operator.truediv
    _math_op_floordiv = operator.floordiv
    _math_op_mod = operator.mod
    _math_op_divmod = divmod


ctypedef object (*math_func)(an, ad, bn, bd)


cdef _math_op(a, b, math_func monomorphic_operator, pyoperator):
    if isinstance(a, Fraction):
        return forward(a, b, monomorphic_operator, pyoperator)
    else:
        return reverse(a, b, monomorphic_operator, pyoperator)


cdef forward(a, b, math_func monomorphic_operator, pyoperator):
    an, ad = (<Fraction>a)._numerator, (<Fraction>a)._denominator
    if type(b) is Fraction:
        return monomorphic_operator(an, ad, (<Fraction>b)._numerator, (<Fraction>b)._denominator)
    elif isinstance(b, (int, long)):
        return monomorphic_operator(an, ad, b, 1)
    elif isinstance(b, (Fraction, Rational)):
        return monomorphic_operator(an, ad, b.numerator, b.denominator)
    elif isinstance(b, float):
        return pyoperator(_as_float(an, ad), b)
    elif isinstance(b, complex):
        return pyoperator(complex(a), b)
    else:
        return NotImplemented


cdef reverse(a, b, math_func monomorphic_operator, pyoperator):
    bn, bd = (<Fraction>b)._numerator, (<Fraction>b)._denominator
    if isinstance(a, (int, long)):
        return monomorphic_operator(a, 1, bn, bd)
    elif isinstance(a, Rational):
        return monomorphic_operator(a.numerator, a.denominator, bn, bd)
    elif isinstance(a, Real):
        return pyoperator(float(a), _as_float(bn, bd))
    elif isinstance(a, Complex):
        return pyoperator(complex(a), complex(b))
    else:
        return NotImplemented


ctypedef fused AnyString:
    bytes
    unicode


cdef enum ParserState:
    BEGIN_SPACE          # '\s'*     ->  (BEGIN_SIGN, SMALL_NUM, START_DECIMAL_DOT)
    BEGIN_SIGN           # [+-]      ->  (SMALL_NUM, SMALL_DECIMAL_DOT)
    SMALL_NUM            # [0-9]+    ->  (SMALL_NUM, SMALL_NUM_US, NUM, NUM_SPACE, SMALL_DECIMAL_DOT, EXP_E, DENOM_START)
    SMALL_NUM_US         # '_'       ->  (SMALL_NUM, NUM)
    NUM                  # [0-9]+    ->  (NUM, NUM_US, NUM_SPACE, DECIMAL_DOT, EXP_E, DENOM_START)
    NUM_US               # '_'       ->  (NUM)
    NUM_SPACE            # '\s'+     ->  (DENOM_START)

    # 1) floating point syntax
    START_DECIMAL_DOT    # '.'       ->  (SMALL_DECIMAL)
    SMALL_DECIMAL_DOT    # '.'       ->  (SMALL_DECIMAL, EXP_E, SMALL_END_SPACE)
    DECIMAL_DOT          # '.'       ->  (DECIMAL, EXP_E, END_SPACE)
    SMALL_DECIMAL        # [0-9]+    ->  (SMALL_DECIMAL, SMALL_DECIMAL_US, DECIMAL, EXP_E, SMALL_END_SPACE)
    SMALL_DECIMAL_US     # '_'       ->  (SMALL_DECIMAL, DECIMAL)
    DECIMAL              # [0-9]+    ->  (DECIMAL, DECIMAL_US, EXP_E, END_SPACE)
    DECIMAL_US           # '_'       ->  (DECIMAL)
    EXP_E                # [eE]      ->  (EXP_SIGN, EXP)
    EXP_SIGN             # [+-]      ->  (EXP)
    EXP                  # [0-9]+    ->  (EXP_US, END_SPACE)
    EXP_US               # '_'       ->  (EXP)
    END_SPACE            # '\s'+
    SMALL_END_SPACE      # '\s'+

    # 2) "NOM / DENOM" syntax
    DENOM_START          # '/'       ->  (DENOM_SIGN, SMALL_DENOM)
    DENOM_SIGN           # [+-]      ->  (SMALL_DENOM)
    SMALL_DENOM          # [0-9]+    ->  (SMALL_DENOM, SMALL_DENOM_US, DENOM, DENOM_SPACE)
    SMALL_DENOM_US       # '_'       ->  (SMALL_DENOM)
    DENOM                # [0-9]+    ->  (DENOM, DENOM_US, DENOM_SPACE)
    DENOM_US             # '_'       ->  (DENOM)
    DENOM_SPACE          # '\s'+


cdef _raise_invalid_input(s):
    raise ValueError(f'Invalid literal for Fraction: {s!r}') from None


cdef _raise_parse_overflow(s):
    raise OverflowError(f"Exponent too large for Fraction: {s!r}") from None


cdef tuple _parse_fraction(AnyString s):
    """
    Parse a string into a number tuple: (nominator, denominator, is_normalised)
    """
    cdef size_t i
    cdef Py_ssize_t decimal_len = 0
    cdef Py_UCS4 c
    cdef ParserState state = BEGIN_SPACE

    cdef bint is_neg = False, exp_is_neg = False
    cdef int digit
    cdef unsigned int udigit
    cdef long long inum = 0, idecimal = 0, idenom = 0, iexp = 0
    cdef ullong igcd
    cdef object num = None, decimal, denom

    for i, c in enumerate(s):
        udigit = (<unsigned int>c) - <unsigned int>'0'  # Relies on integer underflow for dots etc.
        if udigit <= 9:
            digit = <int>udigit
        else:
            if c == u'/':
                if state == SMALL_NUM:
                    num = inum
                elif state in (NUM, NUM_SPACE):
                    pass
                else:
                    _raise_invalid_input(s)
                state = DENOM_START
                continue
            elif c == u'.':
                if state in (BEGIN_SPACE, BEGIN_SIGN):
                    state = START_DECIMAL_DOT
                elif state == SMALL_NUM:
                    state = SMALL_DECIMAL_DOT
                elif state == NUM:
                    state = DECIMAL_DOT
                else:
                    _raise_invalid_input(s)
                continue
            elif c in u'eE':
                if state in (SMALL_NUM, SMALL_DECIMAL_DOT, SMALL_DECIMAL):
                    num = inum
                elif state in (NUM, DECIMAL_DOT, DECIMAL):
                    pass
                else:
                    _raise_invalid_input(s)
                state = EXP_E
                continue
            elif c in u'-+':
                if state == BEGIN_SPACE:
                    is_neg = c == u'-'
                    state = BEGIN_SIGN
                elif state == EXP_E:
                    exp_is_neg = c == u'-'
                    state = EXP_SIGN
                elif state == DENOM_START:
                    is_neg ^= (c == u'-')
                    state = DENOM_SIGN
                else:
                    _raise_invalid_input(s)
                continue
            elif c == u'_':
                if state == SMALL_NUM:
                    state = SMALL_NUM_US
                elif state == NUM:
                    state = NUM_US
                elif state == SMALL_DECIMAL:
                    state = SMALL_DECIMAL_US
                elif state == DECIMAL:
                    state = DECIMAL_US
                elif state == EXP:
                    state = EXP_US
                elif state == SMALL_DENOM:
                    state = SMALL_DENOM_US
                elif state == DENOM:
                    state = DENOM_US
                else:
                    _raise_invalid_input(s)
                continue
            else:
                if c.isspace():
                    if state in (BEGIN_SPACE, NUM_SPACE, END_SPACE, SMALL_END_SPACE, DENOM_START, DENOM_SPACE):
                        pass
                    elif state == SMALL_NUM:
                        num = inum
                        state = NUM_SPACE
                    elif state == NUM:
                        state = NUM_SPACE
                    elif state in (SMALL_DECIMAL, SMALL_DECIMAL_DOT):
                        num = inum
                        state = SMALL_END_SPACE
                    elif state in (DECIMAL, DECIMAL_DOT):
                        state = END_SPACE
                    elif state == EXP:
                        state = END_SPACE
                    elif state == SMALL_DENOM:
                        denom = idenom
                        state = DENOM_SPACE
                    elif state == DENOM:
                        state = DENOM_SPACE
                    else:
                        _raise_invalid_input(s)
                    continue

            digit = Py_UNICODE_TODECIMAL(c)
            if digit == -1:
                _raise_invalid_input(s)
                continue

        # normal digit found
        if state in (BEGIN_SPACE, BEGIN_SIGN, SMALL_NUM, SMALL_NUM_US):
            inum = inum * 10 + digit
            state = SMALL_NUM
            if inum > MAX_SMALL_NUMBER:
                num = inum
                state = NUM
        elif state in (NUM, NUM_US):
            num = num * 10 + digit
            state = NUM
        elif state in (START_DECIMAL_DOT, SMALL_DECIMAL_DOT, SMALL_DECIMAL, SMALL_DECIMAL_US):
            decimal_len += 1
            inum = inum * 10 + digit
            state = SMALL_DECIMAL
            # 2^n > 10^(n * 5/17)
            if inum > MAX_SMALL_NUMBER or decimal_len >= (sizeof(idenom) * 8) * 5 // 17:
                num = inum
                state = DECIMAL
        elif state in (DECIMAL_DOT, DECIMAL, DECIMAL_US):
            decimal_len += 1
            num = num * 10 + digit
            state = DECIMAL
        elif state in (EXP_E, EXP_SIGN, EXP, EXP_US):
            iexp = iexp * 10 + digit
            if iexp > MAX_SMALL_NUMBER:
                _raise_parse_overflow(s)
            state = EXP
        elif state in (DENOM_START, DENOM_SIGN, SMALL_DENOM, SMALL_DENOM_US):
            idenom = idenom * 10 + digit
            state = SMALL_DENOM
            if idenom > MAX_SMALL_NUMBER:
                denom = idenom
                state = DENOM
        elif state in (DENOM, DENOM_US):
            denom = denom * 10 + digit
            state = DENOM
        else:
            _raise_invalid_input(s)

    is_normalised = False
    if state in (SMALL_NUM, SMALL_DECIMAL, SMALL_DECIMAL_DOT, SMALL_END_SPACE):
        # Special case for 'small' numbers: normalise directly in C space.
        if inum and decimal_len:
            denom = pow10(decimal_len)
            igcd = _ibgcd[ullong](inum, denom)
            if igcd > 1:
                inum //= igcd
                denom //= igcd
        else:
            denom = 1
        if is_neg:
            inum = -inum
        return inum, denom, True

    elif state in (NUM, NUM_SPACE, DECIMAL_DOT, DECIMAL, EXP, END_SPACE):
        is_normalised = True
        denom = 1
    elif state == SMALL_DENOM:
        denom = idenom
    elif state in (DENOM, DENOM_SPACE):
        pass
    else:
        _raise_invalid_input(s)

    if decimal_len > MAX_SMALL_NUMBER:
        _raise_parse_overflow(s)
    if exp_is_neg:
        iexp = -iexp
    iexp -= decimal_len

    if is_neg:
        num = -num
    if iexp > 0:
        num *= pow10(iexp)
    elif iexp < 0:
        is_normalised = False
        denom = pow10(-iexp)

    return num, denom, is_normalised
