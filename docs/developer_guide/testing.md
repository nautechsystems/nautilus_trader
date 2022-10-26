# Testing
The test suite is divided into broad categories of tests including:

- Unit tests
- Integration tests
- Acceptance tests
- Performance tests

The performance tests exist to aid development of performance-critical components.

Tests can be run using either [Pytest](https://docs.pytest.org) or the [Nox](https://nox.thea.codes/en/stable/) tool.

If you’re using PyCharm then tests should run directly by right clicking on the respective folder (or top-level tests folder) and clicking ‘Run pytest’.

Alternatively you can use the `pytest .` command from the root level tests directory, or the other subdirectories.

## Nox
Nox sessions are defined within the `noxfile.py`, to run various test collections.

To run unit tests with nox:
    
    nox -s tests

If you have a redis-server up you can run integration tests with nox:

    nox -s tests_integration

Or run the performance tests:

    nox -s tests_performance

Or run the entire test suite:

    nox -s tests_all

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

## Excluded code coverage
The `pragma: no cover` comments found throughout the codebase [exclude code from test coverage](https://coverage.readthedocs.io/en/coverage-4.3.3/excluding.html). 
The reason for their use is to reduce redundant/needless tests just to keep coverage high, such as:

- Asserting an abstract method raises `NotImplementedError` when called.
- Asserting the final condition check of an if-else block when impossible to test (as above).

These tests are expensive to maintain (as they must be kept in line with any refactorings), and 
offer little to no benefit in return. However, the intention is for all abstract method 
implementations to be fully covered by tests. Therefore `pragma: no cover` should be judiciously 
removed when no longer appropriate, and its use *restricted* to the above cases.
