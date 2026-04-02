# Python

The [Python](https://www.python.org/) programming language is used for the majority of user-facing code in NautilusTrader.
Python provides a rich ecosystem of libraries and frameworks, making it ideal for strategy development, data analysis, and system integration.

## Code style

### PEP-8

The codebase generally follows the PEP-8 style guide.
One notable departure is that Python truthiness is not always taken advantage of to check if an argument is `None` for everything other than collections.

As per the [Google Python Style Guide](https://google.github.io/styleguide/pyguide.html), it's discouraged to use truthiness to check if an argument is/is not `None`, when there is a chance an unexpected object could be passed into the function or method which will yield an unexpected truthiness evaluation (which could result in a logical error type bug).

*"Always use if foo is None: (or is not None) to check for a None value. E.g., when testing whether a variable or argument that defaults to None was set to some other value. The other value might be a value that's false in a boolean context!"*

:::note
Use truthiness to check for empty collections (e.g., `if not my_list:`) rather than comparing explicitly to `None` or empty.
:::

We welcome all feedback on where the codebase departs from PEP-8 for no apparent reason.

### Type hints

All function and method signatures *must* include type annotations:

```python
def __init__(self, config: EMACrossConfig) -> None:
def on_bar(self, bar: Bar) -> None:
def on_save(self) -> dict[str, bytes]:
def on_load(self, state: dict[str, bytes]) -> None:
```

**Union syntax**: Use PEP 604 union syntax for optional types:

```python
# Preferred
def get_instrument(self, id: InstrumentId) -> Instrument | None:

# Avoid
def get_instrument(self, id: InstrumentId) -> Optional[Instrument]:
```

**Generic types**: Use `TypeVar` for reusable components:

```python
T = TypeVar("T")
class ThrottledEnqueuer(Generic[T]):
```

### Docstrings

The [NumPy docstring spec](https://numpydoc.readthedocs.io/en/latest/format.html) is used throughout the codebase.
This needs to be followed consistently so the docs build correctly.

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

### Properties vs methods (PyO3 bindings)

When exposing Rust types to Python via PyO3, use `#[getter]` (property) or a plain
method based on what the call site communicates, not whether the value can change:

- **Property (`#[getter]`):** cheap, side-effect-free, attribute-like view of current
  state. Scalar fields, predicates, and lightweight derived values belong here even if
  they change over the object's lifetime.
  Examples: `status`, `side`, `quantity`, `price`, `is_open`, `has_inputs`,
  `realized_pnl`, `venue_order_id`.
- **Method (no `#[getter]`):** actions, mutations, nontrivial work, allocations/copies,
  I/O, or anything that takes arguments.
  Examples: `apply(fill)`, `unrealized_pnl(price)`, `calculate_pnl(...)`.
- **Gray area (prefer method):** getters that clone or allocate a collection each call.
  Using a method signals the cost to the caller.
  Examples: `events()`, `adjustments()`, `client_order_ids()`, `trade_ids()`.

### Test naming

Descriptive names explaining the scenario:

```python
def test_currency_with_negative_precision_raises_overflow_error(self):
def test_sma_with_no_inputs_returns_zero_count(self):
def test_sma_with_single_input_returns_expected_value(self):
```

### Ruff

[ruff](https://astral.sh/ruff) is used to lint the codebase. Ruff rules can be found in the top-level `pyproject.toml`, with ignore justifications typically commented.

## Cython (legacy)

:::note
This section covers Cython conventions for `.pyx` and `.pxd` files.
:::

For `.pyx` and `.pxd` files, make sure all functions and methods returning `void` or a primitive C type (such as `bint`, `int`, `double`) include the `except *` keyword in the signature. Without it, Python exceptions are silently ignored.

For more information, see the [Cython docs](https://cython.readthedocs.io/en/latest/index.html).
