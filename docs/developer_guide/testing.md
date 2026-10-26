# Testing

Automated tests serve as executable specifications for the trading platform. They document intended
behavior, support confident refactoring, and catch regressions before production. Tests also provide
examples of complex flows and rapid CI feedback so issues surface early.

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
system where possible. The testing ladder below escalates the remaining unknowns through larger input spaces
and richer execution models. Each layer extends coverage to inputs or execution states the layer below cannot
reach.

Not every module requires every technique. Use this section to decide which layers apply
before adding tests or `debug_assert!` statements.

### Mechanism ladder

The [Rust guide](rust.md#design-by-contract) covers runtime contracts. The order of preference is:

1. The type system.
1. `check_*` from `nautilus_core::correctness` at API boundaries.
1. `debug_assert!` for internal invariants.
1. `assert!` for soundness-critical or always-on checks.

Test layers follow a parallel escalation. Start at the lowest layer that proves what matters. Climb only if
that layer stops detecting regressions or the input space grows beyond hand-picked cases.

| Layer                    | Trigger condition                                                               |
| ------------------------ | ------------------------------------------------------------------------------- |
| Unit test                | A single function or transition has a small, enumerable set of cases.           |
| Parametrized test        | The same shape repeats across discrete inputs (order side, status, instrument). |
| Property-based test      | An invariant must hold for a whole class of inputs the mind cannot enumerate.   |
| Integration test         | Multiple modules interact through a real (non-mocked) engine or runtime.        |
| Fuzz test                | Untrusted or adversarial bytes cross a parser, decoder, or wire-format handler. |
| Spec acceptance test     | Behavior depends on a live venue contract (see `spec_exec_testing.md`).         |
| Deterministic simulation | Correctness depends on task scheduling, timeouts, or wall-clock ordering.       |
| Formal verification      | A pure function has crisp invariants and a bounded input space worth a proof.   |

The formal verification rung is aspirational: no Kani or Prusti harness has landed in
the workspace. The row records the escalation condition for when a verifier is adopted,
not a current obligation.

### Projection rule

Module shape determines which layers pay off. Some modules do not need the full ladder. Apply the rule to each
module, not each crate. An adapter crate contains pure parsers and I/O-bound client loops. Each row applies to
a different part.

| Module shape                        | Layers that apply                             | Example                                |
| ----------------------------------- | --------------------------------------------- | -------------------------------------- |
| Pure function, crisp invariants     | Unit, parametrized, property, fuzz            | Reconciliation kernels, portfolio math |
| Pure function, no stated invariants | Unit, parametrized, property, fuzz            | Codecs, adapter parsers, formatters    |
| Stateful, synchronous               | Unit, parametrized, property over transitions | Cache, order book                      |
| Stateful, async                     | Unit, integration, deterministic simulation   | Live engine, execution manager         |
| I/O bound, venue contract           | Integration, spec acceptance, boundary fuzz   | Adapter client loops                   |

### When not to add coverage

- Add `debug_assert!` only where a test can reach it. Release builds strip the check, so an unexercised
  assertion has no signal. A targeted unit test counts as a harness. A proptest or fuzz harness amplifies the
  signal.
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
- Replay-sensitive IDs (`trade_id`, `venue_order_id`) are pure functions of their inputs. See
  `crates/execution/src/reconciliation/ids.rs`. Ephemeral event UUIDs on other reconciliation paths do not
  need to be deterministic.

The `surface` probe in `crates/common/src/live/dst.rs` only pins the re-export shape. It does not check that
callers actually use the seam. Enforcement is by review. Run the audit whenever a new async module enters the
workspace or an existing module gains new control-plane scheduling.

## Behavior-driven development

Behavior-driven development (BDD) uses concrete examples to agree on required behavior before
implementing it. Gherkin expresses those examples in a readable form that binds to test code.
**Use Gherkin for policies spanning components with a few distinct outcomes** that reviewers need
to understand. A feature file explains the conditions under which the system makes a decision and
the outcome callers can observe. Step text binds to Rust at runtime, so each feature adds a layer
the compiler does not check. Keep calculations, detailed permutations, and wiring checks in Rust.

**BDD describes how behavior is specified; integration describes the components a test exercises.**
The startup reconciliation scenarios are integration tests written in Gherkin. They run the real
node, execution manager, cache, and portfolio with a scripted execution client. Gherkin does not add
a mandatory layer to the [testing policy](#testing-policy), and scripted venue responses do not
establish [live adapter acceptance](spec_exec_testing.md).

### Selecting rules and examples

Start from a documented contract, an agreed behavior change, or a confirmed defect. Use the code to
check how the behavior is implemented. If the intended rule and implementation disagree, resolve
that disagreement before treating the current result as the specification.

For each rule:

1. State the guarantee and its scope, including the configuration and inputs it depends on.
1. Identify inputs that produce meaningfully different outcomes, including failure and recovery.
1. Choose a concrete example for each distinction that a reader needs to understand the rule.
1. Identify the observation that proves each outcome and the smallest execution boundary that can
   produce it. Raise open behavior questions with the change or in an issue before encoding an expectation.

For example, the [reconciliation contract](../concepts/execution/reconciliation.md) requires an
unresolved in-scope position report to block actors and strategies from starting when reconciliation
is enabled. Priced recovery, missing-price failure, and disabled generation illustrate distinct
outcomes under that rule. Keep those distinctions visible in separate scenarios.

### Test ownership and overlap

**Give each concrete example a primary test.** Use Gherkin for representative behavior contracts and
focused Rust tests for detailed permutations or component interactions that need no separate
domain explanation. For startup reconciliation, a typical division is:

| Concern                                  | Primary coverage                | What it proves                                          |
| ---------------------------------------- | ------------------------------- | ------------------------------------------------------- |
| Recovered exposure before trading starts | Gherkin integration scenario    | Strategies observe recovered exposure at startup.       |
| Unrecoverable exposure blocks startup    | Gherkin integration scenario    | Startup fails before trading and cleans up connections. |
| Price precision and arithmetic bounds    | Rust unit or parametrized tests | Exact values hold across numeric boundaries.            |
| Client identity and report routing       | Rust integration tests          | Reports reach the correct account and execution client. |
| Task ordering and cancellation races     | Deterministic simulation tests  | Required outcomes hold across controlled schedules.     |

Overlapping production paths are expected. Retain tests at different boundaries when they prove
different properties: a reconciliation calculation and its visibility in a strategy callback need
different observations. A second full startup test with the same setup, action, and assertions
needs a distinct purpose.

#### Replacing an existing test

Before replacing an existing test with a Gherkin scenario:

- Compare inputs, assertions, entry points, lifecycle paths, and execution environments.
- Preserve unique checks for identity, precision, filtering, lifecycle, cleanup, and simulation.
- Confirm the replacement runs in the normal test selection and proves the same contract.

A scenario that requests stop from a strategy's start callback does not prove the node reaches
`Running`. Retain a test that checks that transition. Share fixture construction and scripted
clients; do not call an existing test function from a step definition or reproduce reconciliation
logic in the fixture.

### Writing Gherkin

Organize each feature around a coherent capability, such as startup reconciliation. Use the
following structure:

- **`Feature`** names the capability and explains why its behavior matters.
- **`Rule`**, where useful, groups examples of one guarantee or policy.
- **`Scenario`** names one observable behavior and provides the context needed to interpret it.
- **`Scenario Outline`** varies inputs for the same behavior, such as long and short exposure. Keep
  distinct policies in separate scenarios instead of combining them into one conditional matrix.

#### Context and outcomes

Write `Given` steps for initial conditions, `When` for the action, and `Then` for observable results.
Use domain terms such as position reports, recovered exposure, and strategy startup. Name a Rust
method or entry point when its public contract is the subject of the example; otherwise keep
construction and wiring details in the step definitions.

Use `Background` for a short, common starting context. Keep conditions that explain a scenario's
different result in that scenario. An explicit override, such as disabling startup reconciliation,
must remain visible. State rule-relevant configuration even when it matches the default. Include
units and exact financial values in examples. Keep scenarios focused enough that a reader can
explain the rule without reading the Rust steps.

**Identify when an outcome holds if ordering matters.** In the startup feature,
`the strategy observes a long position of 5.000 ETH at 2500.25 USDT when it starts` requires an
observation from the start callback. A final cache assertion cannot prove that ordering. Failure
scenarios must assert the relevant failure reason and prohibited effects, such as an actor or
strategy starting after reconciliation fails.

Use tags to select policies, such as `@recovery`. Normal test discovery must include the suite
without an opt-in filter.

### Rust steps and fixtures

#### Layout and reuse

Use the workspace `cucumber` development dependency for Rust Gherkin suites. Follow the live
crate's `tests/spec/main.rs` runner and `tests/spec/features/<capability>.feature` layout. These are
repository layout choices; the runner configures the feature path. When adding another capability,
give it its own `World` and runner test with an explicit feature path so suites do not consume each
other's features. Keep steps with their workflow, and split Rust modules when distinct workflows
need separate setup or observations.

- **Steps**: Reuse a step only when its words have the same meaning, setup, and assertion contract
  in both callers. Avoid broad patterns that match unrelated behaviors or select expected results
  from hidden state.
- **Fixtures**: Put construction shared by independent test targets in `tests/common/`, as with the
  [startup client fixture](../../crates/live/tests/common/startup.rs). Keep fixtures used by only one
  suite local to it.
- **World**: Scope it to the workflow under test. Do not grow a shared `World` or a general fixture
  framework for hypothetical future features.

#### Step behavior

Step definitions translate domain inputs into production calls and compare observations with
explicit expectations. Use domain types or `Decimal` for financial values and assert the fields
that establish the contract, including identity where relevant. Script responses at the external
client boundary while keeping the behavior under test real. **Keep test controls in test code**:
do not add production flags, configuration, hooks, or accessors solely to drive a scenario.

#### Isolation and timing

Cucumber creates a fresh `World` for each scenario, but scenarios in a suite share a process and
runtime. Nextest's per-test process isolation does not separate them. State outside the `World`,
including callbacks and the thread-local message bus, needs explicit cleanup.

- Run suites using the live node's thread-local state sequentially on one runtime thread.
- Release clients, tasks, subscriptions, and shared state even after a failed step. Arrange cleanup
  so an assertion or timeout cannot bypass disposal.
- Record observations at the lifecycle boundary the contract names, such as a strategy's `on_start`.
  Requesting stop there can prevent entry into `Running`; choose a later observation when needed.
- Bound actions with timeouts and follow the [async waiting conventions](#waiting-for-asynchronous-effects)
  when waiting for effects. A normal timeout scenario does not establish deterministic-simulation coverage.

### Maintaining feature files

Update the rule, its examples, and the implementation together when required behavior changes.
Do not change an expected outcome to match a defect. Fix the defect in the same change; if that
is not possible, record it in an issue and add the regression example with the fix. A code refactor
that preserves behavior should usually require changes only to the Rust bindings or fixtures.
Add an example when it introduces a meaningful distinction; use focused parametrized or property
tests for additional combinations that illustrate the same rule.

#### Runner requirements

Run changed scenarios and affected existing tests. Use a standard Rust test harness with one
`#[tokio::test]` per suite so nextest discovers it. Follow the
[live runner](../../crates/live/tests/spec/main.rs):

- Use `fail_on_skipped()` so unmatched steps fail.
- Assert that `execution_has_failed()` is false and at least one step passed. Failed assertions,
  ambiguous bindings, parse errors, and empty selections must fail the command.
- Parse Cucumber options separately from libtest arguments, using `CUCUMBER_FILTER_TAGS` for filtering.

For a new or changed runner, verify discovery, filtering, and scenario/step failure reporting.
Temporarily use a wrong expectation, an unmatched step, and an empty filter to confirm each exits
nonzero. Restore valid scenarios and filter settings afterward. Count scenarios from Cucumber's
summary; nextest reports each Gherkin suite as one test. See
[running the startup scenarios](#gherkin-startup-reconciliation-scenarios) for commands and tag filtering.

The [Cucumber example-mapping guide](https://cucumber.io/docs/bdd/example-mapping/) describes how to
develop rules and examples. The [Gherkin reference](https://cucumber.io/docs/gherkin/reference/)
defines the syntax used here.

## Property-based testing

Property testing verifies that logic holds for *all* valid inputs, not just hand-picked examples.
We use [`proptest`](https://altsysrq.github.io/proptest-book/intro.html) in Rust to enforce invariants.

- **Use cases**: Core domain types (`Price`, `Quantity`, `UnixNanos`), accounting engines, matching engines, and state machines.
- **Example invariants**:
  - Round-trip serialization: `parse(to_string(value)) == value`
  - Inverse operations: `(A + B) - B == A`
  - Transitivity: `If A < B and B < C, then A < C`

## Fuzzing

Fuzzing introduces unstructured or malicious data to the system to verify it fails gracefully.

- **Use cases**: Network boundaries, exchange data parsers (JSON, FIX, WebSocket feeds), and complex state machines.
- **Goal**: The system returns a `Result::Err` and never panics, hangs, or leaks memory when encountering malformed data.

Adapter fuzz binaries are registered in each adapter package behind its `fuzz` feature. Run all
registered targets for one adapter from the repository root:

```bash
scripts/fuzz-adapter.sh derive
```

The workspace pins `libfuzzer-sys`, and `nautilus-live` owns the shared libFuzzer integration. A separate
`publish = false` package is reserved only for fuzz targets whose dependencies must not enter a published
adapter graph.
Lighter's git-pinned Pornin differential oracle is one such dependency.

When building or modifying core types, write property tests to cover the mathematical boundaries.

Performance tests help evolve performance-critical components.

Run tests with [pytest](https://docs.pytest.org), our primary test runner.
Use parametrized tests and fixtures (e.g., `@pytest.mark.parametrize`) to avoid repetitive code and improve clarity.

## Running tests

CI runs `scripts/ci/check_test_network.py` through `make test-scripts` to flag direct network calls
with non-local literal addresses, explicit live-test switches, and fork-RPC options. It checks test
directories and Rust files from their named test module onward. Loopback addresses and reserved
fixture domains are allowed.

**Check limits**: This heuristic does not resolve variable destinations, follow calls into other
code, or enforce network isolation.

### Python tests

The Python test suite lives under `python/tests/` and tests the Rust-backed PyO3 package. It requires
a built extension module and uses the Python project under `python/`. From the repository root, run:

```bash
make pytest
```

The Makefile target isolates certain test modules in separate pytest processes to avoid
global Rust state conflicts. Use `make pytest` rather than invoking pytest directly.

`make pytest` runs tests in parallel with pytest-xdist, using one worker per CPU up to 32. Set
`PYTEST_XDIST_AUTO_NUM_WORKERS` to change the worker count, for example
`make pytest PYTEST_XDIST_AUTO_NUM_WORKERS=8`.

Local `make pytest` runs use the debug extension from `make build-debug`. CI tests a release wheel. Do not
probe Rust panic paths in process in `python/tests/` with `pytest.raises(BaseException)` or similar broad
catches. These tests can appear to pass against the debug build and abort the interpreter against the release
wheel.

For abort-prone PyO3 or FFI methods, verify the Python signature and parameter names, or isolate the
call in a subprocess.

For the registered Rust benchmark set:

```bash
make cargo-ci-benches
```

No canonical Python performance suite is wired into CI. See the
[Benchmarking guide](benchmarking.md) for focused Criterion and iai commands, profiling, and
measurement policy. Run benchmarks separately from unit tests to avoid interference.

### Rust tests

Before running the full suite, [prepare the large test fixtures](test_datasets.md#dataset-categories).

```bash
make cargo-test
# or
cargo nextest run --workspace --features "$(bash scripts/cargo-features.bash)" --cargo-profile nextest --lib --tests
```

:::info
`cargo nextest` is the supported runner for the full Rust unit and integration suite. The suite
relies on per-test process isolation for process-global and thread-local state, including logging,
the message bus, and deterministic test state.

Plain `cargo test --workspace` runs a test binary's cases in a shared process, so it is not a
supported full-suite gate and is not guaranteed to pass. Plain `cargo test` remains appropriate for
doctests and focused tests known to work with the libtest runner.
:::

#### Gherkin startup reconciliation scenarios

The live-node [feature file](../../crates/live/tests/spec/features/startup_reconciliation.feature)
describes startup reconciliation in Given/When/Then steps.
The [Rust step definitions](../../crates/live/tests/spec/main.rs) run the real node with scripted
venue responses and observe recovered exposure inside the strategy's start callback.

To display all scenarios and their steps, run:

```bash
CARGO_BUILD_JOBS=4 cargo test --locked -p nautilus-live --test spec --profile nextest \
  startup_reconciliation -- --nocapture
```

To select the recovery scenarios, use Cucumber's tag filter:

```bash
CUCUMBER_FILTER_TAGS='@recovery' CARGO_BUILD_JOBS=4 \
  cargo test --locked -p nautilus-live --test spec --profile nextest startup_reconciliation -- --nocapture
```

Nextest discovers the feature suite as one `startup_reconciliation` test. The `spec` target also
contains a separate test for disposal after a panic; these commands select only the feature suite:

```bash
CARGO_BUILD_JOBS=4 NEXTEST_TEST_THREADS=2 \
  cargo nextest run --locked -p nautilus-live --test spec --cargo-profile nextest \
  -E 'test(=startup_reconciliation)' --no-capture
```

The scenarios run sequentially on one runtime thread with a fresh node per scenario. A test guard
disposes the node when the startup step returns or unwinds after a panic. Successful scenarios
request stop from the strategy start callback and do not prove entry into `Running`.
Both `start()` and `run()` use the real execution manager, cache, and portfolio. Missing or ambiguous
step bindings and failed assertions fail the suite. A filter that selects no scenarios also fails.
This suite uses the normal runtime; it does not establish deterministic-simulation coverage.

#### Rust doctests

`cargo nextest` cannot execute doctests, so they run through a separate target:

```bash
make cargo-test-doc
# or
cargo test --doc --workspace --features "$(bash scripts/cargo-features.bash)" --profile nextest
```

Doc examples are a maintained test surface. The scheduled `nightly-tests` workflow runs this target
with Python 3.13 and 3.14. See the [Rust guide](rust.md#doc-examples) for how to annotate a fence so
it compiles.

#### Testing with optional features

Use `EXTRA_FEATURES` to include optional features like `sbe` or `hypersync`:

```bash
# Test with SBE support
make cargo-test EXTRA_FEATURES="nautilus-common/sbe"

# Test with multiple features
make cargo-test EXTRA_FEATURES="nautilus-common/sbe hypersync"

# Legacy shorthand for hypersync
make cargo-test HYPERSYNC=true

# Test specific crate with features
make cargo-test-crate-nautilus-serialization FEATURES="sbe"
```

### IDE integration

- **PyCharm**: Right-click the tests folder or file -> "Run pytest".
- **VS Code**: Use the Python Test Explorer extension.

## Test style

### General

- Name test functions after what they exercise. You do not need to encode the expected assertions in the name.
- Add docstrings when they clarify setup, scenarios, or expectations.
- **Group assertions** when possible: perform all setup/act steps first, then assert
  together to avoid the act-assert-act smell.
- Use `unwrap`, `expect`, or direct `panic!`/`assert` calls inside tests. Clarity and conciseness matter more
  than defensive error handling here.
- For behavior tests, assert return values, state changes, or side effects rather than log messages.
  Log capture uses global state, depends on test execution order, and can break when wording changes.
- Capture logs only when logging itself is the contract, such as excluding secrets and preserving
  public diagnostic context. Serialize capture and join emitting tasks before releasing it.
  Assert the relevant records and secret absence; avoid whole-transcript assertions.

### Python tests (`python/tests/`)

Use **pytest-style free functions and fixtures**. Do not use test classes.

- Write each test as a standalone `def test_*()` function.
- Use `@pytest.fixture` for shared setup (instruments, engine instances, data).
  Prefer `yield` fixtures when teardown is needed (e.g., `engine.dispose()`).
- Use `@pytest.mark.parametrize` to cover multiple inputs without duplicating
  test bodies.
- Import model types from `nautilus_trader.model`, not from
  `nautilus_trader._libnautilus`.
- Test providers live in `python/tests/providers.py`. Use `TestInstrumentProvider`
  and `TestDataProvider` for common instruments and data.
- Mark tests that depend on unfinished features with
  `@pytest.mark.skip(reason="WIP: <description>")` rather than deleting them.

### Rust

For Rust-specific test conventions (module structure, `#[rstest]`, parameterization),
see the [Rust guide](rust.md#testing-conventions).

## Waiting for asynchronous effects

In Rust tests, prefer a notification channel or another event owned by the test over repeated
condition evaluation:

1. Subscribe before reading the authoritative state.
1. Recheck it after every notification so a transition between the read and the await cannot be missed.

When no suitable signal exists, use `wait_until_async(...)` from `nautilus_common::testing`. It stops as soon
as the condition succeeds and applies a bounded timeout. Use a fixed sleep only when the time window itself is
under test.

## Mocks

Prefer hand-written stubs that return fixed values over mocking frameworks. Use `MagicMock` only to
assert call counts or arguments, or to simulate complex state changes. Avoid mocking the objects
you test.

## Code coverage

Aim for high coverage without sacrificing error handling or changing the architecture solely for
tests. Some branches remain untestable without modifying production behavior. For example, the final
condition in a defensive if-else block may trigger only for unexpected values. Leave these checks
in place so future changes can exercise them if needed.

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
(remove `-- --exact` and append `::case_n` where `n` starts at 1). This workaround matches the behavior explained in [rust-analyzer issue 8964](https://github.com/rust-lang/rust-analyzer/issues/8964#issuecomment-871592851).

In VS Code you can pick the specific test case to debug directly.

## Debugging Python and Rust

Build the PyO3 extension with the workspace's `debug-pyo3` Cargo profile when a native debugger
needs Rust symbols:

```bash
make sync
(
  cd python
  CARGO_TARGET_DIR=../target \
    uv run --no-sync maturin develop --profile debug-pyo3
)
```

1. Start the Python program or notebook with the Python debugger.
1. Attach LLDB or GDB to that Python process for Rust breakpoints.

The repository does not generate editor launch configurations, so configure both debugger sessions in the
editor you use.

## Data type testing

Each data type flows through multiple layers of the platform. The table below shows where
existing types are tested, so new types can follow the same pattern.

### Test layer matrix

| Layer                  | Location                                    | What it covers                                             |
| ---------------------- | ------------------------------------------- | ---------------------------------------------------------- |
| DataEngine subscribe   | `crates/data/tests/integration/engine/`     | Engine processes subscribe/unsubscribe commands correctly. |
| DataEngine publish     | `crates/data/tests/integration/engine/`     | Engine routes published data to the message bus.           |
| DataActor subscribe    | `crates/common/src/actor/tests.rs`          | Actor subscribes and receives data via typed publish.      |
| DataActor unsubscribe  | `crates/common/src/actor/tests.rs`          | Actor stops receiving data after unsubscribe.              |
| PyO3 actor dispatch    | `crates/common/src/python/actor.rs`         | Rust handler dispatches to Python `on_*` method.           |
| Python Actor subscribe | `python/tests/unit/common/test_actor.py`    | Python actor subscribes. Command count increments.         |
| Python Actor unsub     | `python/tests/unit/common/test_actor.py`    | Python actor unsubscribes. Subscription list clears.       |
| Adapter live tests     | `docs/developer_guide/spec_data_testing.md` | Live data acceptance tests (DataTester).                   |

### Coverage per data type

The following table shows which layers have test coverage for each data type.
Use this as a checklist when adding a new type.

| Data type           | Engine | Actor (Rust) | PyO3 dispatch | Actor (Python) | Adapter spec |
| ------------------- | ------ | ------------ | ------------- | -------------- | ------------ |
| `InstrumentAny`     | ✓      | ✓            | ✓             | ✓              | ✓            |
| `OrderBookDeltas`   | ✓      | ✓            | ✓             | ✓              | ✓            |
| `OrderBook`         | ✓      | ✓            | ✓             | ✓              | ✓            |
| `QuoteTick`         | ✓      | ✓            | ✓             | ✓              | ✓            |
| `TradeTick`         | ✓      | ✓            | ✓             | ✓              | ✓            |
| `Bar`               | ✓      | ✓            | ✓             | ✓              | ✓            |
| `MarkPriceUpdate`   | ✓      | ✓            | ✓             | ✓              | ✓            |
| `IndexPriceUpdate`  | ✓      | ✓            | ✓             | ✓              | ✓            |
| `FundingRateUpdate` | ✓      | ✓            | ✓             | ✓              | ✓            |
| `InstrumentStatus`  | ✓      | ✓            | ✓             | ✓              | ✓            |
| `InstrumentClose`   | ✓      | ✓            | ✓             | ✓              | ✓            |
| `OptionGreeks`      | ✓      | ✓            | ✓             | ✓              | ✓            |
| `OptionChainSlice`  | -      | ✓            | ✓             | ✓              | ✓            |
| `CustomData`        | ✓      | ✓            | ✓             | ✓              | -            |

`OptionChainSlice` is assembled by the DataEngine's `OptionChainManager` from per-instrument
greeks and quote subscriptions. It does not have its own engine subscribe command.

### Adding a new data type

When introducing a new data type, add tests at each layer:

1. **DataEngine** (`crates/data/tests/integration/engine/`): Add `test_execute_subscribe_<type>` and
   `test_execute_unsubscribe_<type>` tests. Follow the pattern in existing subscribe tests:
   register client, build command, call `engine.execute`, assert subscription list.

1. **DataActor Rust** (`crates/common/src/actor/tests.rs`):
   - Add `received_<type>: Vec<Type>` field to `TestDataActor`.
   - Implement the `on_<type>` handler in the `DataActor` trait impl.
   - Add `test_subscribe_and_receive_<type>` and `test_unsubscribe_<type>` tests.
   - Use the typed publish function (`msgbus::publish_<type>`), not `publish_any`,
     for types that use `TypedHandler` routing.

1. **PyO3 actor dispatch** (`crates/common/src/python/actor.rs`):
   - Add `dispatch_on_<type>` method that calls `py_self.call_method1("on_<type>", ...)`.
   - Add `on_<type>` in the `DataActor` trait impl that calls the dispatch method.
   - Add `#[pyo3(name = "on_<type>")]` method in the `#[pymethods]` block.
   - Add `on_<type>` to `RustTestDataActor` wrapper and the inline Python test class.
   - Add handler test and dispatch test.

1. **Python Actor** (`python/tests/unit/common/test_actor.py`):
   - Add `test_subscribe_<type>` and `test_unsubscribe_<type>` tests.
   - Assert `actor.subscribed_<type>()` returns expected entries after subscribe and
     is empty after unsubscribe.

1. **Documentation**: Add entries to `actors.md` callback table, `strategies.md` handler
   signatures, `adapters.md` subscribe method stubs, and `spec_data_testing.md` test cards.

:::tip
Search for an existing type like `instrument_close` or `funding_rate` across all five layers
to find concrete examples of the patterns described above.
:::
