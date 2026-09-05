# Coding Standards

## Code Style

The current codebase can be used as a guide for formatting conventions.
Additional guidelines are provided below.

### Universal formatting rules

The following applies to **all** source files (Rust, Python, shell, etc.):

- Use **spaces only**, never hard tab characters.
- Lines should generally stay below **100 characters**; wrap thoughtfully when necessary.
- Prefer American English spelling (`color`, `serialize`, `behavior`).

### Shell scripts

Bash is the default shell for repository scripts. Use POSIX `sh` only when a caller cannot rely on
Bash being installed. See [Shell](shell.md) for script selection, extensions, portability,
structure, testing, formatting, and linting requirements.

### Comment conventions

1. Generally leave **one blank line above** every comment block or docstring so it is visually separated from code.
2. Use *sentence case* - capitalize the first letter, keep the rest lowercase unless proper nouns or acronyms.
3. Do not use double spaces after periods.
4. **Single-line comments** *must not* end with a period *unless* the line ends with a URL or inline Markdown link - in those cases leave the punctuation exactly as the link requires.
5. **Multi-line comments** should separate sentences with commas (not period-per-line). The final line *should* end with a period.
6. Keep comments concise; favor clarity and only explain the non-obvious - *less is more*.
7. Avoid emoji symbols in text.

### Doc comment mood

**Rust** doc comments should be written in the **indicative mood** - e.g. *"Returns a cached client."*

This convention aligns with the prevailing style of the Rust ecosystem and makes generated
documentation feel natural to end-users.

### Terminology and phrasing

1. **Error messages**: Avoid using ", got" in error messages. Use more descriptive alternatives like ", was", ", received", or ", found" depending on context.
   - Bad: `"Expected string, got {type(value)}"`
   - Good: `"Expected string, was {type(value)}"`

2. **Spelling**: Use "hardcoded" (single word) rather than "hard-coded" or "hard coded" - this is the more modern and accepted spelling.

3. **Error variable naming**: Use single-letter `e` for caught errors/exceptions:
   - Rust: `Err(e)` not `Err(err)` or `Err(error)`, and `|e|` not `|err|` in closures
   - Python: `except SomeError as e:` not `as err:` or `as error:`

### Naming conventions

1. **Internal fields**: Abbreviations are acceptable for private/internal fields (e.g., `_price_prec`, `_size_prec`) to keep hot-path code concise.

2. **User-facing API**: Use full, descriptive names for public properties, function parameters, return types, and metric names/labels (e.g., `price_precision`, `size_precision`). This prevents abbreviated terminology from leaking into dashboards or alerts.

3. **Error messages and logs**: Use full words for clarity (e.g., "price precision" not "price prec"). The user should never see abbreviated terminology.

4. **Execution terminology**: Use `Execution` in public, project-owned PascalCase type names, such
   as `BinanceExecutionClientConfig`. Internal implementation types may retain established `Exec`
   names. Also reserve `Exec` for the `ExecAlgorithmId` and `ExecTester` families, established
   `exec_*` names, and venue or protocol terms such as `BitmexExecType`. Name protocol-specific
   wire models after the venue concept, such as `HyperliquidExchangeAction`. Preserve established
   public names, historical release entries, and source names in migration tables.

5. **Runtime qualifiers**: Use `Live` when a type selects or configures real-time runtime semantics,
   such as `LiveNode` versus `BacktestNode`, `LiveClock` versus `TestClock`, and the
   `LiveDataEngineConfig`, `LiveRiskEngineConfig`, and `LiveExecutionEngineConfig` family versus
   reusable core engine configs. Omit `Live` from the ordinary adapter client family because a
   connected client is the default. Qualify alternate implementations by their behavior, such as
   `SandboxExecutionClient` or `DatabentoHistoricalClient`. An explicit live/historical protocol
   pair may retain `Live` to distinguish the two implementations.

6. **Adapter factory configs**: Name the data and execution inputs `<Venue>DataClientConfig` and
   `<Venue>ExecutionClientConfig`. Factories consume these client configs directly rather than a
   separate factory config wrapper. `LiveNodeConfig` owns `trader_id`; venue-specific `account_id`
   values belong on execution client configs.

#### Data loading APIs

Use free functions for stateless data ingestion. Use a class only when instances retain reusable
configuration, caches, workers, iteration state, or an open resource across calls. Do not use a
zero-state class solely to group static or class methods.

Follow the semantic distinction in the
[Polars I/O API](https://docs.pola.rs/api/python/stable/reference/io.html), adapted to the
established Nautilus `load_*` vocabulary:

- `load_<source>_<data>` eagerly reads, normalizes, and materializes a complete result.
- `scan_<source>_<data>` creates a lazy query or deferred execution plan.
- `stream_<source>_<data>` incrementally yields records or batches.
- `write_<format>` eagerly writes an in-memory result.
- `sink_<format>` writes through a lazy or streaming execution path.

For ingestion, order names from general to specific: verb, source, logical data, then an optional
representation. Include the representation only when real sibling formats exist. For example,
`load_binance_order_book_deltas` is preferable to a stateless
`BinanceOrderBookDeltaDataLoader.load` class or a generic `load_binance_data` function.

#### Adapter package facades

Each package under `python/nautilus_trader/adapters/` is a thin facade over the private
`_libnautilus` extension. Every adapter `__init__.py` declares a deterministic `__all__` that is
the single source of truth for its public API; `python/generate_stubs.py` copies that list into the
matching `.pyi` so runtime and stub exports agree exactly.

A venue adapter exposes its canonical identity constants plus the supported public surface:

- `<VENUE>`, `<VENUE>_CLIENT_ID`, `<VENUE>_VENUE`, registered from Rust via `m.add` in the adapter's
  `python/mod.rs`
- data types, `*Config`, `*Factory`, user-facing enums (such as `*Environment` and `*ProductType`)
- stateless loaders (`load_*`, `stream_*`, `convert_*`) and intentional utilities
  (`decode_*`, `get_*_arrow_schema_map`)

Keep the facade thin. Never add raw HTTP or WebSocket clients, wire models, endpoint URL resolvers
(`get_*_url`, `*_HTTP_URL`), caches, or other internals to `__all__` merely for structural parity.
Data providers (such as `databento` and `tardis`), the `blockchain` data client, the `sandbox`
execution client, and the multi-venue `interactive_brokers` broker omit venue constants because the
constants would be meaningless for them.

Order `__all__` entries so the `RUF022` pre-commit gate owns sort order; do not hand-order the list.

### Formatting

1. For longer lines of code, and when passing more than a couple of arguments, you should take a new line which aligns at the next logical indent (rather than attempting a hanging 'vanity' alignment off an opening parenthesis). This practice conserves space to the right, keeps important code more central in view, and survives function/method name changes.

2. The closing parenthesis should be located on a new line, aligned at the logical indent.

3. Multiple hanging parameters or arguments should end with a trailing comma:

```python
long_method_with_many_params(
    some_arg1,
    some_arg2,
    some_arg3,  # <-- trailing comma
)
```

## Commit messages

Commit messages use a capitalized, imperative subject naming the affected surface, optionally followed by a body
explaining the change.

### Subject line

- Open with a capitalized imperative verb, so the subject describes what the commit does when applied.
  `Add`, `Fix`, `Improve`, `Refine`, `Update`, `Remove`, `Refactor`, and `Standardize` cover most of the history.
- Name the affected surface (crate, adapter, subsystem, or type) so the log stays scannable.
- Keep the subject at 10 characters or more so it can name the affected surface clearly.
- Aim for 60 characters or fewer for clear GitHub rendering and concise text. The commit-message
  hook warns without failing when the subject exceeds this target. The project plans to enforce
  this limit in the future.
- Do not end the subject with a period.
- Do not put an issue or pull request number in the subject. GitHub appends the pull request number
  on squash merge, and any other reference belongs in the body. The commit-message hook rejects a
  subject containing `#<number>` in any position.

```text
Add Decimal constructors to Instrument trait
Fix non-atomic order event application
Refine cross-platform wheel validation
Remove stale security audit exceptions
```

Avoid these shapes:

```text
feat(bybit): add due_post_only flag        # Conventional Commits type and scope
fix: bug                                   # lowercase, unspecific, too short
Fixed the Bybit post-only rejection flag.  # past tense, trailing period
Update stuff                               # says nothing about the surface
Fix the post-only flag (#4544)             # pull request number added by hand
Fix PR #4544 review feedback               # issue or pull request number in the subject
```

### Conventional Commits

Do NOT use [Conventional Commits](https://www.conventionalcommits.org/) syntax for commit messages or pull
request titles. Many editors and AI assistants emit that format by default, but no commit in this
repository's history uses it, and the type and scope ceremony duplicates what the subject already carries.
Pull request titles matter here too, because a squash merge turns the PR title into the commit subject.

### Body

The body is optional, but anything beyond a trivial change should say why the change was made rather than
restate the diff.

- Separate the body from the subject with a blank line.
- Keep body lines to 79 characters or fewer to align with PEP 8 and traditional Git tooling.
- Use prose paragraphs or bullet points, whichever suits the change. Bullets may keep the same imperative
  voice as the subject, and do not need terminating periods.
- Include informative hyperlinks where they help a future reader.

### Issue references

- Reference issues from the body, typically on a final line: `Resolves #4534` when the commit closes the
  issue, or `Related to #4547` when it is partial work.
- GitHub appends the pull request number to the subject on squash merge, producing subjects such as
  `Fix TWAP child-order sizing and interval validation (#4544)`. Do not add that suffix by hand, and
  do not reference a pull request or issue anywhere else in the subject either. The subject has no
  room for detail the body carries better, and a hand-written number duplicates or contradicts the
  appended one.
- Aim to keep the pull request title short enough for the appended suffix to leave the squash-merged
  subject at 60 characters or fewer.
