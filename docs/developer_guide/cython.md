# Cython

Here you will find guidance and tips for working on NautilusTrader using the Cython language.
More information on Cython syntax and conventions can be found by reading the [Cython docs](https://cython.readthedocs.io/en/latest/index.html).

## Function and method signatures
Ensure that all functions and methods returning `void` or a primitive C type (such as `bint`, `int`, `double`) include the `except *` keyword in the signature.

This will ensure Python exceptions are not ignored, but instead are “bubbled up” to the caller as expected.

## Debugging
For PyCharm there has been a highly up-voted feature request for better debugging support for Cython, 
however it hasn't received any traction for nearly a decade. So unfortunately it's safe to assume that for
PyCharm at least, it will not be receiving first class support for Cython debugging
https://youtrack.jetbrains.com/issue/PY-9476.

The following recommendations are contained in the Cython docs:
https://cython.readthedocs.io/en/latest/src/userguide/debugging.html

The summary is it involves manually running a specialized version of `gdb` from the command line.
We don't recommend this workflow.

When debugging and seeking to understand a complex system such as NautilusTrader, it can be
quite helpful to step through the code with a debugger, however with this not being available
for the Cython part of the codebase, there are a few things we can recommend to help:

- Ensure `LogLevel.DEBUG` is configured for the backtesting or live system you are debugging. This is available on `BacktestEngineConfig(log_level="DEBUG")` or `TradingNodeConfig(log_level="DEBUG")`.
  With `DEBUG` mode active you will see more granular and verbose log traces which could be what you need to understand the flow.
- Beyond this, if you still require more granular visibility around a part of the system, we recommend some well-placed calls
  to a components logger (normally `self._log.debug(f"HERE {variable}"` is enough).
