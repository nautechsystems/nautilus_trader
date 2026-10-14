# Rust

This page defines NautilusTrader conventions for Rust source, Cargo manifests, PyO3 bindings, and tests.

NautilusTrader uses Rust for its mission-critical core because the language combines a strong type
system, an ownership model, and predictable performance. Safe Rust prevents data races and many
memory errors at compile time. `unsafe` code must make explicit the invariants that the compiler
cannot check.

`rustfmt` and the workspace lints own general Rust style. The rules below supplement them.

## Sources of truth

| Concern                    | Source                                             |
| -------------------------- | -------------------------------------------------- |
| Formatting and imports.    | `rustfmt.toml`.                                    |
| Workspace lints.           | `Cargo.toml` and `clippy.toml`.                    |
| Cargo layout.              | `.pre-commit-hooks/check_cargo_conventions.sh`.    |
| Rust layout.               | `.pre-commit-hooks/check_formatting_rs.sh`.        |
| Nautilus type conventions. | `.pre-commit-hooks/check_nautilus_conventions.sh`. |
| Async boundaries.          | `.pre-commit-hooks/check_tokio_usage.sh`.          |
| DST boundaries.            | `.pre-commit-hooks/check_dst_conventions.sh`.      |
| PyO3 bindings.             | `.pre-commit-hooks/check_pyo3_conventions.sh`.     |
| Anyhow.                    | `.pre-commit-hooks/check_anyhow_usage.sh`.         |
| Error names.               | `.pre-commit-hooks/check_error_conventions.sh`.    |
| Logging.                   | `.pre-commit-hooks/check_logging_conventions.sh`.  |
| Rustdoc contracts.         | `.pre-commit-hooks/check_docs_conventions.sh`.     |
| Test style.                | `.pre-commit-hooks/check_testing_conventions.sh`.  |
| Workspace test selection.  | `scripts/ci/check-workspace-test-coverage.sh`.     |

Match nearby code when the tools do not settle a choice. Change generator inputs and regenerate
outputs instead of editing generated files directly.

## Module layout

Arrange each hand-written module so its primary behavior appears before its supporting
implementation details.

### Module roots and declarations

Preserve the surrounding module-root style when adding child modules. In each contiguous
declaration block in a `mod.rs` file, order out-of-line module declarations by these sections:

1. `#[macro_use]` modules.
1. Public modules.
1. Restricted modules such as `pub(crate)`.
1. Non-test `#[cfg(...)]`-gated modules.
1. Private modules.
1. Test-only modules.

Alphabetize declarations within each section and leave one blank line between sections. The
formatting hook enforces this order in `mod.rs` files.

Use the narrowest visibility that serves the caller. The workspace denies unreachable `pub` items.

### Imports

Keep imports at the top of the file or module. Use a local import only when its narrow scope
materially improves clarity.

### Constants and global state

Group module-wide constants and global state near the top of the module. Put `const` items before
`static` and `thread_local!` declarations. Keep a narrowly used constant or static next to its
consumers instead.

### Primary types and implementations

Keep the primary type and its inherent implementation near the top of a module.

### Enums

Place enums by role rather than collecting them in one module-wide block:

- Keep a primary or public enum with the module's primary types.
- Keep a small state enum next to the global whose state it represents.
- Place a private supporting enum below its first caller.

### Supporting definitions

Place private functions and types below their callers. In adapters, place private route types,
decision enums, and parsing functions below the main client implementations.

### Box-style banner comments

Do not add box-style banner or separator comments to divide module contents. Use modules and
implementation blocks to express structure. The standard copyright and license header is the
exception.

## Cargo manifests

### Dependencies and sections

- Use workspace inheritance for shared dependencies, for example
  `serde = { workspace = true }`. Pin a version in a crate only when the dependency is not
  workspace-managed.
- Separate dependency groups with a blank line and alphabetize each group. Manifests normally group
  internal `nautilus-*` crates, required external crates, and optional external crates, but preserve
  a manifest's meaningful local groups.
- Keep optional crates in their own blank-line group, except when every crate in the group is a
  `nautilus-*` crate.
- Keep the standard section order:

  1. Package.
  1. Lints.
  1. Library.
  1. Features.
  1. `cargo-machete` metadata.
  1. docs.rs metadata.
  1. Dependencies.
  1. Development dependencies.
  1. Build dependencies.
  1. Benches.
  1. Binaries.
  1. Examples.
  1. Tests.

- Add `[lints] workspace = true` to every workspace crate with a library or binary target.
- Keep adapter dependencies in the `# Adapter dependencies` section of the workspace
  `Cargo.toml`. Core crates must not depend on entries from that section.
- Keep related dependency families compatible. The Cargo convention hook checks the known
  constraints, including `capnp` with `capnpc`, `arrow` with `parquet`, and
  `dydx-proto` with `prost` and `tonic`.
- List only declared dependencies under `[package.metadata.cargo-machete] ignored`.
- Remove a root `[workspace.dependencies]` entry when no crate uses it. Cargo tools kept only for CI
  are exempt from this check.
- Remove a root `[workspace.package]` field when no crate inherits it.
- List each `[workspace] members` entry as a literal path. The convention hook resolves
  member manifests directly and does not expand Cargo glob members.
- Obtain `libfuzzer-sys` in adapter crates through `nautilus-live`. Do not add it directly to an adapter
  manifest.

When `crates/pyo3/Cargo.toml` groups adapters separately, keep its `# Adapters` block below the core
internal crates.

### Package fields

Crate `[package]` sections use this canonical prefix. Cargo infers `README.md` next to the
manifest, so omit `readme`. The other fields shown are required.

```toml
[package]
name = "nautilus-example"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
description = "Example crate for NautilusTrader"
categories.workspace = true
keywords.workspace = true
documentation.workspace = true
repository.workspace = true
homepage.workspace = true
```

Place the optional `publish`, `build`, and `include` fields after `homepage.workspace`.

### Features

- Preserve the crate's existing default feature contract. Most core crates have empty defaults,
  while many adapter crates enable `high-precision` by default.
- Include `"python"` in every `extension-module` feature that builds a Python artifact. Keep it next
  to `"pyo3/extension-module"`.
- Propagate `high-precision` to dependent Nautilus crates that store or construct fixed-point
  domain values.
- Document each public non-default feature once, in alphabetical order, under `## Feature flags` in the
  crate README and `# Feature Flags` in the crate-level library docs.

### Targets

- Use snake_case filenames for `bin/` sources and kebab-case executable names, for example
  `path = "bin/ws_data.rs"` and `name = "hyperliquid-ws-data"`.
- Set `doc = false` on binary and example targets. Also set `test = false` on binary targets.
- Order library crate types as `rlib`, `staticlib`, then `cdylib` when a crate produces more than
  one type.

## File header requirements

Copy the standard copyright and license header from a neighboring hand-written Rust file. Generated
files retain their generator header instead. The copyright hook checks the year.

Change a generator input and rerun the generator instead of editing generated Rust, C headers,
Python stubs, or wrapper doc comments.

## Formatting and attributes

`rustfmt` groups standard library, external crate, and local imports, then alphabetizes each group.
Run the formatter instead of ordering imports by hand.

Leave one blank line:

- Between functions, including tests.
- Above each `///` or `//!` doc comment.
- Above standalone `if`, `match`, `for`, `while`, and `loop` expressions.
- Above task spawn calls, including `spawn_local` and `spawn_blocking`.
- Before a `let` that contains a multiline braced expression, unless it starts a block.
- Before a multiline struct literal or a direct qualified `Type::new(...)` call, unless it starts a block.
- After a statement that contains a multiline braced expression, before the next statement in the same block.

The control-flow and spawn rules do not apply when the expression starts a block, continues the
previous operation, or has an attached comment or attribute. Wrapped calls and method chains alone
do not need a separator.

Construction spacing also applies to assignments, explicit returns, and calls followed by `?` or `.await`.
Construction must span multiple lines after rustfmt layout. One-line initializers stay together.

Keep comments and attributes attached to their statement when inserting a separator. Skip macro bodies and
`rustfmt::skip` regions. Apply the multiline statement rules around changed code. Leave unrelated code alone.
Add only blank lines that `rustfmt` preserves.

The formatting hook:

- Checks existing control-flow, spawn, and module-ordering rules at changed boundaries.
- Compares staged and unstaged changes against `HEAD`, or against the merge base with `CHANGED_BASE_SHA` when set.
- Falls back to checking all tracked Rust files when a CI base is unavailable.
- Reads complete changed files for context and includes lines used by exemption checks when selecting diagnostics.
- Does not modify files.

These remain review conventions:

- Multiline statement spacing.
- Construction spacing.
- `spawn_local` and `spawn_blocking` spacing.

Keep changed code readable for a human reader. If those separators still leave a dense block hard to follow,
add more blank lines.

Use inline format arguments for existing variables:

```rust
anyhow::bail!("Failed to subtract {n} months from {datetime}");
```

If discarding a returned value is almost certainly a mistake, add `#[must_use]` to these functions:

- Constructors.
- Accessors.
- Pure conversions.
- Consuming `with_*` methods.

A return type such as `Result` already carries its own `must_use` annotation.

When suppressing `missing_panics_doc` or `missing_errors_doc`, include a reason that explains why
the lint does not apply:

```rust
#[allow(
    clippy::missing_errors_doc,
    reason = "result type is retained for API compatibility but no error is returned",
)]
```

Use `try_update` instead of the deprecated `fetch_update` on standard atomics. The pre-commit hook
rejects `fetch_update`.

## Type qualification

| Item                          | Convention                                                                  |
| ----------------------------- | --------------------------------------------------------------------------- |
| `anyhow`.                     | Import only `anyhow::Context`. Fully qualify macros and `Result`.           |
| Nautilus domain types.        | Import the type, then use its short name.                                   |
| Tokio time, sync, and tasks.  | Fully qualify the path. Use `std::time::Duration`.                          |
| `Debug` and `Display`.        | Import the trait.                                                           |
| `Formatter` and `fmt::Result` | Use `std::fmt::Formatter` and `std::fmt::Result` at the implementation.     |
| Nautilus macros.              | Import `nautilus_actor!` or `nautilus_strategy!`, then call it unqualified. |

Use `debug_struct(stringify!(TypeName))` in manual `Debug` implementations.

Use `// nautilus-import-ok` only where a macro, generated path, or conditional import requires a
fully qualified Nautilus type. Put the marker on the affected line or directly above the narrow
block.

## Error handling and contracts

### Design by contract

Use the narrowest mechanism that expresses the contract:

| Situation                                          | Mechanism                                    |
| -------------------------------------------------- | -------------------------------------------- |
| Compile-time state or ownership rule.              | Types, lifetimes, newtypes, and visibility.  |
| Public input precondition.                         | `check_*` from `nautilus_core::correctness`. |
| Validated value construction.                      | `new_checked()` with a `new()` wrapper.      |
| Recoverable parse, I/O, or network failure.        | `Result<T, E>`.                              |
| Internal invariant safe to omit in release builds. | `debug_assert!`, covered by a targeted test. |
| Soundness or always-on invariant.                  | `assert!` or a checked error path.           |

Place an assertion where the code first relies on the invariant. Never use `debug_assert!` for
public input validation or a soundness condition because release builds remove it.

Prefix debug assertion messages with `Invariant:` and state the positive rule. The shared
`Condition failed: ...` prefix marks a caller input violation. `Invariant: ...` marks an internal contract
bug.

### Error boundaries

Choose the error type at the API boundary:

| Boundary                              | Return type                         |
| ------------------------------------- | ----------------------------------- |
| Reusable library or domain API.       | A typed `Result<T, E>`.             |
| Application or adapter orchestration. | `anyhow::Result<T>`.                |
| Public input validation.              | `CorrectnessResult<T>` when suited. |

- Define a typed error with `thiserror` when callers inspect or recover from the failure.
- Bind error patterns and closures as `e`, not `err` or `error`.
- Use `anyhow::bail!` for an early return from an `anyhow::Result` function. Use
  `anyhow::anyhow!` when an error value is required, such as inside `ok_or_else`.
- Start `.context()` messages with lowercase text so chained errors read naturally. Preserve the
  capitalization of a leading proper noun or acronym.
- Do not use `", got"` in an error or assertion. Use `", was"`, `", received"`, or `", found"`
  according to the context.

```rust
parse_timestamp(value).context("failed to parse timestamp")?;
connect().context("Bybit websocket did not become active")?;
```

### Panic policy

Panics are valid for internal invariant violations that callers cannot reasonably recover from.
Keep an API infallible when returning `Result` would only require callers to handle a programming
defect. Use an always-on assertion for an invariant that protects soundness or prevents unsafe code
from continuing with invalid state.

Return `Result` at boundaries that accept untrusted input. Also return `Result` at boundaries with failures
from configuration, I/O, external data, task cancellation, or resource lifecycle. These are operational
failures even when a specific call site expects them never to occur.

For validated values, follow the [`new()` and `new_checked()` constructor pattern](#constructor-patterns).
Expose a fallible path for untrusted values. Use the convenience wrapper only where its caller contract
permits a panic. Document reachable panics in public Rust APIs, and make each panic message name the violated
invariant.

Treat findings for these lints from `make clippy-strict-audit` as review prompts:

- `clippy::panic`
- `clippy::unwrap_used`
- `clippy::expect_used`

The audit also reports `clippy::panic_in_result_fn`, which the required workspace Clippy gate already
enforces. Remove a panic when the failure is recoverable. Retain a justified invariant panic, with a scoped
lint reason when needed. The audit uses forced warnings, so its totals include deliberate sites with local
lint allowances and diagnostics from macro expansions.

### Failure contract examples

APIs with a documented panic contract use panics for:

- Programmer errors (logic bugs, incorrect API usage).
- Data that violates fundamental invariants (negative timestamps, NaN prices).
- Arithmetic that would silently produce incorrect results.

APIs return `Result` or `Option` when callers, including downstream crates, can handle a failure or
absence, including:

- Expected runtime failures (network errors, file I/O).
- Business logic validation (order constraints, risk limits).
- User input validation.

The API determines how an invalid operation fails:

```rust
let later = timestamp + duration_ns; // Panics on overflow.

let price = Price::new_checked(f64::NAN, precision); // Returns Err.

let later = timestamp.checked_add(duration_ns); // Returns None on overflow.
```

This policy is implemented throughout the core types (`UnixNanos`, `Price`, `Quantity`, etc.)
and helps NautilusTrader maintain strong data correctness for production trading.

The repository release profile sets `panic = "abort"`, so a panic terminates the process for a
supervisor or orchestration system to handle. Downstream Rust binaries control their own release
profile.

## Logging

- Fully qualify log macros, for example `log::debug!` and `log::info!`.
- Start messages with a capitalized word and omit terminal periods.
- A terminal period can be part of a logged value instead of sentence punctuation. For this case, use
  `// log-period-ok` on the call or within three lines above it.
- Keep connection and client lifecycle, reconnection, reconciliation, and mass-status summaries at
  `INFO`.
- Keep subscription detail, per-order confirmations, instrument counts, authentication, and
  WebSocket internals at `DEBUG`.
- Log a warning or error when an unexpected, user-actionable, or data-loss condition is handled
  locally. Do not log an error that continues to propagate through `?` or `anyhow::bail!`.
- Leave a blank line above a log call unless it is the first line of the function.
- Do not write to stdout or stderr or terminate the process from production library code. Binaries,
  examples, benches, tests, adapters, the CLI, and testkit may control their process when that is
  part of their role.

## Async code

Synchronous core crates do not take Tokio as a regular dependency. The `common` crate keeps Tokio
optional. The Tokio convention hook owns the exact crate list.

Adapter production code uses the shared runtime because calls may arrive from Python threads without
a thread-local Tokio context:

```rust
use nautilus_common::live::get_runtime;

get_runtime().spawn(async move {
    run_client().await;
});
```

- Import `get_runtime` through `nautilus_common::live`, not `live::runtime`.
- Use `get_runtime().block_on()` when synchronous adapter code must call an async function.
- Use the runtime supplied by `#[tokio::test]` in tests. `tokio::spawn()` is valid there.
- A site can be unable to use the fully qualified form or shared runtime. Only for such sites, put
  `// tokio-import-ok` on the import or spawn line.
- Install a custom runtime before `LiveNode::build()` or any adapter use. Build a multi-threaded
  runtime with all drivers enabled.
- `set_runtime()` bypasses the default initializer, including Python initialization. With the
  `python` feature enabled, initialize Python before installing a custom runtime or keep the default
  runtime.

Code on the deterministic simulation path follows the
[DST determinism contract](../concepts/dst.md#determinism-contract). Route clocks, random values,
task spawning, and network access through the project seams. Use `biased;` in `tokio::select!`
blocks on that path.

## Runtime ownership and access

### Cache order access

`Cache` hides its `SharedCell` order storage behind lifetime-scoped accessors. Use these lifetime-scoped
accessors:

- **Scoped read**: `order_ref()` or `try_order_ref()`.
- **Exclusive write borrow**: `order_mut()`.
- **Snapshot across a boundary**: `order_owned()` or `try_order_owned()`.

The `try_*` forms return `OrderLookupError` when the order is absent.

`order_mut()` requires `&mut Cache`, so adapter-facing `CacheView` code cannot mutate orders. Before event
dispatch or a borrow that can re-enter the same order, drop the `OrderRef` or `OrderRefMut`. Then look up the
order again for post-event state.

### Runtime invariants

The actor registry, component registry, and message bus use `thread_local!` storage. Objects
registered on one thread are not visible from another. `LiveNodeHandle` is the intended
cross-thread control surface.

Keep an `ActorRef` within one synchronous scope. Do not store it in a struct, hold it across
`.await`, or send it to another thread. Capture the actor ID in long-lived callbacks and look up the
actor each time the callback runs.

The component registry rejects aliased mutable access with a scoped borrow guard. Do not make
component lifecycle operations re-entrant.

## Construction and conversion

### Constructor patterns

Validated value types pair a fallible `new_checked()` with a convenience `new()`:

```rust
pub fn new_checked<T: AsRef<str>>(value: T) -> CorrectnessResult<Self> {
    let value = value.as_ref();
    check_valid_string_ascii(value, stringify!(value))?;
    Ok(Self(Ustr::from(value)))
}

pub fn new<T: AsRef<str>>(value: T) -> Self {
    Self::new_checked(value).expect_display(FAILED)
}
```

Use the shared `FAILED` constant with `CorrectnessResultExt::expect_display` so panic messages use
the standard `Condition failed: ...` prefix. Document the error on `new_checked()` and the panic on
`new()`.

If a long constructor mostly has optional fields, use a `bon` builder. For such types, do not expose public
positional `new()` and `new_checked()` methods. Put `#[bon::bon]` on the inherent implementation. Make the
builder's finish method delegate to a private validation function or perform validation directly. Keep one
validation and defaulting path.

```rust
#[builder(start_fn = builder, finish_fn = build)]
pub fn build_checked(/* same inputs as new_checked */) -> CorrectnessResult<Self> {
    Self::new_checked(/* forward inputs */)
}
```

Required fields remain required in `bon` typestate. Optional fields remain omittable, and `build()`
returns the same `CorrectnessResult` as the internal validation path.

### Conversion patterns

- Implement `From` only for infallible, complete conversions.
- Implement `TryFrom` when the source can contain an invalid or unrepresentable value.
- Implement `FromStr` for string parsing.
- Treat generic `From<T: AsRef<str>>` implementations that panic as compatibility surfaces. Do not
  copy that pattern into new APIs.
- Keep venue wire enums distinct from Nautilus domain enums. Use idiomatic Rust variant names,
  express the wire spelling with `serde` or `strum` attributes, and convert explicitly at the
  boundary.
- Deserialize adapter payloads into wire models before constructing domain objects. Keep parsing
  and validation in conversion functions instead of embedding venue wire details in domain types.

### Domain numeric types

Preserve discrete financial values as decimals from ingestion:

| Value                                    | Type and construction                                    |
| ---------------------------------------- | -------------------------------------------------------- |
| Price or quantity.                       | `Price::from_decimal_dp` or `Quantity::from_decimal_dp`. |
| Money, fee, margin, or balance.          | `Decimal`, then `Money::from_decimal` or `Money::zero`.  |
| Continuous signal ratio or timing curve. | `f64` when decimal precision has no domain meaning.      |

Do not route wire values through `f64` constructors. In tests, compare `.as_decimal()` with
`dec!(value)`.

## Identifier storage

### How string interning works

String interning stores one shared copy of each distinct string in a central cache. Repeated values
refer to the same cached bytes instead of allocating another copy. Small handles make the values
cheap to copy and compare, while a cached hash avoids reading the full string again during hashing.

NautilusTrader uses `Ustr` for its interned identifier components. Each `Ustr` is a pointer-sized
`Copy` handle with a precomputed hash and stable direct string access. Composite types such as
`InstrumentId` preserve the same cheap copy semantics by storing these handles.

### Reclamation boundary

The string cache retains every unique value for the process lifetime. This retention keeps copied
handles and returned string slices valid without reference counting, access guards, or explicit
lifetime parameters on identifier types. Process teardown is the normal reclamation boundary.

These guarantees rule out safe reclamation of individual entries. Rust can copy a `Copy` value
without executing code, so an atomic reference count cannot observe every copy. Designs that add
reclamation change the identifier contract:

- Reference counting requires `Clone` and `Drop`, which removes `Copy` from identifiers and types
  that contain them.
- Borrowed or epoch-protected storage requires lifetimes or access guards at string access points.
- Generational handles permit reclamation but make lookup fallible and invalidate stale handles.
- A global cache reset is safe only at a proven quiescent point. All handles, references, and foreign pointers
  must be destroyed first. No task or thread can retain one at that point.

### Storage boundaries

Interning is best suited to identifiers drawn from a bounded process-scoped universe and values that
repeat enough to benefit from deduplication. Identifiers whose distinct values can grow with every
order, trade, or message increase the cache for the process lifetime.

Fixed-capacity inline storage retains `Copy` when the external protocol supplies a suitable maximum.
`TradeId`, for example, uses a 36-character `StackStr`. Owned or reference-counted storage provides
dynamic capacity when reclamation matters more than `Copy`.

The domain model also contains compatibility exceptions. `ClientOrderId`, `VenueOrderId`,
`PositionId`, and `OrderListId` remain `Ustr`-backed and therefore retain every distinct value.
Identifier storage participates in the supported by-value C ABI, so a broader redesign depends on
conversion-based bindings replacing raw layout sharing.

The storage boundary includes an up-front estimate of each unique value and all intermediate strings interned
during parsing. All `Ustr` uses in the process share the cache. Thus, its memory cost comes from the aggregate
set, not a separate budget for each identifier type.

### Polymarket scale example

A Polymarket instrument symbol combines a 66-byte condition ID with a 77- or 78-byte token ID, for a 144- or
145-byte interned symbol. With the 64-bit `ustr` 1.1.0 layout, 600,000 unique `InstrumentId` values need
approximately 150 MiB. This covers retained identifier values, the cache lookup table, and reserved string
storage.

The Polymarket parsing path also interns each raw token ID and each condition ID. For 600,000
instruments from about 300,000 markets, these entries raise the estimate to roughly 300 MiB before
instrument objects, descriptions, maps, and other metadata. The estimate assumes unique instrument
and token IDs and includes capacity reserved by the cache's geometric allocator, so it is not an
exact resident-set measurement.

NautilusTrader accepts this bounded cost to preserve `Copy`, stable direct access, and global
deduplication across the instrument universe. Unbounded streams of unique external IDs remain
outside this storage model.

## Collections

Choose a hash collection by iteration semantics and trust boundary:

| Requirement                                    | Collection                |
| ---------------------------------------------- | ------------------------- |
| Observable insertion-order iteration.          | `IndexMap` or `IndexSet`. |
| Hot lookup with no observable iteration order. | `AHashMap` or `AHashSet`. |
| Keys chosen by an untrusted third party.       | `HashMap` or `HashSet`.   |
| External API requires a standard collection.   | `HashMap` or `HashSet`.   |

Default `AHashMap` and `AHashSet` instances randomize their hasher state, so their iteration order is not
stable. If iteration order affects any of these outcomes, make that order deterministic:

- Emitted events.
- Commands and traffic sent to a venue.
- Persisted output.
- Random number consumption.

An iteration whose order no caller observes does not need stabilization.

Use `IndexMap` or `IndexSet` when insertion order is both deterministic and the required sequence. Otherwise,
keep the hash collection and sort at the point of use. Prefer sorting when ordered traversal is infrequent or
order-preserving removal is hot. `shift_remove` preserves relative order in O(n) average time. `swap_remove`
runs in O(1) average time but can move the last entry.

`AHash` is not cryptographically secure. Use `HashMap` or `HashSet` where untrusted keys make
hash-flooding resistance part of the security boundary.

## Documentation

### Coverage and tone

- Add doc comments to public items. Document private behavior with a normal comment only when
  non-obvious context prevents a likely misreading.
- Add module documentation to public modules and modules with a non-obvious contract. Do not add
  boilerplate to a private leaf module.
- Use the indicative mood: "Returns the account ID", not "Return the account ID".
- End public field and enum variant documentation with a period.
- Match documentation density to neighboring items. Do not add filler comments to make a bare block
  look uniform.
- Put important context for private fields in the type-level documentation instead of documenting
  each field.

Document public functions when their contract is not fully clear from the type and name. Cover
fallible conditions, panic conditions, safety obligations, and non-obvious input semantics.

### Rustdoc sections

Use Title Case for Rustdoc section headings:

- `# Examples`
- `# Errors`
- `# Panics`
- `# Safety`
- `# Notes`
- `# Thread Safety`
- `# Feature Flags`

Start `# Errors` descriptions with `Returns an error if`, or ``Returns `None` if`` for an `Option`
return. Start `# Panics` descriptions with `Panics if`. Describe the failure conditions without
repeating concrete error types from the signature. Keep distinctions that affect caller behavior,
such as retryable versus permanent failures.

Use one sentence ending with a period for a single condition:

```rust
/// # Errors
///
/// Returns an error if the currency conversion fails.
```

For multiple conditions, use the same lead-in followed immediately by a colon:
`Returns an error if:`, ``Returns `None` if:``, or `Panics if:`. Put each condition in a bullet that
starts with a capital letter and ends with a period. A wrapped bullet needs its period at the end
of its final line.

```rust
/// # Errors
///
/// Returns an error if:
/// - The market price cannot be found.
/// - The conversion rate calculation fails.
```

```rust
/// # Panics
///
/// Panics if:
/// - The quantity is zero.
/// - The quantity exceeds the supported range.
```

The documentation hook checks these lead-ins and bullet periods in `# Errors` and `# Panics` sections. It
skips fenced code blocks and stops at the next section heading or the end of the doc block. The suppression
markers below exempt contract checks only. They do not exempt wording or punctuation checks.

Use `# Errors` only on a function that returns `Result`, `PyResult`, or `Option`. Use `# Panics` only when the
function can panic. If the function cannot panic, remove the section. Do not state that it does not panic.

The documentation hook recognizes direct panic sites in a `Result`-returning function. Put
`// panics-doc-ok` immediately above the doc block when a called function supplies the documented
panic. Use `// errors-doc-ok` in the same position only for a special error contract that the
signature check cannot recognize.

### Doc examples

`make cargo-test-doc` compiles and runs doc examples. Annotate every fence:

| Fence          | Behavior                  | Use for                                                |
| -------------- | ------------------------- | ------------------------------------------------------ |
| `rust`         | Compiled and run.         | Self-contained examples with no external dependencies. |
| `rust,no_run`  | Compiled, not run.        | Examples needing a catalog, network, or venue.         |
| `ignore`       | Neither compiled nor run. | Pseudocode. State why it cannot compile.               |
| `compile_fail` | Must fail to compile.     | Demonstrating a rejected usage.                        |
| `text`         | Not code.                 | Directory trees, output, or diagrams.                  |
| `bash`, `json` | Not Rust.                 | Commands or payloads.                                  |

Prefer `no_run` to `ignore` when only the runtime dependency is unavailable. Prefix setup lines with
`#` when they must compile but would obscure the rendered example.

Put examples on public items. Rustdoc also collects fences from private item docs, but those examples
cannot import the item they document.

## Python bindings

### PyO3 names and errors

- Prefix a Rust function renamed with `#[pyo3(name = "...")]` with `py_`.
- When a binding needs a Rust-only wrapper type, prefix it with `Py` and expose the Python name
  without that prefix.
- Use `nautilus_trader.adapters.<adapter_name>` for public adapter stub metadata. Runtime module
  paths use `nautilus_trader._libnautilus.<adapter_name>`.
- Convert standard Python exceptions with `to_pyvalue_err`, `to_pytype_err`, `to_pyruntime_err`,
  `to_pykey_err`, `to_pyexception`, or `to_pynotimplemented_err` from
  `nautilus_core::python`.

The submodules registered in `crates/pyo3/src/lib.rs` are public API. Do not add an internal crate as
a new Python submodule as a refactor side effect. Register an intentional class in an existing
submodule when that preserves the public package structure. A deliberate submodule change also
updates the allowlist in `check_nautilus_conventions.sh`.

Keep each submodule registration as `let n = "<name>"` followed by one
`pyo3::wrap_pymodule!(<path>)` call. The name must match the final component of the target path.

### PyO3 enums

Nautilus domain integer enums use `frozen`, `eq`, `eq_int`, `from_py_object`, and
`rename_all = "SCREAMING_SNAKE_CASE"`.

Do not add PyO3's `hash` attribute to an `eq_int` enum. Its generated hash differs from Python's hash
for the equal integer discriminant and breaks the rule that equal values have equal hashes. Return
the discriminant directly instead:

```rust
#[pymethods]
impl MyEnum {
    const fn __hash__(&self) -> isize {
        *self as isize
    }
}
```

### Type stub annotations

Every Python-exposed type and function needs the matching `pyo3-stub-gen` annotation:

| PyO3 construct    | Stub annotation                                 |
| ----------------- | ----------------------------------------------- |
| `#[pyclass]`      | `pyo3_stub_gen::derive::gen_stub_pyclass`.      |
| Enum `#[pyclass]` | `pyo3_stub_gen::derive::gen_stub_pyclass_enum`. |
| `#[pymethods]`    | `pyo3_stub_gen::derive::gen_stub_pymethods`.    |
| `#[pyfunction]`   | `pyo3_stub_gen::derive::gen_stub_pyfunction`.   |

- Put class and enum stub annotations in `#[cfg_attr(feature = "python", ...)]` directly below the
  runtime `pyo3::pyclass` attribute.
- Put `gen_stub_pymethods` directly below `#[pymethods]`.
- Put `gen_stub_pyfunction` after doc comments and directly above `#[pyfunction]`.
- Set the stub `module` to the package from which Python imports the object.
- Add `pyo3-stub-gen` as an optional dependency and include it in the `python` feature.

### Generated Python artifacts

The Python surface commits generated `.pyi` files under `python/nautilus_trader/` and generated
wrapper doc comments under `crates/**/src/python/`. Regenerate both with:

```bash
make py-stubs
```

Run the target after changing a Python-exposed Rust item, its stub annotation, its core doc comments,
or adapter feature wiring. Commit every changed generated artifact with the source change.

The stub generator removes `extension-module` before invoking Cargo. If a feature is enabled only through
`extension-module`, add it explicitly to `cargo_features` in `python/generate_stubs.py`. If you omit it, the
feature's exported types disappear from the generated stubs. The Interactive Brokers `gateway` feature is the
model.

The Python targets accept the uv minor series defined by `required-version` in
`python/pyproject.toml`. `make sync`, `make py-stubs`, and `make build-debug` stop when the installed
version falls outside that range. Run `make update-uv` to install the exact project version from
`.nautilus-engineering/tools.toml`.

Do not edit wrapper `///` comments in `crates/**/src/python/`. Edit the core Rust item docs and
regenerate. The doc sync:

- Preserves `# Errors` and `# Safety`.
- Drops `# Panics` so a panic contract does not cross the Python API boundary.
- Removes Rust intra-doc links.
- Converts Rust `::` paths to Python `.` paths.

### Rust-Python object ownership

`Py<T>` owns a reference to a Python object. Clone it with `Py::clone_ref` while attached to the
interpreter or with `nautilus_core::python::clone_py_object`. An extra `Arc<Py<T>>` is normally
unnecessary because `Py<T>` already provides shared ownership.

Cloning a Python reference does not break a cycle. Use a Python weak reference when a back-reference
must not keep its target alive.

## Testing conventions

- Use `mod tests` for ordinary inline tests.
- Use `#[rstest]` instead of `#[test]`, including for a non-parameterized test.
- Use `#[tokio::test]` for a non-parameterized async test.
- Keep `#[cfg(test)]` on test modules and test-only files. Do not add test behavior or interfaces to
  production code.
- Store JSON fixtures under the crate's `test_data/` directory and load them with `include_str!`.
- Use distinct, non-default inputs and exact expected values. Assert every stable field.
- Group assertions after setup and actions unless the test checks stepwise state changes.
- Do not use Arrange, Act, Assert separator comments.

Each workspace member appears exactly once in `CORE_CRATES`, `ADAPTER_CRATES`, or `NO_TEST_CRATES` in the
`Makefile`. Add a crate to `NO_TEST_CRATES` only when both conditions hold:

- It defines no Rust test target.
- It contains no `#[test]`, `#[rstest]`, or `#[test_case]` function.

Parametrize cases when the same behavior applies to several inputs:

```rust
#[rstest]
#[case("AUDUSD", false)]
#[case("CL.FUT", true)]
fn test_symbol_is_composite(#[case] input: &str, #[case] expected: bool) {
    assert_eq!(Symbol::new(input).is_composite(), expected);
}
```

### Test specs

Events with many constructor arguments use a fluent `bon` spec next to the event under
`events/order/spec/`. Gate the module with `#[cfg(any(test, feature = "test-support"))]` so downstream
tests can opt in without adding the spec to production builds.

- Derive `bon::Builder` with `finish_fn = into_spec`.
- Give required fields deterministic valid defaults. Leave optional fields as `Option<T>`.
- Default event IDs with `test_uuid()` from `crate::stubs`.
- Implement `build()` by forwarding through the production constructor.
- Return the event directly because spec defaults are valid by construction.
- Pin every default in one test in the spec module.

Override only the fields relevant to the caller:

```rust
let fill = OrderFilledSpec::builder()
    .last_qty(Quantity::from(50_000))
    .trade_id(TradeId::from("TRADE-1"))
    .build();
```

Under plain `cargo test`, call `reset_test_uuid_rng()` before a test that compares UUID sequences.
`cargo nextest` starts each test in a fresh process, so the sequence resets automatically.

### Property-based tests

Use `proptest` when an invariant spans an input class that examples cannot cover. Keep strategies
near the property suite and combine ranges with explicit edge cases.

Prefix property names with `prop_` and retain `#[rstest]` on tests inside `proptest!`. A large suite
may use a `property_tests` module or a dedicated file, but the repository also keeps focused
properties beside their unit tests.

## Unsafe Rust

Unsafe code must make its proof obligations reviewable:

- Give each unsafe function a `# Safety` section that states the caller's complete obligations.
- Put a `SAFETY:` comment directly above each unsafe operation and explain why its preconditions
  hold at that site.
- Wrap each unsafe operation in its own `unsafe { ... }` block. Workspace crates deny
  `unsafe_op_in_unsafe_fn`.
- Use always-on checks for null, alignment, provenance, and other soundness conditions.
- Add targeted tests for observable behavior around unsafe code. Tests support the proof but do not
  establish soundness.
- An unsafe `Send` or `Sync` implementation needs a proof over all reachable behavior:

  - State.
  - Aliases.
  - Callbacks.
  - Generic parameters.
  - Safe methods.
  - Cloning.
  - Destruction.

For raw vectors crossing FFI, follow the [FFI memory contract](ffi.md). Foreign code owns the
allocation after transfer and must call the matching `vec_drop_*` function exactly once.

## Other generated artifacts

### FFI bindings and precision

Only `nautilus-core` and `nautilus-model` expose an `ffi` feature. Check both crates directly when
changing their C ABI:

```bash
cargo check -q -p nautilus-core --features ffi
cargo check -q -p nautilus-model --features ffi,python,high-precision
```

The crate-local `cbindgen.toml` files define the header layout for native consumers. Do not add an
`ffi` feature or `src/ffi` module to another workspace crate.

Run `make check-cbindgen-abi`. It generates both headers and checks their public names and compatibility enum
values. It also compiles a C11 consumer against them. The scheduled nightly tests run this check.

### Cap'n Proto schemas

Schema files live under `crates/serialization/schemas/capnp/`, and generated Rust lives under
`crates/serialization/generated/capnp/`.

- Add fields at the end.
- Do not remove fields or reuse field numbers. Mark obsolete fields as deprecated in comments.
- Regenerate with `make regen-capnp`.
- Review `git diff crates/serialization/generated/capnp`.
- Run `make check-capnp-schemas` to verify the checked-in output.
- Test the serialization crate with `make cargo-test-crate-nautilus-serialization`.

Install the pinned compiler as described in
[Environment setup](environment_setup.md#capn-proto). Generated bindings stay checked in so
docs.rs can build without the compiler.
