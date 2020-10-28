Overview
========

We recommend the PyCharm Professional edition IDE as it interprets Cython syntax.
Unfortunately the Community edition will not interpret Cython syntax.

> https://www.jetbrains.com/pycharm/

To run code or tests from the source code, first compile the C extensions for the package.

    $ python setup.py build_ext --inplace

All tests can be run via the `run_tests.sh` script, or through pytest.
