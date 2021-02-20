Testing
=======

The test suite is divided into broad categories of tests including:
    - Unit tests
    - Integration tests
    - Acceptance/System tests
    - Performance tests

The performance tests are not run as part of the CI pipeline, but exist to aid
development of performance-critical components.

Tests can be run using either Pytest or the Nox tool.

If you're using PyCharm then tests should run directly by right clicking on the
respective folder (or top level tests folder) and clicking 'Run pytest'.

Alternatively you can use the ``pytest .`` command from the root level ``tests``
folder, or the other sub folders.

Nox
---
Nox sessions are defined within the ``noxfile.py``, to run various test collections.

To run unit tests with nox::

    nox -s tests


If you have ``redis-server`` up you can run integration tests with nox::

    nox -s integration_tests


Or both unit and integration tests::

    nox -s tests_with_integration

Mocks
-----
Unit tests will often include other components acting as mocks. The intent of
this is to simplify the test suite to avoid extensive use of a mocking framework,
although ``MagicMock`` objects are currently used in particular cases.

Code Coverage
-------------
Code coverage output is generated using ``coverage`` and reported using ``codecov``.

High test coverage is a goal for the project however not at the expense of
appropriate error handling, or causing "test induced damage" to the architecture.

There are currently areas of the codebase which are `impossible` to test unless
there is a change to the production code. For example the last condition check
of an if-else block which would catch an unrecognized value, these should be
left in place in case there is a change to the production code - which these
checks could then catch.

Other `design-time` exceptions may also be impossible to test for, and so 100%
test coverage is not the ultimate goal.
