# -------------------------------------------------------------------------------------------------
# <copyright file="performance.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import sys
import math
import timeit
import inspect

from time import time


cdef int _MILLISECONDS_IN_SECOND = 1000
cdef int _MICROSECONDS_IN_SECOND = 1000000


cdef class PerformanceProfiler:

    @staticmethod
    def profile_function(function, int iterations, int runs):
        """
        Profile the given function by calling it iteration times for the given
        runs.

        :param function: The function to profile.
        :param iterations: The number of times to call the function per run.
        :param runs: The number of runs for the test
        """
        cdef str signature = str(inspect.getmembers(function)[4][1])
        cdef float total_elapsed = 0

        cdef int x
        for x in range(runs):
            start_time = time()
            timeit.Timer(function).timeit(number=iterations)
            stop_time = time()
            total_elapsed += stop_time - start_time

        print('\n' + f'Performance test: {signature} ')
        print(f'# ~{math.ceil((total_elapsed / runs) * _MILLISECONDS_IN_SECOND)}ms '
              f'({math.ceil((total_elapsed / runs) * _MICROSECONDS_IN_SECOND)}μs) '
              f'average over {runs} runs @ {iterations} iterations')

    @staticmethod
    def object_size(object x) -> int:
        """
        Return the size of the object in bytes and print a message.

        :param x: The object.
        :return: int.
        """
        cdef int size = sys.getsizeof(x)
        print(f'{type(x)} size is {size} bytes')
        return size
