# Cargo Patches

This directory contains local Cargo patches for third‑party crates when the workspace must keep a
specific upstream version but needs a small compatibility fix.

**These patches are temporary. Remove each patch when the upstream crate supports the required
dependency and API versions.**

## Arrow and Parquet

`hypersync-client` 1.4.0 requires `arrow` and `parquet` 57.x. The matching Parquet release depends
on the external `thrift` crate. The local compatibility crates satisfy those version constraints
and re‑export the Arrow and Parquet 59.2.0 APIs without copying upstream source.

Remove both compatibility crates when `hypersync-client` supports Arrow and Parquet 59 or later.

## pyo3-stub-gen

`pyo3-stub-gen` stays pinned to `0.20.0` because later versions reject module paths outside the
`pymodule` root. The stub workflow reads `gen_stub_*` module annotations that target
`nautilus_trader` package paths outside the `nautilus_trader._libnautilus` root.

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
package module paths outside the `pymodule` root.
