# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import inspect
import timeit

import pytest


class PerformanceHarness:
    @pytest.fixture(autouse=True)
    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    def setup(self, benchmark):
        self.benchmark = benchmark


class PerformanceBench:
    @staticmethod
    def profile_function(target, runs, iterations, print_output=True) -> float:
        """
        Profile the given function.
        Return the minimum elapsed time in seconds taken to call the given
        function iteration times.
        Also prints the elapsed time in milliseconds (ms), microseconds (μs) and
        nanoseconds (ns). As a rule of thumb a CPU cycles in 1 nanosecond per
        GHz of clock speed.

        Parameters
        ----------
        target : callable
            The target to call and profile.
        runs : int
            The number of runs for the test.
        iterations : int
            The number of call iterations per run.
        print_output : bool
            If the output should be printed to the console.

        Raises
        ------
        ValueError
            If `runs` is not positive (> 1).
        ValueError
            If `iterations` is not positive (> 1).

        Returns
        -------
        float

        """
        if runs < 1:
            raise ValueError("runs cannot be less than 1")
        if iterations < 1:
            raise ValueError("iterations cannot be less than 1")

        results = timeit.Timer(target).repeat(repeat=runs, number=iterations)
        minimum = min(results)  # In seconds

        if print_output:
            result_milli = minimum * 1000  # 1,000ms in 1 second
            result_micro = minimum * 1_000_000  # 1,000,000μs in 1 second
            result_nano = minimum * 1_000_000_000  # 1,000,000,000ns in 1 second
            print(f"\nPerformance test: {str(inspect.getmembers(target)[4][1])} ")
            print(
                f"# ~{result_milli:.1f}ms "
                f"/ ~{result_micro:.1f}μs "
                f"/ {result_nano:.0f}ns "
                f"minimum of {runs:,} runs @ {iterations:,} "
                f"iteration{'s' if iterations > 1 else ''} each run.",
            )

        return minimum
