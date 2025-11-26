# Coding Standards

## Code Style

The current codebase can be used as a guide for formatting conventions.
Additional guidelines are provided below.

### Universal formatting rules

The following applies to **all** source files (Rust, Python, Cython, shell, etc.):

- Use **spaces only**, never hard tab characters.
- Lines should generally stay below **100 characters**; wrap thoughtfully when necessary.
- Prefer American English spelling (`color`, `serialize`, `behavior`).

### Comment conventions

1. Generally leave **one blank line above** every comment block or docstring so it is visually separated from code.
2. Use *sentence case* – capitalize the first letter, keep the rest lowercase unless proper nouns or acronyms.
3. Do not use double spaces after periods.
4. **Single-line comments** *must not* end with a period *unless* the line ends with a URL or inline Markdown link – in those cases leave the punctuation exactly as the link requires.
5. **Multi-line comments** should separate sentences with commas (not period-per-line). The final line *should* end with a period.
6. Keep comments concise; favor clarity and only explain the non-obvious – *less is more*.
7. Avoid emoji symbols in text.

### Doc comment mood

**Rust** doc comments should be written in the **indicative mood** – e.g. *"Returns a cached client."*

This convention aligns with the prevailing style of the Rust ecosystem and makes generated
documentation feel natural to end-users.

### Terminology and phrasing

1. **Error messages**: Avoid using ", got" in error messages. Use more descriptive alternatives like ", was", ", received", or ", found" depending on context.
   - ❌ `"Expected string, got {type(value)}"`
   - ✅ `"Expected string, was {type(value)}"`

2. **Spelling**: Use "hardcoded" (single word) rather than "hard-coded" or "hard coded" – this is the more modern and accepted spelling.

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

## Commit messages

Here are some guidelines for the style of your commit messages:

1. Limit subject titles to 60 characters or fewer. Capitalize subject line and do not end with period.

2. Use 'imperative voice', i.e. the message should describe what the commit will do if applied.

3. Optional: Use the body to explain change. Separate from subject with a blank line. Keep under 100 character width. You can use bullet points with or without terminating periods.

4. Optional: Provide # references to relevant issues or tickets.

5. Optional: Provide any hyperlinks which are informative.

### Gitlint (optional)

Gitlint is available to help enforce commit message standards automatically. It checks that commit messages follow the guidelines above (character limits, formatting, etc.). This is **opt-in** and not enforced in CI.

**Benefits**: Encourages concise yet expressive commit messages, helps develop clear explanations of changes.

**Installation**: First install gitlint to run it locally:

```bash
uv pip install gitlint
```

To enable gitlint as an automatic commit-msg hook:

```bash
pre-commit install --hook-type commit-msg
```

**Manual usage**: Check your last commit message:

```bash
gitlint
```

Configuration is in `.gitlint` at the repository root:

- **60-character title limit**: Ensures clear rendering on GitHub and encourages brevity while remaining descriptive.
- **79-character body width**: Aligns with Python's PEP 8 conventions and the traditional limit for git tooling.

:::note
Gitlint may be enforced in CI in the future, so adopting these practices early helps ensure a smooth transition.
:::
