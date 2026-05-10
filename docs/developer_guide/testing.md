# Testing

Our automated tests serve as executable specifications for the trading platform.
A healthy suite documents intended behaviour, gives contributors confidence to refactor, and catches regressions before they reach production.
Tests also double as living examples that clarify complex flows and provide rapid CI feedback so issues surface early.

The suite covers these categories:

- Unit tests
- Integration tests
- Acceptance tests
- Performance tests
- Property-based tests
- Fuzzing
- Memory leak tests

## Testing policy

Tests and runtime contracts form one design system. The
[Design by contract](rust.md#design-by-contract) ladder pushes invariants into the type
system where possible; the testing ladder below escalates the remaining unknowns through
larger input spaces and richer execution models. Each layer extends coverage to inputs
or execution states the layer below cannot reach.

Not every module requires every technique. Use this section to decide which layers apply
before adding tests or `debug_assert!` statements.

### Mechanism ladder

Runtime contracts are covered in the [Rust guide](rust.md#design-by-contract): prefer the
type system first, then `check_*` from `nautilus_core::correctness` at API boundaries,
then `debug_assert!` for internal invariants, then `assert!` for soundness-critical or
always-on checks.

Test layers follow a parallel escalation. Start at the lowest layer that proves what
matters; climb only when the layer below stops detecting regressions or when the input
space grows beyond hand-picked cases.

| Layer                    | Trigger condition                                                               |
|--------------------------|---------------------------------------------------------------------------------|
| Unit test                | A single function or transition has a small, enumerable set of cases.           |
| Parametrized test        | The same shape repeats across discrete inputs (order side, status, instrument). |
| Property‑based test      | An invariant must hold for a whole class of inputs the mind cannot enumerate.   |
| Integration test         | Multiple modules interact through a real (non‑mocked) engine or runtime.        |
| Fuzz test                | Untrusted or adversarial bytes cross a parser, decoder, or wire‑format handler. |
| Spec acceptance test     | Behaviour depends on a live venue contract (see `spec_exec_testing.md`).        |
| Deterministic simulation | Correctness depends on task scheduling, timeouts, or wall‑clock ordering.       |
| Formal verification      | A pure function has crisp invariants and a bounded input space worth a proof.   |

The formal verification rung is aspirational: no Kani or Prusti harness has landed in
the workspace. The row records the escalation condition for when a verifier is adopted,
not a current obligation.

### Projection rule

Module shape determines which layers pay off. Not every module warrants the full ladder.
Apply the rule at module granularity, not crate granularity: an adapter crate contains
pure parsers and I/O-bound client loops, and each row applies to a different part.

| Module shape                        | Layers that apply                             | Example                                |
|-------------------------------------|-----------------------------------------------|----------------------------------------|
| Pure function, crisp invariants     | Unit, parametrized, property, fuzz            | Reconciliation kernels, portfolio math |
| Pure function, no stated invariants | Unit, parametrized, property, fuzz            | Codecs, adapter parsers, formatters    |
| Stateful, synchronous               | Unit, parametrized, property over transitions | Cache, order book                      |
| Stateful, async                     | Unit, integration, deterministic simulation   | Live engine, execution manager         |
| I/O bound, venue contract           | Integration, spec acceptance, boundary fuzz   | Adapter client loops                   |

### When not to add coverage

- Add `debug_assert!` only where a test can reach it. Release builds strip the check, so
  an unexercised assertion has no signal. A targeted unit test counts as a harness; a
  proptest or fuzz harness amplifies the signal.
- Prefer a proptest over a hand-written edge-case test when the invariant spans a whole
  class of inputs. Targeted unit tests remain valid for known venue pathologies and as
  regression reproducers for shrunk counterexamples.
- Do not duplicate a live spec acceptance card as an integration test. Link to it instead.
- Do not pad coverage with tests that assert language or framework guarantees
  (`Option::is_some` after `Some(..)`, `Vec::len` after `push`).

### DST readiness

Deterministic simulation testing (DST) requires the runtime to be free of ambient
non-determinism. Before promoting a module to run under DST, verify the following:

- Time, task, runtime, and signal primitives route through `nautilus_common::live::dst`
  rather than `tokio` directly. Wall-clock reads go through the seam in
  `nautilus_core::time` rather than `SystemTime::now()` at call sites.
- State maps with ordering-dependent iteration use `IndexMap` or `IndexSet`, not the
  default hash collections.
- Every `tokio::select!` on a control-plane path sets `biased` so poll order is fixed.
- No calls to `Instant::now()`, `SystemTime::now()`, `tokio::signal::ctrl_c`,
  `std::thread::spawn`, or `tokio::task::spawn_blocking` escape the seam. Blocking-thread
  and OS-thread primitives break madsim determinism the same way an ambient clock read
  does.
- Replay-sensitive IDs (`trade_id`, `venue_order_id`) are pure functions of their inputs;
  see `crates/execution/src/reconciliation/ids.rs`. Ephemeral event UUIDs on other
  reconciliation paths do not need to be deterministic.

The `surface` probe in `crates/common/src/live/dst.rs` only pins the re-export shape;
it does not check that callers actually use the seam. Enforcement is by review. Run the
audit whenever a new async module enters the workspace or an existing module gains new
control-plane scheduling.

## Property-based testing

Property testing verifies that logic holds for *all* valid inputs, not just hand-picked examples.
We use [`proptest`](https://altsysrq.github.io/proptest-book/intro.html) in Rust to enforce invariants.

- **Use cases:** Core domain types (`Price`, `Quantity`, `UnixNanos`), accounting engines, matching engines, and state machines.
- **Example invariants:**
  - Round-trip serialization: `parse(to_string(value)) == value`
  - Inverse operations: `(A + B) - B == A`
  - Transitivity: `If A < B and B < C, then A < C`

## Fuzzing

Fuzzing introduces unstructured or malicious data to the system to verify it fails gracefully.

- **Use cases:** Network boundaries, exchange data parsers (JSON, FIX, WebSocket feeds), and complex state machines.
- **Goal:** The system returns a `Result::Err` and never panics, hangs, or leaks memory when encountering malformed data.

When building or modifying core types, write property tests to cover the mathematical boundaries.

Performance tests help evolve performance-critical components.

Run tests with [pytest](https://docs.pytest.org), our primary test runner.
Use parametrized tests and fixtures (e.g., `@pytest.mark.parametrize`) to avoid repetitive code and improve clarity.

## Running tests

### v1 legacy Python tests

The v1 legacy test suite lives under `tests/` at the repository root and tests
the Cython-based package. From the repository root:

```bash
make pytest
# or
uv run --active --no-sync pytest --new-first --failed-first
```

### Python tests

The Python test suite lives under `python/tests/` and tests the Rust-backed PyO3
package. It requires a built extension module (`make build-debug-v2`) and uses its
own virtualenv under `python/.venv/`.

For new live adapter examples and docs in the v2 path, prefer
`nautilus_trader.live.LiveNode`. `nautilus_trader.live.node.TradingNode` remains the
legacy v1/Cython runtime used by the root-level `tests/` suite and older examples.

```bash
make pytest-v2
```

The Makefile target isolates certain test modules in separate pytest processes to avoid
global Rust state conflicts. Use `make pytest-v2` rather than invoking pytest directly.

Local `make pytest-v2` runs use the debug extension from `make build-debug-v2`.
CI `build-v2` tests a release wheel.
Do not write `python/tests/` cases that probe Rust panic paths in process with
`pytest.raises(BaseException)` or similar broad catches.
Those tests can appear to pass against the debug build and abort the interpreter against the
release wheel.
For abort-prone PyO3 or FFI methods, verify the Python signature and parameter names, or isolate
the call in a subprocess.

For performance tests:

```bash
make test-performance
# or
uv run --active --no-sync pytest tests/performance_tests --benchmark-disable-gc --codspeed
```

The `--benchmark-disable-gc` flag prevents garbage collection from skewing results. Run performance tests in isolation (not with unit tests) to avoid interference.

### Rust tests

```bash
make cargo-test
# or
cargo nextest run --workspace --features "python,ffi,high-precision,defi" --cargo-profile nextest
```

#### Testing with optional features

Use `EXTRA_FEATURES` to include optional features like `capnp` or `hypersync`:

```bash
# Test with capnp feature
make cargo-test EXTRA_FEATURES="capnp"

# Test with multiple features
make cargo-test EXTRA_FEATURES="capnp hypersync"

# Legacy shorthand for hypersync
make cargo-test HYPERSYNC=true

# Test specific crate with features
make cargo-test-crate-nautilus-serialization FEATURES="capnp"
```

### IDE integration

- **PyCharm**: Right-click the tests folder or file → "Run pytest".
- **VS Code**: Use the Python Test Explorer extension.

## Test style

### General

- Name test functions after what they exercise; you do not need to encode the expected
  assertions in the name.
- Add docstrings when they clarify setup, scenarios, or expectations.
- **Group assertions** when possible: perform all setup/act steps first, then assert
  together to avoid the act-assert-act smell.
- Use `unwrap`, `expect`, or direct `panic!`/`assert` calls inside tests; clarity and
  conciseness matter more than defensive error handling here.
- Do not capture log output to assert on log messages. Log capture in tests is fragile
  because loggers are global state, test execution order is non-deterministic, and the
  assertions break when log wording changes. Instead, verify the observable behavior
  (return values, state changes, side effects) that the log message reflects.

### Python tests (`python/tests/`)

Use **pytest-style free functions and fixtures**. Do not use test classes.

- Write each test as a standalone `def test_*()` function.
- Use `@pytest.fixture` for shared setup (instruments, engine instances, data).
  Prefer `yield` fixtures when teardown is needed (e.g., `engine.dispose()`).
- Use `@pytest.mark.parametrize` to cover multiple inputs without duplicating
  test bodies.
- Import model types from `nautilus_trader.model`, not from
  `nautilus_trader.core.nautilus_pyo3`.
- Test providers live in `python/tests/providers.py`. Use `TestInstrumentProvider`
  and `TestDataProvider` for common instruments and data.
- Mark tests that depend on unfinished features with
  `@pytest.mark.skip(reason="WIP: <description>")` rather than deleting them.

### v1 legacy Python tests (`tests/`)

The v1 legacy test suite uses a mix of test classes and free functions. New tests
added to this suite may follow either pattern, but free functions with fixtures
are preferred for new files.

### Rust

For Rust-specific test conventions (module structure, `#[rstest]`, parameterization),
see the [Rust guide](rust.md#testing-conventions).

## Waiting for asynchronous effects

When waiting for background work to complete, prefer the polling helpers `await eventually(...)` from `nautilus_trader.test_kit.functions` and `wait_until_async(...)` from `nautilus_common::testing` instead of arbitrary sleeps. They surface failures faster and reduce flakiness in CI because they stop as soon as the condition is satisfied or time out with a useful error.

## Mocks

Prefer hand-written stubs that return fixed values over mocking frameworks. Use `MagicMock` only when you need to assert call counts/arguments or simulate complex state changes. Avoid mocking the objects you're actually testing.

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
Keep concrete implementations of abstract methods fully covered.
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

## Data type testing

Each data type flows through multiple layers of the platform. The table below shows where
existing types are tested, so new types can follow the same pattern.

### Test layer matrix

| Layer                  | Location                                    | What it covers                                             |
|------------------------|---------------------------------------------|------------------------------------------------------------|
| DataEngine subscribe   | `crates/data/tests/engine.rs`               | Engine processes subscribe/unsubscribe commands correctly. |
| DataEngine publish     | `crates/data/tests/engine.rs`               | Engine routes published data to the message bus.           |
| DataActor subscribe    | `crates/common/src/actor/tests.rs`          | Actor subscribes and receives data via typed publish.      |
| DataActor unsubscribe  | `crates/common/src/actor/tests.rs`          | Actor stops receiving data after unsubscribe.              |
| PyO3 actor dispatch    | `crates/common/src/python/actor.rs`         | Rust handler dispatches to Python `on_*` method.           |
| Python Actor subscribe | `tests/unit_tests/common/test_actor.py`     | Python actor subscribes; command count increments.         |
| Python Actor unsub     | `tests/unit_tests/common/test_actor.py`     | Python actor unsubscribes; subscription list clears.       |
| Backtest client        | `nautilus_trader/backtest/data_client.pyx`  | Backtest client overrides base subscribe/unsubscribe.      |
| Adapter live tests     | `docs/developer_guide/spec_data_testing.md` | Live data acceptance tests (DataTester).                   |

### Coverage per data type

The following table shows which layers have test coverage for each data type.
Use this as a checklist when adding a new type.

| Data type           | Engine | Actor (Rust) | PyO3 dispatch | Actor (Python) | Backtest client | Adapter spec |
|---------------------|--------|--------------|---------------|----------------|-----------------|--------------|
| `InstrumentAny`     | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `OrderBookDeltas`   | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `OrderBook`         | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `QuoteTick`         | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `TradeTick`         | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `Bar`               | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `MarkPriceUpdate`   | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `IndexPriceUpdate`  | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `FundingRateUpdate` | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `InstrumentStatus`  | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `InstrumentClose`   | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `OptionGreeks`      | ✓      | ✓            | ✓             | ✓              | ✓               | ✓            |
| `OptionChainSlice`  | -      | ✓            | ✓             | ✓              | -               | ✓            |
| `CustomData`        | ✓      | ✓            | ✓             | ✓              | ✓               | -            |

`OptionChainSlice` is assembled by the DataEngine's `OptionChainManager` from per-instrument
greeks and quote subscriptions. It does not have its own engine subscribe command or
backtest client override.

### Adding a new data type

When introducing a new data type, add tests at each layer:

1. **DataEngine** (`crates/data/tests/engine.rs`): Add `test_execute_subscribe_<type>` and
   `test_execute_unsubscribe_<type>` tests. Follow the pattern in existing subscribe tests:
   register client, build command, call `engine.execute`, assert subscription list.

2. **DataActor Rust** (`crates/common/src/actor/tests.rs`):
   - Add `received_<type>: Vec<Type>` field to `TestDataActor`.
   - Implement the `on_<type>` handler in the `DataActor` trait impl.
   - Add `test_subscribe_and_receive_<type>` and `test_unsubscribe_<type>` tests.
   - Use the typed publish function (`msgbus::publish_<type>`), not `publish_any`,
     for types that use `TypedHandler` routing.

3. **PyO3 actor dispatch** (`crates/common/src/python/actor.rs`):
   - Add `dispatch_on_<type>` method that calls `py_self.call_method1("on_<type>", ...)`.
   - Add `on_<type>` in the `DataActor` trait impl that calls the dispatch method.
   - Add `#[pyo3(name = "on_<type>")]` method in the `#[pymethods]` block.
   - Add `on_<type>` to `RustTestDataActor` wrapper and the inline Python test class.
   - Add handler test and dispatch test.

4. **Python Actor** (`tests/unit_tests/common/test_actor.py`):
   - Add `test_subscribe_<type>` and `test_unsubscribe_<type>` tests.
   - Assert `actor.subscribed_<type>()` returns expected entries after subscribe and
     is empty after unsubscribe.

5. **Backtest client** (`nautilus_trader/backtest/data_client.pyx`): Override
   `subscribe_<type>` and `unsubscribe_<type>` if the base `MarketDataClient` raises
   `NotImplementedError` for the method.

6. **Documentation**: Add entries to `actors.md` callback table, `strategies.md` handler
   signatures, `adapters.md` subscribe method stubs, and `spec_data_testing.md` test cards.

:::tip
Search for an existing type like `instrument_close` or `funding_rate` across all six layers
to find concrete examples of the patterns described above.
:::
