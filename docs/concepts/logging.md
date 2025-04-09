# Logging

The platform provides logging for both backtesting and live trading using a high-performance logging system implemented in Rust
with a standardized facade from the `log` crate.

The core logger operates in a separate thread and uses a multi-producer single-consumer (MPSC) channel to receive log messages.
This design ensures that the main thread remains performant, avoiding potential bottlenecks caused by log string formatting or file I/O operations.

Logging output is configurable and supports:
- **stdout/stderr writer** for console output
- **file writer** for persistent storage of logs

:::info
Infrastructure such as [Vector](https://github.com/vectordotdev/vector) can be integrated to collect and aggregate events within your system.
:::

## Configuration

Logging can be configured by importing the `LoggingConfig` object.
By default, log events with an 'INFO' `LogLevel` and higher are written to stdout/stderr.

Log level (`LogLevel`) values include (and generally match Rusts `tracing` level filters):
- `OFF`
- `DEBUG`
- `INFO`
- `WARNING` or `WARN`
- `ERROR`

:::info
See the `LoggingConfig` [API Reference](../api_reference/config.md) for further details.
:::

Logging can be configured in the following ways:

- Minimum `LogLevel` for stdout/stderr
- Minimum `LogLevel` for log files
- Maximum size before rotating a log file
- Maximum number of backup log files to maintain when rotating
- Automatic log file naming with date or timestamp components, or custom log file name
- Directory for writing log files
- Plain text or JSON log file formatting
- Filtering of individual components by log level
- ANSI colors in log lines
- Bypass logging entirely
- Print Rust config to stdout at initialization

### Standard output logging

Log messages are written to the console via stdout/stderr writers. The minimum log level can be configured using the `log_level` parameter.

### File logging

Log files are written to the current working directory by default. The naming convention and rotation behavior are configurable and follow specific patterns based on your settings.

You can specify a custom log directory using `log_directory` and/or a custom file basename using `log_file_name`. Log files are always suffixed with `.log` (plain text) or `.json` (JSON).

For detailed information about log file naming conventions and rotation behavior, see the [Log file rotation](#log-file-rotation) and [Log file naming convention](#log-file-naming-convention) sections below.

#### Log file rotation

The logging system is designed to manage log files efficiently through **daily log file rotation** as its default behavior.
This means a new log file is created each day based on the system's date, keeping logs neatly organized by day.
Unless additional rotation settings are applied, all logs for a given day are appended to a single file.

- **Daily rotation**:
  - Enabled by default when using the automatic naming convention.
  - A new log file is created each day based on UTC time.
  - This behavior is disabled if a custom `log_file_name` is specified, meaning the same file will be used indefinitely unless size-based rotation is configured.

- **Size-based rotation**:
  - Enabled by configuring the `log_file_max_size` parameter (e.g., `100_000_000` for 100 MB).
  - Before the log file reaches this size, a new file is started.
  - This allows multiple files to be created within a single day if the size threshold is reached repeatedly.
  - The `log_file_max_backup_count` parameter (default: 5) determines how many backup files are retained. Older backups are deleted once this limit is exceeded.

To keep disk usage in check, the system includes backup file management:

- **Backup file management**:
  - The maximum number of backup files is controlled by `log_file_max_backup_count` (default: 5).
  - When this limit is surpassed, the oldest backup files are automatically removed.

#### Log file naming convention

The default naming convention ensures log files are uniquely identifiable and timestamped.
The format depends on whether file rotation is enabled:

**With rotation enabled**:

- **Format**: `{trader_id}_{%Y-%m-%d_%H%M%S:%3f}_{instance_id}.{log|json}`
- **Example**: `TESTER-001_2025-04-09_210721:521_d7dc12c8-7008-4042-8ac4-017c3db0fc38.log`
- **Components**:
  - `{trader_id}`: The trader identifier (e.g., `TESTER-001`).
  - `{%Y-%m-%d_%H%M%S:%3f}`: Full ISO 8601-compliant datetime with millisecond resolution.
  - `{instance_id}`: A unique instance identifier.
  - `{log|json}`: File suffix based on format setting.

**With rotation disabled**:

  - **Format**: `{trader_id}_{%Y-%m-%d}_{instance_id}.{log|json}`
  - **Example**: `TESTER-001_2025-04-09_d7dc12c8-7008-4042-8ac4-017c3db0fc38.log`
  - **Components**:
    - `{trader_id}`: The trader identifier.
    - `{%Y-%m-%d}`: Date only (YYYY-MM-DD).
    - `{instance_id}`: A unique instance identifier.
    - `{log|json}`: File suffix based on format setting.

**Custom naming**:

If `log_file_name` is set (e.g., `my_custom_log`):

- With rotation disabled: The file will be named exactly as provided (e.g., `my_custom_log.log`).
- With rotation enabled: The file will include the custom name and timestamp (e.g., `my_custom_log_2025-04-09_210721:521.log`).

### Component log filtering

The `log_component_levels` parameter can be used to set log levels for each component individually.
The input value should be a dictionary of component ID strings to log level strings: `dict[str, str]`.

Below is an example of a trading node logging configuration that includes some of the options mentioned above:

```python
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig

config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    logging=LoggingConfig(
        log_level="INFO",
        log_level_file="DEBUG",
        log_file_format="json",
        log_component_levels={ "Portfolio": "INFO" },
    ),
    ... # Omitted
)
```

For backtesting, the `BacktestEngineConfig` class can be used instead of `TradingNodeConfig`, as the same options are available.

### Log Colors

ANSI color codes are utilized to enhance the readability of logs when viewed in a terminal.
These color codes can make it easier to distinguish different parts of log messages.
In environments that do not support ANSI color rendering (such as some cloud environments or text editors),
these color codes may not be appropriate as they can appear as raw text.

To accommodate for such scenarios, the `LoggingConfig.log_colors` option can be set to `false`.
Disabling `log_colors` will prevent the addition of ANSI color codes to the log messages, ensuring
compatibility across different environments where color rendering is not supported.

## Using a Logger directly

It's possible to use `Logger` objects directly, and these can be initialized anywhere (very similar to the Python built-in `logging` API).

If you ***aren't*** using an object which already initializes a `NautilusKernel` (and logging) such as `BacktestEngine` or `TradingNode`,
then you can activate logging in the following way:
```python
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.component import Logger

log_guard = init_logging()
logger = Logger("MyLogger")
```

:::info
See the `init_logging` [API Reference](../api_reference/common) for further details.
:::

:::warning
Only one logging system can be initialized per process with an `init_logging` call, and the `LogGuard` which is returned must be kept alive for the lifetime of the program.
:::

## LogGuard: Managing log lifecycle

The `LogGuard` ensures that the logging system remains active and operational throughout the lifecycle of a process.
It prevents premature shutdown of the logging system when running multiple engines in the same process.

### Why use LogGuard?

Without a `LogGuard`, any attempt to run sequential engines in the same process may result in errors such as:

```
Error sending log event: [INFO] ...
```

This occurs because the logging system's underlying channel and Rust `Logger` are closed when the first engine is disposed.
As a result, subsequent engines lose access to the logging system, leading to these errors.

By leveraging a `LogGuard`, you can ensure robust logging behavior across multiple backtests or engine runs in the same process.
The `LogGuard` retains the resources of the logging system and ensures that logs continue to function correctly,
even as engines are disposed and initialized.

:::note
Using `LogGuard` is critical to maintain consistent logging behavior throughout a process with multiple engines.
:::

## Running multiple engines

The following example demonstrates how to use a `LogGuard` when running multiple engines sequentially in the same process:

```python
log_guard = None  # Initialize LogGuard reference

for i in range(number_of_backtests):
    engine = setup_engine(...)

    # Assign reference to LogGuard
    if log_guard is None:
        log_guard = engine.get_log_guard()

    # Add actors and execute the engine
    actors = setup_actors(...)
    engine.add_actors(actors)
    engine.run()
    engine.dispose()  # Dispose safely
```

### Steps

- **Initialize LogGuard once**: The `LogGuard` is obtained from the first engine (`engine.get_log_guard()`) and is retained throughout the process. This ensures that the logging system remains active.
- **Dispose engines safely**: Each engine is safely disposed of after its backtest completes, without affecting the logging system.
- **Reuse LogGuard**: The same `LogGuard` instance is reused for subsequent engines, preventing the logging system from shutting down prematurely.

### Considerations

- **Single LogGuard per process**: Only one `LogGuard` can be used per process.
- **Thread safety**: The logging system, including `LogGuard`, is thread-safe, ensuring consistent behavior even in multi-threaded environments.
- **Flush logs on termination**: Always ensure that logs are properly flushed when the process terminates. The `LogGuard` automatically handles this as it goes out of scope.
