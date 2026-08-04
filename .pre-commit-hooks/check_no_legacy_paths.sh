#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping legacy path checks"
  exit 0
fi

echo "Checking for legacy paths and FFI surfaces..."

if matches=$(rg -n "nautilus_pyo3" python --glob '!_fixup.py' 2> /dev/null); then
  echo "Error: found legacy nautilus_pyo3 references under python/"
  echo
  echo "$matches"
  echo
  echo "Use the public Python surface or _libnautilus internals instead."
  exit 1
fi

if matches=$(rg -n "cython-compat" crates python Cargo.toml Makefile 2> /dev/null); then
  echo "Error: found removed cython-compat references"
  echo
  echo "$matches"
  exit 1
fi

if matches=$(rg --files crates | rg '/cbindgen_cython\.toml$|\.(pyx|pxd|pxi)$'); then
  echo "Error: found removed Cython or Cython cbindgen files"
  echo
  echo "$matches"
  exit 1
fi

expected_ffi_manifests=$(printf '%s\n' crates/core/Cargo.toml crates/model/Cargo.toml)
actual_ffi_manifests=$(rg -l '^ffi[[:space:]]*=' crates --glob Cargo.toml | sort)
if [[ "$actual_ffi_manifests" != "$expected_ffi_manifests" ]]; then
  echo "Error: ffi features must exist only in nautilus-core and nautilus-model"
  echo
  echo "$actual_ffi_manifests"
  exit 1
fi

expected_ffi_dependency_manifests=crates/model/Cargo.toml
actual_ffi_dependency_manifests=$(
  rg -l 'nautilus-(core|model)/ffi' crates --glob Cargo.toml | sort
)
if [[ "$actual_ffi_dependency_manifests" != "$expected_ffi_dependency_manifests" ]]; then
  echo "Error: only nautilus-model may enable another crate's ffi feature"
  echo
  echo "$actual_ffi_dependency_manifests"
  exit 1
fi

expected_ffi_dirs=$(printf '%s\n' crates/core/src/ffi crates/model/src/ffi)
actual_ffi_dirs=$(
  find crates \( -name target -o -name target-v2 \) -prune -o -type d -path '*/src/ffi' -print | sort
)
if [[ "$actual_ffi_dirs" != "$expected_ffi_dirs" ]]; then
  echo "Error: ffi modules must exist only in nautilus-core and nautilus-model"
  echo
  echo "$actual_ffi_dirs"
  exit 1
fi

expected_staticlib_manifests=$(printf '%s\n' crates/core/Cargo.toml crates/model/Cargo.toml)
actual_staticlib_manifests=$(rg -l 'crate-type[[:space:]]*=.*"staticlib"' crates --glob Cargo.toml | sort)
if [[ "$actual_staticlib_manifests" != "$expected_staticlib_manifests" ]]; then
  echo "Error: static library targets must exist only in nautilus-core and nautilus-model"
  echo
  echo "$actual_staticlib_manifests"
  exit 1
fi

expected_cbindgen_configs=$(printf '%s\n' crates/core/cbindgen.toml crates/model/cbindgen.toml)
actual_cbindgen_configs=$(rg --files crates | rg '/cbindgen\.toml$' | sort)
if [[ "$actual_cbindgen_configs" != "$expected_cbindgen_configs" ]]; then
  echo "Error: cbindgen configuration must exist only in nautilus-core and nautilus-model"
  echo
  echo "$actual_cbindgen_configs"
  exit 1
fi

echo "No legacy paths or unexpected FFI surfaces found"
