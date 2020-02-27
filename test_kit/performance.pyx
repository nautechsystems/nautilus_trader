# -------------------------------------------------------------------------------------------------
# <copyright file="performance.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import math
import timeit
import inspect

from nautilus_trader.core.functions cimport get_size_of


cdef int _MILLISECONDS_IN_SECOND = 1000
cdef int _MICROSECONDS_IN_SECOND = 1000000


cdef class PerformanceHarness:

    @staticmethod
    def profile_function(function, int runs, int iterations, bint print_output=True) -> float:
        """
        Return the minimum time in seconds taken to call the given function iteration times.

        :param function: The function call to profile.
        :param runs: The number of runs for the test.
        :param iterations: The number of call iterations per run.
        :param print_output: If the output should be printed to the console.
        :return float.
        """
        cdef list results = timeit.Timer(function).repeat(repeat=runs, number=iterations)
        cdef double minimum = min(results)

        if print_output:
            result_milliseconds = math.floor(minimum * _MILLISECONDS_IN_SECOND)
            result_microseconds = math.floor(minimum * _MICROSECONDS_IN_SECOND)
            print(f'\nPerformance test: {str(inspect.getmembers(function)[4][1])} ')
            print(f'# ~{result_milliseconds}ms ({result_microseconds}μs) minimum '
                  f'of {runs} runs @ {iterations:,} iterations each run.')

        return minimum

    @staticmethod
    def object_size(object x, bint print_output=True) -> int:
        """
        Return the object size in bytes and optionally print the message.

        :param x: The object to check.
        :param print_output: If the output should be printed to the console.
        :return: int.
        """
        cdef int size = get_size_of(x)

        if print_output:
            print(f'\n# Object size test: {type(x)} is {size} bytes.')

        return size
