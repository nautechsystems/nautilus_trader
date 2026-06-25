# Cargo Patches

This directory contains local Cargo patches for third-party crates when the workspace must keep a
specific upstream version but needs a small compatibility fix.

**These patches are temporary. Remove them when the upstream crate supports the required dependency
version and the v2 Python package layout no longer needs the legacy compatibility path.**

## pyo3-stub-gen

`pyo3-stub-gen` stays pinned to `0.20.0` because later versions reject module paths outside the
`pymodule` root. The current stub workflow still supports two paths:

- The legacy Cython-compatible package layout.
- The v2 package layout without the `nautilus_pyo3` namespace.

The crate is licensed as `MIT OR Apache-2.0`. The local copy includes the upstream `LICENSE-MIT`
and `LICENSE-APACHE` texts from `Jij-Inc/pyo3-stub-gen`.

The vendored crate path is excluded from pre-commit and Ruff style checks so those checks do not
rewrite upstream files. Keep local edits limited to the compatibility changes listed below.

The local patch keeps `pyo3-stub-gen 0.20.0` buildable with `pyo3 0.29.0`. It changes only the
PyO3 compatibility surface:

- `src/util.rs`: replaces three removed `Bound<PyAny>::downcast::<T>()` calls with
  `cast::<T>()` for `PyDict`, `PyList`, and `PyTuple`.
- `src/exception.rs`: removes the `PyEnvironmentError` and `PyIOError` stub type impls.
  PyO3 0.29 aliases both names to `PyOSError`, so keeping those impls creates duplicate trait
  impls for the same concrete type.

The patch does not intentionally change generated stub layout, class relocation, module naming, or
signature normalization. Those behaviors stay controlled by `python/generate_stubs.py` and the
pinned `pyo3-stub-gen 0.20.0` code.

Do not update `pyo3-stub-gen` or remove this patch until stub generation no longer depends on the
dual-path layout.
