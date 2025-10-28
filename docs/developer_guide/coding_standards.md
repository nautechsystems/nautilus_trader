# Coding Standards

## Code Style

The current codebase can be used as a guide for formatting conventions.
Additional guidelines are provided below.

### Universal formatting rules

The following applies to **all** source files (Rust, Python, Cython, shell, etc.):

- Use **spaces only**, never hard tab characters.
- Lines should generally stay below **100 characters**; wrap thoughtfully when necessary.
- Prefer American English spelling (`color`, `serialize`, `behavior`).

### Comment conventions

1. Generally leave **one blank line above** every comment block or docstring so it is visually separated from code.
2. Use *sentence case* – capitalize the first letter, keep the rest lowercase unless proper nouns or acronyms.
3. Do not use double spaces after periods.
4. **Single-line comments** *must not* end with a period *unless* the line ends with a URL or inline Markdown link – in those cases leave the punctuation exactly as the link requires.
5. **Multi-line comments** should separate sentences with commas (not period-per-line). The final line *should* end with a period.
6. Keep comments concise; favor clarity and only explain the non-obvious – *less is more*.
7. Avoid emoji symbols in text.

### Doc comment / docstring mood

- **Python** docstrings should be written in the **imperative mood** – e.g. *"Return a cached client."*
- **Rust** doc comments should be written in the **indicative mood** – e.g. *"Returns a cached client."*

These conventions align with the prevailing styles of each language ecosystem and make generated
documentation feel natural to end-users.

### Terminology and phrasing

1. **Error messages**: Avoid using ", got" in error messages. Use more descriptive alternatives like ", was", ", received", or ", found" depending on context.
   - ❌ `"Expected string, got {type(value)}"`
   - ✅ `"Expected string, was {type(value)}"`

2. **Spelling**: Use "hardcoded" (single word) rather than "hard-coded" or "hard coded" – this is the more modern and accepted spelling.

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

## Python style guide

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

#### Gitlint (optional)

Gitlint is available to help enforce commit message standards automatically. It checks that commit messages follow the guidelines above (character limits, formatting, etc.). This is **opt-in** and not enforced in CI.

**Benefits**: Encourages concise yet expressive commit messages, helps develop clear explanations of changes.

**Installation**: First install gitlint to run it locally:

```bash
uv pip install gitlint
```

To enable gitlint as an automatic commit-msg hook:

```bash
pre-commit install --hook-type commit-msg
```

**Manual usage**: Check your last commit message:

```bash
gitlint
```

Configuration is in `.gitlint` at the repository root:

- **60-character title limit**: Ensures clear rendering on GitHub and encourages brevity while remaining descriptive.
- **79-character body width**: Aligns with Python's PEP 8 conventions and the traditional limit for git tooling.

:::note
Gitlint may be enforced in CI in the future, so adopting these practices early helps ensure a smooth transition.
:::
