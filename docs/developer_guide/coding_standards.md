# Coding Standards

## Code Style

The current codebase can be used as a guide for formatting conventions.
Additional guidelines are provided below.

### Black

[Black](https://github.com/psf/black) is a PEP-8 compliant opinionated formatter and used during the pre-commit step.

We agree with Black’s style, but Black does not format Cython files. We therefore manually maintain Black-style formatting in Cython code for consistency.

### Formatting

1. For longer lines of code, and when passing more than a couple of arguments, you should take a new line which aligns at the next logical indent (rather than attempting a hanging 'vanity' alignment off an opening parenthesis). This practice conserves space to the right, ensures important code is more central in view, and is also robust to function/method name changes.

2. The closing parenthesis should be located on a new line, aligned at the logical indent.

3. Also ensure multiple hanging parameters or arguments end with a trailing comma:

```python
long_method_with_many_params(
    some_arg1,
    some_arg2,
    some_arg3,  # <-- trailing comma
)
```

### PEP-8

The codebase generally follows the PEP-8 style guide. Even though C typing is taken advantage of in the Cython parts of the codebase, we still aim to be idiomatic of Python where possible.
One notable departure is that Python truthiness is not always taken advantage of to check if an argument is `None` for everything other than collections.

There are two reasons for this;

1. Cython can generate more efficient C code from `is None` and `is not None`, rather than entering the Python runtime to check the `PyObject` truthiness.

2. As per the [Google Python Style Guide](https://google.github.io/styleguide/pyguide.html) - it’s discouraged to use truthiness to check if an argument is/is not `None`, when there is a chance an unexpected object could be passed into the function or method which will yield an unexpected truthiness evaluation (which could result in a logical error type bug).

*“Always use if foo is None: (or is not None) to check for a None value. E.g., when testing whether a variable or argument that defaults to None was set to some other value. The other value might be a value that’s false in a boolean context!”*

There are still areas that aren’t performance-critical where truthiness checks for `None` (`if foo is None:` vs `if not foo:`) will be acceptable for clarity.

:::note
Use truthiness to check for empty collections (e.g., `if not my_list:`) rather than comparing explicitly to `None` or empty.
:::

We welcome all feedback on where the codebase departs from PEP-8 for no apparent reason.

## Python Style Guide

### Type Hints

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

**Test method naming**: Descriptive names explaining the scenario:

```python
def test_currency_with_negative_precision_raises_overflow_error(self):
def test_sma_with_no_inputs_returns_zero_count(self):
def test_sma_with_single_input_returns_expected_value(self):
```

### Ruff

[ruff](https://astral.sh/ruff) is utilized to lint the codebase. Ruff rules can be found in the top-level `pyproject.toml`, with ignore justifications typically commented.

### Commit messages

Here are some guidelines for the style of your commit messages:

1. Limit subject titles to 60 characters or fewer. Capitalize subject line and do not end with period.

2. Use 'imperative voice', i.e. the message should describe what the commit will do if applied.

3. Optional: Use the body to explain change. Separate from subject with a blank line. Keep under 100 character width. You can use bullet points with or without terminating periods.

4. Optional: Provide # references to relevant issues or tickets.

5. Optional: Provide any hyperlinks which are informative.
