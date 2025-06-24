# Testing

The test suite is divided into broad categories of tests including:

- Unit tests
- Integration tests
- Acceptance tests
- Performance tests
- Memory leak tests

The performance tests exist to aid development of performance-critical components.

Tests can be run using [pytest](https://docs.pytest.org), which is our primary test runner. We recommend using parametrized tests and fixtures (e.g., `@pytest.mark.parametrize`) to avoid repetitive code and improve clarity.

## Running Tests

### Python Tests

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

### Rust Tests

```bash
make cargo-test
# or
cargo nextest run --workspace --features "python,ffi,high-precision,defi" --cargo-profile nextest
```

### IDE Integration

- **PyCharm**: Right-click on tests folder or file → "Run pytest"
- **VS Code**: Use the Python Test Explorer extension

## Mocks

Unit tests will often include other components acting as mocks. The intent of this is to simplify
the test suite to avoid extensive use of a mocking framework, although `MagicMock` objects are
currently used in particular cases.

## Code Coverage

Code coverage output is generated using `coverage` and reported using [codecov](https://about.codecov.io/).

High test coverage is a goal for the project however not at the expense of appropriate error
handling, or causing “test induced damage” to the architecture.

There are currently areas of the codebase which are impossible to test unless there is a change to
the production code. For example the last condition check of an if-else block which would catch an
unrecognized value, these should be left in place in case there is a change to the production code - which these checks could then catch.

Other design-time exceptions may also be impossible to test for, and so 100% test coverage is not
the ultimate goal.

### Style guidance

- **Group assertions** where possible – perform all setup/act steps first, then assert expectations together at
  the end of the test to avoid the *act-assert-act* smell.
- Using `unwrap`, `expect`, or direct `panic!`/`assert` calls inside **tests** is acceptable. The
  clarity and conciseness of the test suite outweigh defensive error-handling that is required in
  production code.

## Excluded code coverage

The `pragma: no cover` comments found throughout the codebase [exclude code from test coverage](https://coverage.readthedocs.io/en/coverage-4.3.3/excluding.html).
The reason for their use is to reduce redundant/needless tests just to keep coverage high, such as:

- Asserting an abstract method raises `NotImplementedError` when called.
- Asserting the final condition check of an if-else block when impossible to test (as above).

These tests are expensive to maintain (as they must be kept in line with any refactorings), and
offer little to no benefit in return. The intention is for all abstract method
implementations to be fully covered by tests. Therefore `pragma: no cover` should be judiciously
removed when no longer appropriate, and its use *restricted* to the above cases.

## Debugging Rust Tests

Rust tests can be debugged using the default test configuration.

If you want to run all tests while compiling with debug symbols for later debugging some tests individually,
run `make cargo-test-debug` instead of `make cargo-test`.

In IntellijIdea, to debug parametrised tests starting with `#[rstest]` with arguments defined in the header of the test
you need to modify the run configuration of the test so it looks like
`test --package nautilus-model --lib data::bar::tests::test_get_time_bar_start::case_1`
(remove `-- --exact` at the end of the string and append `::case_n` where `n` is an integer corresponding to
the n-th parametrised test starting at 1).
The reason for this is [here](https://github.com/rust-lang/rust-analyzer/issues/8964#issuecomment-871592851)
(the test is expanded into a module with several functions named `case_n`).
In VSCode, it is possible to directly select which test case to debug.
