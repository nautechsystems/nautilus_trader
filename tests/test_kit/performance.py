# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import gc
import inspect
import sys
import timeit


class PerformanceHarness:

    @staticmethod
    def profile_function(function, runs, iterations, print_output=True) -> float:
        """
        Profile the given function.

        Return the minimum elapsed time in seconds taken to call the given
        function iteration times.

        Also prints the elapsed time in milliseconds (ms), microseconds (μs) and
        nanoseconds (ns). As a rule of thumb a CPU cycles in 1 nanosecond per
        GHz of clock speed.

        Parameters
        ----------
        function : callable
            The function call to profile.
        runs : int
            The number of runs for the test.
        iterations : int
            The number of call iterations per run.
        print_output : bool
            If the output should be printed to the console.

        Raises
        ------
        ValueError
            If runs is not positive (> 1).
        ValueError
            If iterations is not positive (> 1).

        Returns
        -------
        float

        """
        if runs < 1:
            raise ValueError("runs cannot be less than 1")
        if iterations < 1:
            raise ValueError("iterations cannot be less than 1")

        results = timeit.Timer(function).repeat(repeat=runs, number=iterations)
        minimum = min(results)  # In seconds

        if print_output:
            result_milli = minimum * 1000          # 1,000ms in 1 second
            result_micro = minimum * 1_000_000     # 1,000,000μs in 1 second
            result_nano = minimum * 1_000_000_000  # 1,000,000,000ns in 1 second
            print(f"\nPerformance test: {str(inspect.getmembers(function)[4][1])} ")
            print(f"# ~{result_milli:.1f}ms "
                  f"/ ~{result_micro:.1f}μs "
                  f"/ {result_nano:.0f}ns "
                  f"minimum of {runs:,} runs @ {iterations:,} "
                  f"iteration{'s' if iterations > 1 else ''} each run.")

        return minimum

    @staticmethod
    def object_size(x, print_output=True) -> int:
        """Return the object size in bytes and optionally print the message.

        Parameters
        ----------
        x : object
            The object to check.
        print_output : bool
            If the output should be printed to the console.

        Returns
        -------
        int

        """
        size = get_size_of(x)

        if print_output:
            print(f"\n# Object size {type(x)} is {size} bytes.")

        return size


def get_size_of(obj) -> int:
    """
    Return the size of the given object in memory.

    Parameters
    ----------
    obj : object
        The object to measure for space complexity.

    Returns
    -------
    int
        The object size in bytes.

    """
    marked = {id(obj)}
    obj_q = [obj]
    size = 0

    while obj_q:
        size += sum(map(sys.getsizeof, obj_q))

        # Lookup all the object referred to by the object in obj_q.
        # See: https://docs.python.org/3.7/library/gc.html#gc.get_referents
        all_refs = ((id(o), o) for o in gc.get_referents(*obj_q))

        # Filter object that are already marked.
        # Using dict notation will prevent repeated objects.
        new_ref = {
            o_id: o for o_id, o in all_refs if o_id not in marked and not isinstance(o, type)
        }

        # The new obj_q will be the ones that were not marked,
        # and we will update marked with their ids so we will
        # not traverse them again.
        obj_q = new_ref.values()
        marked.update(new_ref.keys())

    return size
