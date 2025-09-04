# Testing

The test suite is divided into broad categories of tests including:

- Unit tests
- Integration tests
- Acceptance tests
- Performance tests
- Memory leak tests

The performance tests exist to aid development of performance-critical components.

Tests can be run using [pytest](https://docs.pytest.org), which is our primary test runner.
We recommend using parametrized tests and fixtures (e.g., `@pytest.mark.parametrize`) to avoid repetitive code and improve clarity.

## Running tests

### Python tests

From the repository root:

```bash
make pytest
# or
uv run --active --no-sync pytest --new-first --failed-first
# or simply
pytest
```

For performance tests:

```bash
make test-performance
# or
uv run --active --no-sync pytest tests/performance_tests --benchmark-disable-gc --codspeed
```

### Rust tests

```bash
make cargo-test
# or
cargo nextest run --workspace --features "python,ffi,high-precision,defi" --cargo-profile nextest
```

### IDE integration

- **PyCharm**: Right-click on tests folder or file → "Run pytest"
- **VS Code**: Use the Python Test Explorer extension

## Test style

- Test function naming should be descriptive of what is under test; it is not necessary to include the expected assertions in the function name.
- Test functions *may* have docstrings that can be useful for elaborating on test setup, scenarios, and expected assertions.
- Prefer pytest style free functions for Python tests over test classes with setup methods.
- **Group assertions** where possible - perform all setup/act steps first, then assert expectations together at the end of the test to avoid the *act-assert-act* smell.
- Using `unwrap`, `expect`, or direct `panic!`/`assert` calls inside **tests** is acceptable. The clarity and conciseness of the test suite outweigh defensive error-handling that is required in production code.

## Mocks

Unit tests will often include other components acting as mocks. The intent of this is to simplify
the test suite to avoid extensive use of a mocking framework, although `MagicMock` objects are
currently used in particular cases.

## Code coverage

Code coverage output is generated using `coverage` and reported using [codecov](https://about.codecov.io/).

High test coverage is a goal for the project, however not at the expense of appropriate error handling or causing "test induced damage" to the architecture.

There are currently areas of the codebase which are impossible to test unless there is a change to the production code.
For example, the last condition check of an if-else block which would catch an unrecognized value;
these should be left in place in case there is a change to the production code which these checks could then catch.

Other design-time exceptions may also be impossible to test for, and so 100% test coverage is not the ultimate goal.

## Excluded code coverage

The `pragma: no cover` comments found throughout the codebase [exclude code from test coverage](https://coverage.readthedocs.io/en/coverage-4.3.3/excluding.html).
The reason for their use is to reduce redundant/needless tests just to keep coverage high, such as:

- Asserting an abstract method raises `NotImplementedError` when called.
- Asserting the final condition check of an if-else block when impossible to test (as above).

These tests are expensive to maintain (as they must be kept in line with any refactorings) and offer little to no benefit in return.
The intention is for all abstract method implementations to be fully covered by tests.
Therefore `pragma: no cover` should be judiciously removed when no longer appropriate, and its use *restricted* to the above cases.

## Debugging Rust tests

Rust tests can be debugged using the default test configuration.

If you want to run all tests while compiling with debug symbols for later debugging some tests individually,
run `make cargo-test-debug` instead of `make cargo-test`.

In IntelliJ IDEA, to debug parametrised tests starting with `#[rstest]` with arguments defined in the header of the test,
you need to modify the run configuration of the test so it looks like `test --package nautilus-model --lib data::bar::tests::test_get_time_bar_start::case_1`
(remove `-- --exact` at the end of the string and append `::case_n` where `n` is an integer corresponding to the n-th parametrised test starting at 1).
The reason for this is documented [here](https://github.com/rust-lang/rust-analyzer/issues/8964#issuecomment-871592851) (the test is expanded into a module with several functions named `case_n`).

In VS Code, it is possible to directly select which test case to debug.

## Python + Rust mixed Debugging Guide

This approach allows to debug both Python and Rust code simultaneously from a Jupyter notebook inside VS Code.

### Setup

Install VS Code extensions: Rust Analyzer, CodeLLDB, Python, Jupyter

### Step 0: Compile nautilus_trader with debug symbols

   ```bash
   cd nautilus_trader && make build-debug-pyo3
   ```

### Step 1: Setup Debugging Configuration

```python
from nautilus_trader.test_kit.debug_helpers import setup_debugging

setup_debugging()
```

This creates the necessary VS Code debugging configurations and
starts a debugpy server the Python debugger can connect to.

Note: by default the .vscode folder containing the debugging configurations
is assumed to be one folder above the `nautilus_trader` root directory.
You can adjust this if needed.

### Step 2: Set Breakpoints

- **Python breakpoints:** Set in VS Code in the Python source files.
- **Rust breakpoints:** Set in VS Code in the Rust source files.

### Step 3: Start Mixed Debugging

1. In VS Code: Select **"Debug Jupyter + Rust (Mixed)"** configuration.
2. Start debugging (F5) or press the right arrow green button.
3. Both Python and Rust debuggers will attach to your Jupyter session.

### Step 4: Execute Code

Run your Jupyter notebook cells that call rust functions. The debugger will stop at breakpoints in both Python and Rust code.

### Available Configurations

`setup_debugging()` creates these VS Code configurations:

- **`Debug Jupyter + Rust (Mixed)`** - Mixed debugging for Jupyter notebooks.
- **`Jupyter Mixed Debugging (Python)`** - Python-only debugging for notebooks.
- **`Rust Debugger (for jupyter debugging)`** - Rust-only debugging for notebooks.

### Example

Open and run the example notebook: `debug_mixed_jupyter.ipynb`

### Reference

- [PyO3 debugging](https://pyo3.rs/v0.25.1/debugging.html?highlight=deb#debugging-from-jupyter-notebooks)
