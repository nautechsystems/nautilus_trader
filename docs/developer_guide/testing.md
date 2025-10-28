# Testing

Our automated tests serve as executable specifications for the trading platform.
A healthy suite documents intended behaviour, gives contributors confidence to refactor, and catches regressions before they reach production.
Tests also double as living examples that clarify complex flows and provide rapid CI feedback so issues surface early.

The suite covers these categories:

- Unit tests
- Integration tests
- Acceptance tests
- Performance tests
- Memory leak tests

Performance tests help evolve performance-critical components.

Run tests with [pytest](https://docs.pytest.org), our primary test runner.
Use parametrized tests and fixtures (e.g., `@pytest.mark.parametrize`) to avoid repetitive code and improve clarity.

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

- **PyCharm**: Right-click the tests folder or file → "Run pytest".
- **VS Code**: Use the Python Test Explorer extension.

## Test style

- Name test functions after what they exercise; you do not need to encode the expected assertions in the name.
- Add docstrings when they clarify setup, scenarios, or expectations.
- Prefer pytest-style free functions for Python tests instead of test classes with setup methods.
- **Group assertions** when possible: perform all setup/act steps first, then assert together to avoid the act-assert-act smell.
- Use `unwrap`, `expect`, or direct `panic!`/`assert` calls inside tests; clarity and conciseness matter more than defensive error handling here.

## Waiting for asynchronous effects

When waiting for background work to complete, prefer the polling helpers `await eventually(...)` from `nautilus_trader.test_kit.functions` and `wait_until_async(...)` from `nautilus_common::testing` instead of arbitrary sleeps. They surface failures faster and reduce flakiness in CI because they stop as soon as the condition is satisfied or time out with a useful error.

## Mocks

Use lightweight collaborators as mocks to keep the suite simple and avoid heavy mocking frameworks.
We still rely on `MagicMock` in specific cases where it provides the most convenient tooling.

## Code coverage

We generate coverage reports with `coverage` and publish them to [codecov](https://about.codecov.io/).

Aim for high coverage without sacrificing appropriate error handling or causing "test induced damage" to the architecture.

Some branches remain untestable without modifying production behaviour.
For example, a final condition in a defensive if-else block may only trigger for unexpected values; leave these checks in place so future changes can exercise them if needed.

Design-time exceptions can also be impractical to test, so 100% coverage is not the target.

## Excluded code coverage

We use `pragma: no cover` comments to [exclude code from coverage](https://coverage.readthedocs.io/en/coverage-4.3.3/excluding.html) when tests would be redundant.
Typical examples include:

- Asserting an abstract method raises `NotImplementedError` when called.
- Asserting the final condition check of an if-else block when impossible to test (as above).

Such tests are expensive to maintain because they must track refactors while providing little value.
Ensure concrete implementations of abstract methods remain fully covered.
Remove `pragma: no cover` when it no longer applies and restrict its use to the cases above.

## Debugging Rust tests

Use the default test configuration to debug Rust tests.

To run the full suite with debug symbols for later, run `make cargo-test-debug` instead of `make cargo-test`.

In IntelliJ IDEA, adjust the run configuration for parametrised `#[rstest]` cases so it reads `test --package nautilus-model --lib data::bar::tests::test_get_time_bar_start::case_1`
(remove `-- --exact` and append `::case_n` where `n` starts at 1). This workaround matches the behaviour explained [here](https://github.com/rust-lang/rust-analyzer/issues/8964#issuecomment-871592851).

In VS Code you can pick the specific test case to debug directly.

## Python + Rust Mixed Debugging

This workflow lets you debug Python and Rust code simultaneously from a Jupyter notebook inside VS Code.

### Setup

Install these VS Code extensions: Rust Analyzer, CodeLLDB, Python, Jupyter.

### Step 0: Compile `nautilus_trader` with debug symbols

   ```bash
   cd nautilus_trader && make build-debug-pyo3
   ```

### Step 1: Set up debugging configuration

```python
from nautilus_trader.test_kit.debug_helpers import setup_debugging

setup_debugging()
```

This command creates the required VS Code debugging configurations and starts a `debugpy` server for the Python debugger.

By default `setup_debugging()` expects the `.vscode` folder one level above the `nautilus_trader` root directory.
Adjust the target location if your workspace layout differs.

### Step 2: Set breakpoints

- **Python breakpoints:** Set in VS Code in the Python source files.
- **Rust breakpoints:** Set in VS Code in the Rust source files.

### Step 3: Start mixed debugging

1. In VS Code select the **"Debug Jupyter + Rust (Mixed)"** configuration.
2. Start debugging (F5) or press the green run arrow.
3. Both Python and Rust debuggers attach to your Jupyter session.

### Step 4: Execute code

Run Jupyter notebook cells that call Rust functions. The debugger stops at breakpoints in both Python and Rust code.

### Available configurations

`setup_debugging()` creates these VS Code configurations:

- **`Debug Jupyter + Rust (Mixed)`** - Mixed debugging for Jupyter notebooks.
- **`Jupyter Mixed Debugging (Python)`** - Python-only debugging for notebooks.
- **`Rust Debugger (for Jupyter debugging)`** - Rust-only debugging for notebooks.

### Example

Open and run the example notebook: `debug_mixed_jupyter.ipynb`.

### Reference

- [PyO3 debugging](https://pyo3.rs/v0.25.1/debugging.html?highlight=deb#debugging-from-jupyter-notebooks)
