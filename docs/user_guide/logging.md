# Logging

Logging for the platform is provided uniformly for both backtesting and live trading.
A high-performance logger, implemented in Rust, operates in a separate thread, receiving log messages across a multi-producer single consumer (MPSC) channel.
This keeps the main thread free from log string formatting and file I/O operations.

```{note}
The latest stable Rust MPSC channel is used, which is now based on the `crossbeam` implementation.
```

There are two configurable writers for logging:
- stdout/stderr writer
- log file writer

Infrastructure such as [vector](https://github.com/vectordotdev/vector) can be configured to collect log events from these writers.

## Configuration

Logging can be configured by importing the `LoggingConfig` object.
By default, log events with an 'INFO' `LogLevel` or higher are written to stdout/stderr.

Log levels include:
- 'DEBUG' or 'DBG'
- 'INFO' or 'INF'
- 'WARNING' or 'WRN'
- 'ERROR' or 'ERR'

```{tip}
See the `LoggingConfig` [API Reference](../api_reference/config.md) for further details.
```

Logging can be configured in the following ways:
- Minimum LogLevel for stdout/stderr
- Minimum LogLevel for log files
- Automatic log file naming and daily rotation or custom log file name
- Plain text or JSON log file formatting
- Bypass logging completely

### Standard output logging
The stdout/stderr writers log events to the console, and the minimum log level can be configured with the `log_level` parameter.

### File logging

Log files will be written to the current working directory unless you provide `log_directory`. 
If a `log_file_name` is provided then the suffix will be '.log' for plain text, or '.json' for JSON (no need to include a suffix).

If a `log_file_name` is _not_ provided then log files will be automatically named as follows:
- Trader ID
- Instance ID
- ISO 8601 datetime
- If the log file is the latest active (`_rCURRENT` discriminant)
- The log format suffix

```
{trader_id}_{instance_id}_{ISO 8601 datetime}_{discriminant}.{log | json}
```
e.g. `TESTER-001_635a4539-4fe2-4cb1-9be3-3079ba8d879e_2023-03-22_15-51-48_rCURRENT.json`

Automatically named files will also be rotated daily.

### Component filtering

Per component log levels can be set via the `log_component_levels` parameter. Pass a dictionary of component ID strings to log level strings dict[str, str].

Here is an example trading node logging configuration, showing some of the above options:

```python
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

For backtesting, simply replace the config class with `BacktestEngineConfig`, as the same options are available.
