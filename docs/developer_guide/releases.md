# Release Notes Guide

This guide documents the standards for writing release notes in `RELEASES.md`.

## Sections

Use the following sections in this order:

1. Enhancements
2. Breaking Changes
3. Security
4. Fixes
5. Internal Improvements
6. Documentation Updates
7. Deprecations

Omit sections that have no items for a given release.

### Enhancements

New features and user-visible improvements.

**Format**:

```markdown
- Added `subscribe_order_fills(...)` and `unsubscribe_order_fills(...)` for `Actor`
- Added BitMEX conditional orders support
- Added support for `OrderBookDepth10` requests (#2955), thanks @faysou
```

**Guidelines**:

- Start with "Added".
- Use backticks for code elements.
- Be specific about what was added, not how.

### Breaking Changes

Changes that may break existing code.

**Format**:

```markdown
- Removed `nautilus_trader.analysis.statistics` subpackage - must import from `nautilus_trader.analysis`
- Renamed `BinanceAccountType.USDT_FUTURE` to `USDT_FUTURES`
- Changed `start` parameter to required for `Actor` data request methods
```

**Guidelines**:

- Start with "Removed", "Renamed", or "Changed".
- Explain migration path briefly.

### Security

Security hardening and fixes that prevent crashes, undefined behavior, or data corruption.
Includes significant hardening improvements elevated from Internal Improvements.

**Format**:

```markdown
- Fixed non-executable stack for Cython extensions to support hardened Linux systems
- Fixed divide-by-zero and overflow bugs in model crate that could cause crashes
- Fixed core arithmetic operations to reject NaN/Infinity values and improve overflow handling
```

**Guidelines**:

- Include overflow/underflow fixes, memory safety improvements, FFI guards, data integrity fixes.
- Focus on user impact: what could have happened.
- Exclude routine dependency updates, minor hardening, or test-only fixes.
- Omit this section entirely if there are no security items for the release.

### Fixes

Bug fixes that improve correctness but don't qualify as security issues.

**Format**:

```markdown
- Fixed reduce-only order panic when quantity exceeds position
- Fixed Binance order status parsing for external orders (#3006), thanks for reporting @bmlquant
```

**Guidelines**:

- Start with "Fixed".

### Internal Improvements

Implementation details and infrastructure changes.

**Format**:

```markdown
- Added ARM64 support to Docker builds
- Ported `PortfolioAnalyzer` to Rust
- Improved clock and timer thread safety
- Upgraded Rust (MSRV) to 1.90.0
- Upgraded `pyo3` crates to v0.26.0
```

**Guidelines**:

- Use "Added", "Implemented", "Improved", "Optimized", "Upgraded", "Refined", "Standardized".
- Include version numbers for dependency upgrades.

### Documentation Updates

Changes to guides and examples.

**Format**:

```markdown
- Added rate limit tables with links to official docs
- Improved dark and light themes for readability
- Fixed broken links
```

### Deprecations

Features marked for removal.

**Format**:

```markdown
- Deprecated `convert_quote_qty_to_base`; disable (`False`) to maintain consistent behaviour. Will be removed in future version
```

**Guidelines**:

- Explain migration path and provide alternatives.

## Attribution

- Credit external contributors: `thanks @username` or `thanks for reporting @username`.
- Include issue/PR numbers for community contributions and complex features: `(#1234)`.

## Style

- Use sentence case (capitalize first word only).
- Do not end with periods.
- Use backticks for code elements.
- Focus on **what** changed, not how.

**Be specific**:

```markdown
❌ Improved Binance adapter
✅ Improved Binance fill handling when instrument not cached
```

## Security classification

Include in Security if the change addresses:

- Memory safety (overflow, underflow, divide-by-zero that threatens stability).
- Undefined behavior or crashes that could corrupt state.
- Data integrity (NaN/Infinity propagation, race conditions leading to corruption).
- Input validation preventing injection or exploitation (SQL injection, command injection, path traversal).
- Build hardening (non-exec stack, FFI guards).
- Significant hardening that users should know about.

Otherwise use Fixes (for logic bugs and panics) or Internal Improvements (for minor hardening).

Note: Plain logic panics belong in Fixes unless they threaten system stability or data corruption.

## Examples

**Security** (could cause crashes/corruption):

```markdown
- Fixed divide-by-zero in margin calculations that could crash the engine
- Fixed non-executable stack for Cython extensions to support hardened systems
```

**Fixes** (incorrect but safe):

```markdown
- Fixed Binance order status parsing for external orders
- Fixed position purge logic to prevent purging re-opened position
```

**Enhancements** (user-facing):

```markdown
- Added BitMEX conditional orders support
```

**Internal** (implementation):

```markdown
- Implemented BitMEX ping/pong handling
```

## Release workflow

After publishing a release:

1. Update the published release section in `RELEASES.md` with the actual release date.
2. Add the horizontal separator `---` below the completed release.
3. Copy the template below and paste it at the top of `RELEASES.md` for the next version.
4. Update `<VERSION>` to the next version number.
5. Add items to sections as development progresses.

## Release notes template

```markdown
# NautilusTrader <VERSION> Beta

Released on TBD (UTC).

### Enhancements

### Breaking Changes

### Security

### Fixes

### Internal Improvements

### Documentation Updates

### Deprecations

---
```
