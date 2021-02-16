Coding Standards
================

Code Style
----------
`Black` is a PEP-8 compliant opinionated formatter.

> https://github.com/psf/black

We philosophically agree with the `Black` formatting style, however it does not
currently run over Cython code. So you could say we are "handcrafting towards"
`Blacks` stylistic conventions.

The current codebase can be used as a guide for formatting guidance.

- For longer lines of code, and when passing more than a couple of arguments -
it's common to take a new line which aligns at the next logical indent (rather
than attempting a hanging alignment off an opening parenthesis).

- The closing parenthesis should be located on a new line, aligned at the logical
indent.

- Also ensure multiple hanging parameters or arguments end with a comma `,`::

    LongCodeLine(
        some_arg1,
        some_arg2,
        some_arg3,  # <-- ending comma
    )


PEP-8
-----
The codebase generally follows the PEP-8 style guide.

One notable departure is that Python `truthiness` is not always taken advantage
of to check if an argument is `None`, or if a collection is empty/has elements.

There are two reasons for this;

1- Cython can generate more efficient C code from `is None` and `is not None`,
rather than entering the Python runtime to check the `PyObject` truthiness.

2- As per the `Google Python Style Guide` it's discouraged to use truthiness to
check if an argument is/is not None, when there is a chance an unexpected object
could be passed into the function or method which will yield an unexpected
truthiness evaluation - which could result in a logical error type bug.

_"Always use if foo is None: (or is not None) to check for a None value.
E.g., when testing whether a variable or argument that defaults to None was set
to some other value. The other value might be a value that’s false in a boolean
context!"_

> https://google.github.io/styleguide/pyguide.html

Having said all of this there are still areas of the codebase which aren't as
performance-critical where it is safe to use Python truthiness to check for None,
or if a collection is empty/has elements.

We welcome all feedback on where the codebase departs from PEP-8 for no apparent
reason.

NumPy Docstrings
----------------
The NumPy docstring syntax is used throughout the codebase. This needs to be
adhered to consistently to ensure the docs build correctly during pushes to the
`master` branch.

> https://numpydoc.readthedocs.io/en/latest/format.html

Flake8
------
`flake8` is utilized to lint the codebase. Current ignores can be found in the
`.flake8` config file, the majority of which are required so that valid Cython
code is not picked up as flake8 failures.

Cython
------
Ensure that all functions and methods returning `void` or a primitive C type
(such as `bint`, `int`, `double`) include the `except *` keyword in the signature.

This will ensure Python exceptions are not ignored, but instead are `bubbled up`
to the caller as expected.

More information on Cython syntax and conventions can be found by reading the
Cython docs.

> https://cython.readthedocs.io/en/latest/index.html
