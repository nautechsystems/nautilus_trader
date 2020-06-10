# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import math
import timeit
import inspect


_MILLISECONDS_IN_SECOND = 1000
_MICROSECONDS_IN_SECOND = 1000000


class PerformanceHarness:

    @staticmethod
    def profile_function(function, runs, iterations, print_output=True) -> float:
        """
        Return the minimum time in seconds taken to call the given function iteration times.

        :param function: The function call to profile.
        :param runs: The number of runs for the test.
        :param iterations: The number of call iterations per run.
        :param print_output: If the output should be printed to the console.
        :return float.
        """
        results = timeit.Timer(function).repeat(repeat=runs, number=iterations)
        minimum = min(results)

        if print_output:
            result_milliseconds = math.floor(minimum * _MILLISECONDS_IN_SECOND)
            result_microseconds = math.floor(minimum * _MICROSECONDS_IN_SECOND)
            print(f'\nPerformance test: {str(inspect.getmembers(function)[4][1])} ')
            print(f'# ~{result_milliseconds}ms ({result_microseconds}μs) minimum '
                  f'of {runs} runs @ {iterations:,} iterations each run.')

        return minimum

    # @staticmethod
    # def object_size(x, print_output=True) -> int:
    #     """
    #     Return the object size in bytes and optionally print the message.
    #
    #     :param x: The object to check.
    #     :param print_output: If the output should be printed to the console.
    #     :return: int.
    #     """
    #     size = get_size_of(x)
    #
    #     if print_output:
    #         print(f'\n# Object size test: {type(x)} is {size} bytes.')
    #
    #     return size
