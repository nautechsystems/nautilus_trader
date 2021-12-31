# Cython

Here you will find guidance and tips for working on NautilusTrader using the Cython language.

Ensure that all functions and methods returning `void` or a primitive C type (such as `bint`, `int`, `double`) include the `except *` keyword in the signature.

This will ensure Python exceptions are not ignored, but instead are “bubbled up” to the caller as expected.

More information on Cython syntax and conventions can be found by reading the [Cython docs](https://cython.readthedocs.io/en/latest/index.html).
