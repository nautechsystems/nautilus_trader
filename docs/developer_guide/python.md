# Python

[Python](https://www.python.org/) provides most user-facing code in NautilusTrader. Its ecosystem of
libraries and frameworks supports strategy development, data analysis, and system integration.

## Code style

### PEP-8

The codebase generally follows PEP-8, but does not always use truthiness to check non-collection
values for `None`.

The [Google Python Style Guide](https://google.github.io/styleguide/pyguide.html) discourages truthiness
for `None` checks when a function or method might receive an unexpected object. Its truthiness can
produce an unintended result and cause a logic error:

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

**Generic types**: Use Python 3.12 type parameter syntax for reusable functions and classes:

```python
def first[T](values: list[T]) -> T:
    return values[0]
```

### Docstrings

Follow the [NumPy docstring spec](https://numpydoc.readthedocs.io/en/latest/format.html) consistently
so the docs build correctly.

Write Python docstrings in the **imperative mood**, for example, *"Return a cached client."* This
matches the prevailing Python ecosystem style and makes generated documentation natural for end-users.

#### Private methods

Do not add docstrings to private methods (prefixed with `_`). Docstrings generate public-facing API
documentation and incorrectly imply that these implementation details are part of the public API
for end-users.

Exceptions where docstrings are acceptable:

- Very complex methods with non-trivial logic, multiple steps, or important edge cases.
- Methods requiring detailed parameter or return value documentation due to complexity.

If a private method needs context (such as a tricky precondition or side effect), prefer a short inline
comment (`#`). Place it near the relevant logic. Prefer this comment to a docstring.

### Properties vs methods (PyO3 bindings)

To expose Rust types to Python through PyO3, select `#[getter]` (property) or a plain method. Base the choice
on what the call site communicates, not whether the value can change:

- **Property (`#[getter]`)**: cheap, side-effect-free, attribute-like view of current
  state. Scalar fields, predicates, and lightweight derived values belong here even if
  they change over the object's lifetime.
  Examples: `status`, `side`, `quantity`, `price`, `is_open`, `has_inputs`,
  `realized_pnl`, `venue_order_id`.
- **Method (no `#[getter]`)**: actions, mutations, nontrivial work, allocations/copies,
  I/O, or anything that takes arguments.
  Examples: `apply(fill)`, `unrealized_pnl(price)`, `calculate_pnl(...)`.
- **Gray area (prefer method)**: getters that clone or allocate a collection each call.
  Using a method signals the cost to the caller.
  Examples: `events()`, `adjustments()`, `client_order_ids()`, `trade_ids()`.

## Python live callback routing

Python live nodes keep one runtime invariant: Tokio worker threads do not run
Python code during live trading.

`LiveNode::py_run` releases the GIL while the Rust async runtime runs. Worker-side
work that must trigger Python uses existing live runner event channels instead
of calling `Python::attach` on the worker. Timer callbacks use the time-event
channel. The runner drains that channel during startup buffering and the main
select loop, then executes callbacks on the live event loop thread.

This path handles unavoidable user Python callbacks. Do not move adapter, provider, data, or execution logic
into Python to use it. Python adapter modules configure Rust adapters and register factories. Rust owns
adapter operations.

If worker-side Rust work needs a Python callback, route it through a specific live runner event type.

When adding Python-aware live code:

- Prefer an existing runner event channel.
- Keep callback bodies short because they run synchronously on the live event loop.
- Do not call `Python::attach` from Tokio worker tasks in Python live trading.
- Do not add adapter business logic in Python to fit callback routing.

### Test naming

Use descriptive names that explain the scenario. Keep tests as annotated pytest free functions:

```python
def test_write_and_query_option_greeks_round_trip() -> None: ...


def test_catalog_loaded_greeks_reach_on_option_greeks() -> None: ...


def test_streaming_feather_writer_rejects_unregistered_custom_data() -> None: ...
```

### Ruff

[Ruff](https://astral.sh/ruff) is used to lint the codebase. Its rules are configured in
`python/pyproject.toml`, with ignore justifications typically commented.
