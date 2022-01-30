# Cython

Here you will find guidance and tips for working on NautilusTrader using the Cython language.
More information on Cython syntax and conventions can be found by reading the [Cython docs](https://cython.readthedocs.io/en/latest/index.html).

## Function and method signatures
Ensure that all functions and methods returning `void` or a primitive C type (such as `bint`, `int`, `double`) include the `except *` keyword in the signature.

This will ensure Python exceptions are not ignored, but instead are *'bubbled up'* to the caller as expected.

## Debugging

### IDE Support
For PyCharm, there has been a highly up-voted feature request for better debugging support for Cython.
Unfortunately, it hasn't received any traction for nearly a decade. So it's safe to assume that for
PyCharm at least, it will not be receiving first class support for Cython debugging
https://youtrack.jetbrains.com/issue/PY-9476.

For VS Code, there are some Cython extensions, however these are fairly unmaintained compared to
PyCharm Professional. Also, to our knowledge they don't contain any additional debugging support.

The following debugging recommendations are contained in the Cython docs:
https://cython.readthedocs.io/en/latest/src/userguide/debugging.html.

The summary of which, is to manually run a specialized version of `gdb` from the command line. We don't recommend this workflow.
If any users have figured out a better way to debug Cython, then please let us know!

### Tips
When debugging and seeking to understand a complex system such as NautilusTrader, it can be
quite helpful to step through the code with a debugger. However, with this not being available in a reasonable way
for the Cython part of the codebase - there are a few things we can recommend to help:

- Ensure `LogLevel.DEBUG` is configured for the backtesting or live system you are debugging. This is available on `BacktestEngineConfig(log_level="DEBUG")` or `TradingNodeConfig(log_level="DEBUG")`.
  With `DEBUG` mode active you will see more granular and verbose log traces which could be what you need to understand the flow.
- Beyond this, if you still require more granular visibility around a part of the system, we recommend some well-placed calls
  to a components logger (normally `self._log.debug(f"HERE {variable}"` is enough). You would then need to recompile.
