# Logging

The platform provides logging for both backtesting and live trading using a high-performance logger implemented in Rust
using the `log` crate.
The logger operates in a separate thread and uses a multi-producer single consumer (MPSC) channel to receive log messages.
This design ensures that the main thread is not blocked by log string formatting or file I/O operations.

```{note}
The latest stable Rust MPSC channel is used, which is now based on the `crossbeam` implementation.
```

There are two configurable writers for logging:
- stdout/stderr writer
- log file writer

Infrastructure such as [vector](https://github.com/vectordotdev/vector) can be configured to collect these log events.

## Configuration

Logging can be configured by importing the `LoggingConfig` object.
By default, log events with an 'INFO' `LogLevel` and higher are written to stdout/stderr.

Log level (`LogLevel`) values include:
- 'DEBUG'
- 'INFO'
- 'WARNING' or 'WARN'
- 'ERROR'

```{note}
See the `LoggingConfig` [API Reference](../api_reference/config.md#LoggingConfig) for further details.
```

Logging can be configured in the following ways:
- Minimum `LogLevel` for stdout/stderr
- Minimum `LogLevel` for log files
- Automatic log file naming and daily rotation, or custom log file name
- Directory for writing log files
- Plain text or JSON log file formatting
- Filtering of individual components by log level
- ANSI colors in log lines
- Bypass logging completely
- Print Rust config to stdout at initialization

```{warning}
Only one logger (with Rust backed logging system) can be initialized per process.
```

### Standard output logging
Log messages are written to the console via stdout/stderr writers. The minimum log level can be configured using the `log_level` parameter.

### File logging

Log files are written to the current working directory with daily rotation (UTC) by default. 

The default naming convention is as follows:
- Trader ID
- ISO 8601 datetime
- Instance ID
- The log format suffix

```
{trader_id}_{%Y-%m-%d}_{instance_id}.{log | json}`
```

e.g. `TESTER-001_2023-03-23_635a4539-4fe2-4cb1-9be3-3079ba8d879e.json`

You can specify a custom log directory path using the `log_directory` parameter and/or a custom log file basename using the `log_file_name` parameter. 
The log files will always be suffixed with '.log' for plain text, or '.json' for JSON (no need to include a suffix in file names).

If the log file already exists, it will be appended to.

### Component filtering

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
However, in environments that do not support ANSI color rendering (such as some cloud environments or text editors),
these color codes may not be appropriate as they can appear as raw text.

To accommodate for such scenarios, the `LoggingConfig.log_colors` option can be set to `false`.
Disabling `log_colors` will prevent the addition of ANSI color codes to the log messages, ensuring
compatibility across different environments where color rendering is not supported.
