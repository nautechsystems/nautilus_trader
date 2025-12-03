# Python

The [Python](https://www.python.org/) programming language is used for the majority of user-facing code in NautilusTrader.
Python provides a rich ecosystem of libraries and frameworks, making it ideal for strategy development, data analysis, and system integration.

## Code style

### PEP-8

The codebase generally follows the PEP-8 style guide. Even though C typing is taken advantage of in the Cython parts of the codebase, we still aim to be idiomatic of Python where possible.
One notable departure is that Python truthiness is not always taken advantage of to check if an argument is `None` for everything other than collections.

There are two reasons for this:

1. Cython can generate more efficient C code from `is None` and `is not None`, rather than entering the Python runtime to check the `PyObject` truthiness.

2. As per the [Google Python Style Guide](https://google.github.io/styleguide/pyguide.html) - it's discouraged to use truthiness to check if an argument is/is not `None`, when there is a chance an unexpected object could be passed into the function or method which will yield an unexpected truthiness evaluation (which could result in a logical error type bug).

*"Always use if foo is None: (or is not None) to check for a None value. E.g., when testing whether a variable or argument that defaults to None was set to some other value. The other value might be a value that's false in a boolean context!"*

There are still areas that aren't performance-critical where truthiness checks for `None` (`if foo is None:` vs `if not foo:`) will be acceptable for clarity.

:::note
Use truthiness to check for empty collections (e.g., `if not my_list:`) rather than comparing explicitly to `None` or empty.
:::

We welcome all feedback on where the codebase departs from PEP-8 for no apparent reason.

### Type hints

All function and method signatures *must* include comprehensive type annotations:

```python
def __init__(self, config: EMACrossConfig) -> None:
def on_bar(self, bar: Bar) -> None:
def on_save(self) -> dict[str, bytes]:
def on_load(self, state: dict[str, bytes]) -> None:
```

**Generic Types**: Use `TypeVar` for reusable components

```python
T = TypeVar("T")
class ThrottledEnqueuer(Generic[T]):
```

### Docstrings

The [NumPy docstring spec](https://numpydoc.readthedocs.io/en/latest/format.html) is used throughout the codebase.
This needs to be adhered to consistently to ensure the docs build correctly.

**Python** docstrings should be written in the **imperative mood** – e.g. *"Return a cached client."*

This convention aligns with the prevailing style of the Python ecosystem and makes generated
documentation feel natural to end-users.

#### Private methods

Do not add docstrings to private methods (prefixed with `_`):

- Docstrings generate public-facing API documentation.
- Docstrings on private methods incorrectly imply they are part of the public API.
- Private methods are implementation details not intended for end-users.

Exceptions where docstrings are acceptable:

- Very complex methods with non-trivial logic, multiple steps, or important edge cases.
- Methods requiring detailed parameter or return value documentation due to complexity.

When a private method needs context (such as a tricky precondition or side effect), prefer a short inline comment (`#`) near the relevant logic rather than a docstring.

### Test naming

Descriptive names explaining the scenario:

```python
def test_currency_with_negative_precision_raises_overflow_error(self):
def test_sma_with_no_inputs_returns_zero_count(self):
def test_sma_with_single_input_returns_expected_value(self):
```

### Ruff

[ruff](https://astral.sh/ruff) is utilized to lint the codebase. Ruff rules can be found in the top-level `pyproject.toml`, with ignore justifications typically commented.

## Cython

:::warning[Deprecation notice]
Cython is being phased out in favor of Rust implementations. This section will be removed in a future version.
:::

Here you will find guidance and tips for working on NautilusTrader using the Cython language.
More information on Cython syntax and conventions can be found by reading the [Cython docs](https://cython.readthedocs.io/en/latest/index.html).

### What is Cython?

Cython is a superset of Python that compiles to C extension modules, enabling optional static typing and optimized performance. NautilusTrader historically relied on Cython for its Python bindings and performance-critical components.

### Function and method signatures

Ensure that all functions and methods returning `void` or a primitive C type (such as `bint`, `int`, `double`) include the `except *` keyword in the signature.

This will ensure Python exceptions are not ignored, and instead are "bubbled up" to the caller as expected.

### Debugging

#### PyCharm

Improved debugging support for Cython has remained a highly up-voted PyCharm
feature for many years. Unfortunately, it's safe to assume that PyCharm will not
be receiving first class support for Cython debugging
<https://youtrack.jetbrains.com/issue/PY-9476>.

#### Cython docs

The following recommendations are contained in the Cython docs:
<https://cython.readthedocs.io/en/latest/src/userguide/debugging.html>

The summary is it involves manually running a specialized version of `gdb` from the command line.
We don't recommend this workflow.

#### Tips

When debugging and seeking to understand a complex system such as NautilusTrader, it can be
quite helpful to step through the code with a debugger. With this not being available
for the Cython part of the codebase, there are a few things which can help:

- Ensure `LogLevel.DEBUG` is configured for the backtesting or live system you are debugging.
  This is available on `BacktestEngineConfig(logging=LoggingConfig(log_level="DEBUG"))` or `TradingNodeConfig(logging=LoggingConfig=log_level="DEBUG"))`.
  With `DEBUG` mode active you will see more granular and verbose log traces which could be what you need to understand the flow.
- Beyond this, if you still require more granular visibility around a part of the system, we recommend some well-placed calls
  to a components logger (normally `self._log.debug(f"HERE {variable}"` is enough).
