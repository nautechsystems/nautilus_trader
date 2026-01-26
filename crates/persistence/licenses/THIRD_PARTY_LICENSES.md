# Third-Party Licenses (Persistence crate)

This crate includes code from third-party sources.

- **binary-heap-plus**
  - Usage: The `src/backend/binary_heap.rs` module is vendored from this crate which provides a binary heap with custom comparators.
  - Reason: The `binary-heap-plus` crate depends on the unmaintained `compare` crate.
  - License: MIT/Apache-2.0 (MIT chosen).
  - Source: <https://github.com/sekineh/binary-heap-plus-rs>
  - Full text: `MIT-binary-heap-plus.txt`

- **compare**
  - Usage: The `src/backend/compare.rs` module is vendored from this crate which provides the `Compare` trait.
  - Reason: The `compare` crate is unmaintained.
  - License: MIT/Apache-2.0 (MIT chosen).
  - Source: <https://github.com/contain-rs/compare>
  - Full text: `MIT-compare.txt`
