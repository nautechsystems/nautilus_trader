import tracemalloc


# Number of runs
def trace_memory(runs):
    # Function to trace memory
    def decorator(func):
        def wrapper(*args, **kwargs):
            # Start tracing memory allocations
            tracemalloc.start()

            # Initialize variables to track max memory usage and continuously increasing memory allocations
            max_memory = 0
            snapshot = None
            initial_snapshot = None

            # Run the function n times and measure memory usage each time
            for i in range(runs):
                # Print the max heap memory usage
                print(f"Run: {i}")

                # Run func
                func(args, kwargs)

                # Register snapshots and measure memory
                snapshot = tracemalloc.take_snapshot()
                if i == 0:
                    initial_snapshot = snapshot

                # Get the current heap memory usage
                current_memory = snapshot.statistics("traceback")[0].size / (1024 * 1024)
                # Update max_memory if current_memory is greater
                if current_memory > max_memory:
                    max_memory = current_memory

                # Print the difference in memory usage between runs
                print(
                    f"Memory usage on run {i+1}: {snapshot.compare_to(tracemalloc.take_snapshot('last'))}",
                )

            # Stop tracing memory allocations
            tracemalloc.stop()

            # Print the max heap memory usage
            print(f"Max heap memory usage: {max_memory} MB")

            # Find and display largest memory blocks, since initial run
            top_stats = snapshot.compare_to(initial_snapshot, "lineno")
            stat = top_stats[0]
            print(f"{stat.count} memory blocks: {stat.size / 1024:.1f} KiB")
            for line in stat.traceback.format():
                print(line)

        return wrapper

    return decorator
