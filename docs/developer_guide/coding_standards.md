# Coding Standards

## Code Style
The current codebase can be used as a guide for formatting conventions.
Additional guidelines are provided below.

### Black

[Black](https://github.com/psf/black) is a PEP-8 compliant opinionated formatter and used during the pre-commit step.

We philosophically agree with the *Black* formatting style, however it does not currently run over the Cython parts of the codebase. 
So there you could say we are “handcrafting towards”  *Black* stylistic conventions for consistency.

### Formatting

1. For longer lines of code, and when passing more than a couple of arguments, you should take a new line which aligns at the next logical indent (rather than attempting a hanging 'vanity' alignment off an opening parenthesis). This practice conserves space to the right, ensures important code is more central in view, and is also robust to function/method name changes.

2. The closing parenthesis should be located on a new line, aligned at the logical indent.

3. Also ensure multiple hanging parameters or arguments end with a trailing comma:

```python
long_method_with_many_params(
    some_arg1,
    some_arg2,
    some_arg3,  # <-- trailing comma
)
```

### PEP-8
The codebase generally follows the PEP-8 style guide. Even though C typing is taken advantage of in the Cython parts of the codebase, we still aim to be idiomatic of Python where possible.
One notable departure is that Python truthiness is not always taken advantage of to check if an argument is `None` for everything other than collections.

There are two reasons for this;

1. Cython can generate more efficient C code from `is None` and `is not None`, rather than entering the Python runtime to check the `PyObject` truthiness.

2. As per the [Google Python Style Guide](https://google.github.io/styleguide/pyguide.html) - it’s discouraged to use truthiness to check if an argument is/is not `None`, when there is a chance an unexpected object could be passed into the function or method which will yield an unexpected truthiness evaluation (which could result in a logical error type bug).

*“Always use if foo is None: (or is not None) to check for a None value. E.g., when testing whether a variable or argument that defaults to None was set to some other value. The other value might be a value that’s false in a boolean context!”*

Having said all of this there are still areas of the codebase which aren’t as performance-critical where it is safe to use Python truthiness to check for `None`. 

```{note}
To be clear, it's still encouraged to use Python truthiness `is` and `not` to check if collections are `None` or empty.
```

We welcome all feedback on where the codebase departs from PEP-8 for no apparent reason.

### Docstrings
The [NumPy docstring spec](https://numpydoc.readthedocs.io/en/latest/format.html) is used throughout the codebase. This needs to be adhered to consistently to ensure the docs build correctly.

### Flake8
[Flake8](https://github.com/pycqa/flake8) is utilized to lint the codebase. Current ignores can be found in the top-level `pre-commit-config.yaml`, with the justifications also commented.

### Commit messages
Here are some guidelines for the style of your commit messages:

1. Limit subject titles to 50 characters or fewer. Capitalize subject line; use imperative voice; and do not end with period.

2. Use 'imperative voice', i.e. the message should describe what the commit will do if applied.

3. Optional: Use the body to explain change. Separate from subject with a blank line. Keep under 80 character width. You can use bullet points.
    
4. Optional: Provide # references to relevant issues or tickets.

5. Optional: Provide any hyperlinks which are informative.
