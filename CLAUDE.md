# AGENTS.md

Behavioral guidelines to reduce common LLM coding mistakes. 

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 0. Learn the Project

Key documentation lives in `docs/`:

- `docs/getting_started/` — Installation, quickstart, first backtest
- `docs/concepts/` — Core architecture: actors, strategies, data flow, execution
- `docs/developer_guide/` — Coding standards, Rust/Python conventions, testing, releases
- `docs/how_to/` — Task-oriented guides (adapters, custom data, deployment)
- `docs/integrations/` — Exchange adapter specifics
- `docs/tutorials/` — End-to-end walkthroughs
- `docs/api_reference/` — Module-level API docs

Also see: `README.md`, `ADAPTERS.md`, `ROADMAP.md`, `CONTRIBUTING.md`

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- Simple and effective is the default. Do not add clever helper concepts, transitional abstractions, or convenience APIs without a demonstrated need.
- This project is new: do not preserve backward compatibility by default. Prefer clean breaking changes over compatibility wrappers or legacy aliases unless explicitly requested.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.
- Do not keep deprecated wrappers or compatibility aliases unless the user explicitly asks for a migration window.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.


## 5. Project-Specific Rules

Full details in `docs/developer_guide/`. Key rules summarized below.

### Naming

- Public API: full descriptive names (`price_precision`); internal fields may abbreviate (`_price_prec`)
- Error variables: single-letter `e` — Rust: `Err(e)`, Python: `except SomeError as e:`
- Rust constants: `SCREAMING_SNAKE_CASE`
- PyO3 functions: Rust `py_*` prefix, use `#[pyo3(name = "…")]` to expose without prefix
- Rust bin filenames: snake_case; `[[bin]] name` in kebab-case
- Test names: descriptive — `test_sma_with_no_inputs_returns_zero_count`; property tests prefixed `prop_`

### Rust

- Copyright header required on all `.rs` files
- Use `#[rstest]` for all tests (not `#[test]`)
- Fully qualify: `anyhow::bail!`, `anyhow::Result<T>`, `log::info!`, `tokio::spawn`, `tokio::time::timeout`
- Do NOT qualify Nautilus domain types — import directly
- Inline format strings: `anyhow::bail!("Failed for {n}")` not positional
- Constructor pattern: `new_checked()` → `CorrectnessResult`; `new()` panics via `.expect_display(FAILED)`
- Error handling: new code must use `nautilus_error` (`crates/error`). Use `NautilusError` with `ErrorKind`, `.with_operation()`, `.with_context()`. Avoid raw `anyhow` in new crates.
- Hash collections: `IndexMap`/`IndexSet` when iteration order matters; `AHashMap` for lookup-only hot paths
- Adapter runtime: `get_runtime().spawn()` not `tokio::spawn()` (panics from Python threads)
- No box-style banner comments (`// ======`)
- Test module: `mod tests`; property tests in separate `mod property_tests`

### Python

- All signatures must have type annotations; use PEP 604 union (`X | None` not `Optional[X]`)
- NumPy docstring format, imperative mood
- No docstrings on private methods unless complex
- Tests: pytest-style free functions + fixtures (no test classes); run via `make pytest-v2`
- Import from `nautilus_trader.model`, not `nautilus_trader.core.nautilus_pyo3`
- Cython: all void/primitive-returning functions must have `except *`

### Architecture

- Message immutability: once created, fields never mutate
- Design by contract: type system → `check_*` at boundaries → `debug_assert!` internal → `assert!` for soundness
- Feature flags: additive only; `default = []` minimal
- Never wrap `PyObject` in `Arc`; use `clone_py_object()`
- Python v2 live: Tokio workers must NOT run Python code; route through event channels

### Testing

- Mechanism ladder: Unit → Parametrized → Property → Integration → Fuzz → Spec acceptance
- Don't pad coverage with tests asserting language guarantees
- Don't capture/assert on log messages
- Property tests for invariants (round-trip, inverse ops, transitivity)
- Prefer `await eventually(...)` over arbitrary sleeps
- Rust: `cargo nextest` with features `ffi,python,high-precision,defi`
- New data types need tests at: DataEngine, Actor, PyO3 dispatch, Python actor, Backtest client, Adapter spec

### Commits

- ≤60 char subject, imperative voice, capitalize, no period
- Body ≤100 char width
- Branch model: `develop` → `nightly` → `master`

### General design rules

- Model the domain directly: typed enums over raw strings; parse at boundaries.
- Use strum derives for string-backed enums; serde attributes for wire shapes.
- Explicit struct construction: show every field, no `..Default::default()` hiding important fields.
- Keep APIs minimal: no config structs for one value, no abstractions for single-use code.
- No wrappers that only rename or forward; helpers must encode a real rule.
- Clone only when ownership transfer or thread-send is required; prefer `&T`.
- Default to `pub(crate)`; use `pub` only for documented API.
- No `unsafe` without explicit approval and a `// SAFETY:` comment.
- No speculative abstractions: extract types only after 2+ real uses.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.