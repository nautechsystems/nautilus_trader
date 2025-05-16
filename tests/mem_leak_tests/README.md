# Performance tests

This subpackage provides a suite of performance tests, including scripts which can be run
to profile memory and thread resource usage.

Memory profiling is conducted using [memray](https://github.com/bloomberg/memray).
The package is not a development dependency because it doesn't currently support windows.

You can install the package via PyPI:

```bash
pip install memray
```

To profile using memray, first invoke the script using the memray CLI:

```bash
memray run --live-port 8100 --live-remote tests/mem_leak_tests/memray_backtest.py
```

Then from another shell, connect to the memray profiler dashboard:

```bash
memray live 8100
```
