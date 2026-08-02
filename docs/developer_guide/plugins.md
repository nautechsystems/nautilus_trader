# Plugins

The `nautilus-plugin` crate defines the artifact contract for NautilusTrader plug-ins: an
independently compiled Rust `cdylib` that identifies itself with a versioned manifest and exchanges
values across a C-ABI boundary. The crate covers artifact identity and the boundary primitives only.
It does not load, register, or run plug-ins; the loading host is an internal Nautilus deployment
detail and is not part of this repository.

:::warning
The plug-in ABI is early alpha and the contract is unstable. Pin plug-in builds to the matching
`nautilus-plugin` version.
:::

## Artifact contract

A plug-in is a Rust `cdylib` that exports a single entry symbol, `nautilus_plugin_init`. The
`nautilus_plugin!` macro generates that symbol together with the static manifest carrying the build
identity:

```rust
nautilus_plugin::nautilus_plugin! {
    name: "example-plugin",
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
}
```

`name` and `version` are required, `vendor` defaults to an empty string. Invoke the macro once per
artifact at module scope, set `crate-type = ["cdylib"]` in the artifact's `Cargo.toml`, and depend on
the matching `nautilus-plugin` version.

## Manifest compatibility

`nautilus_plugin_init` takes an opaque host pointer and returns a `PluginManifest` held in
process-lifetime storage, or null when the host pointer is null or the call panics.
`PluginManifest::validate` checks the invariants a host relies on before registration. It reports
every structural problem it finds and fails when:

- `abi_version` does not equal `NAUTILUS_PLUGIN_ABI_VERSION`, or `build_id.schema_version` does not
  equal `PLUGIN_BUILD_ID_VERSION`.
- `plugin_name` or `plugin_version` is empty.
- Any manifest string is malformed: a null pointer with non-zero length, or bytes that are not valid
  UTF-8.
- `build_id.precision_mode` or `build_id.fixed_precision` differs from the host build.

Precision is validated because it changes model type layout across the boundary. The remaining build
identity fields (`nautilus-plugin` version, `rustc` version, target triple, and build profile) are
diagnostic.

## Boundary rules

Only the `#[repr(C)]` types in `nautilus_plugin::boundary`, and `#[repr(C)]` types built from them,
may appear in a signature that crosses the boundary. `String`, `Vec`, and `Box<dyn Trait>` rely on
Rust's unstable ABI and must never cross it. Panics must not unwind across the boundary either; the
generated entry symbol catches them and returns null.

Under the current ABI, plug-ins are not unloaded, so manifest storage stays valid for the life of the
process.

See [the crate docs](https://docs.rs/nautilus-plugin) for the boundary and manifest types.
