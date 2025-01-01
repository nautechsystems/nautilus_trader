# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
import tracemalloc


# Number of runs
def snapshot_memory(runs):
    # Snapshot memory for func
    def decorator(func):
        def wrapper(*args, **kwargs):
            # Start tracing memory allocations
            tracemalloc.start()

            # Initialize variables to track max memory usage and continuously increasing memory allocations
            max_peak_memory = 0
            snapshot = None
            initial_snapshot = None

            # Run the function n times and measure memory usage each time
            for i in range(runs):
                # Print the max heap memory usage
                print(f"Run {i}...")

                # Run func
                func(args, kwargs)

                # Register snapshots and measure memory
                snapshot = tracemalloc.take_snapshot()
                if i == 0:
                    initial_snapshot = snapshot

                (current_memory, peak) = tracemalloc.get_traced_memory()
                current_memory = current_memory / (1024 * 1024)
                peak = peak / (1024 * 1024)

                # Update max_memory if current_memory is greater
                if peak > max_peak_memory:
                    max_peak_memory = current_memory

                # Print the difference in memory usage between runs
                print(
                    f"Memory allocated after run {i+1}: {current_memory} MB",
                )
                print(
                    f"Max peak memory recorded: {max_peak_memory} MB",
                )
                print()

                # reset
                gc.collect()
                tracemalloc.reset_peak()

            # Stop tracing memory allocations
            tracemalloc.stop()

            # Find and display largest memory blocks, since initial run
            top_stats = snapshot.compare_to(initial_snapshot, "lineno")
            print("[ Top 10 differences ]")
            for stat in top_stats[:10]:
                print(stat)

            stat = top_stats[0]
            print(f"{stat.count} memory blocks: {stat.size / 1024:.1f} KiB")
            for line in stat.traceback.format():
                print(line)

        return wrapper

    return decorator
